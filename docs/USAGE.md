# Using the preview

## 1. Build and run the isolated demonstration

```sh
cargo build --workspace --locked
python3 scripts/demo-local.py
```

A passing demo demonstrates local ownership takeover, output replay and a restored working file. It uses a fixture process, not an AI provider. Source code in the demo is an executable client example for register/acquire/start/input/status/events/checkpoint operations.

## 2. Start a foreground node

Run as your normal execution account. Keep this terminal open while experimenting:

```sh
RUNLOOM_ROOT="$HOME/.local/share/runloom-preview"
mkdir -p "$RUNLOOM_ROOT"
chmod 700 "$RUNLOOM_ROOT"
./target/debug/runloom-node --root "$RUNLOOM_ROOT" serve --host demo-host
```

In a second terminal in the repository:

```sh
printf '%s\n' '{"version":1,"op":"ping"}' | \
  ./target/debug/runloom-node --root "$HOME/.local/share/runloom-preview" request
```

This default startup permits observation and metadata operations but does not enable process execution. Add `--allow-interactive` or `--allow-batch` to `serve` only when deliberately testing the corresponding execution capability. These flags do not make the node a sandbox.

A foreground service is suitable for this preview. For unattended operation, the host needs a separately reviewed service-manager setup. Closing the node is not a request to terminate existing supervisors. This preview has no one-command uninstall: preserve the node root until all associated processes are confirmed stopped, then archive/remove only the test root you created.

## 3. Reach the same node through SSH

Build/install the binary on the execution host and authenticate the SSH alias `runloom-host` yourself. For example, place the binary at `~/.local/bin/runloom-node`, then run the foreground node there with the same private root. From a client computer:

```sh
printf '%s\n' '{"version":1,"op":"ping"}' | \
  ssh runloom-host '.local/bin/runloom-node --root "$HOME/.local/share/runloom-preview" request'
```

The alias is a placeholder for your SSH configuration. No TCP management listener, tunnel exposure or copied provider credentials are needed. A private network can carry SSH; Runloom does not configure that network.

## 4. Register and control a work

Each request is one JSON line with `version: 1` and `op`. Responses have `ok` and either `result` or `error`. Requests are limited to 64 KiB and responses to 8 MiB. The current bridge has a 15-second response timeout: a timeout is not proof that a mutation was cancelled; inspect resulting state before retrying.

Use the absolute paths and host identity of the execution machine. A register request has this form (replace the fixture values):

```json
{"version":1,"op":"register","work":{"workspace":"demo","host":"demo-host","provider":"fixture","config_root":"/tmp/demo-profile","native_id":"demo-session","directory":"/tmp/demo-project","title":"Disposable example"}}
```

Registering records identity; it does not launch a process. Keep the returned work ID. Send `acquire` with `workspace`, `work`, `holder` and `ttl`; retain the returned proof privately. A competing controller reads `lease_status` and explicitly passes the observed `takeover_epoch` when acquiring. Managed writes require the current proof. Renew before expiry. Do not log or publish lease tokens.

`events` accepts `after` and `limit` for replay. `status` addresses an execution ID. Interactive `input` has a nested object with workspace/work/execution, request ID, proof and UTF-8 bytes. Preserve request IDs across retries and use `input_status` when delivery is uncertain. See the fixture demo and `crates/runloom-node/src/protocol.rs` for the exact current field contracts.

## 5. Continue official provider sessions

Install and authenticate the official provider CLI in the execution account, separately from Runloom. Register the real provider (`codex` or `claude-code`), its configuration root and native session ID. Use a disposable session first. Send `resume_plan` to inspect the command; `resume` requires explicit execution capability and ownership proof. A plan is not proof that authentication or inference works.

Supply the provider executable path, a request ID for resume, and the intended `headless` mode. Headless resume requires a nonempty prompt. Keep approval prompts available for interactive work. Provider-specific flags and history formats evolve; inspect the adapter code and test the installed version before relying on continuation or branching.

## 6. Save and branch

Use `checkpoint_capture` while the work is idle, choose untracked paths explicitly, and opt into native-history capture when appropriate. Restore with `checkpoint_restore` into a new destination. `checkpoint_fork` is asynchronous; inspect `branch_status` until ready or inspection is required. Never overwrite a working project to test restore.

A failed or interrupted operation may need inspection rather than replay. Preserve the node root and resulting files for diagnosis. Snapshot content can contain secrets even when common credential filenames are excluded.

## 7. Inspect interrupted work

Use `recovery_inspect` with the selected workspace/work to read persisted reservations, unsettled executions and uncertain input IDs. The endpoint never unlocks, retries or verifies process death. See [RECOVERY](RECOVERY.md) for response semantics and limits. Older deployments may not advertise the capability; check `ping` first.
