Status: done — review schedule orders a prose fix after the code fix whose executable path it names

```toml run-note
files_touched = ["tomlctl/src/review/schedule.rs", "tomlctl/tests/review_schedule.rs", "claude/skills/tomlctl/references/review-apply.md", "claude/skills/apply-v2-engine/SKILL.md"]

[[deviations]]
what = "Name matching requires a path boundary: the named path or basename must not be glued to a longer path or identifier (no letter, digit, _, -, ., / or \\ before it; no letter, digit or _ after it)."
why = "A bare substring test would let request.rs match myrequest.rs or a different directory's request.rs; the boundary keeps the edge count to real mentions. Alternative weighed: plain substring (simpler, more false edges)."
means = "detail: 'contains one of B's executable edited paths in full, or that path's file name'"

[[deviations]]
what = "An ordering edge to a task the doc row already needs through depends_on is not added again, reported under ordered, or given a constraint."
why = "Keeps existing depends_on handling unchanged and collapses edges per task pair; the alternative (still adding the check-against-code constraint) would alter rows depends_on already produces."
means = "detail: 'Collapse edges per task'"

[[deviations]]
what = "The new tests were appended to review_schedule.rs with a bash heredoc rather than the Edit tool."
why = "A slip against the file-edit rule; line endings were checked afterwards (all LF, matching the file) and the diff is additions only."
means = "file-edit rule: Edit and Write only"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/src/review/schedule.rs tomlctl/tests/review_schedule.rs"
result = "pass"

[[checks]]
command = "grep -q '\"ordered\"' tomlctl/src/review/schedule.rs && grep -q 'ordered' claude/skills/tomlctl/references/review-apply.md"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_schedule::/)'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `tomlctl/src/review/schedule.rs`: a new `interactions` pass runs after `fold`. When there are at least two groups, it loads each chosen finding's run catalogue once (`run_catalogue`, then `routing().compile()`). A run that fails to load is listed in `ordering_skipped` as `{run, reason}`, and its findings are left out on both sides. For each chosen finding whose edit target (`target_file`, else `file`) is not `FileKind::Code`, it reads `edit`, `old`, `fix`, `fix_sketch`, `failure_scenario` and `summary`. When that text names, in full or by a unique basename, an executable edited path of a finding in another group, it records an interaction. `Compose::plan` now works out the depends_on needs first, unchanged, and then calls a new `Compose::order`. That method adds each interaction's edge to a pending doc row once per task pair, skips folded pairs, drops an edge that would close a cycle over the new rows (reported in `dropped_deps` as `{finding, interacts_with, skipped: "cycle"}`), and lists each edge it adds in `ordered` as `{task, needs, finding, names}`. `Compose::row` adds the constraint "Task <n> changes <file>::<symbol>; once it is done, …" for each upstream edge. Both the real output and the dry-run output carry `ordered` and `ordering_skipped`.
- `tomlctl/tests/review_schedule.rs`: `Repo::with_catalogue` records a temp built-in catalogue with a `[routing]` table on run U1. Six new tests cover these cases:
  - the F48/F62-shaped pair, which gets its edge, its constraint and its `ordered` entry
  - an unrelated path, which gets no edge
  - a basename shared by two code findings, which gets no edge
  - a unique basename, which does get an edge
  - a run with no catalogue, which schedules unordered and is listed under `ordering_skipped`
  - an edge that would close a cycle, which goes to `dropped_deps`

  The existing tests are unchanged and pass.
- `claude/skills/tomlctl/references/review-apply.md`: a new **Ordering** bullet in the schedule section, and the printed shape now lists `ordered` and `ordering_skipped`.
- `claude/skills/apply-v2-engine/SKILL.md`: the dry-run shape and the schedule report both cover `ordered`, `ordering_skipped` and cycle-skipped `dropped_deps`.

The first nextest attempt failed to compile on `tomlctl/src/review/triage.rs` (`super::escalation::FIX`), a sibling's in-flight edit. It compiled clean on the retry.
