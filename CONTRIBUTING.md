# Contributing

Use a branch and a disposable node root. The core has no dependency on a desktop client. Changes should preserve identity and ownership invariants described in [architecture](docs/ARCHITECTURE.md).

## Local verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python3 scripts/smoke-cli.py
python3 scripts/smoke-node.py
python3 scripts/smoke-control.py
python3 scripts/smoke-pty.py
python3 scripts/smoke-checkpoint.py
python3 scripts/smoke-checkpoint-concurrency.py
python3 scripts/smoke-branch-control.py
python3 scripts/test-native-materialization.py
python3 scripts/smoke-task.py
python3 scripts/demo-local.py
```

These use fixture processes and synthetic provider data, not live inference. Several smoke scripts retain disposable directories under `/tmp` to support inspection. Some fixture launchers use the macOS system Python path. Build first because scripts locate binaries under `target/debug`.

Dependency inventory uses `Cargo.lock`. `python3 scripts/audit-dependencies.py` contacts OSV and checks registry package versions against known advisories; a clean result does not prove application security or cover bundled C code. Regenerate notices with `python3 scripts/third-party-notices.py --output THIRD_PARTY_NOTICES.md` after changing dependencies.

For a change, include the observable problem, resulting behavior, commands tested and limitations. Add regression coverage for ownership, persistence and concurrency changes. Live-provider tests require separate explicit authorization and disposable profiles; they may consume paid quota.

## Coverage

Run `bash scripts/coverage.sh` after installing the tooling in [TESTING](docs/TESTING.md). It measures Rust-test coverage separately from the combined offline fixtures and emits a sanitized summary. README badges are local snapshots, not live CI status.

## Release review

Review code and security independently, resolve reproducible high-severity findings, and build from the exported source tree. Follow [ROADMAP](docs/ROADMAP.md) for the distinction between a demonstrable preview and stable acceptance. GitHub Actions is configured but must run in the actual public repository before claiming hosted CI success.
