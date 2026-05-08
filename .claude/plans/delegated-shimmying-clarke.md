# Plan: vet-flow-research SHARED-BLOCK + dispatch_type fields + vet_events log

**Plan path** (final): `docs/plans/vet-flow-research-and-schema-additions.md` (or similar — written after approval)
**Source**: 3 wontfixed cross-file refactors from `.claude/reviews/claude-commands.toml` — items R104, R119, R120. Wontfixed by /review-apply only because each exceeded the 3-file-per-item cap; underlying intent remains valid.

## Context

Three coordinated refactors on the flow-command spec files address audit gaps and code-duplication surfaced by /review:

- **R104 (medium)** — the orchestrator vet-pass procedure (used after research-agent dispatch in `/review`, `/optimise`, `/review-plan`, `/plan-new`, `/plan-update`, `/test-bootstrap`) is currently duplicated as prose across 6 files with subtle wording variance. Recent edits added the >30%-fail re-dispatch rule and mandatory console-line format unevenly — only 2/6 carriers explicitly state the console format, only 5/6 state the >30% rule. Extracting a `vet-flow-research` SHARED-BLOCK fixes the drift and makes future contract changes one-edit-N-files instead of N-edits-prone-to-divergence.
- **R119 (trivial-effort cross-file)** — the lite-vs-deep dispatch decision is currently recorded only in the agent prompt's `DISPATCH:` header, which is conversation-internal and ephemeral. Post-hoc audits ("which tasks went lite, which went deep, why?") require re-reading every agent prompt. Adding a durable `dispatch_tier` + `dispatch_agent` pair to `task-completion` execution-record entries makes the decision queryable via `tomlctl items list`.
- **R120 (trivial-effort cross-file)** — the orchestrator vet pass currently drops/downgrades findings "with rationale logged in console", which evaporates when the run ends. Auditors cannot distinguish "vet ran, dropped 3 findings" from "vet was skipped". Adding a `[[vet_events]]` append-only log (analogous to the existing `[[rollback_events]]`) makes the gate's actions durable and queryable.

Bundled because all three operate on the same set of flow-command spec files and the same shared-block discipline. Sequencing the phases as R104 → R120 → R119 minimises rebase churn (R120 enhances the block R104 creates; R119 is independent).

## Scope

- **In scope**: `claude/commands/{review,optimise,review-plan,plan-new,plan-update,test-bootstrap,review-apply,optimise-apply,implement,tdd}.md`, `scripts/shared-blocks.toml`. Markdown command-spec edits only.
- **Out of scope**: any change to `tomlctl` source (no Rust edits), any change to `flow-implement-{deep,lite}` agent files, any change to `flow-research[-deep]` agent files, automated migration of historical `task-completion` entries (new field is forward-only).
- **Affected areas**: `claude/commands/`, `scripts/`.
- **Estimated file count**: 11 unique files across all three phases.

## Exploration Notes

### Vet-section landscape (6 carrier files)

| Carrier | Section | Lines | Sample size | >30% rule? | Console line format? | Specialization |
|---------|---------|-------|-------------|-----------|---------------------|----------------|
| `review.md` | Step 2.5 | 569–592 | Sonnet ≥5, Opus ≥3 (asymmetric) | Yes (Sonnet→deep) | Yes — `vet: Agent-{n} (<lens>) — N findings sampled, M dropped, K downgraded` | Counter-line vetting for Opus |
| `optimise.md` | Step 2.5 | 519–537 | ≥3 per agent + expand-on-failure | Yes | **Missing** | Strict expand-on-any-failure |
| `review-plan.md` | Step 2.5 | 198–214 | ≥3 per agent | Yes | **Missing** | **Stale-reference verification first-class** |
| `plan-new.md` | Phase 3 + Phase 5 | 364–372, 417 | ≥3 per agent | Yes (Sonnet→deep) | **Missing** | Library version pin verification |
| `plan-update.md` | Phase 1.5 (catchup, Agent 2 only) | 653–665 | ≥3 from Agent 2 | **Missing** | **Missing** | **Deprecation/removal verification first-class** |
| `test-bootstrap.md` | Phase 2.5 | 282–304 | ≥5 per agent (uniform Sonnet) | Yes (Sonnet→deep) | Yes — same format as review.md | Registry-pin / install-syntax / config-schema |

