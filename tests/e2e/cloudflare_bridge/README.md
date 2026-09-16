# Cloudflare bridge integration tests

These tests use the actual proxy updater, bridge code, and a mock Cloudflare API.
They use dummy credentials and loopback sockets. They do not contact public DNS
or issue certificates.

Use the bridge revision that implements ACME TXT updates:

```sh
python3 tests/e2e/cloudflare_bridge/run.py ../cloudflare-rfc2136
```

The runner copies bridge source into a temporary directory and adds proxy tests.
It keeps the source checkout unchanged. `TMPDIR` selects the temporary parent.
The default is `/tmp/agents`. Cargo and cargo-nextest must be available.
The temporary test crate depends on the current proxy checkout.

The tests check concurrent TXT values, duplicate operations,
cleanup retries, and API errors. They also check a missing response after a write.
A staging deployment is necessary for public DNS and certificate issuance tests.
