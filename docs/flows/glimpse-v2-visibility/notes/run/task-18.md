Status: done — authored listing types in authored.rs and App holds it with apply_authored.

Changes:
- glimpse/src/authored.rs: AuthoredKind (kebab-case, unknown-value fallback), AuthoredSource, AuthoredFile, AuthoredList envelope, Authored holder (apply with revision short-circuit, clear, by_path, run_note), four unit tests.
- glimpse/src/app.rs: `authored` field, `apply_authored(list, revision)`, cleared in apply_snapshot when the flow changes.

Deviations: none.

Checks: clippy compiles; only dead-code warnings (no caller yet, later tasks consume). rustfmt applied to authored.rs. Worker accept greps pass. The cargo test accept check is the gate's; not run.

TANGENTIAL: none
