use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::policy::Policy;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    #[default]
    Password,
    Oauth2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    Tls,
    Starttls,
}

impl Transport {
    pub fn default_port(self) -> u16 {
        match self {
            Self::Tls => 993,
            Self::Starttls => 143,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(from = "AccountSettings")]
pub struct Account {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub transport: Transport,
    pub root: String,
    /// A relative folder below root; "." means root itself.
    #[serde(default)]
    pub drafts_folder: Option<String>,
    #[serde(default)]
    pub auth_type: AuthType,
    #[serde(default)]
    pub policy: Policy,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountSettings {
    id: String,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    transport: Transport,
    root: String,
    #[serde(default)]
    drafts_folder: Option<String>,
    #[serde(default)]
    auth_type: AuthType,
    #[serde(default)]
    policy: Policy,
}

impl From<AccountSettings> for Account {
    fn from(settings: AccountSettings) -> Self {
        Self {
            id: settings.id,
            host: settings.host,
            port: settings
                .port
                .unwrap_or_else(|| settings.transport.default_port()),
            transport: settings.transport,
            root: settings.root,
            drafts_folder: settings.drafts_folder,
            auth_type: settings.auth_type,
            policy: settings.policy,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
        }
    }
}

pub fn safe_text(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("Configuration ID must be 1–64 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported configuration version {}", self.version);
        }
        let mut ids = BTreeSet::new();
        for account in &self.accounts {
            validate_id(&account.id)?;
            if !ids.insert(&account.id) {
                bail!("Duplicate configuration ID: {}", account.id);
            }
            if !safe_text(&account.host)
                || account.host.chars().any(char::is_whitespace)
                || account.host.contains(['/', '\\', ':', '*', '%'])
                || account.port == 0
            {
                bail!("Invalid IMAP host or port for {}", account.id);
            }
            if !safe_text(&account.root) || account.root.contains(['*', '%']) {
                bail!("Root must name a specific, nonempty IMAP folder");
            }
            if account
                .drafts_folder
                .as_ref()
                .is_some_and(|s| !safe_text(s))
            {
                bail!("Drafts folder cannot be blank or contain control characters");
            }
        }
        Ok(())
    }

    pub fn account(&self, id: &str) -> Result<&Account> {
        self.accounts
            .iter()
            .find(|account| account.id == id)
            .with_context(|| format!("Unknown configuration: {id}"))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).with_context(|| {
            format!(
                "Read {}; run taxudromeio-mcp auth to configure an account",
                path.display()
            )
        })?;
        let config: Self = toml::from_str(&text).context("Invalid configuration TOML")?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_for_auth(path: &Path) -> Result<Self> {
        match fs::metadata(path) {
            Ok(_) => Self::load(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".taxudromeio-{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut created = false;
        let result = (|| -> Result<()> {
            let mut file = options.open(&temporary)?;
            created = true;
            file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub fn default_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().context("Could not locate the home directory")?;
    Ok(base.config_dir().join("taxudromeio-mcp/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// anchorbook: A-auth-encrypted#transport-settings
    #[test]
    fn omitted_port_follows_transport_and_legacy_configs_keep_tls() {
        let base = "id = 'work'\nhost = 'imap.example.com'\nroot = 'INBOX'\n";
        let legacy: Account = toml::from_str(base).unwrap();
        assert_eq!(legacy.transport, Transport::Tls);
        assert_eq!(legacy.port, 993);
        let starttls: Account = toml::from_str(&format!("{base}transport = 'starttls'\n")).unwrap();
        assert_eq!(starttls.transport, Transport::Starttls);
        assert_eq!(starttls.port, 143);
        let custom: Account =
            toml::from_str(&format!("{base}transport = 'starttls'\nport = 1143\n")).unwrap();
        assert_eq!(custom.port, 1143);
        assert!(toml::from_str::<Account>(&format!("{base}transport = 'plaintext'\n")).is_err());
    }

    fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            host: "imap.example.com".into(),
            port: 993,
            transport: Transport::Tls,
            root: "INBOX".into(),
            drafts_folder: None,
            auth_type: AuthType::Password,
            policy: Policy::default(),
        }
    }

    /// anchorbook: A-config-independent#same-server
    #[test]
    fn identical_servers_are_valid_but_duplicate_ids_are_not() {
        let mut config = Config {
            accounts: vec![account("work"), account("work-project")],
            ..Config::default()
        };
        assert!(config.validate().is_ok());
        config.accounts[1].id = "work".into();
        assert!(config.validate().is_err());
    }

    /// anchorbook: A-auth-keychain#no-file-secrets
    #[test]
    fn config_rejects_credentials_and_unknown_policy_values() {
        let text = "version = 1\n[[accounts]]\nid = 'work'\nhost = 'imap.example.com'\nroot = 'INBOX'\npassword = 'secret'\n";
        assert!(toml::from_str::<Config>(text).is_err());
        assert!(
            toml::from_str::<Config>(&text.replace("password = 'secret'", "username = 'alice'"))
                .is_err()
        );
        assert!(
            toml::from_str::<Config>(&text.replace(
                "password = 'secret'",
                "[accounts.policy]\nallowed_operations = ['send']"
            ))
            .is_err()
        );
    }

    #[test]
    fn save_roundtrip_is_private_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            accounts: vec![account("work")],
            ..Config::default()
        };
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().accounts[0].id, "work");
        config.save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn save_does_not_remove_a_preexisting_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let temporary = dir
            .path()
            .join(format!(".taxudromeio-{}.tmp", std::process::id()));
        fs::write(&temporary, "existing").unwrap();
        let path = dir.path().join("config.toml");
        assert!(Config::default().save(&path).is_err());
        assert_eq!(fs::read_to_string(temporary).unwrap(), "existing");
        assert!(!path.exists());
    }
}
