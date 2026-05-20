# Plan: Harness Progressive-Disclosure Wave 2

**Plan path**: docs/plans/harness-progressive-disclosure-wave-2.md
**Created**: 2026-05-20
**Status**: draft (revised after /review-plan round 1)

## Context

Wave 1 (commits `ca79b22`, `5bdf81f`, 2026-05-20) converted four flow carriers
(`plan-new`, `review-plan`, `implement`, `tdd`) into skim-readable "skeletons" that load
full contract bodies on demand via `flow-contract-*` skill invocations, and extracted
three skills. Wave 2 finishes the migration: it converts the five remaining substantive
carriers (`optimise`, `optimise-apply`, `review-apply`, `plan-update`, `test-bootstrap`)
to skeletons and externalises the four `apply-*` shared blocks into new skills. After this
wave every shared contract **carried by a flow command** lives in exactly one skill. The
`forbidden-working-tree-ops` block stays inline in `flow-implement-{deep,lite}` (those
agents have no Skill tool and so cannot invoke a skill) — it is the only block left in the
parity manifest after this wave.

## Scope

**In scope**
- Extract 4 new skills: `flow-contract-apply-dependency-sort`, `-apply-rollback-protocol`,
  `-apply-constraints`, `-apply-vet-flow-implement-lite` (with the §12 reference-rewrite
  applied during extraction — see Approach).
- Rewrite 5 carriers to skeletons: `optimise.md`, `optimise-apply.md`, `review-apply.md`,
  `plan-update.md`, `test-bootstrap.md`.
- Update `scripts/shared-blocks.toml`: delist the 5 migrated carriers and **delete every
  `[[block]]` entry whose `files[]` becomes empty** (PILOT-LESSONS §6).
- Update `tomlctl/src/cli/dispatch.rs` test `blocks_verify_reproduces_shell_hashes`: remove
  the pinned-hash assertions for the deleted blocks (they will otherwise panic). **Rust
  change is required this wave** — see Risk R1 / Task 6.
- Refresh `CLAUDE.md` prose to reflect the new manifest state.

**Out of scope**
- Editing `claude/agents/flow-implement-{deep,lite}.md` — they carry only
  `forbidden-working-tree-ops`, which is **not** migrated (no Skill tool in those agents).
- Migrating `forbidden-working-tree-ops` to a skill.
- `review.md` and `commit.md` (already thin).

**Affected areas**: claude/commands/, claude/skills/, scripts/shared-blocks.toml,
tomlctl/src/cli/dispatch.rs, CLAUDE.md
**Estimated file count**: 12 (4 new skills + 5 carriers + manifest + dispatch.rs + CLAUDE.md)

## Research Notes

No external research required — the recipe and machinery are fully specified by the wave-1
commits, `PILOT-LESSONS.md`, and the in-repo tooling. Verified internal findings:

- **`apply-*` block residency** (verified `scripts/shared-blocks.toml:66-107`, grep
  `claude/agents/` = no `apply-*` markers): the four `apply-*` blocks are embedded **only**
  in `optimise-apply.md` + `review-apply.md`. The `flow-implement-{deep,lite}` agents embed
  only `forbidden-working-tree-ops` (`flow-implement-deep.md:75/114`). Since wave 2 migrates
  both apply carriers, all four `apply-*` blocks reach **empty `files[]`** — no residual
  embedder remains. The new skills are invoked by name from the apply carriers; they carry
  no manifest entry and are not parity-checked by `verify_skills` (which skips empty
  `files[]`, `blocks.rs:421`). Extraction correctness is therefore guarded by an explicit
  in-task diff (Tasks 1-4), not by tooling.
- **Drift machinery**: `verify_skills()` (`blocks.rs:384-515`) + `normalise_block()`
  (`blocks.rs:338`) read the manifest at runtime; `verify_skills_clean`
  (`dispatch.rs:1641`) and `command_lint` (`dispatch.rs:1674`, scans `claude/commands/*.md`
  + `claude/skills/flow-contract-*/SKILL.md` + `tomlctl/SKILL.md`) gate via `cargo test`.
- **`blocks_verify_reproduces_shell_hashes`** (`dispatch.rs:1394-1631`): `carriers_for()`
  reads file lists from the manifest (wave-1 single-sourced this per PILOT §14), but the
  test still **pins hash assertions** for `flow-context` (1572), `ledger-schema` (1585),
  `execution-record-schema` (1598), `apply-dependency-sort` (1619), `apply-rollback-protocol`
  (1624), `apply-constraints` (1629). With empty `files[]`, `blocks_verify` yields no block
  entry and `expect_hash` panics *"block X missing from report"*. These assertions MUST be
  removed in the same commit as the manifest delete.
