# Initial server

**Roadmap identifier:** `initial-server`

## Goal

Deliver an installable local Rust MCP server with independently scoped IMAP
configurations, native macOS credentials, and filtered triage tools.
This is a prototype assignment from the user's initial request.

## Milestone 1. A scoped triage server is installable and verified {#initial-server-1}

**Outcome:** A local MCP client can list configurations and triage their scoped mail; the installed binary offers interactive Keychain authentication.

**Status:** In progress

**Identifier:** `initial-server-1`

**Dependencies:** None for the assigned prototype. Real-provider acceptance requires an operator's account.

**Design basis:** [Access](../design/access.md), [authentication](../design/authentication.md), and [requirements](../authority/requirements.md).

**Tasks**

- [x] Implement independent TOML configurations and Keychain credential storage.
- [x] Implement interactive auth and local stdio MCP.
- [x] Implement typed policy, folder confinement, UID checks, reads, moves, copies, flags and draft appends.
- [x] Run deterministic unit, CLI, MCP and IMAP boundary tests.
- [x] Validate and render the Book using the pinned Anchor Book engine.
- [ ] Record a real-provider and native Keychain acceptance run.

### Completion criteria

#### Scoped triage {#initial-server-1-scoped-triage}

The supported operations and per-configuration boundaries pass deterministic tests through production adapters.

- **Anchor coverage:** `A-policy-triage` · `operation-filter`, `direct-call-enforcement`, `draft-only` — Full
- **Anchor coverage:** `A-folder-confined` · `root-boundaries`, `wire-boundaries` — Full
- **Anchor coverage:** `A-message-stable` · `stale-identifiers`, `wire-safety` — Full
- **Anchor coverage:** `A-config-independent` · `same-server` — Full
- **Evidence:** Passed on 2026-11-17: 22 unit tests and four CLI/MCP integration tests exercise policy narrowing, same-host IDs, root boundaries, stale UIDVALIDITY, header injection, modified UTF-7, read-only EXAMINE/BODY.PEEK, UID MOVE, copy and flag commands, draft APPEND flags, and rejection before backend access. Protocol fixtures verify no CLOSE, EXPUNGE or deletion fallback. Book validation adds one test, for 27 total.

#### Credential lifecycle {#initial-server-1-credential-lifecycle}

Login and credentials are stored only in Keychain; configuration-save failure restores previous state, and real macOS authentication is observed.

- **Anchor coverage:** `A-auth-keychain` · `no-file-secrets`, `save-failure`, `native-login` — Full
- **Evidence:** Partial on 2026-11-17: TOML rejection, nonsecret CLI/discovery output, and fake-store rollback tests passed. Native Security Framework integration compiled on macOS. The native-login review has not run; ordinary tests do not access real Keychain or accounts.

#### Development checks {#initial-server-1-development-checks}

The binary builds, CLI help works, formatting and Clippy pass, and the Book's structure, records and roadmaps validate and render.

- **Anchor coverage:** None — contributor tooling and packaging.
- **Evidence:** Passed on 2026-11-17: formatting, all-target Clippy with warnings denied, 27 tests, Anchor Book structural/v1 check, record and roadmap validation, and mdBook 0.5.4 rendering. A release installation into a temporary directory succeeded. The installed binary passed a real stdio subprocess smoke check for initialization, tool discovery, selected configuration discovery, send denial and clean EOF shutdown.

### Evidence {#initial-server-1-evidence}

**Revision:** Uncommitted initial working tree on main, with no baseline commit.
Code and design were checked together on 2026-11-17 using Rust 1.98.1 and the
Anchor Book revision pinned in Cargo.toml. The final evidence edit changes only
this roadmap's progress and findings.

**Checks:** `cargo fmt --check`, `cargo test --locked --all-targets`,
`cargo clippy --locked --all-targets -- -D warnings`,
`cargo run --quiet --locked --example anchorbook -- check`, and `mdbook build`.
Release installation used `cargo install --path . --locked --offline --root`
with a temporary destination, followed by a subprocess MCP smoke check.

**Results:** Local checks and release smoke passed. Rendered anchor records and
criterion links were inspected. CI, mutation testing, native Keychain and
real-provider acceptance have not run. No mailbox credentials were supplied.
The upstream imap-proto 0.10.2 dependency emits a macro future-incompatibility
warning; it builds successfully with the tested compiler, but a future Rust
upgrade requires reassessment.

