Verdict: The v2 path leans on `export-v1`/`sync-v1` only through prose (engine SKILL.md:80,199,231-232; triage-and-close.md:134-160; both code carriers), and no gate requires either verb. Unplugging triage is doc work plus four native replacements: selection rules (export.rs:236-312), an in-flight status (fields.rs:153), resolved-at-commit writes and an apply-outcome source for `stats --outcomes` (stats.rs:681). Keep both verbs, and keep `sync-v1` at stage 0, because a live store still has 30 `exported` findings.

## Answer

- **No code or gate wires triage to `export-v1`.** Nothing in `tomlctl/src` calls `export-v1` or `sync-v1` from triage, close or `run finish`. The hand-off exists only as prose in the engine skill and the `/review-v2` and `/optimise-v2` carriers. `command_lint` and `flag_table_lint` (lint.rs:433, :2039) fail only on a `tomlctl …` invocation or table row that clap rejects, so removing every mention of `export-v1` breaks no gate. The gate pushes the other way: every native-apply invocation written into the skills must parse against the new verb, so the verb has to land in the same commit as its docs or before them. `carrier_invokes_required_skills` (skills.rs:1616-1645) requires no apply skill for the v2 carriers today. Add one only if the hand-off is written as a skill invocation.
- **Keep the verbs.** The brief's non-goals keep the v1 commands and ledgers. Deleting `export-v1`/`sync-v1` would cascade: CLI types, dispatch, `stats` (which imports `sync`), `FindingStatus::Exported`, the link fields, `retired_links`, the resolve table, the feature list and four test files. That is a separate retirement plan, not this one's.
- **Four things `export-v1`/`sync-v1` do today need a native replacement before the hand-off can move:**
  - selection and refusal;
  - folding merged duplicates into one fix;
  - an in-flight status that triage cannot move;
  - writing the outcome back as `resolved`, which also feeds measurement.
- **`sync-v1` at stage 0 must stay for now.** A live store holds exported findings that only it can resolve.

## Findings

- **Library/API or file**: claude/skills/review-v2-engine/SKILL.md:80 (stage 0 step 4), :199 (stages 6-8 summary), :231-232 (family deltas); references/triage-and-close.md:102, :134-147, :160; claude/commands/review-v2.md:2, :10, :12, :25-26, :63-65; claude/commands/optimise-v2.md:8, :22-23, :96-106
- **Source**: `rg -n 'export-v1|sync-v1|review-apply|optimise-apply'` over claude/, tomlctl/, CLAUDE.md
- **Evidence-grade**: high
- **Finding**: The whole triage-to-v1 hand-off lives in these prose sites. Rewriting them to the native verb and `/implement-v2` is what takes `export-v1` out of the v2 path.
- **Details**: The sites cover the stage 8 steps 3 (Export) and 4 (Apply hand-off), the triage table row `export F1,F4` / `export all`, the close-summary item "what was exported (v1 ids and ledger)", and each carrier's Bindings rows ("v1 export target", "Apply hand-off"). The `/review-v2` frontmatter description also says "exported to the v1 review ledger". Secondary prose that goes stale: CLAUDE.md:38 ("Code and perf findings reach `/review-apply` … through `review export-v1`"), compose.md:152 (settle duplicates "before the export"), and triage-and-close.md:143 ("Export is … the only bridge to glimpse"). That last one stops being true once findings become task rows, which glimpse already shows. claude/skills/tomlctl/SKILL.md:62 and :185 and references/review.md:17 and :481-497 document the verbs themselves and stay while the verbs exist.
- **Counter**: A test that asserts engine or carrier text naming `export-v1` would make this more than doc work. None was found under tomlctl/src/cli/dispatch/tests or tomlctl/tests (`rg 'review-v2.md|triage-and-close|export'` there hits only the skill-invocation list and JS `export const`).
- **Impact on plan / report**: One doc task in the apply-core milestone: engine SKILL.md, triage-and-close.md, the two code carriers, CLAUDE.md and compose.md. It lands with or after the native verb because of `command_lint`.