- **Pre-commit hook is NOT installed in this clone** (`core.hooksPath` unset). `cargo test`
  is the only enforced gate locally — see R2.
- **Windows**: `scripts/verify-shared-blocks.sh` requires GNU awk (mawk default on Git Bash;
  Bash harness tool unreliable per memory). `cargo test` is the cross-platform gate.

## User Decisions

> Phase-4 gate (2026-05-20) + /review-plan round-1 follow-up (2026-05-20).

1. **Carrier scope** → All 5 substantive carriers.
2. **apply-\* blocks** → Externalise to 4 new skills. *(Original rationale "agents remain
   residual embedders" was FALSE — corrected: the apply carriers are the only embedders, so
   after migration the blocks have no embedder. The decision to externalise still stands;
   the skills hold the canonical contract and the carriers invoke them by name.)*
3. **Terminal cleanup** → **Do it this wave** (revised from "defer"). Since the Rust test
   must change regardless (R1), deferral saved nothing. Follow PILOT §6: delete every
   `[[block]]` entry that reaches empty `files[]`. Manifest shrinks to just
   `forbidden-working-tree-ops`.

### Phase 5 outcome
_Skipped — no external topic; internal migration covered by wave-1 precedent + PILOT-LESSONS._

## Approach

Mirror the wave-1 recipe with the manifest-first ordering refinement and PILOT-LESSONS
§6/§12 alignment.

**Manifest-first ordering (parity-safe).** The pre-commit byte hook and `verify_skills`
only inspect a `(block, file)` pair when the manifest lists that file. Deleting a block's
manifest entry (or delisting a carrier) *before* rewriting the carrier makes the carrier's
still-embedded (dead) block invisible to both checks. So once the manifest is updated, the 5
carriers can be rewritten in any order / parallel batches with zero parity risk.

**PILOT §12 reference-rewrite (mandatory, during extraction).** Verbatim block extraction
leaves stale prose in the new skill: (a) in-block `SHARED-BLOCK:X` cross-refs that resolve
to nothing standalone, and (b) "embedded into <carriers>" embedder-list sentences. Tasks 1-4
must rewrite these — (a) → "the `flow-contract-X` skill", (b) → name the skill as canonical.
Because the `apply-*` blocks have no residual embedder, there is no byte-parity constraint,
so the rewrite is unconstrained and correct.

**Three parity-safe commits:**

1. **Skills** (T1-T4) — create the 4 `apply-*` skill files, extracted from the
   `optimise-apply.md` donor with the §12 rewrite + 2-field frontmatter. `cargo test` stays
   green (manifest unchanged; new skills carry no manifest entry; `command_lint` parses
   their bash blocks).
2. **Manifest + Rust test** (T5 + T6, **same commit**) — delete every `[[block]]` entry that
   reaches empty `files[]`, and remove the matching pinned-hash assertions in
   `dispatch.rs`. After this commit only `forbidden-working-tree-ops` remains in the
   manifest. `cargo test` green. **This commit must land before any Phase-3 task** (the
   carriers still physically embed dead blocks, harmless once delisted).
3. **Carriers + prose** (T7-T12) — rewrite the 5 carriers to skeletons (replace each dead
   block with a one-paragraph summary + `Invoke the \`flow-contract-X\` skill to load ...`,
   binding each invocation to the phase where the contract actually fires per PILOT §4, and
   preserving every `## Step`/`## Phase` header name + numbering verbatim per PILOT §3) and
   refresh `CLAUDE.md`. Parity-trivial; 6 files split across parallel batches.

## Verification Commands

```bash
# Load-bearing, cross-platform gate (reads the same manifest the hook does):
cargo test --manifest-path tomlctl/Cargo.toml      # verify_skills_clean + command_lint + blocks_verify_reproduces_shell_hashes
cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets   # lint (Rust IS changed this wave)
# Optional / GNU-awk-gated (skip if `awk --version` is not GNU awk — mawk on Git Bash):
bash scripts/verify-shared-blocks.sh
```