### Completion record

Pending native Keychain and real-provider acceptance in addition to local checks.

## Milestone 2. IMAP encryption and OAuth renewal support are verified {#initial-server-2}

**Outcome:** Configurations support required STARTTLS and renewable OAuth logins, with failed authentication stopping before mailbox operations.

**Status:** Complete

**Identifier:** `initial-server-2`

**Dependencies:** The local server in milestone 1. Live-provider interoperability remains part of its pending acceptance run.

**Design basis:** [Authentication](../design/authentication.md), [requirements](../authority/requirements.md), and [decisions](../authority/decisions.md). Assigned by the user's 2026-11-17 annotation requesting STARTTLS and automatic OAuth refresh.

**Tasks**

- [x] Implement explicit TLS/STARTTLS configuration and mandatory encrypted login.
- [x] Extend Keychain records and interactive auth with renewable OAuth grants.
- [x] Refresh before expiry and retry rejected authentication once before operations.
- [x] Preserve rotated grants before use and across reauthentication failures.
- [x] Add deterministic TLS, HTTPS, concurrency and failure fixtures.
- [x] Run the complete development checks and record evidence.

### Completion criteria

#### Encrypted IMAP {#initial-server-2-encrypted-imap}

STARTTLS establishes verified TLS before password or XOAUTH2 login and never falls back after upgrade or certificate failures.

- **Anchor coverage:** `A-auth-encrypted` · `transport-settings`, `upgrade-before-login`, `no-downgrade`, `certificate-verification` — Full
- **Evidence:** Passed on 2026-11-17: transport default and compatibility tests; real loopback STARTTLS handshakes for password and XOAUTH2; refused and unsupported upgrades; untrusted certificate and hostname mismatch rejection with no login bytes.

#### Renewable credentials {#initial-server-2-renewable-credentials}

Automatic refresh exchanges grants over HTTPS, preserves rotation in Keychain, reuses fresh tokens, and retries authentication without replaying mailbox operations.

- **Anchor coverage:** `A-oauth-renewable` · `provider-exchange`, `refresh-lifecycle`, `authentication-retry`, `failure-safety`, `legacy-records` — Full
- **Evidence:** Passed on 2026-11-17: HTTPS refresh exchanges for public and secret-bearing clients, expiry and token rotation, concurrent refresh sharing, reauthentication rollback preserving renewal, one authentication retry, failure before IMAP access, redirect rejection, bounded and redacted responses, and legacy Keychain record parsing. Stores and provider grants are fixtures.

#### Updated development checks {#initial-server-2-development-checks}

Formatting, Clippy, all tests, pinned Book checks, Book rendering, and the installed CLI pass with the new authentication behavior.

- **Anchor coverage:** None — contributor tooling and packaging.
- **Evidence:** Passed on 2026-11-17: formatting, all-target Clippy with warnings denied, 36 unit tests plus one Book test and four CLI/MCP integration tests (41 total), pinned Anchor Book structural check and record/roadmap validation, and mdBook rendering. Temporary release installation passed CLI and stdio MCP smoke checks.

### Evidence {#initial-server-2-evidence}

**Revision:** Uncommitted working tree on main, with no baseline commit.

**Checks:** `cargo fmt --check`, `cargo clippy --locked --all-targets --offline -- -D warnings`, `cargo test --locked --all-targets --offline`, `cargo run --quiet --locked --offline --example anchorbook -- check`, and `mdbook build`. Socket tests ran with loopback permission because the workspace sandbox blocks listeners. `cargo install --path . --locked --offline --force --root /private/tmp/taxudromeio-install` succeeded. The installed binary passed auth-help, TLS/STARTTLS default-port, stdio initialization, tool discovery, configuration subset, send-denial and EOF-shutdown checks.

**Results:** All local checks passed. Tests use fake credentials and loopback
services; no real-provider or native Keychain acceptance is claimed.
Browser consent and provider registration are external to the refresh-grant
setup. Refresh serialization covers one process and configuration; sharing a
rotating grant across IDs or concurrent processes requires coordination.
The preexisting imap-proto future-incompatibility warning remains.
Only this roadmap's evidence and completion fields changed after those checks;
Book validation and rendering were rerun on that final evidence.

### Completion record

Completed the assigned STARTTLS and automatic refresh implementation with
deterministic local evidence on 2026-11-17. Native Keychain and live-provider
acceptance remain recorded in milestone 1.
