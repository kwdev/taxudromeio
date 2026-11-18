# Working on Taxudromeio

Read the pinned engine contract with
`cargo run --quiet --locked --example anchorbook -- contract`. Read this Book
through SUMMARY.md. The engine revision is pinned in Cargo.toml and Cargo.lock;
its anchor definitions and roadmap syntax are the authoring references.
The phase is defined only in anchorbook.toml.

Sources are in src/, integration tests in tests/, and the Book in book/.
Generated output is under target/ and must not be committed.
Use Rust stable with edition 2024. This prototype is tested on macOS with Rust
1.98.1. The upstream imap-proto 0.10.2 emits a future-incompatibility warning
about expression macros; the current build passes, and compiler upgrades must
reassess that dependency.

Commands:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --quiet --locked --example anchorbook -- check
mdbook build
cargo install --path . --locked
```

`mdbook build` uses mdBook 0.5.4 and the pinned library adapter. Anchor Book
structure, record, and roadmap validation are also tested directly in Rust.
The engine is a development dependency; the installed mail tool does not
depend on it at runtime.

Ordinary tests never open Keychain or connect to a real mailbox. Fake stores
exercise persistence failures; protocol fixtures exercise actual IMAP commands;
MCP tests exercise initialization, tool discovery, and dispatch. Parsed CLI and
MCP adapter tests compare every supported operation through one fake backend,
and subprocess tests check help examples, configuration output and policy denial.
Authentication
tests use localhost TCP/TLS listeners and a private client trust root; they
need loopback socket permission. The TLS leaf fixture expires in November 2027;
regenerate it with `python3 scripts/regenerate_tls_fixtures.py` using OpenSSL 3.
Manual account
verification uses `taxudromeio auth` and performs no mailbox mutations.
Do not claim real-provider or Keychain acceptance without recording that run.
Profile tests use synthetic XML/binary/CMS data with fake credentials. A
user-authorized inspection may parse a supplied profile without authentication;
report only nonsecret connection settings and credential-presence booleans.
Never copy personal profiles or their decoded contents into the repository.

This scoped prototype uses tests and code review rather than formal proofs.
No proof criteria are declared. Mutation tooling and CI are not configured;
adopting a Building workflow requires agreeing on them first. Local checks do
not establish provider interoperability, crash-atomic cross-store writes, or
MCP cancellation of an operation already accepted by a mail server.

Do not commit, publish, or access actual mailbox data or credentials as part of
development checks. The reference repository was read for behavior and patterns;
its private local contents are not part of this project.
