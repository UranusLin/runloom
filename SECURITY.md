# Security

Runloom is an experimental developer preview. The security boundary is the execution host's OS account and SSH access. Anyone who controls that account can access its processes, session files and credentials. Workspace names do not isolate hostile tenants.

## Deployment boundary

Keep the node root private (owned directory, mode `0700`). Use the local Unix socket or SSH bridge. Do not expose a management bridge directly to the internet. Run as the intended unprivileged user. This source preview intentionally excludes privileged service installers.

Leases fence managed writes from stale clients. They do not constrain commands run outside Runloom. Permission overrides for an agent are explicit per-task choices; a node is not a sandbox. Provider tools retain their own authentication and approval behavior.

Checkpoints can contain proprietary source, selected conversation history and secrets embedded in tracked files. Filename exclusions are not general secret removal. Review content before sharing. Keep authentication data separate from work snapshots and never post private node state in public issues.

A lost supervisor or interrupted reservation can leave an unknown state. Inspect the process and files before recovering; automatic replay could duplicate external side effects. Exactly-once execution is not promised.

## Reporting

Before making a GitHub repository public, the maintainer must enable private vulnerability reporting and verify its Security tab offers **Report a vulnerability**. That repository feature is the intended reporting channel; it is not provisioned by this source export. Until a private channel is available, do not post sensitive reproduction data in public issues.

Report the affected version, a disposable reproduction, expected/observed behavior and impact. Remove credentials, personal session contents and private host details. No support SLA is established for this preview.
