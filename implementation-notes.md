# Cloudflare integration work record

## Status

Work started on 2026-09-16 from `a82fa8f`.
Code changes and local tests are pending.
The Git branch is `feat/cloudflare-rfc2136` on the `hugojosefson` remote.
Each planning document has its own commits. Code commits do not include these documents.
The PR must stay in draft status.

## Source inspection


The updater removes the full TXT record set and accepts unsigned responses.
The DNS-01 flow starts CA validation immediately after the UPDATE response.
The job runner aborts the task when the attempt deadline expires.
The local bridge has protocol tests and a mock Cloudflare API.
Rust 1.98.1 and Docker are available. Test tool installation is in progress.

The `/tmp` user quota prevents new temporary files. Temporary work now uses
`/home/user/.cache/agents/acme-implementation`.

## Decisions

These decisions apply:

- Preserve each TXT value during cleanup.
- Check response TSIG, message ID, message type, and UPDATE opcode.
- Use a connected UDP socket.
- Accept truncation only with correct TSIG before TCP fallback.
- Use the same DNS deadline for UDP and TCP.
- Keep public DNS polling independent of the system resolver and UPDATE server.
- Disable the local propagation resolver cache. Public resolver caches stay.
- Use dummy credentials for local tests.

## Remaining work

Select and record finite deadlines and cancellation cleanup behavior.
Add packet tests, authentication tests, propagation tests, and bridge tests.
Do formatting, Clippy, coverage, documentation, and applicable integration checks.
Push code and document commits independently as work proceeds.

Staging tests await the test domain, Cloudflare zone, dev target, and runtime
credential method. No staging tests or deployment occurred.
