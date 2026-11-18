# Introduction

Taxudromeio is a local Rust CLI and MCP server for IMAP mail triage. A configuration
exposes one root folder, identifies its own Keychain credential, and selects
which operations a CLI or MCP client may invoke. Several configurations may use the
same account and server with different roots and policies.

The Book describes the intended system. The
[roadmap](implementation-roadmaps/initial-server.md) distinguishes implementation
and executed evidence from goals. User setup is documented in the repository README.