> Pre-flight: `git config core.hooksPath` — the parity pre-commit hook is NOT installed in
> this clone, so do NOT rely on it to catch a mis-ordered commit. Commit the T5+T6 manifest+
> Rust change as its own checkpoint and run `cargo test` before the Phase-3 carrier batches.

## Tasks

### Phase 1: Extract apply-* skills (parallel — 4 agents, all new files)

> All four: extract the body between the named `<!-- SHARED-BLOCK:<block> START/END -->`
> markers in `claude/commands/optimise-apply.md`; strip the marker lines; apply the PILOT
> §12 reference-rewrite (rewrite any in-block `SHARED-BLOCK:X` cross-ref to "the
> `flow-contract-X` skill"; correct/remove any "embedded into <carriers>" sentence — the
> skill is now the canonical source with no embedder); prepend 2-field frontmatter.
> **`description:` template** (S1): `Canonical <block-name> contract for the apply-flow
> carriers (/optimise-apply, /review-apply) — <one clause: what the contract defines>.
> Consult before <the action it governs>.` Aim ~500-1500 chars, matching
> `claude/skills/flow-contract-ledger-schema/SKILL.md`.
> **Pre-extraction check**: confirm the donor block in `optimise-apply.md` is identical to
> the copy in `review-apply.md` before extracting (the hook that normally guarantees this is
> not installed here): `diff <(sed -n '/SHARED-BLOCK:<block> START/,/SHARED-BLOCK:<block> END/p' claude/commands/optimise-apply.md) <(sed -n '/SHARED-BLOCK:<block> START/,/SHARED-BLOCK:<block> END/p' claude/commands/review-apply.md)` → must be empty.

#### Task 1 — Extract `flow-contract-apply-dependency-sort` skill
- **Files**: `claude/skills/flow-contract-apply-dependency-sort/SKILL.md` (new)
- **Depends on**: none
- **Action**: Per the Phase-1 preamble, for the `apply-dependency-sort` block.
- **Acceptance**: File exists with 2-field frontmatter; `diff` of (donor block, markers
  stripped) vs (SKILL.md body, frontmatter stripped) differs **only** on the §12-rewritten
  reference lines — no substantive content drift. S

#### Task 2 — Extract `flow-contract-apply-rollback-protocol` skill
- **Files**: `claude/skills/flow-contract-apply-rollback-protocol/SKILL.md` (new)
- **Depends on**: none
- **Action**: As Task 1, for `apply-rollback-protocol`.
- **Acceptance**: As Task 1. S

#### Task 3 — Extract `flow-contract-apply-constraints` skill
- **Files**: `claude/skills/flow-contract-apply-constraints/SKILL.md` (new)
- **Depends on**: none
- **Action**: As Task 1, for `apply-constraints`.
- **Acceptance**: As Task 1. S

#### Task 4 — Extract `flow-contract-apply-vet-flow-implement-lite` skill
- **Files**: `claude/skills/flow-contract-apply-vet-flow-implement-lite/SKILL.md` (new)
- **Depends on**: none
- **Action**: As Task 1, for `apply-vet-flow-implement-lite`. This block contains tomlctl /
  console-line invocations — keep them inside ` ```bash ` fences and byte-verbatim so
  `command_lint` still parses them.
- **Acceptance**: As Task 1, plus: any ` ```bash ` tomlctl lines are unaltered; `cargo test`
  `command_lint` passes once the file exists (it scans `flow-contract-*` skills). S

### Phase 2: Manifest + Rust test (after Phase 1 — same commit, sequential)

#### Task 5 — Prune `scripts/shared-blocks.toml`
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 1, 2, 3, 4
- **Action**: For each `[[block]]`, remove `claude/commands/{optimise,optimise-apply,
  review-apply,plan-update,test-bootstrap}.md` from `files[]`. **If a block's `files[]`
  becomes empty, delete the entire `[[block]]` entry** (PILOT §6). Keep only entries that
  still list ≥1 file — after this wave that is solely `forbidden-working-tree-ops` (the two
  `flow-implement` agents). Do NOT add `skill=` fields for the apply-* blocks (the entries
  are deleted, not externalised-with-mapping). Update the top-of-file comment to record the
  wave-2 prune.
- **Detail**: Blocks deleted: `flow-context`, `ledger-schema`, `ledger-disposition-sweep`,
  `execution-record-schema`, `plansdirectory-prompt`, `vet-flow-research`,
  `apply-dependency-sort`, `apply-rollback-protocol`, `apply-constraints`,
  `apply-vet-flow-implement-lite`.