Common procedure (extractable to block): triage by source-agent + evidence-grade → honour `ESCALATE-TO-DEEP` flag → drop unverified `low` findings → spot-check sample → drop/downgrade with rationale → emit mandatory console line → re-dispatch on >30% systemic failure (Sonnet re-dispatch SHOULD escalate to `flow-research-deep`).

Per-carrier divergence (must remain in carrier prose around the block): sample size, lens-specific verification rules, expand-on-failure (optimise only), Phase-3-vs-Phase-5 reference (plan-new), Agent-2-only scope (plan-update).

### SHARED-BLOCK infrastructure

- `scripts/verify-shared-blocks.sh`: byte-strict SHA256 over content between `<!-- SHARED-BLOCK:name START -->` and `<!-- SHARED-BLOCK:name END -->`. No parametrization, no templating.
- `scripts/shared-blocks.toml`: `[[block]] name = "..." files = [...]`. All carriers are peers. 9 existing blocks.
- Multiple block instances per file are concatenated and hashed together — practical effect: each block must appear ONCE per file. plan-new.md keeps the existing pattern of "Phase 5: vet using same procedure as Phase 3" rather than carrying the block twice.
- Pre-commit hook (`.githooks/pre-commit`) runs the verifier; `--no-verify` is forbidden by CLAUDE.md.
- Precedent block: `apply-vet-flow-implement-lite` (in `optimise-apply.md` lines 518–540, `review-apply.md` lines 523–545). Internal structure to mirror: header → context paragraph → numbered procedure → mandatory console line → closing rationale.

### Schema-block landscape

- **`SHARED-BLOCK:execution-record-schema`** (R119 target): 4 carriers — `plan-new.md`, `implement.md`, `plan-update.md`, `tdd.md` (lines 97–278). Type-vocabulary table at lines 145–153. **Writer scope is narrower than the original brief**: only `implement.md` Phase 2 step 5b appends `task-completion` entries. `review-apply.md` and `optimise-apply.md` orchestrate apply work but don't write execution-record entries. `tdd.md` GREEN delegates to `/implement`, inheriting the writer. So R119 writer changes touch only `implement.md` (one file) plus the 4-carrier schema block (one canonical edit).
- **`SHARED-BLOCK:ledger-schema`** (R120 target): 4 carriers — `review.md`, `review-apply.md`, `optimise.md`, `optimise-apply.md` (lines 97–283). Existing `[[rollback_events]]` definition at review.md:206–213 is the structural template. Existing append pattern at review-apply.md:702–708 (`tomlctl array-append <ledger> rollback_events --json - <<'EOF' ... EOF`).

### DISPATCH-header vocabulary (R119 input)

From `review-apply.md:484` and `implement.md:408–413`, current binary vocabulary:
- `flow-implement-lite` — passes all 4 lite-eligibility criteria
- `flow-implement-deep` — fails ANY criterion; default

No other DISPATCH variants exist in the writers that emit `task-completion` entries.

## Research Notes

_No external library research needed — internal markdown/spec refactor with no third-party dependencies. Phase-3 research (initial) reduced to the precedent mapping the Phase 1 Explore agents already did (apply-vet-flow-implement-lite block, rollback_events log, DISPATCH header vocabulary)._

## User Decisions

1. **Block scope** — *Skeletal (universal rules only)*. Block contains only the universal procedure (triage, drop-low, drop/downgrade, mandatory console line format, >30% re-dispatch with Sonnet→deep escalation). Sample sizes and lens-specific verification rules stay as carrier prose around the block. Preserves all existing per-carrier specialization while consolidating the universal contract. Side-benefit: forces the 4 carriers currently missing the console line format to adopt it (since the format is in the block), and forces `plan-update.md` Phase 1.5 to inherit the >30% rule (currently missing).
   _Prompted by: Exploration Notes §1 — verifier is byte-strict, sample sizes diverge across carriers (3/5/asymmetric)._

