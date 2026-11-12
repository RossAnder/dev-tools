Status: done — six plan submodules declared, probes exposed to the crate

- tomlctl/src/plan/mod.rs: added `pub(crate) mod authored;`, private `mod run_note; note_sources; run_notes; run_gate; run_report;`, and changed `mod probes;` to `pub(crate) mod probes;`.
- Six new files under tomlctl/src/plan/, each only a `//!` doc line.

Deviations: none.
Checks: both accept greps pass; `rustfmt --edition 2024 --check` clean on each new file. No compile check run (new modules are empty; unused-module warnings only).

TANGENTIAL: none
