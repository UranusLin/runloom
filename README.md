<p align="center">
  <img src="docs/assets/hero.svg" alt="Runloom — Your agent's work, beyond one laptop." width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-73dbc4?style=flat-square" alt="MIT license"></a>
  <a href="docs/TESTING.md"><img src="docs/assets/tests.svg" alt="Measured Rust test results"></a>
  <a href="docs/TESTING.md"><img src="docs/assets/coverage.svg" alt="Measured Rust line coverage from local tests and fixtures"></a>
  <img src="https://img.shields.io/badge/status-developer_preview-e9bd76?style=flat-square" alt="Developer preview">
  <img src="https://img.shields.io/badge/core-Rust-c8a8ef?style=flat-square" alt="Rust core">
  <img src="https://img.shields.io/badge/platform-macOS_first-9fb7d7?style=flat-square" alt="macOS first">
</p>

<p align="center">
  <strong>Keep AI work on your host. Pick it up from another device.</strong><br>
  Resume a session, transfer control, or explore a new branch from a checkpoint.
</p>

<p align="center">
  <a href="#see-it-work">Demo</a> · <a href="#try-it-locally">Quick start</a> · <a href="#how-it-fits-together">Architecture</a> · <a href="docs/USAGE.md">Usage</a> · <a href="docs/ROADMAP.md">Roadmap</a>
</p>

## Why Runloom?

Your agent is working on a project. You move to another laptop. The process, conversation and files should still have a clear home—and only the current controller should be able to send commands.

Runloom connects **session identity, execution ownership and code checkpoints** on an execution host. It uses installed official Codex or Claude Code tools, with a Rust core and a private node protocol.

| You want to… | Runloom provides |
| --- | --- |
| Return to an existing conversation | Explicit provider, profile and native session identity |
| Switch controllers | Server-checked leases; stale controllers lose write access |
| Catch up after reconnecting | Recorded process output and request receipts |
| Save a point to return to | Git/file checkpoints and selected native history |
| Explore another direction | Independent worktree and supported native conversation branch |
| Hand an agent a specification | Explicitly started Claude CLI tasks with acceptance criteria |

## See it work

![Animated rendering of actual local demo output: ownership transfer, stale input rejection and checkpoint restore](docs/assets/demo.gif)

*Actual output from `scripts/demo-local.py`, rendered as a terminal animation with reading pauses. Two controllers are simulated locally; this is not a desktop UI recording or physical-device acceptance test. [Plain-text transcript](docs/assets/demo-output.txt) · [Static image](docs/assets/demo.png)*

The demo starts one real fixture process, changes its controller, rejects the old controller's input, and restores a saved working file into a new directory. No AI account, SSH or sudo is needed.

## Try it locally

From the repository root, on macOS with **Rust/Cargo 1.91+**, **Git**, **Python 3.9+** and a **C toolchain**:

```sh
cargo build --workspace --locked
python3 scripts/demo-local.py
```

The demo uses disposable files and does not touch existing sessions. To run a foreground node and connect over SSH, follow the [step-by-step guide](docs/USAGE.md).

## How it fits together

```mermaid
flowchart LR
    A["Laptop A · client"] -->|SSH| N
    B["Laptop B · client"] -->|SSH| N
    subgraph Host["Your execution host"]
        N["Runloom Node"] --> L["Identity · leases · receipts"]
        L --> DB[(SQLite)]
        N --> S["Process supervisor"]
        S --> CLI["Official Codex / Claude Code CLI"]
        N --> C["Checkpoint store"]
        C --> F["New directory / worktree"]
    end
    classDef accent fill:#153b38,stroke:#65d9bd,color:#e9fff9
    class N,C accent
```

The host owns processes and files. Clients connect through a private Unix socket or SSH bridge. Provider credentials stay on the execution host. Desktop UI, character animation and voice can be built as clients; they are not included in this repository.

### One execution, a new controller

```mermaid
sequenceDiagram
    participant A as Laptop A
    participant N as Runloom Node
    participant P as Running process
    participant B as Laptop B
    A->>N: Acquire lease and start work
    N->>P: Launch once
    P-->>N: Persist output events
    B->>N: Acquire with observed takeover epoch
    N-->>B: New ownership proof
    A->>N: Input using old proof
    N-->>A: Reject stale controller
    B->>N: Input using new proof
    N->>P: Continue the same execution
    B->>N: Replay recorded output
```

A lost connection does not mean a mutation was cancelled. Receipts and explicit ownership checks help clients handle uncertain delivery; Runloom does not promise exactly-once external side effects.