2. **dispatch_type vocabulary** — *Two-field split: `dispatch_tier` ∈ {`lite`, `deep`} AND `dispatch_agent` ∈ {`flow-implement-lite`, `flow-implement-deep`}*. Maximum auditability. Tier is the abstract decision signal (what the lite-eligibility gate decided); agent is the concrete subagent_type that ran. The two are tightly correlated today (lite ↔ flow-implement-lite, deep ↔ flow-implement-deep) but the split future-proofs the schema for additional dispatch types (e.g. a future `flow-research-deep` task-completion writer). Schema documents the lite↔flow-implement-lite / deep↔flow-implement-deep invariant; readers MUST treat unknown tier or agent values as fail-soft (treat unknown tier as `deep`, unknown agent as the literal value).
   _Prompted by: Exploration Notes §3 (DISPATCH vocabulary)._

3. **vet_events shape** — *Per-agent record*. One entry per vetted agent. Fields: `timestamp` (ISO 8601 date-time, seconds precision), `command` (`"review"` | `"optimise"` | `"review-plan"` | `"plan-new"` | `"plan-update"` | `"test-bootstrap"`), `agent_index` (integer 1..N), `lens` (string — the lens name as printed in the console line, e.g. `"security"`, `"test-runner"`), `sampled_count`, `dropped_count`, `downgraded_count` (integers), `dropped_ids` (array of `R{n}` / `O{n}` ledger IDs that were vetted-out), `rationale` (multi-line string capping at 8 KiB per the field-length-cap convention). Mirrors the per-agent console line format directly so log entries and console output are 1:1.
   _Prompted by: Exploration Notes §1 (console line format) + §2 (existing `[[rollback_events]]` precedent)._

4. **vet_events writer placement** — *Inside the new `vet-flow-research` SHARED-BLOCK*. The writer directive (`tomlctl array-append <ledger> vet_events --json - <<'EOF' ... EOF`) is part of the universal procedure. Couples R104 → R120 sequencing: Phase 1 creates the block with a writer-directive placeholder, Phase 2 fills it in. Future carriers automatically inherit; parity check enforces uniform writer logic.
   _Prompted by: User Decisions #1 (skeletal block) + #3 (per-agent record shape)._

## Approach

Three sequential phases, each ending in a single git commit so a failure in a later phase can be reverted without losing earlier work.

**Block design (Phase 1 + Phase 2 final state):**

```
<!-- SHARED-BLOCK:vet-flow-research START -->
[Universal context paragraph: what this vet pass does, what build/test verification doesn't catch in research output, why the orchestrator runs it.]

1. Triage by source agent + evidence-grade. Group findings by `(agent_index, evidence-grade)`; emit a one-line summary per group to console.
2. Honour `ESCALATE-TO-DEEP` flags. If any agent prefixed its return with `ESCALATE-TO-DEEP: <reason>`, re-dispatch that lens to `flow-research-deep` before further vetting.
3. Drop unverified `low` / `low-confidence` findings unless explicitly framed as a hypothesis with a concrete verification step.
4. Spot-check sampled findings. Sample size per carrier — see carrier prose below. For each sampled finding: read the cited `file:line`, confirm the code matches the description, verify any cited URLs / library version pins / Context7 IDs.
5. Drop or downgrade findings that fail vetting, with rationale. Downgrade by appending `_orchestrator-downgrade: <reason>` to the evidence-grade line.
6. Append a durable `[[vet_events]]` entry to the ledger via `tomlctl array-append <ledger> vet_events --json -` (see Ledger Schema → vet_events for the field set).
7. Emit the mandatory console line per agent: `vet: Agent-{n} (<lens>) — N findings sampled, M dropped, K downgraded`.
8. >30% systemic failure rule. If more than 30% of an agent's findings fail vetting, re-dispatch that lens with the failure pattern in the prompt. For Sonnet (`flow-research`) agents, the re-dispatch SHOULD escalate to `flow-research-deep`.
<!-- SHARED-BLOCK:vet-flow-research END -->
```

