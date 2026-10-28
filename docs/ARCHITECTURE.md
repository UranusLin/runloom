# Architecture

## Data and execution

`runloom-core` owns work identity, append-only events, leases, execution state, input receipts and directory reservations in SQLite. `runloom-node` admits versioned requests over a private Unix socket and supervises execution. `runloom-checkpoint` captures and verifies Git/file state and supported native history. `runloom-cli` provides local metadata administration.

A Work links one workspace, host, provider, configuration root and native session identity to a project directory. An Execution is a particular process attempt. A lease has an epoch and secret token: a takeover advances ownership and makes stale proofs invalid. Request IDs identify retries; a receipt with uncertain delivery must not be silently treated as safe to resend.

Node and worker lifetimes differ. Closing a request client is not a stop signal for the supervised process. Reopening a node does not automatically rerun previously accepted work. Unverifiable process state is `unknown`.

## Checkpoints and concurrency

A checkpoint records content-addressed objects and metadata for the selected Git and file state. Restores target a new directory. A conversation branch creates a new provider identity, while a code branch uses an independent worktree. Provider formats are version-sensitive; unsupported formats fail explicitly rather than silently degrading to a new unrelated chat.

Directory reservations serialize conflicting managed checkpoint and execution operations without holding a global SQLite write transaction through Git or filesystem work. Ordinary completion/error releases the reservation. Interruption can retain an uncertain reservation; it deliberately blocks conflicting writers pending inspection. This is not a lock on arbitrary external editors or processes.

The node uses bounded concurrent dispatch so slow work does not require serializing every status or lease request. Dispatch permits at most 32 in-flight requests; it does not reserve priority slots for control calls. Output capture retries transient SQLite busy errors with a bounded in-memory chunk and pipe backpressure; this is not a crash-durable output spool. Resource limits and short database transactions remain important even on a private host.

## Schema upgrades and recovery

The current schema is v4. Back up the node root with all managed processes stopped before upgrading. Opening a v3 database with the new core migrates it; an old v3 binary rejects the newer schema. In-place downgrade is unsupported. Retain the backup and matching binary together.

There is no automatic expiration, replay or recovery UI for uncertain checkpoint reservations. Crash or panic can retain one and block overlapping writers until the operator inspects the original process and filesystem state. Never delete a reservation merely because a client timed out.

## Extension seams

Clients consume the versioned protocol, not SQLite internals. Host transport, provider identity and checkpoint storage are distinct concerns. New provider adapters must define supported discovery/resume/branch capabilities and tests. Desktop characters, speech and notifications belong to clients.

The current implementation is a local trusted-account service, not a multi-tenant cloud scheduler. A future network API needs its own authentication, authorization, quotas and audit model.
