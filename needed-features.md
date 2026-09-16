# Cloudflare RFC 2136 integration

## Scope and status

This document contains the proxy requirements from the
[bridge specification](https://github.com/hugojosefson/cloudflare-rfc2136/blob/feat/acme-dns01/needed-features.md).
The [bridge work record](https://github.com/hugojosefson/cloudflare-rfc2136/blob/feat/acme-dns01/implementation-notes.md)
contains bridge decisions and test results.

The proxy needs code changes and deployment configuration. Configuration alone
cannot correct the cleanup request, DNS propagation wait, or response checks.
Implementation started on 2026-09-16. The
[work record](implementation-notes.md) contains decisions, status, and test results.

Source inspection date: 2026-09-16. Proxy revision:
[`f005ffa4a32b1868976d9c48b504f7b00e8786ec`](https://github.com/acme-proxy/acme-proxy/commit/f005ffa4a32b1868976d9c48b504f7b00e8786ec).
No proxy tests or certificate issuance tests ran during this preparation.
All example domains use reserved names. No deployment credentials were accessed.

## Git setup

The repository has these Git settings:

- Upstream remote `origin`: `git@github.com:acme-proxy/acme-proxy.git`.
- Fork remote `hugojosefson`: `git@github.com:hugojosefson/acme-proxy.git`.
- Git branch: `feat/cloudflare-rfc2136`.
- Git tracking branch: `hugojosefson/feat/cloudflare-rfc2136`.

The local directory is `/home/user/code/current/other/acme-proxy`. Keep
temporary planning documents in their own commits. Keep this document current
with decisions, test results, and remaining work. Commit and push changes to
`hugojosefson` as work progresses.

## Initial source findings

The source at the inspection revision showed these behaviors:

| Source | Finding |
| --- | --- |
| [RFC 2136 updater](src/signer/relay/dns01.rs) | `upsert_txt` uses `append`. `delete_txt` uses `delete_rrset`, which removes the full record set. |
| [RFC 2136 updater](src/signer/relay/dns01.rs) | `UPDATE_TIMEOUT` is ten seconds. `Rfc2136Config` has no exchange timeout setting. |
| [RFC 2136 updater](src/signer/relay/dns01.rs) | `send` checks the response ID and status without TSIG verification. UDP reception does not check the sender. |
| [DNS-01 flow](src/signer/relay/flow.rs) | `answer_dns01` calls `trigger_and_await` immediately after `upsert_txt`. |
| [DNS-01 tests](src/signer/relay/tests/dns01_strategy.rs) | The flow tests use a `DnsUpdater` stub. They do not prove the cleanup packet contains the value. |
| [Signer configuration](src/config/types/signer.rs) | `poll_timeout_secs` defaults to 300 seconds. New limits must agree with the relay and job deadlines. |

These are source findings, not executed integration results.

## Required record operations

Two orders can use the same `_acme-challenge` owner name. An order for a domain
and an order for its wildcard can also use that name. Each order must preserve
the other order's TXT value.

The updater must support this sequence:

1. Add `value-a` at `_acme-challenge.example.com.`.
2. Add `value-b` at the same name without removing `value-a`.
3. Remove `value-a` without changing `value-b`.
4. Do each operation again without unintended changes.

Keep `append` behavior for additions. Change `delete_txt` to use
`update_message::delete_by_rdata` with a TXT `RecordSet`. The cleanup packet
must contain the supplied value without changes, class `NONE`, and TTL zero.
Preserve TXT case and bytes.

Do not send class `ANY` cleanup for a TXT record set. The bridge rejects that
request because it does not identify the value to remove. Do not add a bridge
exception for the current proxy behavior. Do not infer ownership from the last
value seen, process memory, or an empty removal request.

Test the actual bytes from `Rfc2136Updater`, with the cleanup operation.
A test of `DnsUpdater` arguments alone does not prove correct packet encoding.
Check class, type, TTL, owner, TXT RDATA, zone, and request TSIG.
See [RFC 2136](https://www.rfc-editor.org/rfc/rfc2136.html), sections 2.5 and 3.4.

## Public DNS propagation

After an update succeeds, poll public DNS for the expected TXT value without
changes. Start CA validation only after that value appears. Keep the wait in
the proxy. The bridge reports Cloudflare API success without a DNS propagation
wait.

Give polling a finite deadline and a bounded interval. Specify the resolver
selection and cache behavior. The resolver must use public DNS answers, not
private split DNS answers or the UPDATE endpoint. Preserve other TXT values in
answers. Specify behavior for missing answers, delayed answers, resolver
errors, and expired deadlines. A polling failure must not start CA validation.

Keep cleanup after validation succeeds or fails. Specify cleanup behavior after
a propagation failure, cancellation, and an expired outer deadline. Cleanup
must only remove the value for that attempt. A cleanup failure must preserve
the primary failure and support safe retries.

Use controlled DNS answers and a CA stub to test the operation sequence. Prove
that the proxy does not start validation before the expected answer. Keep a
different staging test for public DNS propagation.

## Timeout budgets and retries

The initial proxy permitted ten seconds for an UPDATE exchange. The bridge
permits fifteen seconds for each Cloudflare API request. One UPDATE can use
multiple API requests, pagination, and a lock wait.

Specify compatible finite budgets for UPDATE, propagation, validation, and
cleanup. Include the outer relay attempt and job deadlines. Set limits that
cover multiple API requests.

Selected defaults are 60 seconds for UPDATE and
120 seconds for propagation. Other defaults are
5 seconds for each query, and 2000 milliseconds between queries. Cleanup has
120 seconds for a maximum of two attempts. DNS-01 relay attempts have 900 seconds.
CA validation keeps `poll_timeout_secs`, with its 300-second default.

The new settings are `signer.relay.dns01.propagation_resolver`,
`propagation_timeout_secs`, `propagation_interval_ms`, `query_timeout_secs`,
`cleanup_timeout_secs`, and `attempt_timeout_secs`. The UPDATE limit is
`signer.relay.dns01.rfc2136.timeout_secs`. The resolver defaults to
`1.1.1.1:53` with local caching disabled. The
[work record](implementation-notes.md) records cancellation behavior and limits.

Test slow API responses, a missing response after a write succeeds, timeout,
rate limiting, API refusal, and failure after some operations succeed. An
uncertain result must not cause broad cleanup. Additions and exact-value
removals must support safe retries. Cleanup of a missing value must succeed.

## Response authentication and UDP peer checks

Check response TSIG before accepting the response status. Use the signed
request, the selected key and algorithm, and the permitted time window for
verification. A signed bridge response alone does not prove verification by the
proxy.

Reject missing, invalid, changed, stale, or mismatched response signatures.
Keep response ID checks. Check the DNS message type and UPDATE opcode. Do not
accept an unauthenticated success or refusal as the result.

Accept UDP responses only from the configured server address and port. Use a
connected UDP socket or an explicit sender check. Preserve bounded UDP-to-TCP
fallback. Check response signatures on both transports. Specify and test the
truncated-response policy before implementation.

Use dummy TSIG secrets in tests. Test correct replies, invalid signatures,
incorrect IDs, incorrect peers, malformed replies, and TCP fallback. Cover
`hmac-sha256`, `hmac-sha384`, and `hmac-sha512`. Keep secrets and authorization
data out of logs and error responses.

## Bridge contract and deployment configuration

The bridge requires these settings and limits:

- `ENABLE_ACME_TXT=true` explicitly permits ACME TXT updates.
- The UPDATE zone equals `DNS_ZONE`.
- The owner is inside `DNS_ZONE` and `ALLOWED_RECORD_SUFFIX` at DNS label boundaries.
- The first owner label equals `_acme-challenge`, ignoring DNS name case.
- TXT data contains one ASCII string with a maximum of 255 bytes.
- Class `IN` adds a value. Class `NONE` with TTL zero removes the specified value.
- The bridge rejects full-set removal, name-wide removal, and prerequisites.
- `DEFAULT_TTL` controls Cloudflare TTL. The bridge does not copy the UPDATE TTL.
- One bridge process writes each Cloudflare zone. Local locks do not control other processes.

Configure the proxy's `relay` backend, `dns01` challenge strategy, and
`rfc2136` provider through the existing configuration. Set the server address,
zone, TSIG key name, and algorithm to agree with the bridge.

Supply the TSIG secret at runtime.
See [the configuration example](config.toml.example) and
[the configuration types](src/config/types/signer.rs).

The bridge is an UPDATE adapter, not a public authoritative DNS server.
`NOERROR` means that required API calls succeeded. Cloudflare calls do not make
a multi-operation UPDATE atomic. Earlier changes can stay after `SERVFAIL`.

## Acceptance tests

The implementation needs these results:

| Test | Required result |
| --- | --- |
| Packet encoding | Actual proxy packets add one value and remove only that value with class `NONE`. |
| Concurrent orders | Two values at one owner survive overlapping additions and cleanup for each order. |
| Retry | Duplicate additions and cleanup retries preserve unrelated values. |
| Propagation | Delayed public TXT answers delay CA validation. Missing answers expire before the deadline. |
| Authentication | Correct TSIG succeeds. Invalid TSIG, incorrect IDs, and incorrect peers cannot cause accepted success. |
| UDP and TCP | Signed UDP and TCP replies work. Truncation fallback preserves verification and deadlines. |
| Failure | API refusal, timeout, rate limiting, and failure after some operations succeed cause no incorrect success or secret disclosure. |
| Cleanup | Validation success and failure remove only the attempt's value. Propagation failure and cancellation have explicit behavior. |
| Integration | The actual proxy and bridge preserve two values at the same time through issuance and cleanup. |

Start with local mock tests and dummy credentials. The bridge record reports 28
mock and protocol tests with no failures. Those results do not prove deployed
proxy compatibility or public certificate issuance.

Use the checks in [CI](.github/workflows/ci.yml) for code changes. The project
uses `cargo-nextest`, with coverage, rather than only `cargo test`.
Preserve applicable formatting, Clippy, documentation, optional-feature, and integration
checks. No build is necessary for this planning document.

## Staging prerequisites and evidence

A staging trial needs these resources:

- A public test domain controlled by the operator.
- Cloudflare DNS access for only the test zone.
- A runtime TSIG key shared by the proxy and bridge.
- A dev deployment target for both services.
- Access to public DNS and the staging CA.

The operator did not select a domain or deployment target during preparation.
Retrieve credentials at runtime through the operator's approved system. Do not
put credentials in command arguments, logs, this document, or environment files
on disk. Use dummy files for tests of secret-related tools. Do not read actual
password-store contents or private keys during preparation.

Use [Let's Encrypt staging](https://letsencrypt.org/docs/staging-environment/)
before production issuance. Check public TXT propagation, certificate issuance,
cleanup, concurrent orders, and retries. Record both repository revisions,
deployed image versions, test names, dates, and observed results.

Record source inspection, local test results, and staging results in different
sections. Keep PRs for services as drafts until the PR image runs in dev and
the related behavior checks succeed. Record the deployed version and evidence
before review. Do not merge before this dev check.

## Preparation checks

`python3 doc/lint.py` reports 61 pages with no problems. All local links in
this document resolve. The Issue 9 prose check and manual inspection covered
this document. Technical names and source paths keep their required wording.
No application code, tests, dependencies, or deployment settings changed.

## Next steps

The implementation conversation must follow this sequence:

1. Check the current source and repository instructions.
2. Select resolver behavior, finite budgets, and cleanup behavior on cancellation.
3. Add exact-value cleanup and tests of actual packets.
4. Add response verification and UDP peer checks with UDP and TCP tests.
5. Add bounded public DNS polling and compatible timeout settings.
6. Do local integration tests against the bridge with dummy credentials.
7. Select the test domain and dev target.
8. Deploy both revisions and do the staging trial.
9. Record decisions and evidence as work proceeds.

`bd` is unavailable in the preparation environment. This document records the
remaining work. The implementation PR must stay in draft status.

## Implementation status

[PR 1](https://github.com/acme-proxy/acme-proxy/pull/1) is a draft.
Exact-value cleanup, response checks, and public DNS polling have working code.

The full suite passed: 2262 tests. Line coverage is 97.43%.
The relay suite passed all 126 tests after more cleanup tests.
All five bridge integration tests passed with dummy credentials.

Coverage and Clippy passed. The DNS-01 container test is building images.
All 14 DNS-01 strategy tests passed after an added cleanup retry test.

`cargo deny check` returned an error for existing `rustls 0.23.44` (`RUSTSEC-2026-0285`).
The dependency files did not change.

The operator deferred selection of staging resources.
The [work record](implementation-notes.md) tracks discoveries and decisions.
