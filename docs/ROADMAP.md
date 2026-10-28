# Roadmap

**Current stage: locally validated developer preview.** Updated 2026-10-28.

Statuses describe evidence, not elapsed effort: **Done locally** means repeatable fixture validation; **Next** is an acceptance gate; **Planned** is not implemented or promised on a date. We do not assign a percentage to the product roadmap.

```mermaid
flowchart LR
    A["NOW · Developer preview\nRust core + source demo"] --> B["NEXT · Public repository\nHosted CI + reporting"]
    B --> C["THEN · Stable v0.1\nRecovery + real-device acceptance"]
    C --> D["LATER · Ecosystem\nClients + provider contracts"]
    classDef current fill:#153b38,stroke:#65d9bd,color:#e9fff9
    class A current
```

## Short term — make the preview inspectable

| State | Deliverable | Acceptance evidence |
| --- | --- | --- |
| Done locally | MIT source export and private-history separation | Independent repository, allowlist and per-file SHA-256 manifest |
| Done locally | Core ownership, checkpoints and branches | Rust tests and offline process/integration fixtures |
| Done locally | Review and concurrency repairs | Independent code/security re-review; regression fixtures |
| Done locally | Demonstration and contributor docs | Runnable demo, Mermaid diagrams, usage and AI handoff guide |
| Next | Publish the intended GitHub repository | Owner uploads clean history; hosted checks pass |
| Next | Private vulnerability reporting | Repository Security tab verified |
| Done locally | Measured quality baseline | Rust-test and combined-fixture line coverage; explicit scope and reproducible commands |

## Medium term — earn stable v0.1

These are release gates, not claims that the current preview already meets them.

- [ ] **Recover uncertain operations:** inspect/reconcile interrupted reservations and processes without blindly replaying side effects; test every recovery decision.
- [ ] **Physical handoff:** laptop A disconnects/closes, host work continues, laptop B takes control; stale A input is rejected.
- [ ] **Host reboot:** report accepted/running/interrupted work accurately; no accidental duplicate task execution.
- [ ] **Clean-machine lifecycle:** install, upgrade, migration/backup and uninstall on a fresh account; verify ownership and permission boundaries.
- [ ] **Provider compatibility:** publish a versioned Codex/Claude adapter matrix with fresh authenticated disposable-session continuation and branch tests.
- [ ] **Failure-focused coverage:** extend tests for low-covered CLI/node paths, error handling, input uncertainty and recovery; track line coverage without treating it as proof of correctness.

Schema v4 requires a stopped-process backup before upgrade; in-place downgrade is unsupported. Physical-device tests and provider inference must be recorded separately from offline fixtures.

## Long term — a reusable continuity layer

| Direction | First useful outcome |
| --- | --- |
| Stable client protocol | Capability/version negotiation and documented errors for independent clients |
| Phone and companion clients | Explicit remote approvals and ownership, with visible execution status |
| Session history interoperability | Evaluate checkpoint/session formats before adding import/export adapters |
| Remote object storage | A clear privacy and sharing policy with verified checkpoint integrity |
| Wider platform support | Platform-specific process, PTY and filesystem acceptance suites |

[Related-project research](RELATED-PROJECTS.md) informs these priorities. Happy, HAPI, Entire and Conductor already cover substantial adjacent workflows; a smaller core must demonstrate integration value and recovery reliability.

Multi-tenant isolation and a cloud scheduler would need a separate security design. They are not implied by workspace labels or this roadmap. No date or production-readiness guarantee is implied by the package version.
