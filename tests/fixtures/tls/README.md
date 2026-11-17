These certificates and the PKCS#12 private key are public test fixtures for
loopback protocol tests only. The bundle password is `fixture`. The server
certificate is valid for `localhost` from November 2026 to November 2027;
its lifetime is within macOS's limit for TLS leaf certificates.
The CA private key was discarded. Tests add the CA to an individual client;
they never install it in the macOS trust store or disable certificate checks.
Regenerate the fixtures when their validity window no longer covers test runs.
