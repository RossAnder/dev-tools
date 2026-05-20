# Plan: Progressive-Disclosure Propagation — Wave 1 (plan/implement carriers)

**Plan path**: `docs/plans/harness-progressive-disclosure-wave-1.md`
**Flow slug**: `harness-progressive-disclosure-wave-1`
**Created**: 2026-05-20
**Status**: Draft

## Context

The pilot (`docs/plans/harness-progressive-disclosure.md`) migrated `claude/commands/review.md` from 777 → 62 LOC by externalising its four shared blocks into `claude/skills/flow-contract-*/SKILL.md` and replacing inline Step-0 envelope prose with `tomlctl flow envelope build`. The gate-tooling plan (`docs/plans/harness-progressive-disclosure-gate-tooling.md`) then landed the three drift checks that PILOT-LESSONS §§10/13/14 made hard prerequisites for bulk propagation: `tomlctl blocks verify-skills` (skill↔carrier drift), the `command_lint` cargo test (carrier↔CLI flag drift), and the fixtures-read-manifest refactor (single-source carrier lists).

Both prerequisites are confirmed in place (4 pilot skills present with 2-field frontmatter; `verify_skills`/`normalise_block`/`verify_skills_clean`/`command_lint` all in `tomlctl`; `flow envelope build` whitelist covers all five plan/implement carrier names; manifest carries `skill =` fields on the four pilot blocks). The mechanism is **validated live** (2026-05-20 `/review` run). This plan executes the gate-tooling plan's documented next step:

> Wave 1 — extract `flow-contract-execution-record-schema` + `flow-contract-plansdirectory-prompt`, migrate plan-new/plan-update/review-plan/implement/tdd.

**Scope refinement from the user (2026-05-20):** `plan-update` is deferred to a separate overhaul the user has planned independently. This wave therefore migrates **four** carriers — `plan-new`, `review-plan`, `implement`, `tdd` — leaving `plan-update` as the sole remaining embedder of both new blocks. That refinement is what keeps every drift check green with **zero Rust changes** (see Constraints).

## Scope

