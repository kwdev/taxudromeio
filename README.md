# Taxudromeio MCP

A Rust MCP server for local IMAP triage. Login names and credentials live in
macOS Keychain. Each configuration has a unique ID, an exposed root folder,
and its own operation policy. Several configurations can use the same server
and login with different roots or policies.

The default `triage` profile supports reading, searching, moving, copying,
marking read, flagging, and saving drafts. There is no sending or SMTP code.
Reading does not automatically mark messages read.

## Install and authenticate

Requires macOS and Rust stable (tested with Rust 1.98.1).

```sh
cargo install --path . --locked
taxudromeio-mcp auth
```

Setup asks for a configuration ID, IMAP host, encryption mode, port, root folder,
optional relative drafts folder, and login. Secret input is hidden. It verifies
TLS, authentication, root existence, and the drafts folder before saving.
No mail is changed during setup.

To add another configuration or update an existing ID:

```sh
taxudromeio-mcp auth --id work \
  --host imap.example.com --root Projects --drafts-folder Drafts

taxudromeio-mcp auth --id project \
  --host imap.example.com --root Projects/Taxudromeio

taxudromeio-mcp auth --id archive \
  --host imap.example.com --root Archive --profile read-only
```

You still enter the login and credential interactively for each ID. The root and
drafts folder must already exist. Host and root flags are examples, not provider
presets. Implicit TLS defaults to port 993. Required STARTTLS defaults to port
143; custom ports work in either mode. Certificates and hostnames are always
verified. STARTTLS refusal or TLS verification failure stops before login.

```sh
taxudromeio-mcp auth --id work --transport starttls --port 143
taxudromeio-mcp auth --id personal --auth-type oauth2
```

Use your provider's app password, or select OAuth with automatic refresh.
OAuth setup asks for your IMAP login, HTTPS token endpoint, registered client
ID, refresh token, and optional client secret. Secret input is hidden. It
exchanges the refresh token for an access token and verifies the IMAP login.
The login, both tokens, expiry, and OAuth registration are stored together in
Keychain. TOML contains only the account settings and authentication type.

Tokens refresh automatically within 60 seconds of expiry, and once after an
IMAP authentication rejection. Rotated refresh tokens are saved before the
new access token is used. Concurrent requests for one configuration share the
refresh; different configurations remain independent. Refresh failures stop
the operation. Mail mutations are never replayed by authentication retries.
Providers that omit expiry cause a refresh before each operation.

Obtain an IMAP-enabled refresh token through your provider's authorization
flow, using your own registered OAuth client and offline access where required.
The CLI currently accepts that grant; it does not run browser authorization.
It supports public clients and client secrets sent in the token request body.
Provider-specific client authentication schemes require an adapter. Existing
access-token-only Keychain records still load; run `auth --auth-type oauth2`
once to add renewable credentials.

Keychain integration uses Rust's native `security-framework` API. The login
and password or OAuth credentials are one generic-password item, using service
`io.taxudromeio.mcp.imap` and account equal to the configuration ID.
macOS can prompt to permit access. A locked or unavailable Keychain produces an
error; there is no plaintext credential fallback.

## Connect an MCP client

The default command starts a stdio server:

```json
{
  "mcpServers": {
    "taxudromeio": {
      "command": "/Users/YOU/.cargo/bin/taxudromeio-mcp",
      "args": ["serve"]
    }
  }
}
```

Use the absolute path to your installed binary. To expose a subset:

```json
{
  "mcpServers": {
    "taxudromeio-project": {
      "command": "/Users/YOU/.cargo/bin/taxudromeio-mcp",
      "args": ["serve", "--configuration", "project"]
    }
  }
}
```

Repeat `--configuration` to expose several IDs. Unknown IDs fail startup.
Restart the server after editing configuration.

## Folder scope

Every mail tool takes a `configuration` ID. Folder names are relative to its
root; `.` means the root itself. The server reports the IMAP hierarchy delimiter.
For a root `Projects` and delimiter `/`, `Drafts` means `Projects/Drafts`.
For delimiter `.`, `Work.Drafts` means `Projects.Work.Drafts`.
Unicode names are converted to IMAP modified UTF-7 automatically.

The root and its descendants are exposed. A root of `INBOX` does **not** expose
a sibling folder such as `Sent` or `Drafts`; choose a common parent namespace
or a separate configuration when needed. Moves and copies require both source
and destination to be inside the selected configuration's root. Draft saves obey
the same rule and cannot automatically use an out-of-scope provider Drafts folder.

List/search results include UIDs and `uid_validity`. Supply both when reading,
moving, copying, or changing flags:

