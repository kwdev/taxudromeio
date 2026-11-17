# Design

The server connects local MCP clients to explicitly configured IMAP roots.

```mermaid
flowchart LR
    Client["MCP client"] --> Server["Rust stdio server"]
    Config["Nonsecret TOML configurations"] --> Server
    Server --> Policy["Per-configuration policy"]
    Policy --> Scope["Root and UIDVALIDITY checks"]
    Scope --> IMAP["IMAP over verified TLS"]
    Keychain["macOS Keychain: login and credential"] --> IMAP
```

[Access and operations](access.md) owns the `policy`, `folder`, and `message`
capabilities. [Authentication and configurations](authentication.md) owns `auth`
and `config`, and the authentication chapter also owns `oauth`. Mail content is untrusted data. Tool instructions never grant
additional access. The transport is stdio; operational output goes to stderr.
