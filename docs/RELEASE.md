# Runloom 0.1.0-alpha.1

First versioned developer preview of the Rust continuity core. MIT licensed.
This bundle includes `runloom` (metadata CLI) and `runloom-node` (Unix socket
service and request bridge). It does not include a GUI, Telegram bot, artwork,
provider credentials or a privileged service installer.

## Included

- Scoped work/session identity, durable execution records and output events.
- Lease-fenced control, provider resume plans, Claude task intake.
- Checkpoint inspection/restore and isolated conversation/worktree branches.
- Read-only recovery inspection and advisory plans; no automatic repair.

## Install a binary preview

Target: Apple Silicon macOS; deployment target macOS 14.0. Actual compatibility
must be recorded for each tested OS. Intel, Linux and Windows binaries are not
provided in this release. Python 3.9+ is needed for the bundle installer;
build/release verification scripts also support Python 3.9+.

Download the archive and matching `.sha256` file from the same GitHub Release.
Replace `<version>` below with `0.1.0-alpha.1`:

```sh
shasum -a 256 -c runloom-<version>-aarch64-apple-darwin.tar.gz.sha256
tar -xzf runloom-<version>-aarch64-apple-darwin.tar.gz
mkdir -p "$HOME/.local/share/runloom/versions"
python3 runloom-<version>-aarch64-apple-darwin/install.py \
  --prefix "$HOME/.local/share/runloom/versions/<version>"
"$HOME/.local/share/runloom/versions/<version>/bin/runloom" --version
```

The installer verifies bundle hashes and refuses existing prefixes; no sudo,
PATH changes, daemon installation or running-process replacement. Put the new
version's `bin` directory on your PATH explicitly when ready. `BUILD.json` records
source commit, Rust compiler, target, package/schema/protocol versions and hashes.
Checksums detect corruption; they are not a publisher signature. Binaries are
not Developer ID signed or notarized. Downloaded-file Gatekeeper behavior has not
been certified; no global security-disabling instructions are provided. Building
from source is an alternative.

For first use see [usage](USAGE.md) in the source repository. Start a foreground
node with a new private root; the binary bundle does not register a service.

## Versions and upgrades

- Product version: `0.1.0-alpha.1`; future preview changes increment `alpha.N`.
- Git release tag: `v0.1.0-alpha.1`; the package version must match the tag.
- Build identity: source commit + target + compiler; builds are not claimed
  byte-for-byte reproducible. Never replace published assets in place.
- Database schema: v4. Wire request envelope: v1. These evolve independently
  from package versions. Clients should inspect capabilities, not assume that
  identical product versions imply all features are enabled.
- Companion and GUI clients have independent releases and compatibility gates.

Side-by-side binary installation is not a state migration. Before switching a
running node, verify there are no active/unknown workers, stop admission and take
an offline consistent backup of its complete state. Do not copy over a running
executable or point an older binary at a migrated database. In-place schema
downgrade is unsupported. The private guarded daemon updater is not included in
this preview. Removing the installed version folder does not remove node data;
stop any processes using that version before uninstalling it.

## Acceptance boundaries

Offline tests and isolated-HOME installation checks do not establish physical
cross-device handoff, reboot recovery, fresh-machine installation, notarization
or authenticated provider continuation. See the source roadmap for stable-v0.1
gates. Exit code zero and successful task admission are not product acceptance.

## Maintainer release procedure

1. Commit a clean versioned source tree and run contributor checks.
2. Run `python3 scripts/package-release.py` (native macOS arm64), then
   `python3 scripts/smoke-release.py dist/runloom-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz`.
3. Push only the reviewed public repository; confirm hosted CI succeeds.
4. Create/push the matching tag. The tag workflow runs checks again, builds,
   verifies installation and creates a **draft prerelease** with assets.
5. Inspect the draft, build identity, platform evidence and known limitations;
   publish manually. A failed rerun never overwrites an existing release.

Local builds need no tag. Tagged CI builds pass `--tag` to enforce tag/version/HEAD
agreement. The first release has not been published merely because these files
exist. Hosted execution and OS Gatekeeper checks remain separate acceptance steps.
