# Bridge TLS fixtures

Throwaway PEMs for `tests/bridge_tls.rs`: `ca.pem` signs `server.pem` (SAN
`localhost`, `concierge`, `127.0.0.1`), `server-concierge-only.pem` (SAN `concierge`
alone — the name-pinning probe) and `client.pem` (the mTLS identity); `other-ca.pem`
signs nothing here. Nothing outside the test suite trusts any of them, and the CA keys
were not kept. Regenerate with `bash regen.sh` (needs `openssl`); valid to 2036.
