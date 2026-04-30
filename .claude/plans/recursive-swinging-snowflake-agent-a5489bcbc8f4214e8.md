# Agent 3 — Executability & Clarity Review

Review target: `.claude/plans/recursive-swinging-snowflake.md`
Focus: can an AI agent (or a batch of them) execute each task without having to make unresolved architectural or policy decisions?

## Findings

### [Task 7 / review-plan.md / Step 4.4] CRITICAL — merge mechanism is undefined

Plan says: "Filter to selected severities with non-empty `suggested_edit`. Apply each `suggested_edit` to the specified `plan_section` of the plan file."

This is the single biggest executability gap in the plan. "Apply each `suggested_edit` to the specified `plan_section`" does not say:

1. How `plan_section` is resolved against the plan file. Is it a markdown heading (`## Task 3` / `### 3. ...`)? An anchor regex? A line-range hint? The example shows `plan_section = "Task 3"` — but the plan files use `### 3. optimise.md audit fixes [S]` as the actual heading text, so a literal substring search for `Task 3` would not match.
2. Whether `suggested_edit` is a full replacement, a diff/patch, a prose instruction the agent must interpret, or structured content. The example shows a natural-language instruction (`"Change 'auth.verify_token' to 'auth.validate_token' in Task 3 / Action"`) — which means the merge step is implicitly an LLM reasoning step, not a mechanical apply, yet the plan reads as if it is mechanical.
3. Conflict handling when multiple findings target the same section.
4. Where the `plan_section` anchoring contract gets documented so `/review-plan`'s lens agents know how to populate it.

An agent implementing Task 7 will be forced to invent this contract. Recommend: before Task 7 lands, pick one of {(a) `plan_section` = exact heading text, `suggested_edit` = replacement body for that section; (b) `suggested_edit` = a unified-diff hunk; (c) merge is explicitly an Edit-agent pass that takes findings as prompts and the model figures it out}. Document the chosen mechanism inline in Task 7.

Plan Risks section ("Auto-merge produces a malformed plan file") acknowledges this obliquely but only specifies the *mitigation* (write sibling first), not the *mechanism*.

### [Task 7 / Step 4.7 — "malformed" is not defined] WARNING

"On malformed merge, agent skips Q2 and reports failure." Without a definition of what "malformed" means (TOML parse fail? Markdown file empty? `plan_section` unmatched?), the fallback is judgement-based. Recommend: define two concrete fail conditions — (i) `plan_section` anchor does not match any heading/region in the plan, (ii) revised plan is zero-length or fails a trivial round-trip (header preserved). Anything else = success.

### [Task 1 / Detail] WARNING — exact old/new strings not provided

Plan says "Edit with `replace_all: true` on the old→new shared-block strings (identical across carriers)." But Task 1 bundles three distinct shared-block edits (R48 deletion, `plan_review_findings` addition, `testability` + verified-clean sentence) and gives only prose descriptions. An agent must author the exact strings. That is not impossible — the content is described — but it invites byte-drift across the 8/4 carriers. Recommend: pre-compute the exact diff text (showing the exact before/after for each of the three edits) inside Task 1's detail so the executing agent uses `Write` or a canonical `Edit` string that has already been validated. With `scripts/verify-shared-blocks.sh` as the gate, drift fails loudly — but fixing drift costs a round-trip per carrier.

Note: `Edit`'s `replace_all: true` is per-file, so 8 separate Edit calls (one per file) are required; the plan's "single agent edits all 8 files in one pass" phrasing is compatible with this but could be clearer about N=8 Edit calls.

### [Task 3 / "line 309"] WARNING — line-number anchor is brittle and ambiguous

Plan refers to "the second explanation (~line 309)" for the Optimization Focus dedup. Verified against `claude/commands/optimise.md`: line 298 and line 309 both contain posture prose that largely restates the same framing ("framing, not a closed checklist" at 298; "posture is not exhaustive" at 309). The plan does not quote the exact prose the agent should delete. Two failure modes: (a) the agent could delete the wrong one; (b) if other tasks shift line numbers, 309 will not be the target. Recommend: quote the opening 6-10 words of the duplicate paragraph ("When this section is present, agents should use…") as the textual anchor, not the line number.

### [Task 4 / "Delete lines 360-393"] WARNING — line-range anchor is brittle

Verified: lines 360-393 in `claude/commands/implement.md` are the "Parallel-dispatch anti-pattern" section (heading at 360), the anti-pattern narrative, the Commit-to-completion gate, and the 5-item Pre-send checklist. If any prior task shifts line numbers this deletion range becomes wrong. Task 1 does NOT touch `implement.md` outside shared blocks, and Task 4 is the only task that modifies `implement.md`, so for THIS plan the risk is bounded. But the anchor should be content-based: "delete everything from the `### Parallel-dispatch anti-pattern (the one that keeps happening)` heading at line 360 through the end of Pre-send checklist item 5 immediately before `### Agent dispatch rules`." Same for the Prompt-cache tip reposition — Task 4 should anchor on the literal `**Prompt-cache tip**:` bolded phrase rather than "line 420".

### [Task 6 / "Library enumeration" placement] WARNING — location within Phase 3 unspecified

The plan describes WHAT the library-enumeration sub-step does (orchestrator reads package.json / Cargo.toml / pyproject.toml / etc., cross-references `## Approach`, hands list to research agents in prompt) but does not say WHERE inside Phase 3 the sub-step lives. Before the dispatch block? After exploration but before research agents? As a pre-flight inside each research-agent prompt? An executing agent will have to make this architectural decision. Recommend: "Insert as a new sub-step between Phase 3 exploration-agent return and research-agent dispatch; the orchestrator produces the enumerated list, embeds it verbatim in each research agent's prompt under a `## Libraries to research` header." Making this explicit also lets the acceptance check assert structural placement.

