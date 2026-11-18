# Requirements

> **`R-keychain-login`** (draft): **Store login information and credentials in macOS Keychain**
> Statement: Login names and credentials must be stored in macOS Keychain rather than the configuration file.
> Source: The user's initial server request.

> **`R-auth-command`** (draft): **Authenticate after installation with the executable**
> Statement: Running taxudromeio auth must configure and authenticate a mailbox after installing the Rust binary.
> Source: The user's requested login command.

> **`R-multiple-configurations`** (draft): **Manage independent configurations on repeated servers**
> Statement: The server must manage multiple IMAP configurations, including configurations sharing the same server and account.
> Source: The user's multiple-configuration requirement.

> **`R-root-scope`** (draft): **Expose a selected folder tree for each configuration**
> Statement: Each configuration selects a root folder limiting the mailboxes exposed and operated on through either MCP or the CLI.
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

> **`R-cli-setup-help`** (draft): **Explain setup in executable help**
> Statement: The top-level --help explains authentication and MCP client setup, including a usable JSON configuration example.
> Source: The user's 2026-11-17 request for setup instructions and typical client JSON in --help.

> **`R-profile-setup`** (draft): **Configure mail using an Apple profile**
> Statement: Auth can import IMAP setup from a user-selected mobileconfig file, including its login and incoming credential, while keeping scope and policy local and storing credentials in Keychain.
> Source: The user's 2026-11-18 request to configure from an attached Apple profile.

> **`R-cli-operations`** (draft): **Operate on mail using CLI arguments**
> Statement: A Clap-based CLI exposes all supported mail operations through arguments, sharing request types and execution with MCP so policy, scope, credentials and results remain consistent.
> Source: The user's 2026-11-18 request for a CLI mode sharing code with the MCP server.

> **`R-project-name`** (draft): **Name the project and executable taxudromeio**
> Statement: The Rust package and installed executable are named taxudromeio. Existing settings and Keychain records remain usable after the rename.
> Source: The user's 2026-11-18 project rename request.

> **`R-cli-agent-help`** (draft): **Teach agents the CLI workflow through help**
> Statement: taxudromeio --help teaches an LLM to discover configurations, use scoped folders and UIDVALIDITY, read and paginate mail, and perform permitted mutations through arguments with JSON results.
> Source: The user's 2026-11-18 request for help that teaches an LLM how to use the tool.

> **`R-cli-skill`** (draft): **Print detailed agent usage guidance**
> Statement: taxudromeio skill prints a longer Markdown guide explaining CLI-first login, assistance with downloaded credential profiles and interactive setup, mail commands, multiple configurations and MCP tradeoffs. The guide is available without saved settings or credentials.
> Source: The user's 2026-11-18 request for a skill subcommand and MCP efficiency explanation.
