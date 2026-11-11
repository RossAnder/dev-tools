Verdict: a worker could carry out milestone B as written except for task 11's `review triage --addressed` flag shape, so 1 unactionable point and 2 false premises (one material, in task 11; one minor, in task 14's Outcome).

# Cold read: milestone B (tasks 8 to 17)

## Unactionable

### U1. Task 11: the shape of the `--addressed` mode is not settled

Task 11 Detail: "`--addressed` sets status `resolved`, takes `--reason-file` like the other triage modes and `--targets`, which defaults to each finding's own `target`." `design/shapes.md#addressed` writes it as `review triage --addressed <id>[,<id>...] --reason-file <path> [--targets <t>[,<t>...]]`.

The worker still has to work out three things:
- **Where the ids go.** `ReviewTriageArgs` takes the finding ids as a required positional (`tomlctl/src/cli/types/review/triage.rs:11-19`). Shapes puts the ids in `--addressed`'s value instead. Is `--addressed` a bool beside the positional ids, or does it carry the ids?
- **What carries the reason.** Each prose mode in `ReviewTriageMode` (`triage.rs:191` onward) is a value flag that carries its reason, with a `-file` twin: `--resolve <RESOLUTION>` / `--resolve-file <PATH>` (`triage.rs:231`, `:237`), and likewise for dismiss, defer, reopen and park. No mode takes `--reason-file`, and the flag does not exist (see F1). The worker must choose between a bool `--addressed` with a new `--reason-file` flag, and an `--addressed <TEXT>` / `--addressed-file <PATH>` pair that follows the twin convention. `triage-and-close.md:5` tells the orchestrator to pass prose only through those twins.
- **Where the reason is stored.** The plan does not say whether the reason fills the `resolution` companion that `Resolved` already owns (`tomlctl/src/review/fields.rs:146`).

Task 16 writes the triage grammar row and task 17 the flag table, both from whatever task 11 picks. So the choice spreads to three tasks, and the plan should settle it.

## False premises

### F1. Task 11: "takes `--reason-file` like the other triage modes"

No triage mode takes `--reason-file`. `rg 'reason.file|reason_file' tomlctl/src/cli/types/review/ tomlctl/src/review/triage.rs` prints nothing. The modes use per-mode twins, as `ReviewTriageMode` at `tomlctl/src/cli/types/review/triage.rs:191-260` shows (`--dismiss-file`, `--defer-file`, `--reopen-file`, `--park-file`, `--resolve-file`, `--answer-file`). `design/shapes.md#addressed` repeats the same flag name. This premise is the source of U1.

### F2. Task 14 Outcome: "`review reproduce --by checker` records a reproduction"

`review reproduce` has no `--by` flag. `ReviewReproduceArgs` (`tomlctl/src/cli/types/review/verdicts.rs:134-146`) takes only the store flag, `--ndjson` and integrity, and `by` is a field of each ndjson line. `reproduce.rs:157` reads `by` from that text and calls `ReproduceBy::parse`. The task's Detail ("parse it", "upserts on the same key") describes the code correctly, so the risk is small. Still, a worker reading the Outcome literally could add a `--by` flag that no other task documents. Read it as `"by": "checker"` in the ndjson.

## Premises checked and found true