### [Task 2 / "ALL 'four' → 'five' references"] WARNING — "ALL" is not exhaustively listed

Verified via grep that `review.md` contains `four` at lines 40, 424, 442, 444. The plan lists four sites ("dispatch rule, task-tracking, small-diff shortcut prose, design note"). Cross-check:
- Line 40 (`status takes one of four string values`) — unrelated to lens count; MUST NOT be changed.
- Line 424 (`four specialized ones` + `four lenses`) — small-diff shortcut.
- Line 442 (`Launch all four review agents`) — dispatch rule.
- Line 444 (two occurrences of "four") — dispatch rule.

Plan's enumeration mentions "design note" but the current `review.md` design-note area was not verified here to contain "four". An agent could either miss it or mis-replace the line-40 "four string values" line. Recommend: Task 2 should supply the exact grep of sites to change (4 line numbers with 6-word context) AND the exact line(s) to preserve (line 40's unrelated usage). Acceptance criterion `grep -c "Launch \*\*all five\*\*" ≥ 1` catches the dispatch rule; it does NOT catch the small-diff shortcut or task-tracking mentions.

### [Task 8 / acceptance: "≥30 line drop"] WARNING — net-drop target may not be achievable

Task 8 adds a Quick Reference table (8-10 rows), a Common Recipes section (~4-6 patterns), a Sidecar Files consolidated section, a `--verify-integrity` support matrix, plus operator docs for `--where-not`/`--where-missing` and `unset` array prominence. These are significant ADDITIONS. The offsetting removals are the filter-operator prose (-45 + 15 table = -30 lines), threat-model trim (-30 + 10 ≈ -20 lines), and `blocks verify` relocation (shift, not drop). Net drop of ≥30 lines is feasible but tight — the agent may meet acceptance only by aggressively trimming other prose. Recommend: either loosen the acceptance to `≥0 lines` (i.e. "did not grow") or give the agent an explicit budget per added section (e.g. Quick Reference ≤20 lines, Common Recipes ≤60 lines).

### [Dependency graph / batch split] WARNING — plan's own sub-batch classification is inconsistent

Plan says `/implement` should sub-batch into {2, 4, 7} then {3, 5, 6}, with the rationale "three 'L' effort tasks (2, 4, 7) run together first; three 'S/M' tasks run second." But the task headings declare sizes: 2=L, 3=S, 4=M, 5=S, 6=L, 7=L. The three L tasks are **{2, 6, 7}**, not {2, 4, 7}. An agent following the plan literally would dispatch {2, 4, 7} (heterogeneous L+M+L) in batch 1 and {3, 5, 6} (S+S+L) in batch 2 — Task 6 (L) ends up paired with two S tasks, which contradicts the stated balancing rationale. Recommend: correct to {2, 6, 7} + {3, 4, 5}, or drop the L-grouping justification and keep the current pairs with a different rationale (e.g. "Task 4 and Task 7 both modify command-wide dispatch/persistence logic; separate them to reduce cognitive load per batch").

### [Timestamp sensitivity] SUGGESTION — `<today>` may drift across multi-day execution

`last_updated = 2026-04-24` is baked into the plan's schema example, post-approval bootstrap prose, and the Task 7 TOML write steps. If execution spans multiple days, some writes will use the plan's baked date and others will use literal `<today>` substitution. The `/optimise-apply` freshness-gate fix (Task 5) specifically tightens timestamp semantics — inconsistent `last_updated` values between artifacts would undercut that fix. Recommend: every write of `last_updated` uses the literal substitution `<today>` = the day of that specific write, not 2026-04-24 copied from the plan. Add a one-line directive to the "Post-approval execution" section: "All `<today>` tokens resolve to the write-time ISO date, not the plan-approval date — inconsistency across artifacts is intentional and expected."

### [Task 1 scope vs. Task 2 category list] SUGGESTION — confirm Task 1 owns the `testability` addition fully

Verified: the `quality | security | architecture | completeness | db` vocabulary appears in `review.md:182`, `review-apply.md:182`, `optimise.md:182`, `optimise-apply.md:182` (shared-block carriers) AND at `review.md:453` (the agent emission requirements, inside non-shared-block content) AND at `review-apply.md:454` (non-shared-block cluster note). Task 1 will update the shared-block line 182 occurrence to include `testability`. But `review.md:453` is inside Agent 1's brief (non-shared-block territory), so Task 2 must also update it. `review-apply.md:454` is non-shared-block; it's NOT listed under any task's scope. Task 2 covers `review.md`; nothing covers the `review-apply.md:454` update. Recommend: add a line to Task 2 explicitly naming `review.md:453` as a non-shared-block occurrence to update; add a new Task 2.5 (S) to update `review-apply.md:454` or fold it into Task 1's scope. Otherwise `review-apply.md` agents will emit findings with the old category list.

## Summary

Ten findings total. Four are critical-or-warning ambiguities where an executing agent would be forced to make undocumented architectural decisions (F1: merge mechanism; F2: "malformed" definition; F5: anti-pattern deletion anchor; F6: library-enumeration placement). The remaining findings are line-anchor brittleness, acceptance-criteria rigour, task-sizing consistency, timestamp semantics, and scope-completeness in the non-shared-block testability rollout.

Strongest single recommendation: before Task 7 is dispatched, the merge mechanism (F1) must be nailed down with a schema decision and a concrete matcher. The rest of the plan is executable as drafted, with the understood cost of some mid-task agent judgement calls.