- **Acceptance**: Manifest contains exactly one `[[block]]` (`forbidden-working-tree-ops`).
  Co-committed with Task 6. M

#### Task 6 — Update `blocks_verify_reproduces_shell_hashes` in `tomlctl/src/cli/dispatch.rs`
- **Files**: `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 1, 2, 3, 4
- **Action**: Remove the `carriers_for(...)` bindings, `blocks_verify(...)` calls, and
  `expect_hash(...)` assertions for every block Task 5 deletes (`flow-context`,
  `ledger-schema`, `execution-record-schema`, `apply-dependency-sort`,
  `apply-rollback-protocol`, `apply-constraints` — the only ones currently asserted). To keep
  the test guarding a real block, **repoint it to assert `forbidden-working-tree-ops`** (the
  one surviving manifest block, spanning the two agents): bind via
  `carriers_for("forbidden-working-tree-ops")`, run `blocks_verify`, and pin its current
  hash (obtain the hash from the `expect_hash` panic's `actual:` line on first run, or
  `tomlctl blocks verify`). Then grep `tomlctl/` for the deleted block-name string literals
  and the migrated carrier paths and update/remove any other stale references.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes (no panic);
  `cargo clippy --all-targets` clean. Co-committed with Task 5. M

### Phase 3: Rewrite carriers to skeletons + prose (after Phase 2 — parallel, 2 batches)

> Parity-safe: Phase 2 already removed these carriers from the manifest. For each carrier,
> first enumerate its top-level headers (`grep -n "^## " claude/commands/<carrier>.md`) and
> preserve every one verbatim (PILOT §3); bind each skill invocation at the phase where the
> contract is actually consulted (PILOT §4), not a template position.

#### Task 7 — Skeletonise `claude/commands/optimise.md`
- **Files**: `claude/commands/optimise.md`
- **Depends on**: 5, 6
- **Action**: Replace the inline `flow-context`, `ledger-schema`, `ledger-disposition-sweep`,
  and `vet-flow-research` blocks with one-paragraph summaries + skill invocations (skill
  names: `flow-contract-flow-context`, `flow-contract-ledger-schema`,
  `flow-contract-ledger-disposition-sweep`, `flow-contract-vet-research`). Add the
  `> Skim-readable orchestrator...` tagline. Preserve all optimise-specific procedural prose.
- **Acceptance**: `grep -n "SHARED-BLOCK" claude/commands/optimise.md` returns nothing; every
  pre-edit `## ` header still present; four skills invoked by name; `cargo test` `command_lint`
  passes. M

#### Task 8 — Skeletonise `claude/commands/optimise-apply.md`
- **Files**: `claude/commands/optimise-apply.md`
- **Depends on**: 5, 6
- **Action**: Replace `flow-context`, `ledger-schema`, and the four `apply-*` blocks with
  skill invocations (`flow-contract-apply-dependency-sort`, `-apply-rollback-protocol`,
  `-apply-constraints`, `-apply-vet-flow-implement-lite`). Preserve all batch-orchestration /
  rollback / halt prose between the (formerly non-contiguous) blocks.
- **Acceptance**: No `SHARED-BLOCK` markers remain; every pre-edit `## ` header present; six
  skills invoked; `command_lint` passes. M

#### Task 9 — Skeletonise `claude/commands/review-apply.md`
- **Files**: `claude/commands/review-apply.md`
- **Depends on**: 5, 6
- **Action**: As Task 8 (parallel-structured file): replace `flow-context`, `ledger-schema`,
  and the four `apply-*` blocks; preserve review-apply-specific logic.
- **Acceptance**: As Task 8. M

#### Task 10 — Skeletonise `claude/commands/plan-update.md`
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5, 6
- **Action**: Replace `flow-context`, `execution-record-schema`, `plansdirectory-prompt`,
  and `vet-flow-research` blocks with skill invocations. Preserve the nine sub-operation
  procedures (status / complete / deviation / defer / reconcile / reformat / catchup /
  snapshot / migrate) verbatim. **Edits only `plan-update.md`** — CLAUDE.md is owned by
  Task 12.
- **Acceptance**: No `SHARED-BLOCK` markers remain; every pre-edit `## ` header present; four
  skills invoked; `command_lint` passes. M

