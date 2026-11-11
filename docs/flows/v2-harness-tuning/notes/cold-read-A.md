Verdict: a worker could carry out milestone A as written apart from one point: task 2 does not mention an existing test that pins the old behaviour. 1 unactionable point, 0 false premises.

# Cold read: milestone A (tasks 1-7)

## Unactionable

### Task 2: an existing test pins the behaviour the task reverses, and the plan does not say so

The Action says: "Widen the `superseded` filter in `review add` so a replay also supersedes the same cell and run's advisory-kind rows whose status is `open`." The test `resubmitted_cell_supersedes_unverified_rows` in `tomlctl/tests/review_add.rs` (lines 430-458) asserts the opposite:

- `assert_eq!(second["superseded"], json!(["F3"]));` (line 443)
- `assert_eq!(repo.finding("F4")["status"].as_str(), Some("open"), "only unverified rows are withdrawn");` (lines 446-450), where F4 is the cell's `advisory(None)` row, minted `open` (line 439).

Once the change lands, that test fails: `superseded` becomes `["F3","F4"]` and F4 becomes `superseded`. The task's narrow run is `-E 'test(replayed_add_supersedes_open_advisory)'`, so the worker never sees the failure, and the milestone gate does. The file is in the task's `files`, so the worker may edit it. The plan still has to settle whether to rewrite this test's assertions, or delete it in favour of the new `replayed_add_supersedes_open_advisory`. Neither the Detail nor the Constraints mention it. Fix: add one sentence to the Detail saying that `resubmitted_cell_supersedes_unverified_rows` is updated to expect F4 superseded, or that it is replaced.

## False premises

None. Every premise and named symbol was checked against the tree:

- Task 1: `verdict_return()` emits `"allOf": rules` (`tomlctl/src/review/schema_out.rs:326`). The only test that reads the verdict schema's `allOf` is `verdict_return_and_validator_agree_on_unblocking` (`schema_out.rs:535-536`). Its `refused(...)` asserts go through `validate_row(Record::Verdict, ...)`, which bears out D5's claim that the pairing is enforced at write. The bare call sites D5 names exist: `claude/workflows/review-v2.js:220,226` and `claude/workflows/plan-v2-review.js:213`.
- Task 2: the filter is `f.status() == FindingStatus::Unverified` (`tomlctl/src/review/add.rs:146`). An advisory with no suppressing target is minted `FindingStatus::Open` (`add.rs:851`). `Finding` has a `kind: Kind` field (`tomlctl/src/review/store.rs:191`), so the widened filter can key on kind.
- Task 3: `own_work` filters on `Done | InProgress | Failed | Parked` and an empty `commit` (`tomlctl/src/plan/drift.rs:216-228`). `TaskRow` has `agent: String` and `commit: String` (`tomlctl/src/tasks/schema.rs:367-368`). The test module (`drift.rs:293`) has no store helper. `TaskRow` does not derive `Default`, so the worker has to build rows by hand; the `row()`/`store()` helpers in `tasks/check.rs`'s tests show how.
- Task 4: `overlap_findings` (`tomlctl/src/tasks/check.rs:639`) maps every `graph.overlap_pairs()` pair with no status filter. `plan check` reaches it through `crate::tasks::check::check` (`tomlctl/src/plan/check.rs:21`). `Graph::overlap_pairs` has another caller in `tasks/edges.rs:114,117`, which is why the Detail puts the filter in `overlap_findings`. The existing check.rs unit test and the `tasks_graph.rs` GRAPH_FIXTURE pair (tasks 2 and 3) have no done+committed row, so they keep warning.
- Task 5: `validate_row_paths(rows, header, t.paths(), "--template")?` is at `tomlctl/src/output.rs:444`. `validate_row_paths` returns `Ok` on the first matching path (`tomlctl/src/convert.rs:347-353`). The cited `design/shapes.md#template-paths` section exists (`docs/flows/v2-harness-tuning/design/shapes.md:137`, untracked but present in the tree) and gives the exact warning text. `OutputOpts.json_errors` already gates a stderr notice (`output.rs:889-892`), which covers the `--error-format json` constraint. The "Unknown paths" paragraph exists at `claude/skills/tomlctl/SKILL.md:141`.
- Task 6: the role rule is the one-line bullet at `claude/skills/apply-v2-engine/SKILL.md:14`, and the `**Unfinished plan work.**` bullet is at line 102.
- Task 7: the two "block per pass" demands are `claude/agents/verification.md:129` ("ALWAYS emit one report block per command…") and `:191` ("Every block MUST appear…"). Line 144 ("One block per attempted command") and Step 4 at line 111 also say "block" and need matching rewording. The worked example starts at line 158.