- **Library/API or file**: tomlctl/src/cli/dispatch/tests/lint.rs:433 `command_lint`, :2039 `flag_table_lint`; scan set lint.rs:17 `command_lint_scan_set`
- **Source**: lint.rs:418-432 doc comment ("an `UnknownArgument` / `InvalidSubcommand` error is a lint failure"; missing-required values are ignored)
- **Evidence-grade**: high
- **Finding**: These gates fail only when docs name a verb or flag the binary lacks. Removing `export-v1` text is gate-neutral, and adding native-apply text forces the verb to exist first.
- **Details**: The scan set covers every `claude/skills/*/SKILL.md` and `references/*.md`, the catalogue entries, `claude/commands/*.md` and `claude/agents/*.md`, plus backtick spans in `claude/workflows/*.js`. Both tests run in the pre-commit `cli::dispatch::tests` subset. The engine's stage 0 capability check (SKILL.md:77) also needs the new feature name added, so a stale installed binary halts instead of failing mid-triage.
- **Counter**: If `command_lint` also asserted that some named verb is mentioned somewhere, removal would fail it. lint.rs has no such reverse check; flag_table_lint's doc comment says "One direction only".
- **Impact on plan / report**: An ordering edge: tomlctl verb → feature in `capabilities` → engine capability check plus docs, in one commit or in that order.

- **Library/API or file**: tomlctl/src/review/export.rs:236-312 (selection, `--all` exclusions, `fn refusal`)
- **Source**: export.rs:257 (`fix_shape != "needs-plan"`), :266-268 (plausible cleanup, test removal), :329-333 (needs-plan, effort large)
- **Evidence-grade**: high
- **Finding**: Native apply needs its own selection with refusals. Today `export-v1` alone decides which open findings may leave triage for apply.
- **Details**: It refuses advisory, the plan family, any status other than `open`, `fix_shape = "needs-plan"` and `effort = "large"`. `--all` also holds back cleanup whose latest verdict is plausible and test removals; an explicit id still exports those. It folds a finding merged into another into one v1 row (compose.md:152). That is the many-findings-one-task case the brief's success criteria want. The `fix_shape` part moves with the fix-shape-reach lens's blocked-on field.
- **Counter**: A native verb that reused `export.rs` selection as a shared function would need no new rules. Its logic is private to export.rs (`fn refusal`, the `Selection` build) and would have to be lifted out.
- **Impact on plan / report**: Lift the selection out of export.rs into a shared module both verbs call, rather than copying it. needs-plan.

- **Library/API or file**: tomlctl/src/review/fields.rs:153 `FindingStatus::is_settled`, :183 `is_syncable`, :230 `LINK_FIELDS`; tomlctl/src/review/resolve.rs:9-12
- **Source**: fields.rs:149-157 ("owned by a v1 ledger since export"), resolve.rs module table
- **Evidence-grade**: high
- **Finding**: `exported` is the only status that marks a finding as handed to apply, so triage cannot move it. Native apply needs an equivalent, plus a `resolved` write at commit, so the resolve table keeps detecting regressions.
- **Details**: resolve.rs maps a re-find of an `exported` target to "repeat" and of a `resolved` target to "regression". Today `sync-v1` performs the `exported`→`resolved` move. Under native apply, the commit stamp has to write `resolved` with its `resolution` companion (fields.rs:146), or no re-found fixed issue ever reads as a regression. The brief also wants a failed or parked task to leave its finding open, so the in-flight marker could be a task link field on an `open` row rather than a new status. Either way, `triage` must refuse to move a finding while its task is live. That choice belongs to the finding-to-task lens.
- **Counter**: Reusing `exported` with a task link in place of `exported_as` would avoid a new status. But `is_syncable` and `sync-v1` would then treat those rows as v1-owned, and `stats --outcomes` would count them as v1 exports.
- **Impact on plan / report**: A field and status decision in apply core, touching fields.rs, triage.rs, resolve.rs and digest.rs:37. needs-plan.