## What this preview includes

**Available:** Read-only recovery inspection, Rust core, CLI, node protocol, supervisors, leases, PTY, provider continuation adapters, checkpoint restore, conversation/worktree branching and task dispatch.

**Validated locally:** 29 Rust tests and ten offline demo/integration scripts passed for the initial preview. These cover fixture execution, stale ownership, output replay, checkpoint contention and branching. See [CONTRIBUTING](CONTRIBUTING.md) to reproduce the checks. Hosted CI has not yet run.

**Still to validate:** clean-machine installation, physical-device handoff, reboot recovery and fresh authenticated provider compatibility. Other operating systems are not certified. Package version `0.1.0-alpha.1` is a development version, not stable acceptance.

<details>
<summary><strong>Operational boundaries worth knowing</strong></summary>

- The trust boundary is the OS account and SSH access. Workspace labels do not isolate hostile tenants.
- A node is not a sandbox. Official tools retain their authentication and approval behavior.
- Unknown process state and interrupted directory reservations require inspection; no automatic replay or reservation expiry is provided.
- Schema v4 needs a stopped-process backup before upgrade; in-place downgrade is unsupported.
- Checkpoints may contain secrets in source or conversation text. Filename exclusions are not general secret removal.
- Conversation branches use supported native formats. They do not reconstruct hidden model reasoning or merge conversations across providers.

Read the [security model](SECURITY.md) and [architecture](docs/ARCHITECTURE.md) before using real work.

</details>

## Roadmap at a glance

| Now · locally verified | Next · public preview | Then · stable v0.1 | Later · ecosystem |
| --- | --- | --- | --- |
| Rust core, ownership, checkpoints, branching, fixture demo, coverage baseline | Publish clean repo, hosted CI, private reporting | Explicit recovery, real-device handoff, reboot and clean-machine acceptance | Provider contracts, phone clients, history interoperability |

See the [full roadmap](docs/ROADMAP.md) for checklists and acceptance criteria. Coverage badges are **dated local measurements**, not live hosted CI status; [method and scope](docs/TESTING.md).

## Related projects

This is an active space. [Happy](https://github.com/slopus/happy) and [HAPI](https://github.com/tiann/hapi) address remote agent access, [Entire](https://github.com/entireio/cli) connects agent history with Git, and [Conductor](https://www.conductor.build/docs/concepts/parallel-agents) organizes parallel workspaces. Official [Claude Remote Control](https://code.claude.com/docs/en/remote-control) also provides cross-device access.

Runloom focuses on a small, self-hosted Rust session/execution-state core with a client-independent protocol. That is a design direction, not a claim that other projects lack equivalent capabilities. Read the [dated comparison and sources](docs/RELATED-PROJECTS.md).

## Go deeper

| Document | Start here for… |
| --- | --- |
| [Usage](docs/USAGE.md) | Foreground startup, SSH, requests, resume and restore |
| [Architecture](docs/ARCHITECTURE.md) | Identity, persistence, fencing and extension seams |
| [Roadmap](docs/ROADMAP.md) | Preview, stable release and longer-term product goals |
| [Recovery](docs/RECOVERY.md) | Inspect interrupted work without unlocking or replaying it |
| [Testing and coverage](docs/TESTING.md) | Measured coverage, scope and reproduction |
| [Related projects](docs/RELATED-PROJECTS.md) | Adjacent tools and honest product positioning |
| [Contributing](CONTRIBUTING.md) | Build commands and regression checks |
| [AGENTS.md](AGENTS.md) | Context and invariants for your coding agent |
| [Security](SECURITY.md) | Trust boundaries and private reporting setup |

## License

Runloom is [MIT licensed](LICENSE). Dependencies retain their own licenses; see [third-party notices](THIRD_PARTY_NOTICES.md). The source package excludes private deployment history, credentials, desktop clients and character/voice assets.

### Recovery guidance

Runloom can now turn scoped recovery evidence into an ordered, read-only investigation plan. It distinguishes incomplete evidence from recorded blockers and explicitly refuses to treat an empty snapshot as permission to restart work. [Recovery guide](docs/RECOVERY.md) includes the JSON endpoint, command-line reader and disposable failure demo. Automatic repair remains outside this developer preview.

## Versioned preview builds

The first planned release is **v0.1.0-alpha.1**, targeting macOS Apple Silicon.
See [release and installation guide](docs/RELEASE.md) for side-by-side installation,
build identities, checksums, compatibility and draft-release gates. Source availability
does not imply that a binary release or hosted CI has already been published.
