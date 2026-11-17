use std::{
    io::{Read, Write},
    sync::Mutex,
};

use anyhow::Result;
use native_tls::{Certificate, Identity, TlsAcceptor, TlsConnector};

use crate::{
    config::{Account, AuthType, Transport},
    credentials::{CredentialStore, Credentials, OAuthRefresh},
    policy::Policy,
};

pub const CA: &[u8] = include_bytes!("../tests/fixtures/tls/ca.pem");

pub fn acceptor() -> TlsAcceptor {
    let identity = Identity::from_pkcs12(
        include_bytes!("../tests/fixtures/tls/server.p12"),
        "fixture",
    )
    .unwrap();
    TlsAcceptor::new(identity).unwrap()
}

pub fn connector() -> TlsConnector {
    TlsConnector::builder()
        .add_root_certificate(Certificate::from_pem(CA).unwrap())
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .unwrap()
}

pub fn line(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n") {
            return String::from_utf8(bytes).unwrap();
        }
        assert!(bytes.len() < 64 * 1024);
    }
}

pub fn reply(stream: &mut impl Write, text: &str) {
    stream.write_all(text.as_bytes()).unwrap();
    stream.flush().unwrap();
}

pub fn account() -> Account {
    Account {
        id: "work".into(),
        host: "localhost".into(),
        port: 143,
        transport: Transport::Starttls,
        root: "INBOX".into(),
        drafts_folder: None,
        auth_type: AuthType::Oauth2,
        policy: Policy::default(),
    }
}

pub fn credentials() -> Credentials {
    Credentials {
        username: "fixture@example.test".into(),
        secret: "old-access".into(),
        oauth: Some(OAuthRefresh {
            token_endpoint: "https://localhost/token".into(),
            client_id: "test-client".into(),
            client_secret: None,
            refresh_token: "old-refresh".into(),
            expires_at: Some(u64::MAX),
        }),
    }
}

pub struct MemoryStore {
    pub record: Mutex<Credentials>,
    pub saves: Mutex<usize>,
    pub fail_save: bool,
}

impl MemoryStore {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            record: Mutex::new(credentials),
            saves: Mutex::new(0),
            fail_save: false,
        }
    }
}

impl CredentialStore for MemoryStore {
    fn load(&self, _: &str) -> Result<Credentials> {
        Ok(self.record.lock().unwrap().clone())
    }
    fn save(&self, _: &str, credentials: &Credentials) -> Result<()> {
        if self.fail_save {
            anyhow::bail!("Fixture Keychain save failed");
        }
        *self.record.lock().unwrap() = credentials.clone();
        *self.saves.lock().unwrap() += 1;
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<()> {
        anyhow::bail!("unused")
    }
}