- **Library/API or file**: tomlctl/src/review/stats.rs:24 (imports `sync::{…linked_rows}`), :681 `fn outcomes`; claude/skills/review-v2-engine/references/measurement.md:11, :22, :114
- **Source**: stats.rs:690-697 (counts only findings carrying `exported_as`)
- **Evidence-grade**: high
- **Finding**: `review stats --outcomes`, the apply-outcome and post-verification-leak measure, reads only v1 rows through `v2_ref`. Natively applied findings would be invisible to it unless outcomes also join through task rows.
- **Details**: measurement.md:114's leak threshold is "wontfix + verified-clean share of exported outcomes". Native apply has no wontfix. Its analogues are a failed or parked task, a reproduce-check failure and a blocked route, so the measure needs a native definition. measurement.md:22 also says `sync-v1` runs at every stage 0 so the store is current before stats read it.
- **Counter**: If the plan treats measurement as out of scope for this slice, this costs only a stale metric. But the brief's success criterion on blocked rate per lens and agent in the close summary already needs a task-joined outcome source.
- **Impact on plan / report**: Extend `outcomes` (or add a sibling) to join findings through their task link, and update measurement.md. Without it the measurement goes blind for apply-core output.

- **Library/API or file**: .claude/flows/jiggly-exploring-robin/review-v2.toml (live store); claude/skills/review-v2-engine/SKILL.md:80
- **Source**: `tomlctl review list --slug jiggly-exploring-robin --status exported --select id --lines` → 30 lines; `--status resolved` → 1
- **Evidence-grade**: high
- **Finding**: Keep stage 0's `review sync-v1` as a legacy read-back rather than removing it. A live store holds 30 `exported` findings that no other verb can move, because `exported` is settled.
- **Details**: The sync is a no-op on a store with no `exported_as` links, so leaving it in costs nothing for natively applied stores. `/review-plan-v2.md:37,46` and plan-flow.md:40 skip it for the plan family and need no change. The step's wording should change from "Read v1 apply outcomes" to describe it as legacy.
- **Counter**: If the brief's "`sync-v1` no longer part of it" means stage 0 too, those 30 findings stay `exported` for good unless the plan adds a one-time sync or migration before dropping the step.
- **Impact on plan / report**: A plan decision to record: stage 0 sync stays, and triage alone drops `export-v1`.

- **Library/API or file**: tomlctl/src/cli/types/mod.rs:147-148 (features `review_sync_v1`, `review_export_v1`); tomlctl/tests/capabilities.rs:146, :157, :1789-1790; tomlctl/tests/output_options.rs:761, :870; tomlctl/tests/review_export.rs; tomlctl/tests/review_sync.rs
- **Source**: rg sweep above
- **Evidence-grade**: high
- **Finding**: These tests pin the two verbs' existence and output shape, not their place in the v2 path. Keeping the verbs leaves all of them unchanged. Deleting the verbs would mean editing every one of them, plus `stats.rs`'s `sync` import.
- **Details**: The v1 apply carriers (`claude/commands/review-apply.md` and `optimise-apply.md`, pinned at skills.rs:1563 and :1577) and the v1 command names in flow/envelope.rs:31-32, capabilities.rs:55-56 and inputs.rs:1114 belong to v1 and are untouched under the brief's non-goals.
- **Counter**: A plan that also retires the verbs makes this a cascade across the type, dispatch, status, stats and test files above. Nothing in the brief asks for that.
- **Impact on plan / report**: No change if the verbs are kept. Record "verbs retained for v1 users and legacy stores" as a decision.

Outside this lens: `v1_category` (catalogue.rs:111, store.rs:171, export.rs:448) is read by `export-v1` and by the measurement baseline (measurement.md:79), so it outlives the hand-off.

Coverage: read in full the brief and the relevant ranges of engine SKILL.md (70-100, 190-235), triage-and-close.md (90-160), review-v2.md (1-30, 55-75), optimise-v2.md (15-30, 90-110), lint.rs (17-60, 410-480, 1960-2080), skills.rs (1595-1760), fields.rs (105-235), resolve.rs header, stats.rs header and `outcomes`, export.rs header and outline, and the capabilities and output_options test sites. Checks run: an rg sweep for `export-v1|sync-v1|export_v1|sync_v1|review-apply|optimise-apply` over claude/, tomlctl/src, tomlctl/tests, scripts, .githooks, CLAUDE.md and the README, and a `tomlctl review list` count of exported findings in the live store. Ruled out: a gate asserting the hand-off text (none exists); glimpse code depending on the v2 store (it reads v1 ledgers and task rows only, triage-and-close.md:143); review-plan-v2 and plan-flow (they already skip `sync-v1` and never export).
