Status: done — plan-family review runs stamp a per-target plan digest; stage, gate and approve read one shared review delta, and a first approval refuses it unless waived

```toml run-note
files_touched = [
  "tomlctl/src/plan/target.rs",
  "tomlctl/src/review/runs.rs",
  "tomlctl/src/review/store.rs",
  "tomlctl/src/review/fields.rs",
  "tomlctl/src/plan/store.rs",
  "tomlctl/src/plan/stage.rs",
  "tomlctl/src/plan/approve.rs",
  "tomlctl/tests/plan_approve.rs",
  "tomlctl/tests/review_runs.rs",
]

[[deviations]]
what = "The re-check target set is derived again in review/runs.rs (recheck_targets) rather than shared with review/recheck.rs"
why = "review/recheck.rs only prints its set and is outside this task's files; the copy reads the same two keys (applied.target, addressed) over the same findings. Alternatives: request recheck.rs to expose a shared function (cleaner, one more file), or shell the verb (no)."
means = ""

[[deviations]]
what = "Brief sections are hashed over the section check::design_section_in returns for derive_ref(section), heading line included, not over review::add::section_spans' body"
why = "section_spans is private to the review module and review/add.rs is not this task's file; the digest only needs a stable text per section that changes when the section does."
means = ""

[[deviations]]
what = "recheck_scope without rechecks is refused at review run start; --waive-review-delta with an empty reason is refused; a waiver is stored only when the delta it waives is non-empty"
why = "Malformed payloads are refused naming the field, and records.md says delta_waiver is empty otherwise. Easy to unwind and invisible to a correct caller."
means = ""

[[deviations]]
what = "plan stage reports review_delta as a field only; it does not add a blocker or change stage or next"
why = "The delta can be waived at approval, so a blocker would steer next away from plan approve for a choice the user may decline; the engine prose (a milestone B task) decides how to act on the field."
means = ""

[[tangential]]
kind = "debt"
area = "claude/skills/tomlctl/references/review-runs.md"
summary = "The review run start --set row still lists only scope, models, substrate and rechecks; recheck_scope (delta) and the computed plan_digest are not documented, nor is the plan stage review_delta field or the plan gate review-delta risk row"
why = "Agents reading the flag reference will not know a delta-scoped re-check exists"

[[tangential]]
kind = "debt"
area = "tomlctl/src/review/recheck.rs"
summary = "The recheck-targets set is computed in recheck.rs and again in runs.rs::recheck_targets; a shared pub(crate) function would keep them from drifting"
why = "A digest scope that disagrees with recheck-targets would leave targets in the delta that the re-check did cover"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "rustfmt --edition 2024 --check (each edited file)"
result = "pass"

[[checks]]
command = "grep -q 'plan_digest' tomlctl/src/review/runs.rs && grep -q 'waive_review_delta' tomlctl/src/plan/approve.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/target.rs`: `PlanDigest`, `plan_digest(slug)` and `digest_of` map every field-level target to a SHA-256: each task row field and each spec field as its whole value (no `[n]` keys), each milestone field per checkpoint, each decision, contract and handover review key, each brief section present in `brief.md`, and each heading anchor of every `design/**/*.md` (fence-aware, deduped the way `plan check` slugs them, contained by `check::design_path`). `digest_key` folds an entry target into its list; `review_delta(current, finished)` lists the targets whose digest differs from, or is absent in, the latest finished digest holding them, and is empty when no finished run carries a digest. Unit tests cover the key set, a one-field edit, `digest_key` and the delta rule.
- `tomlctl/src/review/store.rs`: typed `Run.plan_digest` (read, written, refused when not a table of strings), carried on `PlanRun` by `plan_family`.
- `tomlctl/src/review/fields.rs`: `recheck_scope` (values `delta`) and computed `plan_digest` on runs.
- `tomlctl/src/review/runs.rs`: `recheck_scope` joins `START_FIELDS`. A plan-family start on a flow store with `plan.toml` stamps `plan_digest` inside the store write: every target, or for `rechecks = <run>` the targets `recheck-targets` yields for that run, plus the review delta at start when `recheck_scope = delta`. Code and perf runs and scope stores are unchanged.
- `tomlctl/src/plan/store.rs`: `PlanReviewState.delta`, computed in `plan_review_state`, the one review view stage, gate and approve share.
- `tomlctl/src/plan/stage.rs`: prints `review_delta`; the state is loaded once and passed to `review_blockers`.
- `tomlctl/src/plan/approve.rs`: gate adds a `{kind: "review-delta", targets}` risk; `approve --review ran` refuses a non-empty delta on a first approval unless `--waive-review-delta <why>` is given, which is stored as `delta_waiver`; under `--amend` it never refuses.
- Tests: `review_runs.rs` (a plan-family start on a v2 flow stores a digest; code runs and scope stores do not; `recheck_scope` needs `rechecks` and a known value; `plan_digest` cannot be supplied). `plan_approve.rs` (unchanged plan approves with no waiver; an edit refuses, an empty waiver refuses, a waiver approves and is stored; a full run started after the edit leaves the delta while open and clears it once finished; a plain re-check leaves a hand edit in the delta and a delta-scoped one clears it; an amendment with a delta approves and stage and gate report it).

## Checks

The clippy run compiled the library with no diagnostic in this task's files; it fails building the lib test target on `tomlctl/src/plan/records.rs:937` (an `upsert_handover` call missing its fourth argument), a sibling's in-flight edit. Because of it, the new unit tests in `plan/target.rs` and the integration tests were not compiled or run here; the nextest accept check is the gate's.
