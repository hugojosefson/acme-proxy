# Cloudflare integration work record

## Status

Work started on 2026-09-16 from `a82fa8f`.
Code commits are pushed. The local DNS-01 container test passed.
The Git branch is `feat/cloudflare-rfc2136` on the `hugojosefson` remote.

Each planning document has its own commits. Code commits do not include these documents.
The PR must stay in draft status.

## Source inspection


The initial updater removed the full TXT record set and accepted unsigned responses.
The initial DNS-01 flow started CA validation immediately after the UPDATE response.
The job runner aborts the task when the attempt deadline expires.
The local bridge has protocol tests and a mock Cloudflare API.
Rust `1.98.1`, Docker, and the test tools are available.

The `/tmp` user quota prevents new temporary files. Temporary work uses
`/home/user/.cache/agents/acme-implementation`.

## Decisions

These decisions apply:

- Preserve each TXT value during cleanup.
- Check response TSIG, message ID, message type, and UPDATE opcode.
- Use a connected UDP socket.
- Accept truncation only with correct TSIG before TCP fallback.
- Use the same DNS deadline for UDP and TCP.
- Keep public DNS polling isolated from the system resolver and UPDATE server.
- Disable the local propagation resolver cache. Public resolver caches stay.
- Use dummy credentials for local tests.

## Remaining work

The bridge repository needs its own dependency correction before deployment.

Staging tests await the test domain, Cloudflare zone, dev target, and runtime
credential method. No staging tests or deployment occurred.

## Progress

