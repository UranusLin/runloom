# Related projects and Runloom's focus

Research snapshot: **2026-10-28**. This is a primary-source documentation review, not a hands-on benchmark or security audit. Features and product boundaries change; a capability not mentioned here is **not** evidence that a project lacks it.

There are already substantial projects in this space. Remote control, session continuation and Git-based history are established directions; Runloom should not present those ideas as inventions or claim to be the first combined solution.

## Where the overlap is

| Project | Documented capabilities | Why it matters to Runloom |
| --- | --- | --- |
| **Happy** | An open-source multi-provider app with desktop orchestration, mobile access and shared sessions. Its repository separates the mobile/legacy CLI/relay components from the desktop app and runtime. [Repository](https://github.com/slopus/happy), [current product](https://happy.engineering/) | Closest product-level overlap with controlling agents on your own hardware from multiple devices. A phone UI alone would not differentiate Runloom. |
| **HAPI** | Runs official coding agents with local/remote access. The support matrix includes Claude Code and Codex resume; Codex uses native terminal integration and `codex app-server`. [Repository](https://github.com/tiann/hapi), [agent matrix](https://hapi.run/docs/guide/agents) | Strong overlap with retaining the native agent workflow and continuing from another device. Its documented integration choices are useful adapter research. |
| **Entire CLI** | Captures prompts, transcripts and touched files as checkpoints linked to Git commits; supports restoring a branch's sessions through `entire session resume`. The wider product includes Git hosting, review Trails and session search. [README and session commands](https://github.com/entireio/cli#agent-sessions) | Strong overlap with versioned agent context and restoration. It is more than a transcript logger; compare actual checkpoint semantics before designing interoperability. |
| **Conductor** | Separate workspaces provide independent branches and working trees; agents can also share one workspace. Conductor Cloud supplies isolated cloud workspaces and shared conversations that continue after a client laptop closes. [Parallel agents](https://www.conductor.build/docs/concepts/parallel-agents), [Cloud](https://www.conductor.build/docs/cloud) | Overlap with worktree orchestration and persistent work environments. This is Conductor at conductor.build, not the unrelated Conductor workflow-engine projects. |
| **Claude Code Remote Control** | Official browser/mobile access to a local Claude Code session. Execution and filesystem access remain on the host, which must stay awake with the process running. [Official documentation](https://code.claude.com/docs/en/remote-control) | A direct alternative for the narrower requirement of controlling Claude Code remotely. Runloom's multi-provider scope needs to earn its extra operational complexity. |

## Details that prevent misleading comparisons

**Happy has evolved.** The original CLI is now the `happy` npm package, formerly `happy-coder`, and its documentation marks it as maintenance mode. New development is described around the desktop app and Happy Harness. The current mobile app works with both. Comparing Runloom only against an old Happy Coder screenshot would miss the present product. [Legacy CLI documentation](https://happy.engineering/docs/), [repository component map](https://github.com/slopus/happy#how-happy-fits-together)

**Handoff semantics differ.** HAPI's Codex documentation describes simultaneous terminal/remote access with no local/remote ownership switch, and says cold resume rejects another live owner. Runloom currently uses explicit lease takeover that fences the previous controller. These are different interaction models, not evidence that one is generally safer or better. [HAPI Codex shared sessions](https://hapi.run/docs/guide/codex-shared-sessions), [Runloom architecture](ARCHITECTURE.md)

**Session storage has sharing consequences.** Entire's README says checkpoint refs normally travel alongside code and warns that transcripts in public remotes become public; separate private destinations and upload controls are documented. Runloom's local snapshots can also contain sensitive code or conversation text. Storage integration should explicitly preserve the user's sharing policy. [Entire security and privacy](https://github.com/entireio/cli#security--privacy), [Runloom security](../SECURITY.md)

## A defensible direction for Runloom

Our positioning recommendation, rather than a claim about competitors' missing features:

> A small, self-hosted Rust core that connects agent session identity, execution ownership and restorable project state through a client-independent protocol.

The current implementation puts these concerns together: explicit provider/profile/session identity, server-side lease proofs, durable request receipts, directory reservations and supported native-history/worktree branching. Desktop companions and future phone clients can consume that protocol. See [architecture](ARCHITECTURE.md) for the actual guarantees and limits.

The next differentiator should be **demonstrated reliability and integration clarity**: reproducible disconnect/reconnect tests, recovery from interrupted operations, provider-version compatibility, and an inspectable mapping from checkpoint to conversation and code. These are priorities to prove, not comparative performance claims.

Runloom remains an experimental source preview. It does not yet offer the packaged mobile experience described by Happy/HAPI or the cloud collaboration described by Conductor. Physical-device handoff, reboot recovery and clean-machine operations still need acceptance evidence. See the [roadmap](ROADMAP.md).

## What to investigate next

- Evaluate Happy or HAPI when immediate phone access is the primary need; use disposable projects to compare real continuation behavior.
- Study Entire's checkpoint/ref and session-restore formats before inventing another sharing format. Compatibility is a research opportunity, not implemented integration.
- Keep Runloom's core independent from its UI, and prioritize recovery and provider contracts before expanding into a full competing IDE.

No competitor software was installed, authenticated or executed for this comparison.
