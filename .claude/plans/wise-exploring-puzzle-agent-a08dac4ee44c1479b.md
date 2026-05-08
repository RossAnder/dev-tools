# Review Findings — wise-exploring-puzzle plan (A + B)

Read-only review of `.claude/plans/wise-exploring-puzzle.md`. 8 findings ordered by severity.

## 1. [Plan A Task 1] (content preservation) — "Optionally launch up to 2 Plan agents" paragraph is not explicitly carried forward

Plan A renumbers the current Phase 4 (Design) to Phase 6. Today's `claude/commands/plan-new.md:214-216` contains the "**Optionally launch up to 2 Plan agents**" block (subagent_type: "Plan"). Plan A's Task 1 says "Preserve all existing phase content verbatim except the renumbering and the Phase 1 step-4 edit" but does not call this block out by name, and the "Text changes to plan-new.md" list (lines 107-112) only mentions adding "and user decisions" to the opening sentence of the new Phase 6 — easy for an implementer to miss the Plan-agent block.

**Fix**: In Plan A Task 1's Detail, add an explicit bullet: "Preserve the 'Optionally launch up to 2 Plan agents' subsection at the tail of Phase 6 Design — do not drop or move it."

## 2. [Plan A] (downstream references) — `.claude/plans/command-suite-improvements.md` and `staging/` copies reference old phase numbers

Grep for `Phase [4-8]` across the repo turns up:
- `.claude/plans/command-suite-improvements.md:157,427` — historical plan file, references "Phase 4.5" (implement, not plan-new). Not affected.
- `claude/commands/staging/plan-new.md:71,96,98,131,241` — a staging/shadow copy of plan-new.md with the same phase numbering. This is outside Plan A's declared scope ("plan-new.md only") but the staging copy will drift silently.

No other command files cross-reference `/plan-new`'s Phase 4-6 numbers (Risks section's grep claim is correct for `claude/commands/*.md` at the non-staging level).

**Fix**: Plan A Task 1 should either (a) add `claude/commands/staging/plan-new.md` to the Files list and apply the same restructure, or (b) explicitly document the staging copy is out of scope / stale and note the drift.

## 3. [Plan B Task 1] (shared-block enforcement misread) — `## Flow Context` is NOT a parity-enforced shared block in all 7 files

`scripts/shared-blocks.toml` enforces `flow-context` parity across **only 4 files**: optimise.md, review.md, optimise-apply.md, review-apply.md (confirmed by reading `scripts/shared-blocks.toml:5-12` and `.githooks/pre-commit:6`). The `<!-- SHARED-BLOCK:flow-context START/END -->` markers exist in those 4 files only; plan-new.md, plan-update.md, implement.md, review-plan.md carry the same prose but **without the markers** — so their `[artifacts]` blocks are hand-maintained and currently drift-tolerant.

Plan B Task 1 says "Replicate byte-identically across all 7 files" and claims "the pre-commit hook validates parity". This is false for 3 of those 7 files. The Approach section at line 300 correctly notes this uncertainty ("verify current state; some of these may already be enforced or intentionally exempt"), but Task 1's acceptance ("`scripts/verify-shared-blocks.sh` passes") does not cover the 3 unenforced files.

Also note: `review-plan.md` (a 7th file that has the `[artifacts]` block at `review-plan.md:25-27,49-50,70`) is omitted from Plan B Task 1's file list entirely.

**Fix**: Plan B Task 1 should (a) add `claude/commands/review-plan.md` to the Files list (8 files total), and (b) either extend the `flow-context` entry in `scripts/shared-blocks.toml` to include plan-new/plan-update/implement/review-plan (and add the SHARED-BLOCK markers to those files), or explicitly acknowledge that parity of non-enforced files is manual-only and add an acceptance step that greps all 8 files for the new `execution_record` line.

## 4. [Plan B Task 2] (shared-block infra) — `scripts/shared-blocks.toml` needs SHARED-BLOCK markers added to plan-new/plan-update/implement, not just a manifest entry

Plan B Task 2 proposes adding `execution-record-schema` to `scripts/shared-blocks.toml` covering 3 files. But `verify-shared-blocks.sh:34-40,74-82` requires each listed file to carry `<!-- SHARED-BLOCK:execution-record-schema START/END -->` literal markers around the block; missing-marker is a hard error. Task 2's acceptance says "manifest entry parses correctly" but never mentions inserting the markers.

