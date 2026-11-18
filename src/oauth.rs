use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{
    config::{Account, AuthType, safe_text},
    credentials::{CredentialStore, Credentials, OAuthRefresh},
};

const MAX_RESPONSE: u64 = 64 * 1024;
const REFRESH_MARGIN: u64 = 60;

fn now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before the Unix epoch")?
        .as_secs())
}

pub fn validate(oauth: &OAuthRefresh) -> Result<()> {
    let url = Url::parse(&oauth.token_endpoint)
        .map_err(|_| anyhow::anyhow!("Invalid OAuth token endpoint"))?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "OAuth token endpoint must be HTTPS without user information, query or fragment"
    );
    ensure!(
        safe_text(&oauth.client_id) && safe_text(&oauth.refresh_token),
        "OAuth client ID and refresh token must be nonempty and contain no control characters"
    );
    ensure!(
        oauth.client_secret.as_ref().is_none_or(|s| safe_text(s)),
        "Invalid OAuth client secret"
    );
    Ok(())
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

fn client() -> Result<Client> {
    Client::builder()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| anyhow::anyhow!("Could not initialize OAuth HTTPS client"))
}

/// Called only on blocking workers. HTTP errors and provider bodies are not logged.
pub fn refresh(credentials: &mut Credentials) -> Result<()> {
    let oauth = credentials
        .oauth
        .as_ref()
        .context("OAuth token cannot be refreshed; run taxudromeio auth --auth-type oauth2")?;
    validate(oauth)?;
    refresh_using(credentials, &client()?)
}

fn refresh_using(credentials: &mut Credentials, http: &Client) -> Result<()> {
    let oauth = credentials
        .oauth
        .as_ref()
        .context("Missing OAuth refresh credentials")?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", oauth.refresh_token.as_str()),
        ("client_id", oauth.client_id.as_str()),
    ];
    if let Some(secret) = &oauth.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    let response = http
        .post(&oauth.token_endpoint)
        .form(&form)
        .send()
        .map_err(|_| {
            anyhow::anyhow!("OAuth refresh request failed; check provider connectivity")
        })?;
    ensure!(
        response.status().is_success(),
        "OAuth refresh rejected (HTTP {}); run taxudromeio auth if consent or the refresh token was revoked",
        response.status().as_u16()
    );
    let mut bytes = Zeroizing::new(Vec::new());
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Could not read OAuth token response"))?;
    ensure!(
        bytes.len() as u64 <= MAX_RESPONSE,
        "OAuth token response is too large"
    );
    let token: TokenResponse = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid OAuth token response"))?;
    ensure!(
        safe_text(&token.access_token) && token.token_type.eq_ignore_ascii_case("bearer"),
        "OAuth provider did not return a usable bearer token"
    );
    ensure!(
        token.refresh_token.as_ref().is_none_or(|s| safe_text(s)),
        "OAuth provider returned an invalid refresh token"
    );
    ensure!(
        token.expires_in.is_none_or(|s| s > 0 && s <= 31_536_000),
        "OAuth provider returned an invalid token lifetime"
    );
    let expires_at = token
        .expires_in
        .map(|seconds| now().map(|n| n + seconds))
        .transpose()?;
    // Only change the record after the entire response has been validated.
    credentials.secret.zeroize();
    credentials.secret = token.access_token.clone();
    let oauth = credentials.oauth.as_mut().unwrap();
    oauth.expires_at = expires_at;
    if let Some(rotated) = &token.refresh_token {
        oauth.refresh_token.zeroize();
        oauth.refresh_token = rotated.clone();
    }
    Ok(())
}

fn due(credentials: &Credentials) -> Result<bool> {
    let deadline = now()?.saturating_add(REFRESH_MARGIN);
    Ok(credentials
        .oauth
        .as_ref()
        .is_some_and(|oauth| oauth.expires_at.is_none_or(|expiry| expiry <= deadline)))
}

pub struct CredentialManager {
    store: Arc<dyn CredentialStore>,
    // Each configuration serializes refresh and reloads Keychain inside the lock.
    // Other configurations can refresh independently.
    locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    refresher: Arc<Refresher>,
}

type Refresher = dyn Fn(&mut Credentials) -> Result<()> + Send + Sync;