**Carrier prose around the block** continues to specify (in each carrier's own words) the sample size, the lens-specific verification rules (stale-refs / deprecations / registry-pin / library-versions / Counter-line), and any per-carrier scope (Agent-2-only for plan-update; Phase-3-vs-Phase-5 reference for plan-new; uniform-Sonnet for test-bootstrap).

**Schema additions (Phase 2 + Phase 3 final state):**

`SHARED-BLOCK:ledger-schema` gains:

```toml
[[vet_events]]
timestamp = 2026-05-08T14:32:00Z
command = "review"
agent_index = 2
lens = "security"
sampled_count = 5
dropped_count = 1
downgraded_count = 0
dropped_ids = ["R47"]
rationale = "R47 cited file:line that does not exist on disk"
```

with field documentation analogous to the existing `[[rollback_events]]` documentation block (append-only, archive convention, etc.).

`SHARED-BLOCK:execution-record-schema` gains, on `task-completion` entries:

```toml
[[items]]
id = "E7"
type = "task-completion"
date = 2026-04-18
agent = "implement"
task_ref = "add-retry-logic"
dispatch_tier = "lite"
dispatch_agent = "flow-implement-lite"
status = "done"
files = ["src/retry.rs"]
commits = ["abc1234"]
```

with the type-vocabulary table updated:

| Type | Required fields |
|------|-----------------|
| `task-completion` | `task_ref`, `status` ∈ {`done`, `failed`, `skipped`}, `files[]`, `dispatch_tier` ∈ {`lite`, `deep`}, `dispatch_agent` ∈ {`flow-implement-lite`, `flow-implement-deep`}; `commits[]` OPTIONAL |

Documentation notes: tier↔agent invariant (lite↔flow-implement-lite, deep↔flow-implement-deep); fail-soft on unknown values (treat unknown `dispatch_tier` as `deep`); fields are forward-only (historical entries without these fields render as `dispatch_tier = "(unknown)"` in derived views, never auto-backfilled).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
shared-block: bash scripts/verify-shared-blocks.sh
```

(Note: build/test/lint commands are project-wide and won't be exercised by these markdown-only edits, but `/implement` runs them anyway by convention. The `shared-block` command is the primary gate; the pre-commit hook also runs it automatically.)

## Tasks

### 1. Add `vet-flow-research` to the manifest [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: —
- **Action**: Add a `[[block]]` entry for `vet-flow-research` listing all 6 carrier files in alphabetical order: `claude/commands/optimise.md`, `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/review.md`, `claude/commands/review-plan.md`, `claude/commands/test-bootstrap.md`.
- **Detail**: Follow the existing TOML schema (`name = "..."`, `files = [...]`). Place the new entry at the end of the file (no sort constraint enforced by the verifier).
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes (the block doesn't yet exist in any carrier, so the verifier's missing-marker error fires for all 6 carriers — that's expected; this task only adds the manifest entry. Verification deferred to task 2's acceptance.)

### 2. Insert `vet-flow-research` block + reconcile carrier prose in 6 vet-section files [M]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-plan.md`, `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/test-bootstrap.md`
- **Depends on**: 1
- **Action**: For each of the 6 vet-section locations identified in Exploration Notes, insert the byte-identical `vet-flow-research` SHARED-BLOCK (as drafted in the Approach section, but WITHOUT step 6 — the `vet_events` writer directive — that lands in Phase 2). Adjust surrounding prose so the carrier-specific specialization (sample size, lens checks) reads naturally before/after the block.
- **Detail**: 
  - Block content for Phase 1 (no vet_events yet): steps 1, 2, 3, 4, 5, 7 (renumber to 1-6) and 8 (becomes 7) of the Approach-section sketch.
  - Carrier prose to adjust: in each carrier, the existing vet steps that are now in the block must be REMOVED from carrier prose (otherwise duplicated content). Each carrier retains: section heading (Step 2.5 / Phase 1.5 / Phase 2.5 / Phase 3 / etc.), context paragraph framing (e.g. "this vet pass runs after Phase 3 research returns"), sample-size statement (e.g. "Spot-check ≥3 per agent" or "Spot-check ≥5 per agent" or "Sonnet agents: 5; Opus agents: 3"), lens-specific verification rules (stale-refs / deprecations / registry-pin / library-versions / Counter-line), then the START marker, block content, END marker, then any per-carrier closing prose (e.g. plan-update's Agent-2-only scope note).
  - For `plan-update.md` Phase 1.5: prose must add the >30% rule explicitly to its Agent-2-only context (the carrier currently lacks it; the block's step 7 will inherit, but plan-update's prose around the block must clarify "this vetting and >30% rule applies to Agent 2 only; Agents 1 + 3 are vet-exempt for the reasons in the existing prose").
  - For `plan-new.md`: only ONE block instance (per the multi-instance constraint). Place it in Phase 3; Phase 5 retains its existing back-reference ("Vet returned findings with the same procedure as Phase 3 — see SHARED-BLOCK:vet-flow-research above").
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0 with `shared-block parity: OK`. All 6 carrier files have the new block markers + identical content between markers.

### 3. Add `[[vet_events]]` schema definition to `ledger-schema` SHARED-BLOCK [S]
- **Files**: `claude/commands/review.md`, `claude/commands/review-apply.md`, `claude/commands/optimise.md`, `claude/commands/optimise-apply.md`
- **Depends on**: 2
- **Action**: Add a `[[vet_events]]` schema definition + field documentation block to the existing `SHARED-BLOCK:ledger-schema`, positioned immediately after the existing `[[rollback_events]]` definition (lines 206–223 in review.md). The edit is byte-identical across all 4 carriers (same SHARED-BLOCK).
- **Detail**: Mirror the structure of the rollback_events block exactly (TOML example → field-by-field documentation → append-only convention paragraph). Schema fields (per User Decision #3): `timestamp`, `command`, `agent_index`, `lens`, `sampled_count`, `dropped_count`, `downgraded_count`, `dropped_ids`, `rationale`. Document append-only convention identically to rollback_events (existing entries never rewritten or deleted; archive to `<ledger>.vet-history.toml` if log grows unwieldy; no command automates archiving). Document field-length cap on `rationale` (≤8 KiB per the existing field-length-cap convention).
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0. All 4 ledger-schema carriers have the new schema block at the same line range.

### 4. Add the vet_events writer directive to `vet-flow-research` SHARED-BLOCK [S]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-plan.md`, `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/test-bootstrap.md`
- **Depends on**: 3
- **Action**: Insert step 6 ("Append a durable `[[vet_events]]` entry…") into the `vet-flow-research` SHARED-BLOCK in all 6 carriers. Renumber subsequent steps (the previous step 6 becomes step 7, previous step 7 becomes step 8). Edit is byte-identical across all 6 carriers.
- **Detail**: New step text: `6. Append a durable `[[vet_events]]` entry to the ledger via the canonical heredoc form: `cat <<'EOF' | tomlctl array-append <ledger> vet_events --json -` followed by the JSON payload conforming to Ledger Schema → vet_events. One entry per vetted agent (the `agent_index` field discriminates).` Also add a brief reference to the schema location: `(see SHARED-BLOCK:ledger-schema → vet_events for the field set)`. Carrier prose around the block stays unchanged.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0. The vet-flow-research block in all 6 carriers contains the new step 6 byte-identically.

### 5. Update carrier vet-section prose to remove "rationale logged in console" stub language [S]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-plan.md`, `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/test-bootstrap.md`
- **Depends on**: 4
- **Action**: At the writer sites identified in Exploration Notes (review.md:587, optimise.md:532, review-plan.md:209, plan-new.md:369, plan-update.md:660, test-bootstrap.md:296), remove or update any lingering "rationale logged in console" / "log the drop" prose that is now superseded by step 6's durable writer directive in the block.
- **Detail**: The block's step 6 is the durable record; carrier prose should NOT also instruct console logging as a separate action (the console line format is still in the block as step 7, so console output is preserved). Rationale-string content is captured in `vet_events.rationale`. Light prose edit per carrier — likely 1–2 lines per file.
- **Acceptance**: `grep -n "rationale logged in console"` returns no matches in the 6 carrier files. `bash scripts/verify-shared-blocks.sh` exits 0 (no SHARED-BLOCK content was touched, only carrier prose).

### 6. Add `dispatch_tier` + `dispatch_agent` to `execution-record-schema` SHARED-BLOCK [S]
- **Files**: `claude/commands/plan-new.md`, `claude/commands/implement.md`, `claude/commands/plan-update.md`, `claude/commands/tdd.md`
- **Depends on**: 5 (sequential commits keep phases independently revertible)
- **Action**: Update the `SHARED-BLOCK:execution-record-schema` to add `dispatch_tier` and `dispatch_agent` fields to `task-completion` entries. Edit is byte-identical across all 4 carriers.
- **Detail**:
  - Update the canonical TOML example (line ~115 in the schema block) to include both fields on the task-completion `[[items]]` entry — values `dispatch_tier = "lite"` and `dispatch_agent = "flow-implement-lite"`.
  - Update the type-vocabulary table (lines 145–153 within the block) — task-completion row gains `, dispatch_tier ∈ {lite, deep}, dispatch_agent ∈ {flow-implement-lite, flow-implement-deep}` to its Required-fields cell.
  - Add a new sub-section "`dispatch_tier` / `dispatch_agent` fields (task-completion)" right after the `commits` field note. Document: (a) tier is the abstract dispatch decision signal; agent is the concrete subagent_type; (b) tier↔agent invariant; (c) fail-soft on unknown values (unknown `dispatch_tier` → treat as `deep`; unknown `dispatch_agent` → preserve verbatim); (d) forward-only — historical entries without these fields are not auto-backfilled, render as `dispatch_tier = "(unknown)"` in PROGRESS-LOG-derived views.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0. The execution-record-schema block in all 4 carriers contains the new fields and documentation byte-identically.

### 7. Update `implement.md` Phase 2 step 5b writer to emit `dispatch_tier` + `dispatch_agent` [S]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 6
- **Action**: Update the canonical JSON heredoc template at `implement.md:483` and the surrounding writer prose (lines 464–486) to include `dispatch_tier` and `dispatch_agent` in the task-completion JSON payload.
- **Detail**:
  - JSON template: insert `"dispatch_tier":"<lite|deep>","dispatch_agent":"<flow-implement-lite|flow-implement-deep>"` between `"task_ref":"..."` and `"summary":"..."`.
  - Writer prose: add a new bullet after the existing `task_ref` / `status` / `files` / `commits` bullets describing dispatch_tier / dispatch_agent population: "`dispatch_tier` — the lite-eligibility-gate decision recorded in the DISPATCH header for this task. `dispatch_agent` — the concrete subagent_type used (`flow-implement-lite` or `flow-implement-deep`). Both MUST be populated; the orchestrator has these values when assembling the prompt's DISPATCH header (Phase 2 step 2)."
  - Update the example payload at line 483 to include the two new fields.
- **Acceptance**: The JSON template and prose mention both fields. `bash scripts/verify-shared-blocks.sh` exits 0 (only the writer-prose section was touched, not the SHARED-BLOCK).

### 8. Mark R104, R119, R120 as fixed in the ledger [S]
- **Files**: `.claude/reviews/claude-commands.toml`
- **Depends on**: 7
- **Action**: Use `tomlctl items apply --ops -` (heredoc) to transition R104, R119, R120 from `wontfix` to `fixed`. Set `resolved = 2026-05-08`, `resolution = "Refactored via plan vet-flow-research-and-schema-additions; commits <SHA1>, <SHA2>, <SHA3>"` (one SHA per phase), and clear `wontfix_rationale` (set to empty string or omit). Bump `last_updated`.
- **Detail**: One ops payload with three `update` ops + one `set last_updated` call. Use the canonical heredoc form per `feedback_tomlctl_stdin_never_tempfile` (no tempfile staging).
- **Acceptance**: `tomlctl items list .claude/reviews/claude-commands.toml --where-in id=R104,R119,R120 --pluck status` returns `["fixed","fixed","fixed"]`.

## Dependency Graph

```
1 (manifest) → 2 (block + carrier prose) → 3 (vet_events schema) → 4 (writer directive in block) → 5 (carrier prose cleanup)
                                                                                                  → 6 (dispatch_type schema) → 7 (writer in implement.md) → 8 (ledger update)
```

All 8 tasks are sequential. Phase boundaries:
- **Phase 1 (R104)**: tasks 1, 2 — manifest entry + block insertion + carrier prose. One commit at end.
- **Phase 2 (R120)**: tasks 3, 4, 5 — schema definition + writer directive + carrier prose cleanup. One commit at end.
- **Phase 3 (R119)**: tasks 6, 7 — schema + writer. One commit at end.
- **Final**: task 8 — ledger reconciliation. Same commit as Phase 3 (or separate commit, either is fine).

No parallelism within phases — byte-strict shared-block edits are best done by a single agent with the full file list to avoid drift between near-simultaneous writes.

## Verification

End-to-end:
1. After each phase commit: `bash scripts/verify-shared-blocks.sh` → expect `shared-block parity: OK`.
2. After Phase 1: spot-check that the new block's step 7 (>30% rule) is now uniformly enforced — `grep -n "30%" claude/commands/{review,optimise,review-plan,plan-new,plan-update,test-bootstrap}.md` shows the rule appearing inside the block markers in all 6 files.
3. After Phase 2: spot-check that `[[vet_events]]` schema appears in all 4 ledger-schema carriers — `grep -n "\[\[vet_events\]\]" claude/commands/{review,review-apply,optimise,optimise-apply}.md` shows it exactly once per carrier (in the schema block, not in any carrier-specific writer prose).
4. After Phase 3: spot-check the type-vocabulary table — `grep -n "dispatch_tier" claude/commands/{plan-new,implement,plan-update,tdd}.md` shows it in all 4 carriers.
5. Pre-commit hook fires automatically on each `git commit` and runs the verifier; refuses commits that drift.
6. No tomlctl source changes — `cargo build` / `cargo test` are not exercised by this plan but should be run as a sanity check after Phase 3 to confirm no spec edit accidentally references a removed/renamed tomlctl flag.
7. Ledger reconciliation (task 8) is a /review hygiene step, not a verification step per se.

## Risks

- **Risk**: A carrier's prose edit accidentally drifts the block content (e.g. an off-by-one indent change inside the block markers). **Mitigation**: pre-commit hook catches all drift; the implementing agent must re-run `bash scripts/verify-shared-blocks.sh` locally before each phase commit.
- **Risk**: `plan-new.md`'s Phase 5 retains its back-reference but the prose drifts from "see Phase 3 above" to "see SHARED-BLOCK:vet-flow-research" or vice versa, causing reader confusion. **Mitigation**: task 2 detail explicitly mandates the back-reference text.
- **Risk**: A future writer adds a new dispatch type (e.g. `flow-research-deep` for orchestrator-side deep research) and the schema's hardcoded vocabulary in the type table goes stale. **Mitigation**: the schema's fail-soft rule (unknown `dispatch_tier` → treat as `deep`, unknown `dispatch_agent` → preserve verbatim) keeps readers compatible; the schema doc references the type table as advisory rather than exhaustive.
- **Risk**: Existing `task-completion` entries in `.claude/flows/*/execution-record.toml` files lack the new `dispatch_tier` / `dispatch_agent` fields. **Mitigation**: forward-only schema; the schema doc explicitly notes that historical entries are NOT auto-backfilled and render as `(unknown)` in derived views. No migration needed because all referenced flows are already `status = "complete"`.
- **Risk**: An interleaved /review run during Phase 2 writes a new finding while the ledger-schema is being edited, hitting parity-check failure. **Mitigation**: Phase 2 is a single commit; parity check is on commit, not during edit. If a /review runs during the edit window, its writes target ledger-schema's `[[items]]` section (data), not the schema block itself (definition), so no conflict.
