Status: done — a task with a spec shows its spec, change trail, attempts and a Links section in its details, with every related record in its links; a task without one draws as before

## What changed

- `glimpse/src/view/task_spec.rs`: new `sections(app, task, out, links)`, which returns at once for a task with no `spec`. In order it draws Outcome (markdown), Constraints (a markdown bullet list), Means (binding, `required` in the warning token, then text, with its detail as indented markdown), Executor, Design (each entry linked as `Target::Authored` of the path before `#`), Precedent (each linked as `Target::Source` with no line and no `read_at`), Accept (behaviour, check, kind, owner with `by <covered_by>` when set, and the probe: the verdict with its `at`, `unprobed` for a probeable entry with no probe, or `not probed` for one the emitter skips), Premises (claim, check, when, and the same verdict rule against `holds`), Changes (the trail rows for the task's ref: at, field, old → new, then actor: reason), Attempts (run, status, summary, then deviations, checks and the proposed handover) and Links. The Links section draws one line per related record the sections above have not drawn, and links each one: the milestone, binding decisions, contracts, the run note from the authored listing, the commit when stamped, finding ids (`fixes` and `settles`), backlog rows (`closes` and `refers to`), and the handovers recorded from this task. A link is added once, at its first mention. The module has six tests over the fixture.
- `glimpse/src/view/details.rs`: the body loop is split so `task_spec::sections` runs after Action and Detail and before Acceptance and Dependency note. In the Files section, a file with a note in `file_notes` is drawn as `path  — note`, for a task with a spec only. A file with no note keeps the same `Line::styled` it had before. `INDENT`, `section` and `indented` are now `pub(super)`, so the new module can use the existing helpers. Two existing tests changed because fixture task 5 now has a spec: the Files expectation includes the note, and the related-tasks link assertion keeps only the `Target::Task` links.

## Deviations

- The attempt's proposed handover has no id, so it cannot be a `Target::Handover`. The Attempts section shows its action as text. The Links section links the recorded handovers whose `source_task` is this task (`PlanIndex::handovers_for`), and those are what the orchestrator records from attempts. The other option was matching each proposal to a record by its action text, which is fragile.
- Links order: design and precedent are linked where their sections draw them. Every other kind of link has a line in the closing Links section, in the order the detail lists them, with handovers last. That keeps the links in the order the text shows them. The other option was to link the milestone at the header fact line, but that would put it ahead of the task links that details.rs already collects.
- File notes are drawn only when the task has a spec. This follows the constraint that a task with no spec draws exactly as before, even if its row has `file_notes`.
- details.rs also made three private helpers `pub(super)`, which goes beyond "only the call and the file-note marks". Copying them into task_spec.rs would have duplicated the indent and section rules.

## Handover

None.

## Checks

- `rustfmt --edition 2024` on both files: applied, and `--check` is clean.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles with no warnings in either file. The three remaining dead-code warnings are in `authored.rs` and `model/plan.rs` (`by_path`, `latest_report`, `gate_rounds_for`/`health`), which this task does not hold.
- Accept "the spec sections are built in their own module and called from the task builder" (worker): pass.
- Accept "the task spec view carries its own tests" (worker): pass.
- Accept `cargo test --manifest-path glimpse/Cargo.toml task_spec` is the gate's and was not run. I checked the exact-string assertions by reading the code and fixture, not by running them.
- Risk for the gate: a frame test elsewhere, such as `tests/cli.rs` `--once`, that shows the details of fixture task 5, 6 or 8 would now see the new sections. Within the files I hold, the only such tests are the two in details.rs, which I updated.

TANGENTIAL: none