impl CredentialManager {
    pub fn new(store: Arc<dyn CredentialStore>) -> Self {
        Self {
            store,
            locks: Mutex::new(BTreeMap::new()),
            refresher: Arc::new(refresh),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_refresher(
        store: Arc<dyn CredentialStore>,
        refresher: Arc<Refresher>,
    ) -> Self {
        Self {
            store,
            locks: Mutex::new(BTreeMap::new()),
            refresher,
        }
    }

    pub fn fresh(&self, account: &Account) -> Result<Credentials> {
        self.obtain(account, None)
    }

    pub fn after_rejection(
        &self,
        account: &Account,
        rejected: &Credentials,
    ) -> Result<Credentials> {
        self.obtain(account, Some(&rejected.secret))
    }

    fn obtain(&self, account: &Account, rejected: Option<&str>) -> Result<Credentials> {
        let lock = self
            .locks
            .lock()
            .map_err(|_| anyhow::anyhow!("Credential lock failed"))?
            .entry(account.id.clone())
            .or_default()
            .clone();
        let _guard = lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Credential lock failed"))?;
        let mut credentials = self.store.load(&account.id)?;
        if matches!(account.auth_type, AuthType::Oauth2) {
            let forced = rejected.is_some_and(|token| token == credentials.secret);
            if forced || due(&credentials)? {
                (self.refresher)(&mut credentials)?;
                self.store.save(&account.id, &credentials)
                    .context("Refreshed OAuth credentials could not be saved in Keychain; authentication stopped")?;
            }
        }
        Ok(credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self as fixture, MemoryStore};
    use std::{
        io::Write,
        net::TcpListener,
        sync::atomic::{AtomicUsize, Ordering},
        thread,
    };

    fn http_fixture(
        status: &str,
        headers: &str,
        body: &str,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!(
            "https://localhost:{}/token",
            listener.local_addr().unwrap().port()
        );
        let acceptor = fixture::acceptor();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let handle = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut tls = acceptor.accept(tcp).unwrap();
            let mut request = String::new();
            let length = loop {
                let line = fixture::line(&mut tls);
                request.push_str(&line);
                if line == "\r\n" {
                    break request
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                }
            };
            let mut form = vec![0; length];
            tls.read_exact(&mut form).unwrap();
            request.push_str(std::str::from_utf8(&form).unwrap());
            tls.write_all(response.as_bytes()).unwrap();
            request
        });
        (endpoint, handle)
    }

    fn http_client() -> Client {
        Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .add_root_certificate(reqwest::Certificate::from_pem(fixture::CA).unwrap())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    /// anchorbook: A-oauth-renewable#provider-exchange
    #[test]
    fn refresh_posts_encoded_grant_and_preserves_or_rotates_refresh_token() {
        for rotated in [false, true] {
            let body = if rotated {
                r#"{"access_token":"fresh-access","token_type":"Bearer","expires_in":3600,"refresh_token":"rotated-refresh"}"#
            } else {
                r#"{"access_token":"fresh-access","token_type":"bearer","expires_in":3600}"#
            };
            let (endpoint, server) =
                http_fixture("200 OK", "Content-Type: application/json\r\n", body);
            let mut credentials = fixture::credentials();
            let oauth = credentials.oauth.as_mut().unwrap();
            oauth.token_endpoint = endpoint;
            oauth.client_secret = rotated.then(|| "private+secret&value".into());
            oauth.refresh_token = "refresh+token&value".into();
            validate(oauth).unwrap();
            refresh_using(&mut credentials, &http_client()).unwrap();
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /token HTTP/1.1\r\n"));
            assert!(request.contains("grant_type=refresh_token"));
            assert!(request.contains("refresh_token=refresh%2Btoken%26value"));
            assert!(request.contains("client_id=test-client"));
            if rotated {
                assert!(request.contains("client_secret=private%2Bsecret%26value"));
            } else {
                assert!(!request.contains("client_secret"));
            }
            assert_eq!(credentials.secret, "fresh-access");
            let oauth = credentials.oauth.as_ref().unwrap();
            assert_eq!(
                oauth.refresh_token,
                if rotated {
                    "rotated-refresh"
                } else {
                    "refresh+token&value"
                }
            );
            assert!(oauth.expires_at.unwrap() > now().unwrap() + 3500);
        }
    }

    /// anchorbook: A-oauth-renewable#failure-safety
    #[test]
    fn provider_errors_redirects_and_invalid_bodies_leave_credentials_intact_and_redact_secrets() {
        let cases = [
            (
                "400 Bad Request",
                "",
                r#"{"error":"invalid_grant","error_description":"private-echo old-refresh"}"#,
            ),
            (
                "302 Found",
                "Location: https://localhost/redirect\r\n",
                "private-echo",
            ),
            ("200 OK", "", "private-echo malformed-json"),
            (
                "200 OK",
                "",
                r#"{"access_token":"private-echo","token_type":"MAC"}"#,
            ),
            (
                "200 OK",
                "",
                r#"{"access_token":"private-echo","token_type":"Bearer","expires_in":0,"refresh_token":"rotated"}"#,
            ),
            (
                "200 OK",
                "",
                r#"{"access_token":"private-echo","token_type":"Bearer","refresh_token":""}"#,
            ),
        ];
        for (status, headers, body) in cases {
            let (endpoint, server) = http_fixture(status, headers, body);
            let mut credentials = fixture::credentials();
            credentials.oauth.as_mut().unwrap().token_endpoint = endpoint;
            let error = refresh_using(&mut credentials, &http_client())
                .unwrap_err()
                .to_string();
            server.join().unwrap();
            assert!(!error.contains("private-echo"));
            assert!(!error.contains("old-refresh"));
            if status.starts_with("302") {
                assert!(error.contains("HTTP 302"));
            }
            assert_eq!(credentials.secret, "old-access");
            assert_eq!(
                credentials.oauth.as_ref().unwrap().refresh_token,
                "old-refresh"
            );
        }
        let huge = "x".repeat(MAX_RESPONSE as usize + 1);
        let (endpoint, server) = http_fixture("200 OK", "", &huge);
        let mut credentials = fixture::credentials();
        credentials.oauth.as_mut().unwrap().token_endpoint = endpoint;
        assert!(
            refresh_using(&mut credentials, &http_client())
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
        server.join().unwrap();
    }

    /// anchorbook: A-oauth-renewable#failure-safety
    #[test]
    fn token_endpoint_rejects_plaintext_credentials_and_redirect_parameters() {
        for endpoint in [
            "http://localhost/token",
            "https://user:secret@localhost/token",
            "https://localhost/token?secret=value",
            "https://localhost/token#fragment",
            "invalid",
        ] {
            let mut credentials = fixture::credentials();
            credentials.oauth.as_mut().unwrap().token_endpoint = endpoint.into();
            assert!(validate(credentials.oauth.as_ref().unwrap()).is_err());
        }
    }

    /// anchorbook: A-oauth-renewable#refresh-lifecycle
    #[test]
    fn simultaneous_expiring_tokens_refresh_once_and_save_rotation() {
        let mut credentials = fixture::credentials();
        credentials.oauth.as_mut().unwrap().expires_at = Some(now().unwrap() + 30);
        let store = Arc::new(MemoryStore::new(credentials));
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let manager = Arc::new(CredentialManager::with_refresher(
            store.clone(),
            Arc::new(move |c| {
                count.fetch_add(1, Ordering::SeqCst);
                c.secret = "new-access".into();
                let oauth = c.oauth.as_mut().unwrap();
                oauth.refresh_token = "new-refresh".into();
                oauth.expires_at = Some(now()? + 3600);
                Ok(())
            }),
        ));
        let start = Arc::new(std::sync::Barrier::new(8));
        let workers = (0..8)
            .map(|_| {
                let manager = manager.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    assert_eq!(
                        manager.fresh(&fixture::account()).unwrap().secret,
                        "new-access"
                    );
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(*store.saves.lock().unwrap(), 1);
        assert_eq!(
            store
                .record
                .lock()
                .unwrap()
                .oauth
                .as_ref()
                .unwrap()
                .refresh_token,
            "new-refresh"
        );
        // Another worker's rejection of the old token reuses the saved replacement.
        let old = fixture::credentials();
        assert_eq!(
            manager
                .after_rejection(&fixture::account(), &old)
                .unwrap()
                .secret,
            "new-access"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// anchorbook: A-oauth-renewable#refresh-lifecycle
    #[test]
    fn valid_tokens_are_reused_and_unknown_expiry_refreshes() {
        let store = Arc::new(MemoryStore::new(fixture::credentials()));
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let manager = CredentialManager::with_refresher(
            store.clone(),
            Arc::new(move |c| {
                count.fetch_add(1, Ordering::SeqCst);
                c.secret = "new".into();
                c.oauth.as_mut().unwrap().expires_at = Some(now()? + 3600);
                Ok(())
            }),
        );
        assert_eq!(
            manager.fresh(&fixture::account()).unwrap().secret,
            "old-access"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        store
            .record
            .lock()
            .unwrap()
            .oauth
            .as_mut()
            .unwrap()
            .expires_at = None;
        assert_eq!(manager.fresh(&fixture::account()).unwrap().secret, "new");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// anchorbook: A-oauth-renewable#failure-safety
    #[test]
    fn refresh_or_keychain_failure_does_not_return_new_credentials() {
        let mut credentials = fixture::credentials();
        credentials.oauth.as_mut().unwrap().expires_at = Some(1);
        let mut store = MemoryStore::new(credentials);
        store.fail_save = true;
        let store = Arc::new(store);
        let manager = CredentialManager::with_refresher(
            store.clone(),
            Arc::new(|c| {
                c.secret = "new".into();
                Ok(())
            }),
        );
        assert!(manager.fresh(&fixture::account()).is_err());
        assert_eq!(store.record.lock().unwrap().secret, "old-access");
        assert_eq!(*store.saves.lock().unwrap(), 0);
        let manager = CredentialManager::with_refresher(
            store.clone(),
            Arc::new(|_| anyhow::bail!("Fixture provider refused refresh")),
        );
        assert!(manager.fresh(&fixture::account()).is_err());
        assert_eq!(store.record.lock().unwrap().secret, "old-access");
    }

    /// anchorbook: A-oauth-renewable#legacy-records
    #[test]
    fn original_password_and_access_token_records_still_deserialize() {
        let credentials: Credentials =
            serde_json::from_str(r#"{"username":"old","secret":"credential"}"#).unwrap();
        assert!(credentials.oauth.is_none());
        assert!(
            !serde_json::to_string(&credentials)
                .unwrap()
                .contains("oauth")
        );
    }
}
