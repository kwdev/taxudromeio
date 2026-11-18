//! Import mail settings as data; never install profiles or trust their signatures.
use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};
use der::{
    Decode, Reader, Tag, TagNumber, Tagged,
    asn1::{AnyRef, ContextSpecific, ObjectIdentifier, OctetStringRef},
};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{
    config::{Account, AuthType, Config, Transport, safe_text},
    policy::Policy,
};

const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
const SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");

// No Debug or Serialize: an imported profile can contain login credentials.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ImportedAccount {
    pub host: String,
    pub port: u16,
    #[zeroize(skip)]
    pub transport: Transport,
    pub username: Option<String>,
    pub password: Option<String>,
    pub root_hint: Option<String>,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct RawProfile {
    #[serde(rename = "PayloadType")]
    kind: String,
    #[serde(rename = "PayloadContent")]
    payloads: Vec<RawPayload>,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct RawPayload {
    #[serde(default, rename = "PayloadType")]
    kind: String,
    #[serde(default, rename = "EmailAccountType")]
    account_type: String,
    #[serde(default, rename = "IncomingMailServerHostName")]
    host: String,
    #[serde(rename = "IncomingMailServerPortNumber")]
    port: Option<u64>,
    #[serde(rename = "IncomingMailServerUseSSL")]
    ssl: Option<bool>,
    #[serde(default, rename = "IncomingMailServerAuthentication")]
    authentication: String,
    #[serde(rename = "IncomingMailServerUsername")]
    username: Option<String>,
    #[serde(rename = "EmailAddress")]
    email: Option<String>,
    #[serde(rename = "IncomingPassword")]
    password: Option<String>,
    #[serde(rename = "IMAPPathPrefix")]
    prefix: Option<String>,
}

/// Only returns the encapsulated data from a DER CMS SignedData container.
/// ASN.1 decoding borrows the zeroized input, avoiding copies of its credentials.
/// Signers/certificates are not used to establish trust or install anything.
fn cms_payload(bytes: &[u8]) -> der::Result<&[u8]> {
    AnyRef::from_der(bytes)?.sequence(|outer| {
        if outer.decode::<ObjectIdentifier>()? != SIGNED_DATA {
            return Err(der::ErrorKind::Failed.into());
        }
        let signed: ContextSpecific<AnyRef<'_>> = outer.decode()?;
        if signed.tag_number != TagNumber::N0 {
            return Err(der::ErrorKind::Failed.into());
        }
        signed.value.sequence(|reader| {
            let _version: u8 = reader.decode()?;
            let algorithms: AnyRef<'_> = reader.decode()?;
            algorithms.tag().assert_eq(Tag::Set)?;
            let content: AnyRef<'_> = reader.decode()?;
            let payload = content.sequence(|reader| {
                if reader.decode::<ObjectIdentifier>()? != DATA {
                    return Err(der::ErrorKind::Failed.into());
                }
                let data: ContextSpecific<OctetStringRef<'_>> = reader.decode()?;
                if data.tag_number != TagNumber::N0 {
                    return Err(der::ErrorKind::Failed.into());
                }
                Ok(data.value.as_bytes())
            })?;
            // Optional certificates and CRLs, followed by signerInfos.
            for number in [TagNumber::N0, TagNumber::N1] {
                if reader.peek_tag()?
                    == (Tag::ContextSpecific {
                        number,
                        constructed: true,
                    })
                {
                    let _: AnyRef<'_> = reader.decode()?;
                }
            }
            let signers: AnyRef<'_> = reader.decode()?;
            signers.tag().assert_eq(Tag::Set)?;
            Ok(payload)
        })
    })
}

pub fn load(path: &Path) -> Result<Vec<ImportedAccount>> {
    let file = File::open(path).context("Could not open the mobileconfig file")?;
    ensure!(
        file.metadata()?.is_file(),
        "Mobileconfig input must be a regular file"
    );
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("Could not read the mobileconfig file")?;
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> Result<Vec<ImportedAccount>> {
    ensure!(
        bytes.len() as u64 <= MAX_PROFILE_BYTES,
        "Mobileconfig file exceeds 1 MiB"
    );
    let plist = if bytes.first() == Some(&0x30) {
        cms_payload(bytes).map_err(|_| anyhow::anyhow!(
            "Unsupported or malformed CMS profile; use a plain plist or DER SignedData mobileconfig with embedded content"
        ))?
    } else {
        bytes
    };
    // Ignore unrelated payload keys, including SMTP/calendar/contact credentials.
    // Do not include parser details: malformed input may contain secret text.
    let mut profile: RawProfile = plist::from_bytes(plist)
        .map_err(|_| anyhow::anyhow!("Invalid mobileconfig property list"))?;
    ensure!(
        profile.kind == "Configuration",
        "Expected an Apple configuration profile"
    );
    ensure!(
        profile.payloads.len() <= 128,
        "Mobileconfig has too many payloads"
    );
    let mut accounts = Vec::new();
    for payload in &mut profile.payloads {
        if payload.kind != "com.apple.mail.managed" || payload.account_type != "EmailTypeIMAP" {
            continue;
        }
        ensure!(
            payload.authentication == "EmailAuthPassword",
            "IMAP profile authentication is unsupported; configure credentials with auth instead"
        );
        let port = payload
            .port
            .map(u16::try_from)
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid IMAP port in mobileconfig"))?;
        let transport = if payload.ssl == Some(true) && port != Some(143) {
            Transport::Tls
        } else {
            // Non-SSL profiles must still establish verified encryption.
            Transport::Starttls
        };
        let port = port.unwrap_or_else(|| transport.default_port());
        let root_hint = payload.prefix.take().filter(|s| !s.is_empty());
        let account = Account {
            id: "import".into(),
            host: payload.host.clone(),
            port,
            transport,
            root: root_hint.clone().unwrap_or_else(|| "INBOX".into()),
            drafts_folder: None,
            auth_type: AuthType::Password,
            policy: Policy::default(),
        };
        Config {
            accounts: vec![account],
            ..Config::default()
        }
        .validate()
        .map_err(|_| anyhow::anyhow!("Invalid IMAP connection or folder prefix in mobileconfig"))?;
        let username = payload
            .username
            .take()
            .filter(|s| !s.is_empty())
            .or_else(|| payload.email.take().filter(|s| !s.is_empty()));
        let password = payload.password.take().filter(|s| !s.is_empty());
        let imported = ImportedAccount {
            host: payload.host.clone(),
            port,
            transport,
            username,
            password,
            root_hint,
        };
        ensure!(
            imported.username.as_ref().is_none_or(|s| safe_text(s))
                && imported.password.as_ref().is_none_or(|s| safe_text(s)),
            "Invalid mail login or password in mobileconfig"
        );
        accounts.push(imported);
    }
    ensure!(
        !accounts.is_empty(),
        "Mobileconfig contains no supported IMAP mail accounts"
    );
    Ok(accounts)
}

pub fn select(mut accounts: Vec<ImportedAccount>, number: usize) -> Result<ImportedAccount> {
    ensure!(
        number > 0 && number <= accounts.len(),
        "Mail account number must be between 1 and {}",
        accounts.len()
    );
    Ok(accounts.remove(number - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Value};

    fn mail(host: &str) -> Value {
        let mut fields = Dictionary::new();
        for (key, value) in [
            ("PayloadType", "com.apple.mail.managed"),
            ("EmailAccountType", "EmailTypeIMAP"),
            ("IncomingMailServerAuthentication", "EmailAuthPassword"),
            ("IncomingMailServerHostName", host),
            ("IncomingMailServerUsername", "fixture@example.test"),
            ("IncomingPassword", "fixture-app-password"),
            ("OutgoingPassword", "ignored-smtp-password"),
        ] {
            fields.insert(key.into(), value.into());
        }
        fields.insert("IncomingMailServerPortNumber".into(), 993_u64.into());
        fields.insert("IncomingMailServerUseSSL".into(), true.into());
        Value::Dictionary(fields)
    }

    fn document(payloads: Vec<Value>, binary: bool) -> Vec<u8> {
        let mut fields = Dictionary::new();
        fields.insert("PayloadType".into(), "Configuration".into());
        fields.insert("PayloadContent".into(), payloads.into());
        let document = Value::Dictionary(fields);
        let mut bytes = Vec::new();
        if binary {
            document.to_writer_binary(&mut bytes).unwrap();
        } else {
            document.to_writer_xml(&mut bytes).unwrap();
        }
        bytes
    }

    /// anchorbook: A-auth-profile-import#profile-formats
    #[test]
    fn xml_binary_and_real_signed_cms_extract_only_imap_settings() {
        for bytes in [
            document(vec![mail("imap.example.test")], false),
            document(vec![mail("imap.example.test")], true),
            include_bytes!("../tests/fixtures/mail-profile.der").to_vec(),
        ] {
            let accounts = parse(&bytes).unwrap();
            assert_eq!(accounts.len(), 1);
            let account = &accounts[0];
            assert_eq!(account.host, "imap.example.test");
            assert_eq!(account.port, 993);
            assert_eq!(account.transport, Transport::Tls);
            assert_eq!(account.username.as_deref(), Some("fixture@example.test"));
            assert_eq!(account.password.as_deref(), Some("fixture-app-password"));
        }
    }

    /// anchorbook: A-auth-profile-import#local-boundaries
    #[test]
    fn absent_credentials_stay_optional_and_prefix_is_only_a_hint() {
        let mut payload = mail("imap.example.test");
        let fields = payload.as_dictionary_mut().unwrap();
        fields.remove("IncomingMailServerUsername");
        fields.remove("IncomingPassword");
        fields.insert("EmailAddress".into(), "fallback@example.test".into());
        fields.insert("IMAPPathPrefix".into(), "Projects".into());
        let mut account =
            select(parse(&document(vec![payload.clone()], false)).unwrap(), 1).unwrap();
        assert_eq!(account.username.as_deref(), Some("fallback@example.test"));
        assert!(account.password.is_none());
        assert_eq!(account.root_hint.as_deref(), Some("Projects"));
        payload.as_dictionary_mut().unwrap().remove("EmailAddress");
        account = select(parse(&document(vec![payload], false)).unwrap(), 1).unwrap();
        assert!(account.username.is_none());
    }

    /// anchorbook: A-auth-profile-import#encrypted-connection
    #[test]
    fn non_ssl_and_port_143_require_starttls_without_plaintext_mode() {
        for (ssl, port, expected) in [
            (true, Some(143), Transport::Starttls),
            (false, Some(143), Transport::Starttls),
            (false, Some(1143), Transport::Starttls),
            (true, Some(1993), Transport::Tls),
            (true, None, Transport::Tls),
            (false, None, Transport::Starttls),
        ] {
            let mut payload = mail("imap.example.test");
            let fields = payload.as_dictionary_mut().unwrap();
            fields.insert("IncomingMailServerUseSSL".into(), ssl.into());
            if let Some(port) = port {
                fields.insert(
                    "IncomingMailServerPortNumber".into(),
                    Value::from(port as u64),
                );
            } else {
                fields.remove("IncomingMailServerPortNumber");
            }
            let accounts = parse(&document(vec![payload], false)).unwrap();
            assert_eq!(accounts[0].transport, expected);
            assert_eq!(
                accounts[0].port,
                port.unwrap_or_else(|| expected.default_port())
            );
        }
    }

    /// anchorbook: A-auth-profile-import#account-selection
    #[test]
    fn repeated_server_accounts_remain_distinct_and_selection_is_one_based() {
        let bytes = document(
            vec![mail("same.example.test"), mail("same.example.test")],
            false,
        );
        let accounts = parse(&bytes).unwrap();
        assert_eq!(accounts.len(), 2);
        assert!(select(parse(&bytes).unwrap(), 0).is_err());
        assert!(select(parse(&bytes).unwrap(), 3).is_err());
        assert_eq!(select(accounts, 2).unwrap().host, "same.example.test");
    }

    /// anchorbook: A-auth-profile-import#failure-safety
    #[test]
    fn malformed_unsupported_and_oversized_profiles_fail_without_echoing_values() {
        for (key, value) in [
            (
                "IncomingMailServerPortNumber",
                Value::from("private-secret"),
            ),
            ("IncomingMailServerPortNumber", Value::from(65536_u64)),
            (
                "IncomingMailServerHostName",
                Value::from("private-secret\r\nLOGIN"),
            ),
            (
                "IncomingMailServerAuthentication",
                Value::from("private-secret"),
            ),
            ("IMAPPathPrefix", Value::from("private-secret*")),
            (
                "IncomingMailServerUsername",
                Value::from("private-secret\n"),
            ),
        ] {
            let mut payload = mail("imap.example.test");
            payload
                .as_dictionary_mut()
                .unwrap()
                .insert(key.into(), value);
            let error = parse(&document(vec![payload], false))
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains("private-secret"));
            assert!(!error.contains("fixture-app-password"));
        }
        let mut pop = mail("pop.example.test");
        pop.as_dictionary_mut()
            .unwrap()
            .insert("EmailAccountType".into(), "EmailTypePOP".into());
        assert!(parse(&document(vec![pop], false)).is_err());
        assert!(parse(&vec![0; MAX_PROFILE_BYTES as usize + 1]).is_err());
        for bytes in [
            b"not a plist".as_slice(),
            &include_bytes!("../tests/fixtures/mail-profile.der")[..40],
        ] {
            assert!(parse(bytes).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_err());
        let file = dir.path().join("profile.mobileconfig");
        std::fs::write(&file, document(vec![mail("imap.example.test")], false)).unwrap();
        assert_eq!(load(&file).unwrap().len(), 1);
    }
}
