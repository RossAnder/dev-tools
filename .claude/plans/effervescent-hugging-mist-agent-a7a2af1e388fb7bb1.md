# Agent 1 (Feasibility, Codebase Alignment & Dependencies) — review of `effervescent-hugging-mist.md`

## Verifications performed

- Read full plan file (296 lines).
- Read `scripts/shared-blocks.toml` and confirmed: `flow-context` carries 8 commands (matches plan), `execution-record-schema` carries 3 (`plan-new`, `plan-update`, `implement` — matches), `ledger-schema` carries 4 (matches), `apply-*` blocks carry 2 (matches).
- Read `claude/commands/review.md` regions around every cited line. All five lens lines verified in place: 459 (Quality), 484 (Security), 490 (Architecture), 514 (Completeness), 520 (Testability). Line 425 (small-diff shortcut) verified. Line 443 (parallel-launch instruction) verified. Line 532 (closing line of Agent 5) verified — Agent 6 insertion point is correctly identified. Line 183 (review category enum) verified verbatim: `quality | security | architecture | completeness | db | testability | verified-clean`.
- Read `claude/commands/implement.md` head + shared blocks (lines 1-280). Confirmed: NO matches for `RED`, `GREEN`, `TDD`, `test-first`, or `--tests-as-acceptance` — plan's "no TDD mode" claim is correct. Argument-hint frontmatter is `[plan path or task description]` — no explicit `--flow` advertised, but flow-context block (line 71) documents `--flow <slug>` as a valid resolution-step-1 input, so dispatch syntax does work.
- Confirmed via `ls`: no `claude/commands/test-bootstrap.md`, no `claude/commands/test-bootstrap/`, no `claude/commands/tdd.md`, no `claude/skills/test-author/`. All seven Wave-1/Wave-2 new files are greenfield.
- Confirmed `claude/skills/` only contains `tomlctl/` today.
- Verified shared-block sizes: `flow-context` ≈ 90 lines (lines 6-95 in implement.md), `execution-record-schema` ≈ 182 lines (lines 97-278). Combined ≈ 272 lines that /tdd would carry before its own logic. Schema content is generic (type vocabulary covers `task-completion`, `verification`, `deferral`, `checkpoint` — exactly what /tdd's RED/GREEN/REFACTOR FSM needs to write to parent flow). Block fits /tdd's role.
- Tooling versions verified via WebSearch (April 2026):
  - `cargo-mutants` — actively maintained, sourcefrog/cargo-mutants, releases ~every 1-2 months. CURRENT.
  - `cargo-llvm-cov` — 0.8.5 released March 20 2026. CURRENT.
  - `vitest` + `@fast-check/vitest` — official integration package exists, recommended pairing in 2026. CURRENT.
  - `mutmut` — 3.5.0 released Feb 22 2026, supports Python 3.10-3.14. CURRENT.
  - `stryker-mutator` (stryker-js) — actively maintained as of April 14 2026. CURRENT.
  - `gotestsum` — gotestyourself/gotestsum, actively maintained. CURRENT.
  - `gremlins` (go-gremlins/gremlins) — still 0.x with active 2026 issues. CURRENT but pre-1.0 (worth mentioning).
  - `go-mutesting` — original (zimmski) limited maintenance; **avito-tech/go-mutesting** is the maintained fork. Plan should cite the fork explicitly.
  - `pytest` ≥7.0, `pytest-asyncio`, `hypothesis`, `pytest-cov` — all standard, no deprecation flags.

## Findings

```toml
[[findings]]
severity = "warning"
category = "feasibility"
plan_section = "### 9. Add 6th `package-quality` lens to /review [M]"
anchor_old = "**Small-diff shortcut**: If 3 or fewer files are in scope, launch a single comprehensive review agent instead of five specialized ones. Give it all five lenses, all mandatory tool-use requirements (Context7 and WebSearch), the prior findings context, and a cap of 15 findings."
anchor_new = ""
summary = "Task 9 must edit the small-diff-shortcut text at line 425 (currently 'cap of 15 findings') in addition to the three edit locations it lists."
description = "Plan's Task 9 enumerates 3 edit locations: (a) conditional dispatch after small-diff shortcut, (b) ledger enum at line 183, (c) Agent 6 subsection after line 532. But the Risks section explicitly notes the small-diff path will collapse 5+6 into a combined agent with a 20-finding cap (vs current 15). That cap bump requires a fourth edit at line 425 itself — the existing prose says 'cap of 15 findings' and 'all five lenses'. Both substrings drift if Task 9 only adds a sibling conditional. Either add a 4th sub-step to Task 9 or note that the small-diff text gets rewritten in place. The risk is that an agent following Task 9's literal sub-step list (a/b/c only) leaves the cap at 15 and the 'five lenses' wording in place, contradicting the documented behaviour."

[[findings]]
severity = "warning"
category = "feasibility"
plan_section = "### 7. Write `/tdd` command spec [L]"
anchor_old = "/implement dispatch via `Skill(\"implement\", \"<plan-path> --flow <cycle-slug>\")`"
anchor_new = ""
summary = "/implement's argument-hint frontmatter does not advertise `--flow` — works at runtime, but consider also updating implement.md's hint, or document the contract explicitly in tdd.md."
description = "Verified: `/implement` accepts `--flow <slug>` per flow-context resolution step 1 (line 71 of implement.md). However, implement.md's frontmatter argument-hint is `[plan path or task description]` — no mention of `--flow`. The dispatch will work, but if a future contributor refactors implement.md's argument parsing based on the hint, /tdd's dispatch silently breaks. Cheapest fix: add to Task 7's acceptance a smoke check that `/implement <plan-path> --flow <slug>` actually resolves; better fix: extend implement.md's argument-hint in a separate (out-of-scope-for-this-plan) edit, or add a Risks bullet acknowledging the implicit contract."

[[findings]]
severity = "suggestion"
category = "feasibility"
plan_section = "### 5. Write `/test-bootstrap` Go reference [M]"
anchor_old = "`go-mutesting` or `gremlins` (mutation, opt-in)"
anchor_new = "`gremlins` (active 2026; pre-1.0) or `avito-tech/go-mutesting` (maintained fork; original `zimmski/go-mutesting` is unmaintained)"
summary = "Cite the maintained fork of go-mutesting (avito-tech), not the original (zimmski) which has limited maintenance."
description = "WebSearch confirmed: zimmski/go-mutesting (the original) has limited recent maintenance; avito-tech/go-mutesting is the actively-maintained fork. gremlins is 2026-active but still 0.x.x — per its README, only the current minor branch is maintained between releases. The reference file should call this out so users know which import path / install URL to use, otherwise they'll google 'go-mutesting' and land on the unmaintained upstream."

[[findings]]
severity = "suggestion"
category = "feasibility"
plan_section = "### 8. Widen shared-blocks manifest [S]"
anchor_old = "Add `claude/commands/tdd.md` to the `files` array of the `flow-context` block AND the `execution-record-schema` block."
anchor_new = ""
summary = "Verified the right shape — but flag that `tdd.md` will carry ~272 lines of shared-block content before its own logic. Consider whether /tdd genuinely needs the full execution-record-schema verbatim (vs. a one-line reference + the Write contract)."
description = "Verified the proposed shape (8→9 for flow-context, 3→4 for execution-record-schema, exclude ledger-schema, exclude apply-*) is structurally correct and matches how the parity check is enforced. Concern: the `execution-record-schema` shared block is ~182 lines and `flow-context` is ~90 lines (verified by reading implement.md lines 6-278). /tdd carries both = 272 lines of boilerplate before its own FSM content. Plan-update / plan-new / implement carry these blocks because they are *primary writers/readers* of the schema; /tdd is also a writer (per Approach: it appends task-completion + verification entries to the parent flow's record), so the choice IS justified. But it bears explicit acknowledgement in Task 7's detail that this is a deliberate cost — keeps an implementer from trying to slim the carrier and breaking parity. Acceptance criterion 'both shared blocks present byte-identical to canonical' is necessary but does not communicate the *why*."

[[findings]]
severity = "suggestion"
category = "feasibility"
plan_section = "Wave 1 (parallel — 6 files, hits the 6-file batch ceiling exactly):\n  Tasks 1, 2, 3, 4, 5, 6"
anchor_old = ""
anchor_new = ""
summary = "Wave 1's six file paths are non-conflicting (verified). Coexistence of `claude/commands/test-bootstrap.md` (file) and `claude/commands/test-bootstrap/` (directory) is filesystem-legal."
description = "Verified the six Wave-1 paths: claude/commands/test-bootstrap.md (Task 1), claude/commands/test-bootstrap/references/{rust,python,typescript,go}.md (Tasks 2-5), claude/skills/test-author/SKILL.md (Task 6). All are distinct files in distinct directories — no path collisions. The file-vs-directory at `claude/commands/test-bootstrap` is supported by Linux (and the Claude Code harness treats command discovery purely by `.md` filename presence per the plan's own research notes). No race condition on parallel writes — six separate paths. Recording as a positive verification, not an issue. The only possible-confusion path is whether `.md` files inside `claude/commands/test-bootstrap/references/` get accidentally registered as slash-commands by directory walk; the plan's research note 'Discovery is purely by directory presence' suggests not, but Task 1's acceptance should add a smoke check that ONLY `/test-bootstrap` (not `/test-bootstrap/references/rust`) shows up in the slash-command list after Wave 1 lands."

[[findings]]
severity = "suggestion"
category = "feasibility"
plan_section = "### 7. Write `/tdd` command spec [L]"
anchor_old = "Anti-cheat enforcement via SHA256 test-file fingerprint diff (RED→GREEN)."
anchor_new = ""
summary = "Define the 'project test glob' precisely — fingerprint scope is load-bearing for the anti-cheat invariant."
description = "The Approach section says RED captures `red_test_fingerprint = sha256` over 'project test glob' and GREEN requires equality. But 'project test glob' is undefined. If the glob includes test fixtures or generated test files, anti-cheat fires false positives on legitimate fixture refresh; if it excludes too much (e.g. only the new test file's path), an attacker — or a confused agent — can mutate sibling test files to make the new test pass. Suggest Task 7 detail enumerates: (a) the glob is per-language (cargo: `tests/**/*.rs` + `**/*.rs` files containing `#[cfg(test)]`; python: `tests/**/*.py` + `**/test_*.py`; etc.), (b) the glob is captured at RED time and persisted in the cycle sub-flow's context.toml so GREEN re-runs against the same set even if files are added mid-cycle, (c) on mismatch the diff is shown to the user before halt+revert (to distinguish legitimate fixture additions from anti-cheat violations)."

[[findings]]
severity = "suggestion"
category = "feasibility"
plan_section = "### 1. Write `/test-bootstrap` command spec [M]"
anchor_old = "Append marked stack block to target CLAUDE.md"
anchor_new = ""
summary = "Define behaviour when target project has no CLAUDE.md — bootstrap should create one rather than skip the marker block."
description = "Task 1's phase 5 says 'Append marked stack block to target CLAUDE.md'. If the target project has no CLAUDE.md (common for greenfield projects — exactly the audience for /test-bootstrap), the implementer might silently skip this step or error. Spec should state: if CLAUDE.md does not exist, /test-bootstrap creates it with at least the testing-stack block; if it does exist, append. The marker delimiters (`<!-- TEST-BOOTSTRAP:STACK START/END -->`) are designed for the append+idempotent path, but the create path needs explicit treatment. Otherwise the re-run check (which keys on the marker) breaks because the first run never wrote the marker."
```

## Summary

Plan is structurally sound. All cited line numbers verified, shared-block manifest math is correct, all referenced tooling is current as of April 2026 (with one nit on go-mutesting fork). Two warnings worth addressing before /implement:

1. **Task 9's edit list is incomplete** — the small-diff shortcut text at line 425 needs an in-place edit (5→6 lenses, 15→20 cap), not just a sibling conditional. Risks section acknowledges the cap bump but Task 9's three sub-steps don't include it.
2. **/implement's argument-hint doesn't advertise `--flow`** — runtime works, but documenting the implicit contract (or extending implement.md's hint) protects /tdd's dispatch path against future refactors.

Five suggestions cover: the unmaintained `zimmski/go-mutesting` upstream, the ~272-line shared-block carrying cost in tdd.md, the file/directory coexistence at `test-bootstrap` (verified safe; recommend a smoke test in Task 1's acceptance), the underspecified test-file glob for the anti-cheat fingerprint, and the missing-CLAUDE.md bootstrap path in Task 1.

No critical findings. No race conditions in Wave 1 (six distinct paths). Task dependency graph (1-6 || 7→8 || 9,10) is correct given the parity-check gate on Task 8.
