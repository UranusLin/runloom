#!/usr/bin/env bash
# Local fixtures only. Rebuilds target/debug with coverage; no live providers.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  echo 'Unset CARGO_TARGET_DIR: fixture scripts use this repository target/debug.' >&2
  exit 2
fi
cargo llvm-cov --version
coverage_env=$(mktemp)
trap 'rm -f "$coverage_env"' EXIT
cargo llvm-cov show-env --sh > "$coverage_env"
source "$coverage_env"
cargo llvm-cov clean --workspace
mkdir -p target/coverage
cargo build --workspace --locked
cargo test --workspace --locked 2>&1 | tee target/coverage/rust-tests.log
cargo llvm-cov report --json --summary-only --output-path target/coverage/rust-tests.json
for fixture in smoke-recovery-plan smoke-cli smoke-node smoke-control smoke-pty smoke-checkpoint smoke-checkpoint-concurrency smoke-branch-control test-native-materialization smoke-task demo-local; do
  python3 "scripts/$fixture.py" 2>&1 | tee "target/coverage/$fixture.log"
done
cargo llvm-cov report --json --summary-only --output-path target/coverage/combined.json
python3 scripts/coverage-report.py
