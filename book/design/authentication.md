# Authentication and configurations

The configuration ID is the unit of identity, policy, scope, and credential storage.

> **`A-auth-keychain`** (draft): **Login information remains in macOS Keychain**
> Property: The authentication command verifies new logins and roots before storing the login name and credential together in a Keychain item named by configuration ID. Renewal of an existing OAuth grant is preserved before verification to avoid losing rotated tokens. The TOML file and MCP discovery contain no login or credential; configuration-save failure attempts to restore the prior credential, including any renewal.
> Authority: `R-keychain-login`; `R-auth-command`
> - Criterion `no-file-secrets` [test]: TOML deserialization rejects username and password fields, and discovery and configuration-list outputs contain only nonsecret account settings.
> - Criterion `save-failure` [test]: A fake store observes restoration of prior credentials or removal of a new record after a configuration write failure.
> - Criterion `native-login` [review]: A macOS operator runs auth against a real provider, verifies a native Keychain item contains the login and credential, and confirms authentication saves no secrets to the TOML file.

> **`A-config-independent`** (draft): **The same server can back separate configurations**
> Property: Distinct configuration IDs can reuse a host, port and login while choosing independent roots, policies and Keychain items. Duplicate IDs are rejected; clients can expose an explicitly selected subset.
> Authority: `R-multiple-configurations`
> - Criterion `same-server` [test]: Identical hosts under different IDs load and route independently through MCP; duplicate IDs fail validation and unknown subset IDs fail CLI startup.

> **`A-auth-encrypted`** (draft): **Login follows verified encryption in either IMAP mode**
> Property: Configurations select implicit TLS or required STARTTLS. TLS verifies the certificate and hostname with a minimum protocol of TLS 1.2. No login is sent before successful encryption, and failed STARTTLS never falls back to plaintext.
> Authority: `R-starttls`; `R-keychain-login`; `D-imap-only`
> - Criterion `transport-settings` [test]: Legacy settings retain TLS and port 993, STARTTLS defaults to port 143, custom ports are preserved, and plaintext mode is rejected.
> - Criterion `upgrade-before-login` [test]: A loopback IMAP server observes STARTTLS before password or XOAUTH2 authentication over actual TLS, without a second greeting.
> - Criterion `no-downgrade` [test]: Unsupported or refused STARTTLS closes the connection without any login bytes.
> - Criterion `certificate-verification` [test]: Untrusted certificates and hostname mismatches fail before login.

> **`A-oauth-renewable`** (draft): **OAuth refresh renews credentials before operations**
> Property: Auth stores a renewable provider grant in Keychain. Operations refresh within 60 seconds of expiry, or before every operation if expiry is unavailable, and retry an authentication rejection at most once. Rotation is durably saved before the replacement token is used. Per-configuration locks serialize refresh within a process. A refresh or save failure stops authentication; no mailbox operation is replayed.
> Authority: `R-oauth-refresh`; `R-keychain-login`; `D-oauth-refresh-grant`
> - Criterion `provider-exchange` [test]: An actual loopback HTTPS exchange verifies the encoded refresh grant and optional client secret, expiry calculation, and rotated or preserved refresh tokens.
> - Criterion `refresh-lifecycle` [test]: Expiring and unknown-expiry tokens refresh, fresh tokens are reused, simultaneous requests share one refresh, and reauthentication preserves renewed grants across later failures.
> - Criterion `authentication-retry` [test]: Authentication rejection triggers one refresh and reconnect, persistence precedes reconnect, and a second rejection or another connection error is returned without further retry.
> - Criterion `failure-safety` [test]: HTTPS endpoint validation, redirect refusal, bounded response parsing, redacted provider errors, and failed refresh or Keychain writes prevent returning new credentials.
> - Criterion `legacy-records` [test]: Existing username-and-secret Keychain records deserialize without OAuth metadata.

The TOML file contains configuration version, ID, host, port, transport, root,
drafts_folder, credential type and policy. macOS's configuration directory is
`~/Library/Application Support/taxudromeio-mcp/config.toml`. A global `--config`
option overrides it. New configurations default to triage and TLS port 993.
Required STARTTLS defaults to port 143. Repeated auth for an existing ID preserves policy and its allowlist unless an
explicit profile flag changes the profile.

The native Security Framework stores JSON containing username and secret, plus
OAuth token endpoint, client ID, optional client secret, refresh token and
access-token expiry for renewable logins, in a
generic-password Keychain item. The service is `io.taxudromeio.mcp.imap` and the
item account is the configuration ID. IDs must therefore be unique across
configuration files on one macOS user account. Distinct IDs on the same server
receive distinct items, even with the same login.

Auth prompts for the secret with terminal echo disabled. No credential is
accepted as a command-line argument or environment override. Verification
checks the server's certificate, authenticates, discovers the root, and examines
the configured drafts mailbox if present. A root may be a nonselectable parent.
Save uses Keychain first and an atomic owner-only TOML replacement second.
An ordinary file-save error triggers Keychain rollback. Reusing an existing
OAuth grant saves renewal before mailbox verification so later failures cannot
restore an invalidated refresh token; rollback preserves that renewal.
Process crashes between the two stores and concurrent auth processes are outside
that transaction.

Credentials are zeroized on drop where owned by this application; libraries and
system APIs can make copies outside its control. Authentication error messages
omit server text that might echo credentials. Keychain failures never fall back
to plaintext. An unavailable or locked Keychain produces an actionable error.

Password auth accepts app passwords. OAuth auth accepts an already authorized,
IMAP-enabled refresh token, token endpoint, client ID and optional secret.
There is no browser authorization or automatic client registration. Refresh
uses verified HTTPS without redirects and a 30-second total timeout. Token
responses are limited to 64 KiB; errors omit response bodies and credential
values. Client secrets are sent in the form body; other provider authentication
schemes require an adapter. Access-token-only records remain readable but need
another auth run to gain refresh credentials.

Refresh and retry happen before mailbox operations. Tokens are reloaded from
Keychain inside the configuration lock. Concurrent server/auth processes and
separate IDs sharing a rotating grant are outside the lock's scope; use separate
provider grants or external coordination. Non-macOS credential stores remain
outside this design.
