`mail-profile.der` is a synthetic DER CMS SignedData profile signed with the
public localhost TLS fixture in `tls/`. It contains only fake test login and
password values, plus unrelated calendar and certificate payloads. It is used
to test data extraction, not profile trust or installation. No user-provided
profile is included in this repository.
