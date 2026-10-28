# Roadmap and acceptance

## Short term: demonstrable developer preview

Deliver the Rust source, MIT license, third-party notices, generic documentation, fixture demo and repeatable local checks. Review execution ownership, checkpoint concurrency and privilege boundaries independently. Export only the public allowlist; private deployment history and credentials stay outside the package.

Before publication, run checks from the clean export, inspect its contents, run hosted CI in the destination repository and activate private vulnerability reporting. A locally generated source export is not an uploaded release.

## Medium term: stable v0.1

Validate a clean-machine install/upgrade/uninstall flow; real laptop disconnect and second-device takeover of the same host execution; host reboot and interrupted-operation recovery; and supported authenticated provider versions using disposable sessions. Record observed results separately from fixture results.

Improve explicit recovery tooling for uncertain state, provider compatibility reporting and operational diagnostics. Add repeatable acceptance evidence before promising unattended reliability.

## Long term: product and ecosystem

Develop portable clients, phone access, adapter contracts and richer session branching/version management. Evaluate remote object storage and synchronization while preserving host ownership and credential boundaries. Multi-tenant isolation and cloud deployment require a separate security design.

No date or completion claim is implied by this roadmap. The package version is currently a development version; local protocol tests do not establish stable cross-device or reboot behavior.
