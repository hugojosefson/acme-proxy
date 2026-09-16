# Cloudflare integration work record

## Status

Work started on 2026-09-16 from `a82fa8f`.
Code changes and local tests are in progress.
The Git branch is `feat/cloudflare-rfc2136` on the `hugojosefson` remote.
Each planning document has its own commits. Code commits do not include these documents.
The PR must stay in draft status.

## Source inspection


The updater removes the full TXT record set and accepts unsigned responses.
The DNS-01 flow starts CA validation immediately after the UPDATE response.
The job runner aborts the task when the attempt deadline expires.
The local bridge has protocol tests and a mock Cloudflare API.
Rust `1.98.1` and Docker are available. Test tool installation is in progress.

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

Select and record finite deadlines and cancellation cleanup behavior.
Add packet tests, authentication tests, propagation tests, and bridge tests.
Do formatting, Clippy, coverage, documentation, and applicable integration checks.
Push code and document commits independently as work proceeds.

Staging tests await the test domain, Cloudflare zone, dev target, and runtime
credential method. No staging tests or deployment occurred.

## Progress

[PR 1](https://github.com/acme-proxy/acme-proxy/pull/1) is a draft.
The operator did not select staging resources.
Changes must satisfy these requirements only. Unrelated changes are outside scope.

The working code uses exact-value cleanup and checks response authentication.
Packet tests cover duplicate additions and removals with two TXT values.
Authentication tests cover three algorithms and incorrect response fields.
The relay test suite has 123 tests with no failures.
Clippy with all targets passed.

More tests and aggregate checks are pending.
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
and changelog entries. `bd` is unavailable. this record contains the remaining work.