Additionally, `.githooks/pre-commit:6` hard-codes the regex `^claude/commands/(optimise|review|optimise-apply|review-apply)\.md$` — commits touching only plan-new.md / plan-update.md / implement.md will NOT trigger `verify-shared-blocks.sh` at all under the current gate. If Plan B wants the new schema block policed, the pre-commit regex needs extending to include those three files.

**Fix**: Add two sub-tasks (or expand Task 2's Detail):
- Insert the `<!-- SHARED-BLOCK:execution-record-schema START/END -->` markers around the new section in all 3 files.
- Update `.githooks/pre-commit:6` regex to include `plan-new|plan-update|implement`.
- Acceptance: touch only plan-new.md and verify the hook runs the script (not a no-op).

## 5. [Plan B Task 5/6] (D/DF backward reference breakage) — existing PROGRESS-LOG.md files in archived plans reference D1..Dn literally, which users (and `/plan-update reformat`'s "Preserve existing deviation numbering" rule at `plan-update.md:340`) may still cite

Searching `claude/commands/*.md` shows live instructions that specifically depend on D/DF literal numbering:
- `plan-update.md:148,156`: "Assign the next sequential D-number" / "Assign a DF-number"
- `plan-update.md:231,295-296,304`: example tables with `D1`, `D2`, `DF1`, `Superseded by D25`
- `plan-update.md:340`: "Preserve existing deviation numbering — if deviations already have D-numbers, keep them"
- `plan-update.md:454`: "Bidirectional supersession — When creating a deviation that supersedes an earlier one, always link both directions"

Plan B Task 5 says "D-numbering and DF-numbering are dropped — supersessions use `supersedes_entry = 'E<n>'` field" and Task 7 says to grep for stale "append to PROGRESS-LOG.md" text — but the ACT of replacing line 340's preservation rule and the `Bidirectional supersession` rule at line 454 is not explicitly called out. Plan B Risks line 399 addresses it for the `migrate` op (`legacy_id = "D3"`), but the in-file prose in plan-update.md that teaches users to assign D-numbers needs removal.

**Fix**: Plan B Task 7's Detail should explicitly list the line ranges in plan-update.md that teach D/DF numbering (148, 156, 231, 293-304, 340, 454) and replace/remove each — not just generic "append to PROGRESS-LOG.md" text. Also: `review-plan.md:105` mentions `PROGRESS-LOG.md` as a "Progress/status" tracking document — Plan B should confirm this docstring remains accurate after PROGRESS-LOG becomes a rendered artifact.

## 6. [Plan B Task 4/5] (tomlctl query capability gap) — the plan relies on query flags that tomlctl does not currently expose

Plan B uses these query patterns extensively:
- Task 4: `tomlctl items list --where type=task-completion --where status=done --pluck task_ref`
- Task 5 render routine: `tomlctl items list --where type=deviation` etc. and `--group-by @date:timestamp`
- Task 6: `tomlctl items list execution-record.toml --where type=task-completion --where status=done --pluck task_ref | jq 'unique | length'` ("or equivalent via `tomlctl --count` flag")

Plan B Scope Out-of-scope line 175 says "tomlctl code changes (existing surface suffices)". Checking `tomlctl/src/cli.rs` and `tomlctl/src/items.rs` — the existing `items list` surface supports listing and basic filtering, but `--where`, `--pluck`, `--group-by`, and `--count` are not documented as existing flags. The Exploration Summary (line 35) does not list them. If they don't exist, the plan either needs tomlctl code changes (contradicting its scope) or must fall back to `items list --json | jq '...'` per-query.

**Fix**: Before approval, verify each cited `tomlctl items list` flag exists by reading `tomlctl/src/cli.rs`. If they do not, either (a) move tomlctl additions **into** scope with explicit tasks, or (b) rewrite the plan's Detail sections to use `tomlctl items list --json <file> | jq ...` pipelines (consistent with the CLAUDE.md memory rule: heredoc-stdin for writes; jq for post-processing of reads is fine).

## 7. [Plan B Task 3 / Task 8] (tomlctl test coverage + fixtures) — existing tomlctl test fixtures and integration tests don't cover execution-record semantics; self-dogfood backfill is unaddressed

Two gaps:

(a) `tomlctl/tests/fixtures/context.toml` (read in full — 17 lines) contains the canonical `[artifacts]` block with only 2 keys (`review_ledger`, `optimise_findings`). If Plan B's schema claims `execution_record` is mandatory-after-write, this fixture should be updated, or the integration tests that load it should be audited to confirm they don't assume 2-key `[artifacts]`. Plan B does not mention this fixture.

(b) `.claude/flows/command-suite-improvements/context.toml` on disk (status = "complete") also has the 2-key `[artifacts]`. Plan B's Risks line 393 relies on "absent `[artifacts]` member → compute and write back" — but this flow is COMPLETE, so commands won't re-resolve it (per `review.md:302` and the Completed-flow handling at `plan-new.md:87-89`). The historical PROGRESS-LOG (if any) for that flow will therefore never be back-filled via Task 5's `migrate` op unless explicitly invoked. The question posed in the review prompt ("self-dogfood step... backfilling that flow in scope or intentionally skipped?") is NOT addressed in Plan B.

**Fix**: Plan B Task 8 (end-to-end verification) should add a step: "Run `/plan-update migrate --flow command-suite-improvements` (explicit slug, bypassing completed-flow skip) to exercise the migrate op on a real historical plan, OR document that backfill of completed flows is intentionally out of scope." Also add to Task 2 Detail: "update `tomlctl/tests/fixtures/context.toml` to include the new `execution_record` key so existing test assertions remain consistent."

## 8. [Plan B Approach / Task 6] (`[tasks].in_progress` semantics contradiction) — derivation rule is self-contradictory

Plan B Approach line 293: "`in_progress` = count of task-completion entries NOT present in the log minus the latest status-transition (practical heuristic: the number of TaskCreate entries in-session, minus `completed`)". This is both ungrammatical ("task-completion entries NOT present in the log" while we're counting things in the log) and depends on in-session TaskCreate state that persists nowhere on disk.

Plan B Task 6 then says "Keep `in_progress` best-effort (derivable from active TaskCreate in-session only — note in the shared schema block that it's advisory)" — which contradicts the Approach formula and effectively makes `[tasks].in_progress` undefined across sessions.

Currently (per `implement.md:118,247` and `plan-update.md:144,159,185,440`) `in_progress` is written synchronously: `/implement` increments it in Phase 1, and `/plan-update status` recomputes it. If Plan B's execution record log is the new source of truth, a clean definition is: `in_progress = 1 if /implement is currently running AND not yet written its task-completion entry; else 0` (or more precisely: count plan tasks whose latest event in the log is a started-but-not-completed marker).

**Fix**: Pick ONE definition and document it in Task 2's schema contract:
- **Option A** (simpler): Drop `in_progress` from `[tasks]` entirely — it was always fuzzy — and just keep `total` + `completed`. Document that concurrent-run tracking is not a supported use case.
- **Option B**: Add a new event type `type=task-started` to the 8-value vocabulary; define `in_progress = count of task_refs whose latest event is task-started (no subsequent task-completion)`.

The current Plan B text commits to both the derivation AND the "advisory" handwave; implementers will either implement it inconsistently or skip it silently.

---

## Cross-cutting notes (not findings, per the 10-cap limit)

- **Plan A description frontmatter** (line 2 of plan-new.md): still accurate after restructure — the description "Create a structured implementation plan using parallel exploration, research, and design" remains correct; directed questions are a refinement of the same role. No change needed.
- **CLAUDE.md at repo root**: read in full (on file during session) — references `.githooks/`, `scripts/verify-shared-blocks.sh`, `scripts/shared-blocks.toml`, and lists the 4 parity-checked files. Plan B's Task 2 adding a 5th block (`execution-record-schema`) should include a CLAUDE.md update documenting the new block and its file list, otherwise CLAUDE.md's "block enumeration" prose goes stale.
- **Staging directory** (`claude/commands/staging/`): contains a parallel `plan-new.md` and `implement.md`. Both plans' scopes ignore it. If it's a live mirror, both plans need staging tasks; if dead, the plans should note this.
- **cargo audit**: no impact — confirmed, no new deps implied by Plan B.
- **Append-only TOML log patterns**: `tomlctl`'s existing `items add` / `items apply` / `array-append` cover the append-only semantics; no new library research needed.
