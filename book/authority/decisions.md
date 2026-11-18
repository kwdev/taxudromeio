# Decisions

> **`D-imap-only`** (draft): **Implement a local IMAP-only server**
> Choice: Use local stdio MCP and CLI interfaces to IMAP over verified implicit TLS or required STARTTLS, without SMTP or a sending operation. Save drafts using IMAP APPEND.
> Requirements: `R-triage-default`; `R-rust-anchorbook`; `R-starttls`
> Rationale: The requested triage workflow can be implemented entirely over IMAP, with no send path to accidentally expose.
> Alternatives:
> - Add SMTP with a configurable send policy, which expands the access boundary and setup.
> - Run a hosted server, which transfers mailbox credentials to another machine.
> Reconsider when: Sending or remote clients become an explicitly requested scope.

> **`D-scoped-uids`** (draft): **Use mailbox UIDs with required UIDVALIDITY**
> Choice: Require positive UIDs and the current mailbox's UIDVALIDITY for content reads, moves, copies and flag changes.
> Requirements: `R-root-scope`; `R-triage-default`
> Rationale: Sequence numbers change when mail arrives or disappears; UIDVALIDITY detects a mailbox identifier reset.
> Alternatives:
> - Sequence numbers are simpler but can silently refer to different messages.
> - Opaque persisted message tokens add state and still require validating the underlying UID namespace.
> Reconsider when: A persistent mail index or higher-level message identity is added.

> **`D-oauth-refresh-grant`** (draft): **Use a provider's registered refresh grant**
> Choice: Auth accepts a provider-issued IMAP refresh token and client registration interactively. Use HTTPS refresh-token requests, with an optional client secret in the form body. Serialize refresh per configuration within a process; preserve rotation in Keychain before authenticating.
> Requirements: `R-oauth-refresh`; `R-keychain-login`; `R-auth-command`
> Rationale: The token lifecycle can support multiple providers without assuming one browser authorization endpoint, registration, or scope model.
> Alternatives:
> - Add provider-specific browser authorization and PKCE, which needs provider registration and consent behavior.
> - Ask for a new access token on expiry, which fails the automatic refresh requirement.
> Reconsider when: Browser authorization, additional client authentication schemes, or coordinated refresh across server processes becomes assigned work.

> **`D-shared-mail-service`** (draft): **Use one mail service behind both interfaces**
> Choice: Clap and MCP deserialize the same typed mail arguments. A shared service resolves the selected configuration, checks its policy, limits blocking workers and calls the same IMAP backend. Both interfaces use the same nonsecret configuration discovery and JSON operation results. Retain the existing settings directory and Keychain service identity across the executable rename.
> Requirements: `R-cli-operations`; `R-root-scope`; `R-triage-default`; `R-project-name`
> Rationale: Policy and protocol implementations shared by both adapters prevent a CLI-specific access path or lost credentials after renaming.
> Alternatives:
> - Duplicate operation implementations in the CLI, creating separate enforcement and verification obligations.
> - Make the CLI spawn an MCP subprocess, adding a protocol round trip to each command.
> Reconsider when: A remote service or persistent CLI session becomes assigned work.