#### Task 11 — Skeletonise `claude/commands/test-bootstrap.md`
- **Files**: `claude/commands/test-bootstrap.md`
- **Depends on**: 5, 6
- **Action**: Replace the inline `vet-flow-research` block with a `flow-contract-vet-research`
  invocation; preserve the Phase 1-5 bootstrap procedure.
- **Acceptance**: No `SHARED-BLOCK` markers remain; Phase 1-5 headers present; skill invoked;
  `command_lint` passes. S

#### Task 12 — Refresh `CLAUDE.md` prose
- **Files**: `CLAUDE.md`
- **Depends on**: 5, 6
- **Action**: Update `## Developer setup` and the integrity/drift prose to the post-wave-2
  state: the parity manifest now lists only `forbidden-working-tree-ops` (spanning the two
  `flow-implement` agents); the five command carriers and all ten other blocks are gone. Fix
  the "currently `claude/commands/{optimise,optimise-apply,review-apply,plan-update,
  test-bootstrap}.md` and `claude/agents/flow-implement-{deep,lite}.md`" sentence — only the
  two agents remain. Name `forbidden-working-tree-ops` explicitly. Do not claim "every shared
  contract lives in exactly one skill" (false: `forbidden-working-tree-ops` stays inline in
  agents, and is also referenced un-parity-checked in `claude/agents/verification.md:30`).
- **Acceptance**: Prose matches the post-Task-5 manifest; no stale carrier/block references;
  `grep -n "optimise-apply" CLAUDE.md` shows no claim that it embeds shared blocks. S

## Dependency Graph

```
Phase 1 (parallel):   T1   T2   T3   T4         (4 new skill files)
                        \   |    |   /
Phase 2 (same commit):     T5  +  T6            (manifest prune + Rust test fix; both Depends on T1-T4)
                              |
Phase 3 (parallel, 2 batches; all Depends on T5,T6):
   Batch A: T7  T8  T9        (optimise, optimise-apply, review-apply)
   Batch B: T10  T11  T12     (plan-update, test-bootstrap, CLAUDE.md)
```

## Verification

- [ ] `cargo test --manifest-path tomlctl/Cargo.toml` — `verify_skills_clean`,
      `command_lint`, and `blocks_verify_reproduces_shell_hashes` all green.
- [ ] `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — clean.
- [ ] `git grep -n "SHARED-BLOCK" claude/commands/{optimise,optimise-apply,review-apply,plan-update,test-bootstrap}.md`
      returns nothing.
- [ ] `scripts/shared-blocks.toml` contains exactly one `[[block]]`
      (`forbidden-working-tree-ops`).
- [ ] `git diff --stat` does NOT include `claude/agents/flow-implement-{deep,lite}.md`.
- [ ] Each migrated carrier opens with the `> Skim-readable orchestrator...` tagline, invokes
      its skills by name, and retains every pre-edit `## ` header.
- [ ] (Optional, GNU-awk only) `bash scripts/verify-shared-blocks.sh` — no drift.

## Risks

- **R1 — Rust test breakage (the reason "no Rust changes" was wrong).**
  `blocks_verify_reproduces_shell_hashes` pins hashes for blocks that wave 2 empties; without
  Task 6 it panics *"block X missing from report"* and `cargo test` fails. Mitigation: Task 6
  co-commits with Task 5; repoint the test at `forbidden-working-tree-ops` so it still guards
  a real block.
- **R2 — Pre-commit hook absent locally.** `core.hooksPath` is unset in this clone, so the
  byte hook will not reject a mis-ordered commit. Mitigation: `cargo test` is the load-bearing
  gate; commit T5+T6 as their own checkpoint and run `cargo test` before Phase 3.
- **R3 — Windows verification.** `bash scripts/verify-shared-blocks.sh` needs GNU awk (mawk on
  Git Bash) and the Bash harness tool is unreliable here. Mitigation: treat it as optional;
  `cargo test` covers the same parity cross-platform.
- **R4 — apply-\* skills are unverified by tooling** (empty `files[]` → `verify_skills` skips
  them). Mitigation: Tasks 1-4 carry an explicit donor-diff acceptance; pre-extraction diff
  confirms `optimise-apply` == `review-apply` copies.
- **R5 — Inter-block prose loss.** The blocks are non-contiguous in the apply carriers;
  skeletonisation could delete adjacent procedural logic. Mitigation: header-enumeration
  acceptance per carrier (PILOT §3); replace ONLY shared blocks, never carrier-specific prose.
