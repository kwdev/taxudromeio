# Requirements

> **`R-keychain-login`** (draft): **Store login information and credentials in macOS Keychain**
> Statement: Login names and credentials must be stored in macOS Keychain rather than the configuration file.
> Source: The user's initial server request.

> **`R-auth-command`** (draft): **Authenticate after installation with the executable**
> Statement: Running taxudromeio-mcp auth must configure and authenticate a mailbox after installing the Rust binary.
> Source: The user's requested login command.

> **`R-multiple-configurations`** (draft): **Manage independent configurations on repeated servers**
> Statement: The server must manage multiple IMAP configurations, including configurations sharing the same server and account.
> Source: The user's multiple-configuration requirement.

> **`R-root-scope`** (draft): **Expose a selected folder tree for each configuration**
> Statement: Each configuration selects a root folder limiting the mailboxes exposed and operated on by MCP.
> Source: The user's per-configuration root-folder requirement.

> **`R-triage-default`** (draft): **Default to triage operations including drafts without sending**
> Statement: Filter operations per configuration, defaulting to reading, moving, read markers, flags and draft saving while excluding sending.
> Source: The user's requested triage behavior.

> **`R-rust-anchorbook`** (draft): **Build the server in Rust using Anchor Book**
> Statement: The server is written in Rust and uses the Anchor Book method and toolkit to connect its design and verification.
> Source: The user's language and Anchor Book requirements.

> **`R-starttls`** (draft): **Support required IMAP STARTTLS**
> Statement: Configurations can use STARTTLS, with verified encryption established before any login credential is sent and no plaintext fallback.
> Source: The user's annotation requesting the previously omitted STARTTLS feature on 2026-11-17.

> **`R-oauth-refresh`** (draft): **Refresh OAuth access tokens automatically**
> Statement: Renewable OAuth credentials remain in macOS Keychain and renew automatically so normal expiry does not require another auth run.
> Source: The user's annotation requesting the previously omitted automatic OAuth refresh feature on 2026-11-17.
