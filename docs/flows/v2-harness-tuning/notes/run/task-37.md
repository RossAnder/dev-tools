Status: done — write-and-prove.md now covers run_by/covered_by at Write, gate probes at Design, and gate probe runs at Prove.

Changes (claude/skills/plan-v2-engine/references/write-and-prove.md):
- Section 1 Milestones: paragraph on `--gate-probe '<gate> :: <probe>'`.
- Section 3 spec rules: `run_by` bullet (gate, covered + covered_by, worker's narrow run named in detail) after the writes_build rule. Example row left unchanged.
- Section 4d: gate probes emit/run/record with `--emit --gates` and `--record <capture> --gates`; `fails`/`broken` verdict handling.

Deviations: none. Note: design/shapes.md does not exist under the skill's references; the anchors are cited as the spec instructed.

Checks: both accept greps pass; `cargo test --lib -- cli::dispatch::tests` 55 passed.

TANGENTIAL: none
