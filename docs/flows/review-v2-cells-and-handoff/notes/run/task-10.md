Status: done — `review escalation fix <cell>:<n>` mints an unverified, evidence-blocked, verifier-routed finding into the escalating cell through convert's shared mint path

```toml run-note
files_touched = ["tomlctl/src/review/escalation.rs", "tomlctl/src/review/triage.rs", "tomlctl/tests/review_escalations.rs", "tomlctl/tests/output_options.rs"]

[[deviations]]
what = "The fix leaf's defect rows carry `fix` as well as the fields the detail lists; the field catalogue requires `fix` on every defect, blocked or not, and the leaf adds no waiver"
why = "fields::validate_row refused the detail's field list with '`fix` is required'; a defect without a candidate fix would also be refused by `review add`, so the escalation path keeps the same row contract"
means = "detail: for a defect fix, failure_scenario, severity, effort, counter, grade, evidence"

[[deviations]]
what = "The shared mint path is a `triage::Source` enum (Advisory or Escalation {id, cell, family}) taken by `replacement_row` and `finish_row`, with the NDJSON read split out as `one_object` and the orchestrator verdict and minted report as `orchestrator_verdict` and `minted_entry`"
why = "The escalation leaf must check the row's kind and blocked fields before the shared validation, so the parsed row is passed in rather than the source path. An escalation source defaults only `lens` (the cell's first concern) and seeds no `related` entry; `related` is written only when non-empty, which leaves convert's rows unchanged since an advisory source always names itself first"
means = "detail: generalise replacement_row and finish_row keyed on a source"

[[deviations]]
what = "The fix leaf prints `{ok, escalation, status, minted: {id, kind, severity, route, status}}`; refusals name every reason in one error and write nothing, and a finished run is refused like convert's"
why = "Mirrors convert's `minted` entry so the orchestrator reads both the same way; the detail fixes no output shape"
means = "detail: output unspecified"

[[deviations]]
what = "The output_options fixture's sealed C1 now holds an open escalation C1:1 so the isolated write case has one to fix"
why = "Each write case runs on a fresh copy of the fixture, so the record case's escalation is not there for the fix case"
means = "detail: add the leaf's case to tests/output_options.rs"

[[tangential]]
kind = "debt"
area = "claude/skills/tomlctl/references/review.md"
summary = "The flag table has no row yet for `review escalation fix` (positional CELL:N, --slug|--scope, --ndjson, integrity flags)"
why = "The reference is held by another task in this milestone; whoever documents the escalation group should include the fix leaf and its refusals"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/review/escalation.rs tomlctl/src/review/triage.rs tomlctl/tests/review_escalations.rs tomlctl/tests/output_options.rs"
result = "pass"

[[checks]]
command = "grep -q 'Fix' tomlctl/src/review/escalation.rs && grep -q 'blocked_missing' tomlctl/src/review/escalation.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'test(/^review_escalations::/) | test(/^review_triage::/) | test(/^output_options::/)'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `tomlctl/src/review/triage.rs`: `replacement_row` and `finish_row` now take a `Source` (an advisory finding, or an escalation with its cell and run family) and are `pub(super)`; the source supplies the verb in refusals, the family, the provenance cell, the defaults and the `related` seed. `one_object` reads the single NDJSON object (convert keeps its "exactly one replacement object" message), and `orchestrator_verdict` and `minted_entry` hold the verdict and report convert wrote inline. Convert's output, refusals and stored rows are unchanged; the `review_triage` tests pass as before.
- `tomlctl/src/review/escalation.rs`: a `Fix` leaf, `review escalation fix <cell>:<n> --slug|--scope --ndjson <src>`. It refuses a malformed id, an unknown cell or escalation, an escalation not `open` (naming the finding it was fixed as), and a finished run; requires `kind` defect or cleanup, `blocked_on = "evidence"` and a non-blank `blocked_missing`; then validates through the shared path (the lens defaults to the cell's first concern, and a lens outside its concerns is refused), sets `route = "verifier"` whatever the kind, and under the lock re-checks the escalation is open, mints the row `unverified`, records the `orchestrator` verdict, and marks the escalation `fixed` with `finding` set to the new id, all in one write.
- `tomlctl/tests/review_escalations.rs`: three tests. Fixing C1:1 mints F1 in C1 with lens `ripple`, route `verifier`, status `unverified`, blocked on evidence, one orchestrator verdict, and the escalation `fixed` with `finding = F1`. A foreign lens, a missing `blocked_missing`, an unknown escalation or cell, a second fix and a finished run are each refused with the store's bytes unchanged. After a verifier confirms F1 it is `open`, and `review schedule F1 --scope t --dry-run` lists it under `settle`.
- `tomlctl/tests/output_options.rs`: an open escalation on the fixture's C1 and a `review escalation fix C1:1` write case keyed on `minted`.

Security: none of this code touches authentication, input trust boundaries beyond the store's own field validation, or tokens; the row passes the same field catalogue, record-target and text checks `review add` and convert use.