- Task 8: `fn edited` is at `schedule.rs:381` and feeds `fold` (`:394`) and `Compose::row` (`:608`). `file_notes` with `"(new)"` is already written for settle rows (`:567`). `Finding::edited_paths` (`store.rs:406`) has the two other callers the constraint names: `select.rs:180` (inside the test-removal hold-back, `select.rs:62`/`:164`) and `fields::route` (`fields.rs:1426`). `Finding` keeps unknown keys in `rest`, so `schedule.rs` can read `fix_files` without editing `store.rs`, which is not in the task's files. The field builder has `.kinds(DEFECT_OR_CLEANUP)` and `.items(n)`. The role matrix sits in `fields.rs` tests (`MATRIX`, about line 1716) with column order given at `fields.rs:522`. `fix_files` needs a new `Vis` array, because `EDIT_DETAIL` (`fields.rs:537`) hides fields from the finder and is shown to the verifier only when the finding is routed to it. That is derivable, not a gap.
- Task 9: `cited`, `citation_holds` and `named_paths` are at `tier.rs:547`, `:564` and `:583`. `cited` gets `row`, so `new` is in reach. `slug` is already passed down, so `docs/flows/<slug>/` is computable. `stamp` already freezes a stored tier.
- Task 10: the refusal string is at `apply.rs:325`. `by = "user"` under `--user-accepted` is at `apply.rs:229`. A non-defect/cleanup kind is refused at `apply.rs:303`.
- Task 11: `ALL_COMPANIONS` (`fields.rs:231`) is derived from `FindingStatus::companions()`, so adding `addressed` to `Resolved`'s list (`fields.rs:146`) is enough, plus the hard-coded list in the test at about `fields.rs:1931`. `.plan_only()` exists (`fields.rs` builder). `applied.target` is written at `apply.rs:734`. Dispatch is `ReviewOp` in `cli/types/review/mod.rs:24` and `review/dispatch.rs`.
- Task 12: `ReviewDigestArgs` is at `cli/types/review/read.rs:95`, and the keys are inserted at `digest.rs:159-160`. No Workflow script reads either array (`rg` over `claude/`); only prose does, and task 16 covers that.
- Task 13: the `ReviewAddArgs` and `ReviewCoverArgs` probes hold. `cover` refuses through `fields::check_cap` at `cover.rs:129`. `prepare` (`add.rs:290`) and `finish_row` (`add.rs:800`) exist.
- Task 14: `str_enum!(ReproduceBy { Verifier, Orchestrator })` is at `fields.rs:252`. `store.rs:975` and `reproduce.rs:157` parse it.
- Task 15: the sweeper's step 6 (`NAMES`) and step 12 ("600 is the hard limit") exist. The method skill's Length section says the limit is "refused after the work is done" (`SKILL.md:185`). The finder and finder-cf bodies are byte-identical today. `plan view --for human` is a valid role.
- Task 16: `plan-flow.md` sections 1, 7 (Advisory), 10 and 11 exist, as do the triage-and-close grammar table (`:119` onward) and plan-review-lead settle steps 3 and 4 (`plan-review-lead.md:94`, `:101`).
- Task 17: the feature list, `capabilities.rs`, README and the SKILL.md feature table all carry existing tokens such as `review_settle`. `output_options.rs` has `every_leaf_command_has_a_case` (`:2227`). Needs 5 and 2 are milestone A rows that edit `claude/skills/tomlctl/SKILL.md` and `add.rs`.
- Sequencing: the shared files are ordered by `needs`. `fields.rs` goes 8 → 11 → 14, `read.rs` goes 11 → 12, and `add.rs` goes 2 → 13. Tasks 15 and 16 have no files in common.

## Premise probes (run on the present tree)

Every `Premises` command for tasks 8 to 14 exits 0: `fn edited`, `citation_holds`/`named_paths`, the advisory refusal string, `ALL_COMPANIONS`, `ReviewDigestArgs`, `ReviewAddArgs`/`ReviewCoverArgs`, and `str_enum!(ReproduceBy`. Tasks 15, 16 and 17 have no premises.

## Check commands

I ran every accept check on the present tree. Each one exited 1 (none of the work is done yet):