```json
{
  "configuration": "work",
  "folder": ".",
  "uid": 123,
  "uid_validity": 456
}
```

UIDs belong to a configuration and folder. Relist the destination after moving
or copying; its UIDs may differ. A stale UIDVALIDITY is refused.

## Tools and policies

| Tool | Triage | Read-only |
| --- | --- | --- |
| `mail_list_configurations` | yes | yes |
| `mail_list_folders` | yes | yes |
| `mail_folder_status` | yes | yes |
| `mail_list_messages` | yes | yes |
| `mail_search` | yes | yes |
| `mail_read_message` | yes | yes |
| `mail_move_messages` | yes | no |
| `mail_copy_messages` | yes | no |
| `mail_set_flags` | yes | no |
| `mail_save_draft` | yes | no |

`mail_set_flags` adds or removes `seen`, `flagged`, `answered`, or `draft`.
It cannot set `deleted`. Moves require the IMAP MOVE capability. There is no
copy/delete/expunge fallback, so unrelated mail marked Deleted is left alone.

Draft creation accepts `from`, `to`, `cc`, `bcc`, `subject`, `body`, optional
`in_reply_to`, and an optional relative `folder`. Without `folder`, it uses the
configuration's `drafts_folder`. Addresses are bare email addresses. Drafts are
appended with `\Draft` and `\Seen`; repeated calls create additional drafts.
No message is transmitted to its recipients.

The default config path is
`~/Library/Application Support/taxudromeio-mcp/config.toml`. The file is
owner-only and contains **no login name or credential**. Override it with
`--config /absolute/path/config.toml`. Configuration IDs must be unique across
config files on the same macOS user, because Keychain lookup uses the ID.

Example configuration:

```toml
version = 1

[[accounts]]
id = "work"
host = "imap.example.com"
port = 993
transport = "tls" # or "starttls", usually with port 143
root = "Projects"
drafts_folder = "Drafts"
auth_type = "password"

[accounts.policy]
profile = "triage"

[[accounts]]
id = "project"
host = "imap.example.com"
port = 993
transport = "tls"
root = "Projects/Taxudromeio"
auth_type = "password"

[accounts.policy]
profile = "read_only"
allowed_operations = ["list_folders", "list_messages", "read_message"]
```

`allowed_operations` intersects the profile. Omit it to allow all operations in
that profile; an empty list disables every account operation. Valid names are
`list_folders`, `folder_status`, `list_messages`, `search_messages`, `read_message`,
`move_messages`, `copy_messages`, `set_flags`, and `save_draft`.
The server advertises the union across exposed configurations and independently
enforces the selected configuration's policy on every call. Configuration
discovery remains available.

```sh
taxudromeio-mcp configurations
taxudromeio-mcp remove project
```

Removal deletes that ID's configuration and Keychain item. Reauthentication
preserves an existing policy and allowlist unless `--profile` changes the profile.

## Limits and verification

List/search pages contain at most 100 messages and return `next_before_uid`
for pagination. Reads accept messages up to 5 MiB, return up to 40,000 text
characters and attachment metadata, and use BODY.PEEK. Attachment downloads,
folder creation, full-text indexing, draft replacement, and sending are outside
this release. HTML is returned as untrusted text.

Four blocking workers limit concurrency, with 30-second socket connect and I/O
timeouts. OAuth HTTPS requests have a 30-second total timeout, verified TLS,
no redirects, and a 64 KiB response limit. There is no overall DNS/IMAP operation
deadline. A cancelled call may
already have changed mail; inspect the mailbox before retrying a mutation.
IMAP can ignore UIDs that disappeared concurrently, so mutation responses
report requested UIDs rather than verified affected counts.

Refresh serialization is per configuration within one server process.
Concurrent server/auth processes using the same ID, or configurations sharing
one rotating refresh token, need separate grants or external coordination.
When reauthentication reuses an existing grant, its renewed token is preserved
even if later mailbox verification or configuration saving fails.

The project uses Anchor Book, pinned in Cargo.toml and Cargo.lock, as a
development dependency. Its design, requirements, access guarantees, and
verification criteria live in `book/`. The installable server has no runtime
Anchor Book dependency.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --quiet --locked --example anchorbook -- check
mdbook build
```

Book rendering uses mdBook 0.5.4. Ordinary tests use fake credentials, in-memory
MCP transport, and deterministic IMAP and HTTPS fixtures, including actual
STARTTLS handshakes and certificate failures. Socket tests need loopback access.
The test certificate expires in November 2027; regenerate it using
`python3 scripts/regenerate_tls_fixtures.py` before then. Native Keychain and
real-provider interoperability need an interactive acceptance run using `auth`.
