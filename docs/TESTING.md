# Testing and coverage

README badges are checked-in **local measurement snapshots**, not live GitHub CI badges. Test pass rate answers whether the checks passed; line coverage answers how much instrumented Rust source was observed executing. Neither proves security, correctness or production readiness.

<!-- coverage:start -->
Measured 2026-10-30T10:31:22.571422+00:00 with cargo-llvm-cov 0.9.1; rustc 1.91.1 (ed61e7d7e 2025-11-07).

Rust tests: **29/29 passed (100%)**.

| Rust crate | Rust tests | Tests + offline fixtures |
| --- | ---: | ---: |
| runloom-checkpoint | 571/649 (87.98%) | 571/649 (87.98%) |
| runloom-cli | 5/52 (9.62%) | 52/52 (100.00%) |
| runloom-core | 790/924 (85.50%) | 816/924 (88.31%) |
| runloom-node | 14/1279 (1.09%) | 447/1279 (34.95%) |
| **Workspace** | **1380/2904 (47.52%)** | **1886/2904 (64.94%)** |

[Machine-readable summary](coverage-summary.json) includes the source fingerprint and per-file counts.
<!-- coverage:end -->

## Measurement scope

The baseline runs `cargo test --workspace --locked`: Rust unit **and integration** tests, not exclusively unit tests. The combined measurement adds eleven offline Python-driven process/demo fixtures. Both reports include the same Rust workspace source denominator, including CLI and node source with little/no baseline execution.

The metric is **line coverage**, not branch or condition coverage. cargo-llvm-cov defaults exclude test/example/bench files and external dependencies. Python fixture/native-history code, bundled SQLite C, desktop UI and provider internals are outside this percentage. Inline Rust test code may remain included by the tool.

Some fixtures intentionally terminate or kill persistent node/supervisor processes. Those processes may not flush LLVM profiles on shutdown, so observed coverage can undercount exercised paths. A low node percentage is a reason to improve measurement and failure tests, not proof that every unreported path was untested. No live AI invocation, physical handoff or reboot acceptance is included.

## Reproduce

On macOS, with the normal project prerequisites:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.1 --locked
bash scripts/coverage.sh
```

The script clears previous workspace coverage, rebuilds `target/debug` with instrumentation, runs both suites and produces private raw reports plus sanitized `target/coverage/summary.json`. It stops on a failing test, fixture or report command. Use the default target directory because fixtures resolve binaries there. Do not run it alongside another build of the same checkout. A normal `cargo build --workspace --locked` afterward rebuilds without instrumentation.

To refresh the committed snapshot and badges in a public checkout:

```sh
python3 scripts/coverage-report.py --docs docs
```

The internal source template uses `--docs docs/public`. The summary includes tool/compiler versions, UTC timestamp, relative file metrics and a hash of relevant source/test inputs. Raw reports can contain local absolute paths and are intentionally excluded from source export.

## CI and next improvements

The coverage job runs the same script and uploads the sanitized summary as a GitHub artifact. The [first hosted core and coverage run](https://github.com/UranusLin/runloom/actions/runs/42153908748) passed. The checked-in measurements above remain dated local snapshots; hosted summaries are available as workflow artifacts. Badges remain explicitly dated snapshots, without a Codecov token or external coverage account.

Use the per-file breakdown to prioritize node lifecycle, input uncertainty and recovery tests. Establish a reproducible baseline before adding a coverage threshold; do not hide low-covered source files or add assertions solely to inflate a badge. See [roadmap](ROADMAP.md) and [contributing](../CONTRIBUTING.md).