[PR 1](https://github.com/acme-proxy/acme-proxy/pull/1) is a draft.
The operator did not select staging resources.
Changes must satisfy these requirements only. Unrelated changes are outside scope.

The working code uses exact-value cleanup and checks response authentication.
Packet tests cover duplicate additions and removals with two TXT values.
Authentication tests cover three algorithms and incorrect response fields.
The relay test suite has 126 tests with no failures.
Clippy with all targets passed.

The updated default suite passed: 2266 tests, with 35 skipped container tests.
The HSM suite passed: 2285 tests, with 35 skipped container tests.
Line coverage is 97.44%, above the 97% minimum.
The latest cleanup tests also passed in the 126-test relay suite.

Coverage and Clippy passed after the cleanup tests.
The DNS-01 container test passed with Certbot and controlled BIND answers.
`cargo-nextest` and `cargo-llvm-cov` are installed.

## Deadline and cleanup decisions

These defaults apply only to DNS-01:

| Phase | Limit |
| --- | --- |
| UPDATE with UDP and TCP | 60 seconds |
| Public DNS propagation | 120 seconds |
| Each DNS query | 5 seconds |
| Query interval | 2000 milliseconds |
| Cleanup with a maximum of two attempts | 120 seconds |
| Complete relay attempt | 900 seconds |

CA validation uses the existing 300-second default. The phase limits total
600 seconds. The attempt limit includes all identifiers and certificate retrieval.
The order deadline can decrease the attempt. Other challenge strategies keep
their existing limits. Startup checks reject DNS-01 limits that are not compatible.

The public resolver defaults to `1.1.1.1:53`. It must return public answers.
Local resolver caching is disabled. Public resolver TTL caches stay active.
Missing answers, query errors, and query timeouts cause another query until the
propagation deadline. Only the same TXT value permits CA validation.

A worker independently retains the UPDATE on cancellation, then attempts cleanup.
A lock on the owner and value prevents a retry from preceding earlier cleanup.
The lock applies in this process, across configuration reloads.
Cleanup can continue after the outer deadline for at most 180 seconds with
default limits. Records can stay after process termination.

Removal by value permits retries.
Cleanup errors preserve an earlier failure. A `valid` authorization can retry
cleanup without another addition or validation request.

## Contribution instructions

The contribution instructions, PR template, parent AGENTS instructions, and
applicable CLAUDE instructions were inspected. The PR uses the fork and a feature
branch. Unit tests use nextest. Configuration changes must have example, book,
and changelog entries. `bd` is unavailable. This record contains the remaining work.

## More tests

The existing DNS-01 container test uses the private `lab.` zone. Its BIND fixture
has another query port, `5353`, for controlled propagation answers. The UPDATE
endpoint stays on port `53`.

A new local bridge test runner copies the bridge source into a temporary crate.
It uses the actual proxy updater and bridge with a mock API and dummy credentials.
The first test iteration identified two fixture errors: a trailing DNS root dot
in mock API records and the DNS status display spelling. Both fixtures changed.

All five bridge tests passed. They do not prove public certificate issuance.
The tests use bridge revision `6f8f609b34bd700e9988b7f56cf98ff75176015e`.
They cover concurrent values, duplicate operations, API refusal, rate limiting,
cleanup failures after some removals, missing responses after writes, and an 11-second API response.

The book build and documentation lint passed. The Rust documentation test passed.
Clippy with all targets and all features passed. The complete coverage suite
passed. Staging resources are undecided, as the operator specified.

## Code checkpoints

The code changes are in `b10218c`. The bridge tests are in `5123598`.
These commits are pushed. Planning documents have their own commits.
The cleanup retry test is in `d93229a`. Commit `4013c6f` applies the requested
English rules to new comments and configuration errors only.

An added test checks cleanup retries after CA validation succeeds. It confirms
one addition, one CA validation request, and three cleanup requests after two
cleanup failures. All 14 DNS-01 strategy tests passed with this test.

Before the dependency update, the coverage report included the complete suite
and the added cleanup worker tests. Line coverage was 97.43%. Rust API documentation and bridge Clippy
checks also passed.

Issuance remains in the existing durable job queue. The cleanup worker only
retains the current network operation after cancellation. It does not add a
second issuance scheduler or promise cleanup after a process crash.

## Dependency check

The first `cargo deny check` returned an error for `rustls 0.23.44`.
The reported advisory is `RUSTSEC-2026-0285`. Bans, licenses, and sources passed.
At that checkpoint, the dependency files had no changes from the base revision.
The subsequent dependency update has its own change, tests, and SBOM update.

The first checks omitted MSRV and HSM runtime tests because the tools were missing.
All-feature Clippy checked compilation of the HSM feature.

## Container test evidence

On 2026-09-16, `relay_signer::test_relay_signer_dns_01` passed with nextest.
The test took 1252 seconds, with seven image builds. It checked issuance
through the relay and DNS-01 validation by the upstream CA fixture.
The proxy image ID is
`sha256:b4998024d60ed52376e9611eb0ba30e1b368c8057e531fce8a1b3af9bfb6236c`.
The image contains the DNS behavior from `b10218c`. This local test does not
replace dev deployment or public staging tests with the Cloudflare bridge.

## Continued local checks

The operator requested local work without staging resources. The rustls advisory
specifies `0.23.45` as the corrected version. The manifest minimum and lockfile
use that version. Commit `78d9c11` contains only this dependency correction and its changelog entry.

Rust 1.97, SoftHSM, and the pinned SBOM generator are available. SoftHSM tests use only
the temporary dummy token store from the existing test harness.

The lockfile changes only rustls from `0.23.44` to `0.23.45`. The generated SBOM
changes only the related version references and checksum. The new `cargo deny`
check passed advisories, bans, licenses, and sources. The container
check passed with the corrected dependency.

`cargo +1.97 check --locked --all-targets --all-features` passed with Rust 1.97.1.
The HSM suite uses `ACME_PROXY_REQUIRE_SOFTHSM=1`, so a missing module causes failure.

The updated default suite passed all 2266 tests, with 35 container tests skipped.
Line coverage is 97.44%, above the 97% minimum. All five bridge tests passed against bridge
revision `1821c2cec7798659d30de1c45fdbb1c9ff09738e` with dummy credentials.

The eight SoftHSM tests passed. The complete HSM suite passed all 2285 tests.
Clippy, the Rust documentation test, and API documentation checks passed.

The bridge tests use a temporary lockfile that resolves rustls `0.23.45`.
The source bridge lockfile at `1821c2c` contains rustls `0.23.40`, which is
also in the advisory range. That repository needs its own dependency correction
before deployment. The proxy change does not modify the bridge checkout.

## Container check after the dependency update

The DNS-01 container test passed with rustls `0.23.45` on 2026-09-16.
The test took 162 seconds with image builds. It used the source from `78d9c11`.
The proxy image ID is
`sha256:621784cda18131d5ae56d5316084872abe103bed3cce79eb57998418b1145546`.
Public staging and dev deployment did not occur. The PR stays in draft status.