## Minor observations (not counted)

- Task 1: "rewrite the tests in the same file that read `schema["allOf"]`" also matches line 471 (`schema_and_validator_agree_on_blocked_detail`), which reads the *finding* schema's `allOf`. The constraint "Leave the batch schema and every other schema … as they are" makes clear only the verdict test at line 536 is meant, so a careful worker gets this right.
- Task 7: the current worked example is pass, then flaky, then fail. The Detail asks for "the line form above a full fail block" and does not say whether the flaky block stays. The constraint keeping flaky blocks' fields suggests it does.

## Check commands

Premises (all must hold at base):

- `grep -q '"allOf"' tomlctl/src/review/schema_out.rs` → exit 0
- `grep -q 'f.status() == FindingStatus::Unverified' tomlctl/src/review/add.rs` → exit 0
- `grep -q 'fn own_work' tomlctl/src/plan/drift.rs` → exit 0
- `grep -q 'fn overlap_findings' tomlctl/src/tasks/check.rs` → exit 0
- `grep -q 'validate_row_paths(rows, header, t.paths(), "--template")' tomlctl/src/output.rs` → exit 0

Accept checks (all forward; each fails on the present tree for the missing change, as it should):

- T1 `grep -q 'fn verdict_return_has_no_top_level_combinator' tomlctl/src/review/schema_out.rs` → exit 1
- T2 `grep -q 'fn replayed_add_supersedes_open_advisory' tomlctl/tests/review_add.rs` → exit 1
- T3 `grep -q 'fn started_row_reset_to_pending_is_own_work' tomlctl/src/plan/drift.rs` → exit 1
- T4 `grep -q 'fn unreachable_claim_skips_done_committed_rows' tomlctl/tests/tasks_graph.rs` → exit 1
- T5 `grep -q 'fn template_unknown_path_warns' tomlctl/tests/output_trim.rs` → exit 1
- T5 `grep -q 'fn template_optional_path_is_quiet' tomlctl/tests/output_trim.rs` → exit 1
- T6 `grep -E 'Triage decided the selection' claude/skills/apply-v2-engine/SKILL.md | grep -qi 'unfinished'` → exit 1. This depends on the rule staying on one line, which the Detail requires ("Keep the rule on one bullet").
- T7 `grep -q 'outcome: pass | command:' claude/agents/verification.md` → exit 1

The accept checks are name-only greps. Each shows that a test exists, not that it passes, so the behaviour is proven only by the narrow runs and the milestone gate. I did not run the narrow `cargo nextest` commands or `cli::dispatch::tests`. They compile the crate, and the read-only rules leave builds to the orchestrator.

## Coverage

Read the worker views and `files` for tasks 1-7. Read the relevant parts of `tomlctl/src/review/schema_out.rs`, `tomlctl/src/review/add.rs`, `tomlctl/tests/review_add.rs`, `tomlctl/src/plan/drift.rs`, `tomlctl/src/tasks/check.rs`, `tomlctl/tests/tasks_graph.rs`, `tomlctl/src/convert.rs`, `tomlctl/src/output.rs`, `claude/skills/tomlctl/SKILL.md`, `claude/skills/apply-v2-engine/SKILL.md`, `claude/agents/verification.md`, and `docs/flows/v2-harness-tuning/design/shapes.md` (the Template paths section). Did not open `tomlctl/src/output/template.rs` or `tomlctl/tests/output_trim.rs` beyond confirming they exist. Did not check task 5's seed survey of the optional-field table: which documented template placeholders name an omittable field.
