# Project guidance

This repository uses Anchor Book at the full revision pinned in Cargo.toml and
Cargo.lock. Read `cargo run --quiet --locked --example anchorbook -- contract`,
then `book/working-on-taxudromeio.md`, `anchorbook.toml`, and all chapters in
`book/SUMMARY.md`. Project phase is authoritative in anchorbook.toml.

Run `cargo run --quiet --locked --example anchorbook -- check` when starting
work and after Book edits. Tests validate records and roadmap references too.

For implementation changes run `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, and
`cargo test --locked --all-targets`. Book changes also require `mdbook build`.

Credentials and login names belong only in Keychain. Never add password CLI
arguments, environment overrides, credential logging, SMTP, unrestricted IMAP
commands, or folder-scope bypasses. Every operation must enforce its policy on
the selected configuration. Do not access real mail or Keychain during ordinary
tests. Tests use fake credential stores and deterministic protocol fixtures.
