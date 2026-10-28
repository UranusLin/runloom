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