- **In scope**: Externalise two shared blocks into new skills (`execution-record-schema`, `plansdirectory-prompt`); extract `plan-new`'s embedded plan-output-format template into a third skill; rewrite four carriers (`plan-new`, `review-plan`, `implement`, `tdd`) to thin skeletons; shrink the four affected manifest blocks and add `skill =` fields to the two newly-externalised entries.
- **Out of scope**: `plan-update` (deferred — separate user-led overhaul); the `apply-*` wave (`review-apply`, `optimise-apply`); `optimise` and `test-bootstrap`; the §12 reference-rewrite of the two new skill bodies (deferred to plan-update's overhaul — see Approach §E); any change to `tomlctl` Rust source (none needed); deleting the `execution-record-schema` / `plansdirectory-prompt` manifest entries (they retain `plan-update` and so are not yet fully migrated).
- **Affected areas**: `claude/commands/` (4 carriers rewritten), `claude/skills/` (3 new skills), `scripts/shared-blocks.toml`, `CLAUDE.md` (stale-reference refresh).
- **Estimated file count**: 9 unique files (under the 15-file guard; largest agent batch ≤ 5 isolated files).

## Research Notes

Research for this wave is codebase-internal and was re-confirmed by fresh exploration (2026-05-20):

- **Whitelist confirmed** — `tomlctl/src/flow/envelope.rs:24-35` `VALID_COMMANDS` includes `plan-new`, `plan-update`, `implement`, `review-plan`, `tdd`. The skeleton `flow envelope build --command <name>` calls are safe at runtime and structurally lintable.
- **Partial-migration keeps fixtures green automatically** — `blocks_verify_reproduces_shell_hashes` (`dispatch.rs:1394`) now derives carrier lists from the manifest via `carriers_for(name)` (`dispatch.rs:1424`) and asserts each block's pinned content-hash. Shrinking a block's `files` list does **not** change the block's content hash (remaining copies are byte-identical), so `flow-context` (8→4 carriers, pinned `d837b01f…`) and `execution-record-schema` (4→1 carrier `plan-update`, pinned `1935f8dd…`) both still reproduce. The graceful-skip guard (`dispatch.rs:1454`) only short-circuits on *absent files*, not empty lists — but neither list goes empty this wave, so the test runs and passes. **No `dispatch.rs` edit required.** (The local binding names `flow_context_eight` / `execution_record_four` become numerically stale; an optional cosmetic rename is noted as a non-blocking follow-up.)
- **`verify_skills_clean` stays green with verbatim extraction** — it compares `normalise(skill body, frontmatter-stripped)` against `normalise(extract_block(carrier))` for every manifest block carrying a `skill =` field with a non-empty `files` list (`dispatch.rs:1641`, engine `blocks.rs:384`). Because the new skill bodies are extracted verbatim and `plan-update` keeps its byte-identical embedded copies, the two sides match pre-normalisation — so the check passes without touching `normalise_block`.
- **Why verbatim, not §12-rewritten, this wave** — `normalise_block` (`blocks.rs:338`) drops whole *lines* matching the cross-reference patterns (`SHARED-BLOCK:`, backtick `flow-contract-`, embedder-sentence). The two new blocks' cross-references (`see the `## Flow Context` section`, `see `flow-bootstrap.md` Contract`, `shared verbatim across …`) sit **on the same physical lines as substantive content** and do not use `SHARED-BLOCK:` notation. A §12 rewrite would therefore either (a) break byte-parity with `plan-update`'s un-rewritten copy → false drift, or (b) require dropping a whole substantive line → masking real drift (gate-tooling Risk 1). Deferring the rewrite until `plan-update` migrates (both blocks fully externalise, comparison constraint disappears) is the clean resolution.
- **`command_lint` unaffected by content moves** — it globs `claude/commands/*.md` AND `claude/skills/flow-contract-*/SKILL.md`. The `tomlctl` invocations in the two blocks (`items add`, `items list … --verify-integrity`, `json set … --json -`) already pass today inside the carriers; moving them verbatim into globbed skill files leaves the lint result unchanged. The skeletons' new `flow envelope build` calls mirror the validated `review.md` form.

### Sources
- `docs/plans/harness-progressive-disclosure/PILOT-LESSONS.md` §§5, 6, 11, 12, 13, 14
- `tomlctl/src/{flow/envelope.rs, blocks.rs, cli/dispatch.rs}`, `scripts/shared-blocks.toml`, `claude/commands/{plan-new,plan-update,review-plan,implement,tdd}.md` (fresh exploration 2026-05-20)

## User Decisions

> Phase 4 user-engagement gate — answers captured 2026-05-20. Authoritative.

### Q1 — Wave scope
**Chosen: All five as documented, minus `plan-update`.** The user confirmed the documented Wave-1 set but added: *"Leave plan-update for now since I have some plans to overhaul this separately."* Net wave = `plan-new`, `review-plan`, `implement`, `tdd`. Deferring `plan-update` also keeps it as the residual embedder that lets the drift checks stay meaningful (no empty `files` lists → no `dispatch.rs` churn).
> Prompted by: gate-tooling "Next Steps" + the partial-migration parity math.

### Q2 — LOC ceiling for the large planning carriers
**Chosen: Relax the ceiling, with careful consideration of what moves to skills.** User: *"Relax ceiling but give careful consideration to how much should move into skills."* `review-plan` / `implement` / `tdd` hold the ≤100 LOC ceiling. `plan-new` gets a relaxed ceiling (~150 LOC) and one deliberate extraction beyond the shared blocks: its ~88-line embedded plan-output-format template (consulted only at Phase 7) moves to `flow-contract-plan-output-format`. Phase prose stays inline as compressed 1-paragraph summaries (skim-readability), not externalised.
> Prompted by: `plan-new` (672 LOC) carries 10 phases + a large output template that are carrier-specific prose, not shared blocks.

### Q3 — Live smoke-run validation
**Chosen: User tests once all are converted.** User: *"I will test once all are converted."* The automated `cargo test` (verify-skills, command_lint, fixtures) + `verify-shared-blocks.sh` parity script are the hard gate run during execution. The PILOT-LESSONS §11 live smoke-run is a single post-conversion pass the user performs across all four carriers — not per-carrier blocking tasks in this plan.
> Prompted by: slash commands fire from the interactive session, not from the planning/implement agent context.

### Phase 5 outcome
**Skipped.** Phase 4 answers introduced no unresearched topic — all three decisions are strategic/codebase-internal and fully covered by exploration + PILOT-LESSONS. No library or API was introduced.

## Approach

### A. Two new shared-block skills (verbatim extraction + 2-field frontmatter)

Create `claude/skills/flow-contract-execution-record-schema/SKILL.md` and `claude/skills/flow-contract-plansdirectory-prompt/SKILL.md`. Each body is the verbatim block content extracted from a current carrier (markers stripped), prepended with the **2-field** frontmatter convention (`name` + `description` only — the on-disk reality per PILOT-LESSONS §1; no `when_to_use` / `user-invocable` / `disable-model-invocation`). Descriptions run ~500–1500 chars, single long line combining what the contract defines and when to consult it (matching the four pilot skills).

- Extract `execution-record-schema` from `claude/commands/implement.md` (block at lines 81–266; ≈16,774-byte body).
- Extract `plansdirectory-prompt` from `claude/commands/plan-new.md` (block at lines 83–103; ≈3,691-byte body).

**No §12 reference-rewrite this wave** (see Research Notes + §E). Bodies are byte-identical to `plan-update`'s still-embedded copies so `verify_skills_clean` passes without a `normalise_block` change.

### B. plan-new output-format skill (carrier-prose extraction)

Create `claude/skills/flow-contract-plan-output-format/SKILL.md` from `plan-new`'s embedded `# Plan: {Descriptive Title}` … through the `## Risks` template block plus the "Format rules" list (currently inline in Phase 7). This was **never a `SHARED-BLOCK`**, so it has no manifest entry and no parity/verify-skills coupling — it is a pure `plan-new`-local refactor and the body may be lightly polished for standalone clarity. `plan-new`'s Phase 7 then carries a one-paragraph summary + `Invoke the `flow-contract-plan-output-format` skill to load the plan document structure and format rules before writing.`

### C. Carrier skeletons

Rewrite each carrier to the pilot skeleton format (`claude/commands/review.md` is the reference template): YAML frontmatter → `# /<cmd> — tagline` → skim-readable blockquote → 1-paragraph overview → `> **Effort**` directive where the original had one → per-section 1-paragraph summary + skill-invocation directive(s) + dispatch/CLI line(s). **Preserve every existing top-level `## ` header verbatim and in order** (PILOT-LESSONS §3 — headers are the reviewer's grep anchor; do not impose a generic template). Step-0 envelope construction uses `tomlctl flow envelope build --command <name> …` (validated form, all four names whitelisted) instead of inline JSON-template prose.

**User-engagement gates and carrier-specific logic that MUST survive verbatim:**
- `plan-new`: Phase 4 "*A session-level autonomy directive … does NOT apply to this phase*" gate; the fresh-plan Step-0 note (`resolved == false` is expected, not a halt); Phase 8 `ExitPlanMode` boundary; Phase 9 `tomlctl flow init` bootstrap (slug sanitiser, scope/branch validation, idempotent re-run) — this is plan-new-unique flow-creation logic, **not** a shared block, and stays inline.
- `review-plan`: the Step-4 empty-answer rule (`acceptEdits`/headless → skip merge, persist findings only).
- `implement`: the Phase-2 git-checkpoint halt (commit fails → do not append task-completion) and the `git stash pop` conflict halt.
- `tdd`: the resume protocol (`tomlctl flow resolve --flow <parent-slug> --json`, `tomlctl --version` check), anti-cheat fingerprint, `.tdd.lock`.

Skill invocations bind at the phase where the contract actually fires (PILOT-LESSONS §4): flow-context at Step 0; execution-record-schema before the first execution-record write; plansdirectory-prompt at Step 0.5; vet-research after research agents return; plan-output-format at Phase 7.

**In-prose pointers to now-externalised sections** — e.g. implement.md's repeated `see the ## Execution Record Schema shared block` (`:301`, `:436`, `:448`, `:466`) and the `## Flow Context` section refs — MUST be rewritten into skill-invocation directives during each carrier rewrite. This is **in-scope carrier hygiene**, distinct from the §E-deferred *skill-body* cross-refs: the carrier's `## Execution Record Schema` / `## Flow Context` headings vanish on rewrite, so a pointer to them goes dangling and would send the model hunting for an absent section.

### D. Manifest migration

Edit `scripts/shared-blocks.toml` in the same change as the carrier rewrites (parity must stay green throughout):
- **`flow-context`** — remove `plan-new`, `review-plan`, `implement`, `tdd` → leaves `[optimise, optimise-apply, review-apply, plan-update]`. Keep entry + existing `skill =`.
- **`execution-record-schema`** — **add** `skill = "claude/skills/flow-contract-execution-record-schema/SKILL.md"` (placed between `name` and `files`, outside the array so the awk parser ignores it); remove `plan-new`, `implement`, `tdd` → leaves `[plan-update]`.
- **`vet-flow-research`** — remove `plan-new`, `review-plan` → leaves `[optimise, plan-update, test-bootstrap]`. Keep entry + existing `skill =`.
- **`plansdirectory-prompt`** — **add** `skill = "claude/skills/flow-contract-plansdirectory-prompt/SKILL.md"`; remove `plan-new`, `review-plan` → leaves `[plan-update]`.
- Add a per-block migration comment line above each modified `files` array (mirroring the pilot's `# <carrier>.md migrated … (date)` convention, §6). Do **not** delete any entry — none reaches empty `files` this wave.

`flow-contract-plan-output-format` gets **no** manifest entry (never a `SHARED-BLOCK`).

### E. Deferred §12 reference-rewrite (documented, not executed here)

The two new skill bodies retain their original cross-references verbatim (`see the `## Flow Context` section`, `see `flow-bootstrap.md` Contract`, the `shared verbatim across /plan-new, /plan-update, /review-plan` sentence). These are mildly stale when the skill is read standalone but remain functional (the model can still invoke `flow-contract-flow-context`). The §12 rewrite is deferred to `plan-update`'s separate overhaul: when `plan-update` migrates, both blocks reach empty `files`, their manifest entries are deleted per §6, and the skill bodies can then be polished free of the byte-parity comparison constraint. This deferral is recorded so the future overhaul picks it up rather than discovering it.

### F. Canonical skill names

All carrier tasks reference these exact skill names — copy verbatim, never retype (drift between T1–T3 creation and T4–T7 references breaks skill loading silently):

- `flow-contract-flow-context` (existing)
- `flow-contract-execution-record-schema` (new — T1)
- `flow-contract-plansdirectory-prompt` (new — T2)
- `flow-contract-plan-output-format` (new — T3)
- `flow-contract-vet-research` (existing)

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
parity: bash scripts/verify-shared-blocks.sh
```

## Tasks

### Batch 1 (parallel — independent new files)

#### 1. Create `flow-contract-execution-record-schema` skill [S]
- **Files**: `claude/skills/flow-contract-execution-record-schema/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `execution-record-schema` block from `claude/commands/implement.md` (between `<!-- SHARED-BLOCK:execution-record-schema START/END -->`, lines 81–266) verbatim, strip the marker comments, prepend 2-field frontmatter. Do **not** rewrite any cross-reference line (verbatim — see Approach §E).
- **Detail**: `name: flow-contract-execution-record-schema`. `description`: single long line (~500–1500 chars) covering the append-only `[[items]]` log schema for `.claude/flows/<slug>/execution-record.toml` — type vocabulary, required/type-specific fields, the two-call write idiom, render-from-log routine, `[tasks].completed` derivation, read-path integrity contract — and when to consult it ("Consult before any read or write of a flow's execution-record.toml by /plan-new, /implement, /plan-update, or /tdd").
- **Acceptance**: File exists; frontmatter parses (2 fields); the skill body (frontmatter-excluded) is byte-identical to plan-update's embedded execution-record-schema block (the verify_skills_clean comparison target) with the START/END marker lines removed — verify with a `diff`, not an approximate byte count; `bash scripts/verify-shared-blocks.sh` still exits 0 (manifest not yet edited).

#### 2. Create `flow-contract-plansdirectory-prompt` skill [S]
- **Files**: `claude/skills/flow-contract-plansdirectory-prompt/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `plansdirectory-prompt` block from `claude/commands/plan-new.md` (lines 83–103) verbatim, strip markers, prepend 2-field frontmatter. No cross-reference rewrite (verbatim — §E).
- **Detail**: `name: flow-contract-plansdirectory-prompt`. `description`: covers the first-use `plansDirectory` prompt contract — the `envelope.plans_directory == null` gate, option-list construction, headless empty-answer detection, `Don't ask again`/`__DONT_ASK__` sentinel, free-text follow-up, the `tomlctl json set` persist idiom, and the in-memory bind — plus when to consult it (`/plan-new`, `/plan-update`, `/review-plan` Step 0.5).
- **Acceptance**: File exists; frontmatter parses; the skill body (frontmatter-excluded) is byte-identical (via `diff`) to plan-update's embedded plansdirectory-prompt block with the marker lines removed; parity script still exits 0.

#### 3. Create `flow-contract-plan-output-format` skill [S]
- **Files**: `claude/skills/flow-contract-plan-output-format/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract `plan-new`'s Phase-7 plan-document template from plan-new.md lines 502–589 (the `# Plan: {Descriptive Title}` structure through `## Risks`, plus the "Format rules" list) into the skill body with 2-field frontmatter. **Strip the OUTER ``` fence (plan-new.md:502 and :580) but PRESERVE the nested build/test/lint fence (:539/:543)** — extracting the outer fence verbatim would render a broken code block in the SKILL.md. Light polish for standalone clarity is permitted (not a `SHARED-BLOCK`, no parity constraint).
- **Detail**: `name: flow-contract-plan-output-format`. `description`: defines the on-disk plan document structure (`## Context` / `Scope` / `Research Notes` / `User Decisions` / `Approach` / `Verification Commands` / `Tasks` / `Dependency Graph` / `Verification` / `Risks`), task-effort sizing (S/M/L), and format rules; consult when writing or reformatting a plan document (`/plan-new` Phase 7, and later `/plan-update reformat`, `/review-plan`).
- **Acceptance**: File exists; frontmatter parses; contains the full plan-template structure and the format-rules list; `command_lint` (run in Batch 4) stays green.

### Batch 2 (parallel — independent carrier rewrites; each touches one file)

> All four carriers are independent files; the manifest edit (Task 8) is deliberately deferred to Batch 3 so a single commit flips carriers + manifest together and parity never goes red mid-batch. **Commit discipline**: stage Tasks 4–8 into ONE commit — the pre-commit hook runs `verify-shared-blocks.sh`, which rejects any intermediate commit where a carrier has dropped its block markers while the manifest still lists it.

#### 4. Rewrite `claude/commands/plan-new.md` [M]
- **Files**: `claude/commands/plan-new.md` (REWRITE)
- **Depends on**: 1, 2, 3
- **Action**: Collapse to a skeleton per Approach §C, relaxed ceiling. Replace the four embedded blocks (`flow-context`, `execution-record-schema`, `plansdirectory-prompt`, `vet-flow-research`) with skill-invocation directives at their firing phases, and the Phase-7 output template with an invocation of `flow-contract-plan-output-format`. Step-0 uses `tomlctl flow envelope build --command plan-new …`. Preserve every `## `/`# ` header verbatim and in order; preserve the Phase-4 autonomy-gate phrase, the fresh-plan no-flow note, and the entire Phase-9 `tomlctl flow init` bootstrap logic inline.
- **Detail**: Phases 1–10 each keep a standalone 1-paragraph summary so a cold reader follows the flow without loading skills. Skill-invocation lines are natural-language directives (`Invoke the `flow-contract-X` skill …`), matching `review.md`. **Phase-9 bootstrap (slug sanitiser regex, collision handling, idempotency, path-traversal guard) is preserved VERBATIM** — the ≤150 LOC ceiling yields to it; never trim Phase-9 fidelity to hit the line count.
- **Acceptance**: `wc -l` ≤ 150; all original headers present (`grep -n "^#"`); references all five skills (`flow-contract-flow-context`, `-execution-record-schema`, `-plansdirectory-prompt`, `-vet-research`, `-plan-output-format`); Phase-4 autonomy phrase and Phase-9 `flow init` block retained; no `<!-- SHARED-BLOCK:` markers remain.

#### 5. Rewrite `claude/commands/review-plan.md` [M]
- **Files**: `claude/commands/review-plan.md` (REWRITE)
- **Depends on**: 1, 2, 3
- **Action**: Skeleton per §C, ≤100 LOC. Replace `flow-context`, `plansdirectory-prompt`, `vet-flow-research` blocks with skill invocations; Step-0 uses `--command review-plan`. Preserve all `## Step` headers; preserve the Step-4 empty-answer rule verbatim.
- **Acceptance**: `wc -l` ≤ 100; all headers present; references `flow-contract-flow-context`, `-plansdirectory-prompt`, `-vet-research`; empty-answer rule retained; no SHARED-BLOCK markers.

#### 6. Rewrite `claude/commands/implement.md` [M]
- **Files**: `claude/commands/implement.md` (REWRITE)
- **Depends on**: 1
- **Action**: Skeleton per §C, ≤100 LOC. Replace `flow-context` + `execution-record-schema` blocks with skill invocations (execution-record-schema bound before the Phase-2 step-5b execution-record write); Step-0 uses `--command implement`. Preserve all Phase headers; preserve the git-checkpoint halt and `git stash pop` conflict halt verbatim.
- **Acceptance**: `wc -l` ≤ 100; all Phase headers present; references `flow-contract-flow-context` + `-execution-record-schema`; both halt gates retained; no SHARED-BLOCK markers.

#### 7. Rewrite `claude/commands/tdd.md` [M]
- **Files**: `claude/commands/tdd.md` (REWRITE)
- **Depends on**: 1
- **Action**: Skeleton per §C, ≤100 LOC. Replace `flow-context` + `execution-record-schema` blocks with skill invocations; Step-0 uses `--command tdd`. Preserve all `## ` headers (Overview, Cycle FSM, sub-flow layout, anti-cheat, bootstrap-missing fallback, concurrency lockfile, edge-cases, resume protocol, acceptance smoke-check); preserve the resume-protocol `tomlctl` calls, the `failure_reason` discriminator reasoning (tdd.md:342) that drives the resume FSM, and anti-cheat/lockfile logic inline.
- **Acceptance**: `wc -l` ≤ 100; all headers present; references `flow-contract-flow-context` + `-execution-record-schema`; resume/anti-cheat/lock logic retained; no SHARED-BLOCK markers.

### Batch 3 (sequential — after Batch 2)

#### 8. Shrink manifest + add `skill =` fields + refresh CLAUDE.md [S]
- **Files**: `scripts/shared-blocks.toml`, `CLAUDE.md`
- **Depends on**: 4, 5, 6, 7
- **Action**: Per Approach §D — remove the four migrated carriers from `flow-context`; add `skill =` to `execution-record-schema` and remove `plan-new`/`implement`/`tdd`; remove `plan-new`/`review-plan` from `vet-flow-research`; add `skill =` to `plansdirectory-prompt` and remove `plan-new`/`review-plan`. Add per-block migration comment lines. Delete no entries. **Then refresh `CLAUDE.md`**: its Developer-setup prose names the manifest's carrier file list and asserts "execution-record-schema parity across `plan-new` / `plan-update` / `implement`" — after this wave only `plan-update` embeds that block, so correct both the carrier-file enumeration and the parity sentence to reflect the post-wave manifest.
- **Detail**: The `skill =` line MUST sit between `name` and `files`, outside the array, so `verify-shared-blocks.sh`'s awk parser ignores it (verified pattern from the pilot/gate work). The CLAUDE.md edit is prose-only — do not weaken or restate the supply-chain/hook guidance, just correct the stale carrier/parity references.
- **Acceptance**: `tomlctl parse scripts/shared-blocks.toml` succeeds; `bash scripts/verify-shared-blocks.sh` exits 0; `execution-record-schema` files == `[plan-update]`, `plansdirectory-prompt` files == `[plan-update]`, `flow-context` files == 4 carriers, `vet-flow-research` files == 3 carriers; `CLAUDE.md` no longer names `plan-new`/`implement`/`tdd` as `execution-record-schema` embedders.

### Batch 4 (sequential — after Batch 3)

#### 9. Full automated gate [M]
- **Files**: none (verification)
- **Depends on**: 8
- **Action**: Run the full acceptance gate and confirm green. `cargo test` exercises `verify_skills_clean` (must report `ok` for the now-`skill`-bearing `execution-record-schema` / `plansdirectory-prompt` against `plan-update`'s copies, plus the unchanged pilot blocks), `command_lint` (must parse the four new skeletons' `flow envelope build` calls and the moved `tomlctl` invocations in the new skills), and `blocks_verify_reproduces_shell_hashes` (must reproduce all pinned hashes from the shrunk manifest lists).
- **Detail**: If `verify_skills_clean` reports drift, it indicates the verbatim extraction diverged from `plan-update`'s copy (fix the extraction — do **not** weaken normalisation). If `command_lint` fails, fix the offending `tomlctl` line or mark a genuinely-illustrative snippet `ignore-command-lint`. If `blocks_verify_reproduces_shell_hashes` fails, the block content changed unexpectedly during a rewrite (a carrier's skeleton must not alter the residual `plan-update`-shared block — but the rewrite removes the block from these carriers entirely, so the only contributor is `plan-update`, untouched).
- **Acceptance**: `cargo build` clean; `cargo test` all pass; `cargo clippy --all-targets` no new warnings; `bash scripts/verify-shared-blocks.sh` exits 0; `tomlctl blocks verify-skills | jq .` exits 0.

## Dependency Graph

```
Batch 1 (parallel):  T1, T2, T3                  (3 new skill files, independent)
Batch 2 (parallel):  T4←{1,2,3}  T5←{1,2,3}  T6←{1}  T7←{1}   (4 carriers, independent files)
Batch 3 (sequential): T8 ← {4,5,6,7}             (manifest shrink + skill= fields)
Batch 4 (sequential): T9 ← {8}                   (automated gate)
```

Batch 2 has 4 parallel agents — at the upper edge of the guidance but each touches a distinct file with no shared state.

## Verification

Automated gate (run all — Task 9):
- `cargo build --manifest-path tomlctl/Cargo.toml` → clean
- `cargo test --manifest-path tomlctl/Cargo.toml` → all pass (incl. `verify_skills_clean`, `command_lint`, `blocks_verify_reproduces_shell_hashes`)
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` → no new warnings
- `bash scripts/verify-shared-blocks.sh` → exit 0
- `tomlctl blocks verify-skills | jq .` → exit 0, valid JSON
- `wc -l` per carrier: `plan-new` ≤ 150; `review-plan` / `implement` / `tdd` ≤ 100

Post-conversion human gate (user, per Q3): once all four carriers are converted, run `/plan-new` first as a canary, then `/review-plan`, `/implement`, and `/tdd` live — exercising each carrier's UNIQUE logic (Phase-9 flow-init, the git-checkpoint/stash halts, the tdd resume protocol), not merely confirming skills load and confirm (a) each loads its referenced skill bodies at the right phase boundary, (b) Step-0 `flow envelope build` dispatch + `flow-bootstrap` binding works, (c) behaviour matches pre-overhaul (execution-record writes use the schema from the loaded skill; plansDirectory prompt fires correctly; plan output matches the format skill).

## Risks

1. **`plan-new` cannot hit a reasonable ceiling even relaxed** — 10 phases + Phase-9 flow-init logic are inherently verbose. *Mitigation*: the ~88-line output template moves to a skill (Task 3); phases compress to 1-paragraph summaries; ~150 LOC is the target, not a hard fail — if it lands at ~160–170 with all gates intact and skim-readable, that is acceptable (the user relaxed the ceiling explicitly).
2. **Verbatim skill bodies carry mildly-stale standalone cross-refs** (§E) — e.g. `plansdirectory-prompt`'s "do not edit one carrier's copy" sentence reads oddly in a standalone skill. *Mitigation*: documented as a deliberate deferral to `plan-update`'s overhaul; functionally harmless (refs still resolve to invokable skills); rewriting now would break `verify_skills_clean` parity or mask substantive content.
3. **A carrier rewrite accidentally drops a user-engagement gate or carrier-specific logic** (Phase-9 flow-init, git-checkpoint halt, resume protocol). *Mitigation*: each carrier task lists the must-survive lines explicitly; Task 9's human smoke-run is the backstop; review against the pre-rewrite file in git.
4. **Skeleton `flow envelope build` call drifts from the real CLI** — e.g. a mistyped flag. *Mitigation*: `command_lint` (Task 9) parses every `tomlctl` line in the new skeletons against the real clap surface; this catches structural flag/subcommand drift. **Caveat**: `--command` is a clap `String` checked against `VALID_COMMANDS` only at runtime (`envelope.rs:57`), so `command_lint` will NOT catch a typo'd command *value* (e.g. `--command plannew`) — eyeball each carrier's `--command <name>` token against the whitelist, or add a small test asserting membership.
5. **Stale local fixture binding names** (`flow_context_eight` holds 4, `execution_record_four` holds 1) confuse a future reader. *Mitigation*: out of scope (cosmetic, touches Rust); noted here as an optional follow-up rename to be folded into a later wave or the `plan-update` overhaul. The test is functionally correct.
