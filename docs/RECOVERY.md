# Recovery: inspect first

The first recovery milestone is **read-only diagnosis**, not automatic repair. It exposes persisted evidence without editing the database, clearing a reservation, replaying input or starting a provider.

## Inspect a work

Use your existing node root, workspace and work ID. Send this through the normal `request` bridge (or the SSH bridge in [USAGE](USAGE.md)):

```json
{"version":1,"op":"recovery_inspect","workspace":"demo","work":"WORK_ID"}
```

Check `ping` for `recovery_inspect: true` before calling this on an older deployment. New nodes also report `schema_version` and `package_version`; package version alone does not identify a deployed build, so record its binary SHA-256 during deployment.

The response contains:

| Field | Meaning | Safe interpretation |
| --- | --- | --- |
| `reservations` | Reservation ID, source/destination path, creation time and native-identity fencing flag | A persisted fence exists; age does not prove it is abandoned |
| `unsettled_executions` | Execution ID and persisted `queued` / `running` state | Database evidence, not a live process check |
| `uncertain_inputs` | Execution/request IDs whose outcome was recorded as uncertain | Do not automatically resend; inspect provider/native history |
| `truncated` | Any category exceeded the 100-row response cap | The response is incomplete; do not infer that omitted evidence is absent |
| `process_liveness_checked` | Always false in this milestone | No process death or ownership-generation claim is made |
| `automatic_recovery_available` | Always false in this milestone | No safe-to-unlock or safe-to-replay verdict is provided |

The endpoint is scoped to the selected work and workspace. It does not enumerate another work's overlapping reservations; a reservation-conflict error can therefore refer to evidence outside this response. Empty arrays do not prove that the directory is safe to mutate. Tokens, commands, input payloads and conversation contents are omitted.

## What to do with the evidence

Preserve the node root and any partially restored files. Inspect the original operation, relevant worker and provider history before deciding what to recover. Do not delete a reservation solely because its lease expired or the node restarted. Do not repeat an uncertain input solely because no reply reached the client.

The next milestone will add operation ownership/liveness evidence and an explicit, validated resolution plan. Controlled resolution must recheck that evidence at apply time, require current authority, record the decision and leave an audit event. It must refuse live or unverifiable owners and must never replay uncertain external side effects automatically.

## Validation boundary

Unit/integration tests cover scoped queries, bounded output, omission of sensitive payloads, retained uncertain receipts and retained crash reservations after inspection. The slow-checkpoint fixture calls the endpoint after killing its disposable service and verifies that a new writer remains blocked. These tests do not prove physical host reboot recovery or provide an unlock operation.

## Advisory recovery plan (plan version 1)

`ping.recovery_plan: true` advertises the next diagnostic stage. Request:

```json
{"version":1,"op":"recovery_plan","workspace":"demo","work":"WORK_ID"}
```

Or use the local command-line reader on the node host:

```sh
python3 scripts/recovery-plan.py --node ./target/debug/runloom-node \
  --root /absolute/path/to/node --workspace demo --work WORK_ID
```

Add `--json` to include scoped evidence IDs/paths; these can reveal private project information. The reader checks capability first, starts no service and never applies a repair. For remote use, the same JSON request works through the existing SSH request bridge.

| Disposition | Meaning |
| --- | --- |
| `evidence_incomplete` | At least one evidence category was capped; missing rows prevent a complete inventory |
| `manual_investigation_required` | Recorded reservations, unsettled executions or uncertain inputs need investigation |
| `no_recorded_blockers` | None in the three inspected categories; **not** a safe-to-resume or healthy-process verdict |

The plan contains the same consistent inspection snapshot, ordered steps with stable codes, prohibited actions and explicit limitations. Resolve incomplete evidence first, preserve checkpoint artifacts, establish execution ownership and reconcile uncertain input against provider/output history. Branch/task receipt files, overlapping other work, unmanaged processes and external effects are outside this snapshot.

`apply_supported` remains false. Plans are advisory snapshots, not persistent approvals, apply tokens or locks; they become stale as work changes. `recovery_apply` is not implemented. No plan may authorize deletion of a reservation, input replay or task restart. Ownership/generation evidence and an audited, rechecked repair protocol remain separate milestones.

Run `python3 scripts/smoke-recovery-plan.py` after building. It creates a disposable node with synthetic interrupted-operation evidence, checks scope/refusal/CLI guidance and verifies that all fences and uncertain records survive. No AI provider or existing session is used.
