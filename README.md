# Runloom

Keep AI work on its execution host. Reconnect, take over control, or branch from a saved checkpoint.

**Developer preview · MIT · macOS-first.** Runloom is a Rust execution and session-state core, with a private node protocol. It is not a hosted service or a replacement model. The source preview excludes desktop UI, character artwork, voice assets, credentials, deployment profiles and private development history.

## What it does

- **Resume:** retain provider identity and continue through installed official Codex or Claude Code tools.
- **Handoff:** a server-checked lease fences stale controllers when another client takes over.
- **Replay:** inspect recorded process output and request receipts after reconnecting.
- **Checkpoint:** save Git state, selected files and optionally supported native conversation history; restore to a new directory.
- **Branch:** create an independent worktree and native conversation identity through a supported adapter.
- **Tasks:** store a design brief and acceptance criteria for an explicitly started Claude CLI task.

A provider's completion message is not proof that acceptance tests passed. Runloom does not reconstruct hidden model reasoning or merge conversations across different providers.

## Try it without an AI account

Requirements: macOS, Rust/Cargo 1.91 or newer, Git, Python 3.9 or newer, and a C toolchain for bundled SQLite. Other operating systems are not certified by this preview.

```sh
cargo build --workspace --locked
python3 scripts/demo-local.py
```

The demo creates a temporary Git project and private node, starts a real local fixture process, switches between two simulated laptop controllers, rejects the stale controller, replays output and restores a checkpoint. It removes its temporary directory afterward. It makes no AI calls, needs no sudo or SSH, and does not touch existing sessions.

This demonstrates local protocol behavior. Physical device handoff, host reboot recovery, clean-machine installation and provider authentication are separate acceptance checks.

## Architecture

```text
CLI / your client
       | private Unix socket, optionally bridged over SSH
       v
Runloom Node ---- SQLite: identity, events, leases, receipts
       |---- supervisor ---- installed official provider CLI
       |---- checkpoint object store ---- new directory / worktree
```

Work identity includes workspace, host, provider, configuration root and native session ID. The execution host owns the process and files; clients control it through the node. Workspace labels are logical organization, not tenant isolation. Use separate OS accounts when isolation is required.

## Start using your host

Follow [USAGE](docs/USAGE.md) for foreground startup, SSH access and the request lifecycle. Start with a disposable project and keep provider authentication on the execution host.

- [Architecture and invariants](docs/ARCHITECTURE.md)
- [Development and tests](CONTRIBUTING.md)
- [AI contributor entry](AGENTS.md)
- [Roadmap and acceptance boundaries](docs/ROADMAP.md)
- [Security model and reporting](SECURITY.md)
- [License](LICENSE) and [third-party notices](THIRD_PARTY_NOTICES.md)

The preview ships source and test fixtures, not a signed installer. Package version `0.1.0` does not mean stable v0.1 acceptance is complete.
