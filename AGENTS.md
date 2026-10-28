# Runloom contributor entry

Before changing storage, ownership, execution or provider adapters, read `docs/ARCHITECTURE.md`. Before changing user commands or protocol examples, read `docs/USAGE.md` and the current `protocol.rs`. For release claims, use `docs/ROADMAP.md` acceptance boundaries.

Preserve work identity: workspace, host, provider, configuration root and native session ID. Keep catalog inspection read-only. All managed writes require server-side ownership checks; a client UI lock is insufficient.

Use disposable repositories, profiles and sessions for tests. Leave existing user processes and provider credentials untouched. Preserve official CLI approval and authentication behavior. Unsupported adapter capabilities must fail explicitly.

Keep historical schema migrations intact. Add migrations for persisted schema changes. Checkpoint operations must avoid long SQLite write transactions and retain durable exclusion against overlapping managed writes. Unknown process or reservation state must fail closed until inspected.

Run the checks in `CONTRIBUTING.md` relevant to the change. Report commands, environment, results and remaining uncertainty. Fixture success does not establish authenticated inference, physical-device handoff or reboot recovery. Keep desktop UI, artwork and voice orchestration outside the Rust core.

Module entry points: core `model.rs` / `store.rs` / `reservation.rs`; node `protocol.rs` / `server.rs` / `worker.rs`; checkpoint `lib.rs`; CLI `main.rs`. Read adjacent tests before modifying a contract.