| Task | Check | rc |
|---|---|---|
| 8 | `grep -q 'fn fix_files_widen_the_minted_row' tomlctl/tests/review_schedule.rs` | 1 |
| 8 | `grep -q 'fn fix_files_new_path_is_noted_new' tomlctl/tests/review_schedule.rs` | 1 |
| 9 | `grep -q 'fn compound_citation_holds' tomlctl/src/review/tier.rs` | 1 |
| 9 | `grep -q 'fn citation_of_a_path_in_new_holds' tomlctl/src/review/tier.rs` | 1 |
| 9 | `grep -q 'fn citation_of_a_flow_document_holds' tomlctl/src/review/tier.rs` | 1 |
| 10 | `grep -q 'fn user_accepted_applies_advisory_tier_edit' tomlctl/tests/review_apply.rs` | 1 |
| 10 | `grep -q 'fn advisory_tier_edit_refused_without_user_accepted' tomlctl/tests/review_apply.rs` | 1 |
| 11 | `grep -q 'fn triage_addressed_records_targets' tomlctl/tests/review_triage.rs` | 1 |
| 11 | `grep -q 'fn reopen_clears_addressed' tomlctl/tests/review_triage.rs` | 1 |
| 11 | `grep -q 'fn recheck_targets_union_applied_and_addressed' tomlctl/tests/review_triage.rs` | 1 |
| 11 | `grep -q 'fn plan_outcomes_count_author_addressed' tomlctl/tests/review_stats.rs` | 1 |
| 12 | `grep -q 'fn digest_candidates_are_counts_by_default' tomlctl/tests/review_digest.rs` | 1 |
| 12 | `grep -q 'fn digest_with_prints_named_arrays' tomlctl/tests/review_digest.rs` | 1 |
| 13 | `grep -q 'fn cover_cuts_an_over_cap_note' tomlctl/tests/review_cover.rs` | 1 |
| 13 | `grep -q 'fn add_dry_run_writes_nothing' tomlctl/tests/review_add.rs` | 1 |
| 14 | `grep -q 'fn reproduce_accepts_by_checker' tomlctl/tests/review_reproduce.rs` | 1 |
| 15 | `grep -q 'fix_files' claude/skills/review-v2-method/SKILL.md` | 1 |
| 15 | `grep -q 'fix_files'` on both finder agents | 1 |
| 15 | `grep -q 'plan view --slug' claude/agents/review-sweeper.md` | 1 |
| 16 | `--addressed` and `recheck-targets` in plan-flow.md | 1 |
| 16 | `--with duplicates` in plan-review-lead.md | 1 |
| 16 | `--addressed` in triage-and-close.md | 1 |
| 17 | the five tokens in `tomlctl/src/cli/types/mod.rs` | 1 |
| 17 | `recheck-targets` and `--addressed` in references/review.md | 1 |
| 17 | `recheck-targets` in tomlctl/tests/output_options.rs | 1 |

Each check separates the present tree from a finished one. Most only look for a test function's name, so a stub test would pass them. The behaviour is proven by the narrow nextest runs and the milestone gate, not by these checks. That is a known limit, not a defect.

Task 15's `bash scripts/verify-shared-blocks.sh` prints `shared-block parity: OK` on the present tree. I did not run `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (tasks 15, 16 and 17) or any narrow `cargo nextest` run, because they build the crate and building is left to the orchestrator under the build discipline. Tasks 15 and 16 already say the dispatch subset fails until tasks 10 to 13 land.

## Coverage

- **Tasks:** 8 to 17. For each I read the worker view and `tasks show --select id,files`.
- **Design doc:** `docs/flows/v2-harness-tuning/design/shapes.md`, which the tasks cite.
- **Code read at the symbols checked:** `tomlctl/src/review/{schedule,store,select,fields,tier,apply,cover,add,digest,reproduce,dispatch}.rs`, `tomlctl/src/cli/types/review/{triage,verdicts,read,cells,mod}.rs` and the `tests/output_options.rs` leaf-case test.
- **Prose read:** `claude/skills/review-v2-method/SKILL.md`, `claude/agents/review-sweeper.md`, the finder pair (parity diff only), `claude/skills/review-v2-engine/references/{plan-flow,triage-and-close,compose}.md`, `claude/skills/review-v2-engine/SKILL.md` and `claude/agents/plan-review-lead.md`.
- **Not checked:** whether `review stats --plan-outcomes` has a natural slot for `author-addressed`; I read only its `dismissed` and `resolved` counting.
- **Not read:** the other notes under `notes/` and any earlier cold-read note.
