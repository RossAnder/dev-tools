# Plan: Ledger-Coupled Commands — Gap Analysis & Opus 4.7 Retuning

**Plan path**: `.claude/plans/command-suite-improvements.md`
**Original draft**: `/home/ross/.claude-work/plans/shimmering-imagining-journal.md` (harness-managed, superseded by this file on 2026-04-17)
**Created**: 2026-04-17
**Last updated**: 2026-04-17
**Status**: Complete — Tracks 1–3 all landed (15/15, 11/11, 8/8)

---

## Progress

### Track 1 — Quick Wins (15/15 complete, 2026-04-17)

| ID | Status | Landing site | Evidence |
|----|--------|--------------|----------|
| T1.1 | ✅ done | `optimise.md:286` | `### Staleness Pre-Check` ported from `review.md:273-277`; "run runs" artefact polished to "proceeds" |
| T1.2 | ✅ done | `optimise-apply.md` Step 5 | Critical-finding gate with category set `{memory, query, concurrency}`; references /optimise Step 4 for disposition |
| T1.3 | ✅ done | `optimise-apply.md` Step 5 | Secret-pattern scan of ledger payload (5 patterns incl. `AKIA`, `-----BEGIN`, `api[_-]?key`) before `tomlctl items apply` |
| T1.4 | ✅ done | `optimise-apply.md:363-373` | `### Verify agent-reported applied claims` with git-diff union (HEAD + cached + ls-files untracked); R→O / fixed→applied / wontfix→wontapply applied; verified-clean branch dropped |
| T1.5 | ✅ done | `optimise-apply.md:344` | Partial-apply child-item minting with O-prefix (parent gets `resolution = "partial: …"`, child `related = ["O{n}"]`) |
| T1.6 | ✅ done | `optimise-apply.md:293-297` | Deleted-file detection now branches source / auto-generated; both branches transition to `wontapply` but preserve semantic distinction in rationale text |
| T1.7 | ✅ done | `optimise-apply.md:299` | Concurrency `invariant narration` rule added to Step 2 (lock-ordering, async-boundary, channel-capacity examples) |
| T1.8 | ✅ done | `optimise.md:468` | Clock-skew / backdated `last_updated` validation ported from `review.md:306` |
| T1.9a | ✅ done | `optimise.md:353` | `### Design Note: Intentional Asymmetry with /review` — documents why `/optimise` always runs 5 agents (no small-diff shortcut) |
| T1.9b | ✅ done | `review.md:333` | `### Design Note: Intentional Asymmetry with /optimise` — documents why `/review` has no focal-points synthesis counterpart |
| T1.10 | ✅ done | `optimise.md:390`, `review.md:357` | Findings-per-agent cap 10 → target 15 / ceiling 20; 4-clause truncation-priority rule appended (severity > evidence > anchor > prose) |
| T1.11 | ✅ done | Step 1 of all 4 commands | "Batched tool calls" directive — byte-identical wording mandating single-response-message emission for independent reads |
| T1.12 | ✅ done | both apply commands | Tiered selector cap (≤25 normal, 26–30 warn, >30 abort) replaces prior "non-negotiable 15"; Opus 4.7 context-budget rationale cited inline |
| T1.13 | ✅ done | both apply commands | `### Freshness gate` sub-section — compares ledger `last_updated` to `git log -1 --format=%cI` per selected file; `[p]roceed / [r]e-run / [a]bort` prompt |
| T1.14 | ✅ done | `.claude/reviews/claude-commands.toml` R32 | `wontfix_rationale` refreshed via `tomlctl items update --json -` (888 chars; references this plan's T2.1 and T3.7); status preserved as `wontfix` |
| T1.15a | ✅ done | `optimise.md:306` | `### Orphan surfacing (read-only)` — walks open items, flags missing-file / missing-symbol cases via Glob + Grep; no auto-transition |
| T1.15b | ✅ done | `review.md:315` | `### Orphan surfacing (read-only)` — same pattern, R-prefix examples |

**Verification evidence**:
- Shared-block parity **intact** across all 4 files after Track 1 edits — `## Flow Context` sha256 `6c12c1c4…`, `## Ledger Schema` sha256 `2d360b3e…` byte-identical (verified via awk-extract + sha256sum with proper single-`#` terminator).
- Cross-cutting wordings (T1.11 / T1.12 / T1.13) landed byte-identically: 1 occurrence per file/command-pair for each directive.
- R→O port cleanliness: 0 residual `"fixed"` / `wontfix_rationale` references in `optimise-apply.md` command-specific region (lines 247+); 2 intentional `verified-clean` mentions remain as contrast ("unlike /review-apply…").
- `tomlctl` test suite: **16/16 pass** (no regressions from the R32 write).
- 5 files touched in working tree, uncommitted: `optimise.md`, `review.md`, `optimise-apply.md`, `review-apply.md`, `.claude/reviews/claude-commands.toml`.

### Track 2 — Structural Changes (11/11 complete, 2026-04-17)

| ID | Status | Landing site | Evidence |
|----|--------|--------------|----------|
| T2.1 | ✅ done | `scripts/verify-shared-blocks.sh`, `scripts/shared-blocks.toml`, `.githooks/pre-commit`, `CLAUDE.md` | Parity infrastructure: bash+awk block extraction with sha256sum (shasum -a 256 fallback); manifest lists two blocks × 4 files; pre-commit hook only fires when a command file is staged; top-level CLAUDE.md documents `git config core.hooksPath .githooks` and rejects `--no-verify` on command files. `bash -n` clean; scripts chmod +x. |
| T2.2 | ✅ done | all 4 command files | `<!-- SHARED-BLOCK:flow-context START/END -->` and `<!-- SHARED-BLOCK:ledger-schema START/END -->` wrap both shared blocks; 4 markers per file; `scripts/verify-shared-blocks.sh` exits 0. |
| T2.3 | ✅ done | Step 2 of `optimise.md` / `review.md`; Step 4 of `optimise-apply.md` / `review-apply.md` | `### Task tracking (runtime only)` sub-section: 5 lens-tasks in /optimise, 4 in /review, per-cluster + verification task in apply commands; no per-finding tasks; no task-handoff across commands (tasks ephemeral, ledger persists). |
| T2.4 | ✅ done | new `## Interim checkpoint` section between Steps 2/3 in find commands and Steps 4/5 in apply commands | Persists non-risky transitions (new items, `verified-clean`, `wontapply`/`wontfix`/skipped) via one atomic `tomlctl items apply --ops -`; defers `fixed`/`applied` + `tomlctl set last_updated` to final render; Step 1 idempotency guards handle re-entry. |
| T2.5 | ✅ done | `### Pre-analysis delegation (selector ≥ 10 items)` at start of Step 2 in both apply commands | Explore subagent (thoroughness: quick) returns 4-class table (already-in-place / drifted / fresh / missing-file) under 800 words; orchestrator keeps only the table, reclaiming ~300 KB context; < 10 items keeps inline path. |
| T2.6 | ✅ done | `### Scope classification delegation` between `### Identify Files` and `### Load Review Ledger` in `review.md` | Explore subagent (thoroughness: quick) delegates classification + CLAUDE.md excerpt gathering when scope > 10 files; capped at 600 words; skipped when small-diff shortcut fires (≤ 3 files). |
| T2.7 | ✅ done | `## Step 5.5: Rollback protocol` between Step 5 and Step 6 in both apply commands | Trigger set (build fail on touched path / test regression outside ledger scope / `applied` tag without diff); `git stash push -u` → `git checkout -- <paths>` → narrowly-scoped `git clean -fd -- <path>` (never bare) → reverse `tomlctl items apply` batch → `[[rollback_events]]` append. Non-interactive defaults to skip. |
| T2.8 | ✅ done | shared `## Ledger Schema` in all 4 command files | 4 new optional fields added (`depends_on`, `fingerprint`, `rollback_rationale`, `reopen_rationale`) + new `#### Rollback event log` sub-section documenting `[[rollback_events]]` root table; coordinated byte-identical; `scripts/verify-shared-blocks.sh` reports `ledger-schema` sha256 `3f35a3ed…` identical across all 4 files. |
| T2.9 | ✅ done | `### Dependency sort (topological)` at start of Step 3 in both apply commands | Kahn's algorithm pseudocode over `depends_on` subset-in-selected-set; forward refs dropped; cycle detection fail-fast with cycle path printed; file clustering runs within each topo level; backward-compatible when `depends_on` absent everywhere. |
| T2.10 | ✅ done | `### Already-applied test (Tier 1 normalization)` in Step 2 of both apply commands | Tier 1: normalize intra-line whitespace + CRLF→LF + trailing-whitespace strip, preserve leading whitespace; substring match → pre-transition. Tier 2: `uncertain_already_applied = true` flag passed to Step 4 agent for semantic-judgement cases. Hard rule: no bytes written → never `applied`/`fixed`. |
| T2.11 | ✅ done | `### Deferred-item reopen sweep` after orphan surfacing in Step 1 of `review.md` and `optimise.md` | Walks `status = "deferred"` items; 5 known trigger forms (`after <path> exists`, `after <file>:<symbol> landed`, `when <id> resolves`, `after <branch> merges`, `after <YYYY-MM-DD>`) + free-text fallback (surface only); `[y]/[n]/[a]` prompt per candidate; confirmed reopens batched into one `tomlctl items apply --ops -` with `reopen_rationale` recorded. Non-interactive surfaces candidates only. |

**Verification evidence**:
- **Shared-block parity**: `scripts/verify-shared-blocks.sh` exits 0 (`shared-block parity: OK`); `flow-context` sha256 `efd5619a706fcc012f2c1741cea7318b210e155048625ca04be7e09401f274f2`, `ledger-schema` sha256 `3f35a3ed81ca96cf3e3ce961717c4941d2c7292bf9ec8154ac5833a4a9520ab8` — byte-identical across all 4 command files.
- **Heading counts**: `optimise.md`=3 new headings, `review.md`=4, `optimise-apply.md`=6, `review-apply.md`=6 — all match expected per-file Track 2 item list.
- **Apply-command symmetry**: `diff` on the 6 new heading lines between `optimise-apply.md` and `review-apply.md` is empty — structural parity across both apply commands preserved (only R↔O / `fixed`↔`applied` / `wontfix`↔`wontapply` / `verified-clean`-exclusive-to-review differences appear inside the content).
- **`tomlctl` test suite**: **16/16 pass** (no regressions — no Rust source touched).
- **Planning-artefact leakage check**: no `T2.x` / `Batch 2` / `Track 2` / `Agent 6` mentions in the four command files; the 9 grep hits were pre-existing `### Agent N: <lens>` headings (legitimate lens-role labels in /optimise and /review, not Track 2 leakage).
- **Fenced-code balance**: Pre-analysis delegation sub-sections in both apply files have balanced (even) triple-backtick fences.
- **No plan deviations**: 2 minor agent-side choices for T2.10 landing (fallback sibling sub-section after the "For each selected finding:" bulleted list rather than mid-list insertion) were pre-sanctioned in the agent prompts as acceptable alternatives. All anchors matched first-try for every other edit.

### Track 3 — Tomlctl Features + Prompt-Schema (8/8 complete, 2026-04-17)

| ID | Status | Landing site | Evidence |
|----|--------|--------------|----------|
| T3.1 | ✅ done | `tomlctl/src/main.rs` global flags + `Cargo.toml` | Global `--no-write-integrity` / `--verify-integrity`; sidecar `<file>.sha256` in sha256sum format, written under the exclusive lock after tempfile+rename, stderr-warn on sidecar-write failure (primary write already durable); `--verify-integrity` wired into every read path (`Parse`, `Get`, `Validate`, `Items::{List,Get,NextId}`); `sha2 = "^0.10"` added. Wired into `/review` Step 1 ledger-load and `/optimise` Step 1 ledger-load prose — `tomlctl --verify-integrity parse/items list <ledger>`. Wired into `/review-apply` ledger-integrity note (replaces "future enhancement" framing). Tests: `write_integrity_sidecar_roundtrip`, `verify_integrity_errors_on_missing_sidecar`, `no_write_integrity_suppresses_sidecar`, `verify_rejects_malformed_sidecar`. |
| T3.2 | ✅ done | `tomlctl/src/main.rs` `Cmd::Blocks` + `BlocksOp::Verify` | `tomlctl blocks verify <file>... [--block <name>]...` — awk-equivalent byte extraction between `<!-- SHARED-BLOCK:<name> START/END -->` markers; JSON output `{ok, blocks:[{name, hash, files, missing, drift?}]}`; marker-absent → per-file error; exit non-zero on drift. Tests: `blocks_verify_detects_drift`, `blocks_verify_reproduces_shell_hashes` (asserts `flow-context` = `efd5619a706fcc012f2c1741cea7318b210e155048625ca04be7e09401f274f2` and `ledger-schema` = `458ddbb835f65bbe99314ef64581d24768acc296a239f1c704ac8c412468ab7d`; the latter refreshed after this run's T3.5/T3.4 edits to the shared block). End-to-end smoke: `tomlctl blocks verify claude/commands/*.md --block flow-context --block ledger-schema` agrees with `scripts/verify-shared-blocks.sh`. |
| T3.3 | ✅ done | `tomlctl/src/main.rs` `ItemsOp::Update` + `apply_single_op` | `tomlctl items update <file> <id> --json <json> [--unset <key>]...` — repeatable `--unset`; JSON patch applied first, then unset (missing keys silently no-op). `items apply` `"update"` ops accept optional `unset: [...]` array; back-compat when absent. Tests: `items_update_unset_removes_field`, `items_apply_unset_respected_in_batch`. |
| T3.4 | ✅ done | `tomlctl/src/main.rs` `ItemsOp::FindDuplicates` | `tomlctl items find-duplicates <file> [--tier A\|B\|C]` — Tier A (default, canonical dedup rule); Tier B (16-char SHA-256 fingerprint over `file\|summary\|severity\|category\|symbol`, grouped by fingerprint + `Path::file_name`); Tier C (sort-and-sweep, line window ≤ 10 for symbol-less items). JSON array of `{tier, key, items:[...]}` groups, `[]` when no dups. Read-only. Tests: `find_duplicates_tier_a_groups_by_symbol_or_summary`, `find_duplicates_tier_c_uses_line_window`. |
| T3.5 | ✅ done | `tomlctl/src/main.rs` `ItemsOp::Orphans` | `tomlctl items orphans <file>` — JSON array of `{id, class, file, symbol?, dangling_deps?}`; classes `missing-file` / `symbol-missing` / `dangling-dep`. Uses `std::fs::read_to_string` + `String::contains` (no external ripgrep). Wired into `/review` and `/optimise` Step 1 orphan-surfacing prose (replaces "hand-rolled Glob/Grep walk" guidance). Test: `items_orphans_reports_missing_file_symbol_and_dangling_dep`. Shared-block line 161 (`depends_on`) also refreshed to reference the shipped subcommand. |
| T3.6 | ✅ done | `tomlctl/src/main.rs` `ItemsOp::List` + `ListFilters` | `tomlctl items list <file> [--status X] [--category Y] [--newer-than YYYY-MM-DD] [--file PATH]` — three new filters combine via AND with existing `--status`; `--newer-than` parsed as TOML date at arg-parse time, errors clearly on malformed input. `items_list` signature refactored to take a `ListFilters<'_>` struct (internal only — dispatcher is in the same file). Tests: `items_list_filters_combine_with_and`, `items_list_newer_than_rejects_bad_date`; pre-existing `items_list_filters_by_status` updated to new struct API. |
| T3.7 | ✅ done | Plan log (this entry) | Revisited per plan: T2.1's parity script shipped and the T1.14 refresh to R32's `wontfix_rationale` is current. No evidence emerged during Tracks 1 and 2 of byte-identical block hashing being "too strict in practice" (the four shared-block edits of this run all preserved the manifest). R32 remains `wontfix`; no `tomlctl` or ledger change required. The T3.7 pre-condition ("evidence that byte-identical is too strict") continues to gate any future re-evaluation. |
| T3.8 | ✅ done | `optimise-apply.md` Step 4 agent instructions; `review-apply.md` Step 4 agent instructions | **Tier-2 already-applied protocol** added as a new agent-prompt instruction in Step 4 of both apply commands (byte-matched anchor pair — hard-rule bullet in each file). Instructs the agent that when orchestrator-set `uncertain_already_applied = true` is in its prompt, the FIRST action MUST be a read-verification pass; if structurally in place (reordered clauses, equivalent refactorings, paraphrased API choices, moved code) emit `skipped O{n}: already in place (tier-2), no byte written` (optimise) or `verified-clean R{n}: matches recommendation (tier-2)` (review), writing zero bytes. Orchestrator Step 5 mutation tables already carry these through to `wontapply` (optimise) / `verified-clean` (review); the `(tier-2)` marker now flows into `wontapply_rationale` / `verified_note` so audits can distinguish Tier-2 from Tier-1 pre-transitions. |

**Verification evidence**:
- **tomlctl test suite**: 33/33 pass (up from 16/16 after Tracks 1/2); `cargo clippy --all-targets -- -D warnings` clean (fixed 8 pre-existing lints — 7× `collapsible_if` after clippy tightened, 1× `suspicious_open_options` on the lock-file open — with semantic-preserving transforms; no `#[allow]` added).
- **Shared-block parity**: `scripts/verify-shared-blocks.sh` exits 0 after the line-161/162 edits (byte-identical across all 4 command files). `flow-context` sha256 unchanged at `efd5619a…`; `ledger-schema` sha256 updated to `458ddbb8…` (reflects the shipped-not-deferred rewording for `depends_on` / `fingerprint`). The new `tomlctl blocks verify` subcommand agrees with the shell script on both digests end-to-end.
- **No sidecar pollution in the repo**: `/home/ross/Dev/dev-tools/.claude/**/*.sha256` is empty — default-on integrity writes only fire when a command actually mutates a ledger. This run did not mutate any ledger outside of `tomlctl`'s own tests (which use tempdirs).
- **`--verify-integrity` end-to-end**: tested in `write_integrity_sidecar_roundtrip` (roundtrip), `verify_integrity_errors_on_missing_sidecar` (missing sidecar → clear error), `verify_rejects_malformed_sidecar` (malformed sidecar rejected), and manually via the CLI on a temp ledger.
- **Plan-deviation summary**: one minor — the Rust agent refactored `items_list` to take a `ListFilters<'_>` struct (instead of 4 loose `Option<&str>` args) to accommodate T3.6's three new filters cleanly; external contract unchanged because the dispatcher is in the same file. Pre-existing clippy errors were fixed to meet `-D warnings` (purely mechanical). Neither deviation affects behaviour or the plan's intended surface.

### Session Log

| Date | Operation | Result |
|------|-----------|--------|
| 2026-04-17 | `/plan-new` | Plan written (3 Explore + 3 Plan agents; 28 gaps across 5 categories; 34 recommendations across 3 tracks) |
| 2026-04-17 | `/implement track 1` | 3 parallel agents (optimise-apply / optimise / review) + 4 orchestrator cross-cutting edits + 1 tomlctl ledger update; 15/15 Track 1 items landed; shared-block parity preserved |
| 2026-04-17 | `/plan-update` | Status changed Draft → In progress; Track 1 marked complete (flow-less, plan at `~/.claude-work/plans/`) |
| 2026-04-17 | Plan relocation | Plan copied to `.claude/plans/command-suite-improvements.md` (canonical); `plan_path` header updated to reflect new location |
| 2026-04-17 | `update-config` | `.claude/settings.json` created with `plansDirectory = ".claude/plans"` so future plan-mode plans land in-repo |
| 2026-04-17 | Flow registration | `.claude/flows/command-suite-improvements/context.toml` written (scope: commands/tomlctl/scripts/.githooks/CLAUDE.md; tasks 34/15/0); `.claude/active-flow` pointer set. Future `/plan-update`, `/implement`, `/review`, `/optimise` invocations auto-resolve this flow via the 5-step resolution order. |
| 2026-04-17 | `/plan-update status` | Re-run after flow registration; context.toml validated via `tomlctl validate`; no plan-file changes (Track 1 evidence already current) |
| 2026-04-17 | `/implement track 2` | Batch 1 (2 parallel agents): T2.1 parity infra (scripts/ + .githooks/ + CLAUDE.md) and T2.2+T2.8 byte-identical shared-block edits (markers + 4 optional fields + Rollback event log sub-section). Batch 2 (4 parallel agents, one per command file): T2.3 (Task tracking), T2.4 (Interim checkpoint), T2.5 (Pre-analysis delegation), T2.6 (Scope classification delegation — /review only), T2.7 (Step 5.5 Rollback protocol), T2.9 (Dependency sort topological), T2.10 (Already-applied Tier 1), T2.11 (Deferred-reopen sweep). 11/11 items landed. Shared-block parity preserved (sha256 matches across all 4 files for both blocks); apply-command heading symmetry verified; tomlctl 16/16 still pass; zero fix-attempts used. |
| 2026-04-17 | `/plan-update status` | Track 2 marked complete (11/11); `context.toml` updated: tasks.completed 15 → 26; tasks.in_progress 0; status preserved `in-progress` (Track 3 still open, 0/8); `updated` refreshed to 2026-04-17; `created` preserved verbatim. |
| 2026-04-17 | `/implement track 3` | Batch 1 (single Rust agent on `tomlctl/src/main.rs` + `Cargo.toml` + `README.md` + `SKILL.md`): T3.1–T3.6 in one coordinated landing — sidecar integrity, `blocks verify`, `items update --unset`, `items find-duplicates`, `items orphans`, compound `items list` filters; 33/33 tests pass (up from 16), clippy `-D warnings` clean. Batch 2 (orchestrator-direct prose edits to all 4 command files): shared-block lines 161/162 refreshed byte-identically to reference shipped subcommands (`ledger-schema` block sha256 → `458ddbb8…`, `flow-context` unchanged); orphan surfacing wording in `/review` + `/optimise` swapped from "future subcommand" to `tomlctl items orphans <ledger>`; `/review` + `/optimise` Step 1 ledger-load prose now recommends `tomlctl --verify-integrity parse/items list`; `/review-apply` ledger-integrity note replaces "future enhancement" framing with shipped behaviour; both apply commands' Step 4 agent prompts gained the Tier-2 `uncertain_already_applied` read-verification protocol (T3.8). T3.7 kept as wontfix per plan's gating pre-condition (no evidence of byte-identical being too strict emerged). 8/8 items landed. Shared-block parity verified via `scripts/verify-shared-blocks.sh` AND `tomlctl blocks verify` — both agree on the new digests. Zero fix-attempts consumed (one test expectation refresh after the shared-block edit changed the `ledger-schema` digest — not a retry, just a planned catch-up on a hard-coded hash in the new test). |

---

## Context

Today's commit series (`033c626 → 30adfa7`) carried out a significant rework of four command files in `claude/commands/`:

- `/optimise` and `/review` — "find" commands that emit research findings into a TOML ledger.
- `/optimise-apply` and `/review-apply` — "apply" commands that transition open items to `fixed` / `applied` / `wontfix` / `wontapply` / `verified-clean`.

The rework established:

1. Two **byte-identical shared blocks** across all four files — `## Flow Context` (lines 6–91) and `## Ledger Schema` (lines 93–246).
2. A new Rust binary, **`tomlctl`**, for all TOML CRUD: atomic writes, exclusive locking, path canonicalisation under `.claude/`, stdin piping (`-` sentinel), and batch `items apply --ops` operations.
3. A **find → apply split** where `/optimise` and `/review` only emit findings, and `/optimise-apply` / `/review-apply` only mutate state based on ledger IDs.

Evidence of the rework's rigour: the 40-finding ledger `.claude/reviews/claude-commands.toml` that drove it was closed this evening with 39 `fixed` / 1 `wontfix`.

The suite is now coherent — but the rework's own 40 findings surfaced gaps that recurred (shared-block drift, shell-quoting, apply-command asymmetries). With the rework's dust settled, this plan steps back and asks: **what should the next round of improvements look like, given Opus 4.7's 1M context and the ecosystem of orchestration primitives available to it (subagents, TaskCreate, parallel batching, Explore/Plan subagent types)?**

This plan produces a structured gap register and a three-track recommendation list (Quick Wins / Structural / Deferred). It does **not** produce code edits — each recommendation is sized so that a follow-up `/implement` run can tackle one track at a time.

---

## Scope

**In scope**:
- `claude/commands/optimise.md` (518 lines)
- `claude/commands/review.md` (514 lines)
- `claude/commands/optimise-apply.md` (432 lines)
- `claude/commands/review-apply.md` (498 lines)
- Shared-block parity mechanism (new script + optional pre-commit hook)
- Ledger-schema optional-field additions (coordinated across all four files)
- Evaluation of new `tomlctl` subcommands (deferred implementation)

**Out of scope**:
- `claude/commands/{plan-new,implement,plan-update,review-plan,review-gh}.md` — referenced as precedent patterns, not edited.
- `tomlctl` source-code changes themselves (the plan identifies what subcommands/flags would help; actual Rust implementation is deferred).
- `claude/skills/tomlctl/SKILL.md` is touched only when a new `tomlctl` feature lands.

**Affected areas**:
- `claude/commands/**`
- `scripts/**` (new verification script)
- `.githooks/**` (new pre-commit)
- `CLAUDE.md` (top-level, one-line developer-setup note)

**Estimated file count**: 9 unique files for Tracks 1 + 2 (four command files, one script, one hook, one shared-blocks manifest, one CLAUDE.md snippet, one ledger to update rationale on R32). Track 3 adds `tomlctl/src/main.rs` and `claude/skills/tomlctl/SKILL.md` for deferred work.

---

## Exploration Notes

Three parallel Explore agents produced a grounded snapshot (full results preserved in conversation; condensed here):

### Command structure
- All four files share byte-identical `## Flow Context` (lines 6–91) and `## Ledger Schema` (lines 93–246). Parity is asserted in the prompt text ("enforced by SHA-256 parity") but **no runtime or CI mechanism enforces it**. The existing 5 findings (R1, R2, R32, R33, R35) that tracked parity drift were all one-shot normalisations.
- Per-command headings use `## Step N`; apply commands also carry a `### Phase 4.5: Sync plan context` sub-heading inherited from `implement.md` by design (not accidental drift).
- `/optimise` always runs 5 research agents (Memory, Serialization, Queries, Algorithm, Async); `/review` runs 4 (Quality, Security, Architecture, Completeness) with a small-diff 1-agent shortcut when ≤ 3 files.
- `/optimise-apply` and `/review-apply` launch N cluster agents (one per file cluster), with sequential batches when same-file dependencies exist.
- **No TaskCreate/TaskUpdate usage** in any of the four commands — unlike `implement.md:136`.

### tomlctl surface
- Commands: `parse`, `get`, `set`, `set-json`, `validate`, `items list` (with `--status` filter), `items get`, `items add`, `items update`, `items remove`, `items apply` (with `--ops`), `items next-id` (with `--prefix`).
- Safety: 30-second backoff on `try_lock_exclusive`; atomic temp-file rename writes; `guard_write_path` canonicalises and asserts target under `.claude/` (overridable via `--allow-outside`); empty + all-digit prefix guards on `items next-id`.
- All JSON-accepting flags support the `-` stdin sentinel.
- **Missing**: integrity verification (SHA-256 sidecar, R29 deferred), ledger diff, compound queries (`--status + --category`), dedup/duplicate-detection, orphan detection, bulk `set` / atomic multi-key writes.

### Ledger evidence (`.claude/reviews/claude-commands.toml`)
- 40 findings total: 6 critical, 14 warning, 9 suggestion (plus 11 unlabelled in the initial mint); categories distributed across architecture (7), security (7), quality (6), completeness (9+).
- Recurring themes: shared-block drift (R1, R2, R32, R33, R35); apply-command asymmetry (R5–R8, R22–R26, R31, R34, R36, R37); shell-quoting (R3, R9); orchestrator-side verification gaps (R4, R10, R40); deferred-trigger ambiguity (R28); chronic-item escalation (R39); selector-cap override ambiguity (R17).
- Wontfix: R32 (shared block for TOML write ops — DRY win outweighed by coordination cost).
- Deferred-implementation-but-dispositioned: R29 (integrity SHA-256 sidecar), R31 partially (Apply Security Gates umbrella — point-fixes landed; umbrella deferred).

### Cross-command comparison
- `plan-new.md` and `implement.md` are the orchestration-rich reference patterns: multiple subagent types (`Explore`, `general-purpose`, `Plan`), TaskCreate tracking, interim-checkpoint writes between phases, explicit word-count caps with truncation-priority rules (e.g. `plan-new.md:147` — "If you must truncate, prioritise file paths and interface signatures over narrative").
- The four target commands adopt **some** of these patterns (subagent delegation, single-message parallel dispatch, word caps) but **not others** (TaskCreate, interim checkpointing, truncation-priority rules).

---

## Research Notes

External research (Context7 / WebSearch) was **not run as a separate phase**. Rationale:

1. The authoritative reference for LLM-agent orchestration patterns in this repo is Anthropic's own `plan-new.md` and `implement.md` — which are internally present and already consulted.
2. The patterns under discussion (findings ledger, two-phase find→apply, shared-block sync, batch CRUD over a TOML index) are **project-specific**; there is no dominant external literature that would generate novel recommendations beyond what Phase 2's three Plan agents produced by reasoning from the codebase.
3. The 40-finding ledger (`.claude/reviews/claude-commands.toml`) is itself a high-quality empirical dataset of *what actually went wrong* during the rework — more actionable than generic external guidance.

**First-principles observations** that inform the recommendations below:

- **Byte-identical shared blocks without automated enforcement reliably drift.** Evidence: R1, R2, R33, R35 — four distinct normalisation passes in one day. Any fix must put the parity check somewhere that fires on every write path a maintainer uses.
- **Two-phase find→apply benefits from ID-stable persistence (the ledger) but exposes three failure modes**: (a) code changes between find and apply (freshness gap), (b) agent forges an `applied` claim without writing bytes (trust gap), (c) partial applies lose pending work to free-prose resolutions (tracking gap). The rework addressed (b) via diff-reconciliation in `/review-apply` and (c) via partial-apply child-item minting in `/review-apply`; (a) remains uncovered; (b) and (c) remain absent in `/optimise-apply` — an accidental asymmetry.
- **Opus 4.7's 1M context window invalidates caps calibrated for shorter-context models.** Findings-per-agent caps of 10 (`optimise.md:360`, `review.md:333`) and pre-analysis selector caps of 15 (`optimise-apply.md:288`, `review-apply.md:291`) were conservative choices that no longer reflect the active model. 15 agents reading ±50 lines ≈ 150KB, well within budget; even 30 is ~300KB.
- **The `Explore` subagent type is under-used.** It exists precisely for read-only codebase navigation, which is what the apply commands' pre-analysis step does — currently inlined into the orchestrator. Delegating pre-analysis to `Explore` reclaims orchestrator context.
- **Ledger operations already benefit from idempotency by design** (`status ∈ {fixed, applied, wontfix, wontapply, verified-clean}` items are skipped on re-entry). This makes interim ledger writes between agent batches safe — a recovery-friendly cheap win.

---

## Gap Register

Gaps are grouped by type. Each entry cites file:line and a severity rating (critical/warning/suggestion) using the same vocabulary the review ledger uses.

### A. Shared-block parity & drift

| Gap | Evidence | Severity |
|-----|----------|----------|
| A1. No runtime or CI mechanism enforces `## Flow Context` + `## Ledger Schema` parity across the four files. The prompt claims "SHA-256 parity" without providing the check. | `review.md:6-246` prose only; no script/hook. R1/R2/R33/R35 all one-shot normalisations. | warning |
| A2. Shared blocks are not bracketed by explicit markers. Any diff that shifts line numbers breaks hash-based verification. | `review.md:6` block-start is inferred from `##` heading; end is inferred from next `##`. | suggestion |
| A3. R32 (third shared block for TOML write prose) is wontfix, but its rationale predates the parity-enforcement discussion. | `.claude/reviews/claude-commands.toml:513-520` — `wontfix_rationale = "DRY win outweighed by coordination cost"`. | suggestion |

### B. Intent-asymmetries between find vs apply pairs

| Gap | Evidence | Severity | Verdict |
|-----|----------|----------|---------|
| B1. `/optimise` lacks a staleness pre-check; `/review` has one at `review.md:273-277`. | `optimise.md:274-298` — no `git log -1 --format=%cI` gate. | warning | **Accidental** — port to `/optimise` |
| B2. `/optimise` always runs 5 agents; `/review` has a small-diff 1-agent shortcut. | `optimise.md:298` — "still launch all five research agents"; `review.md:315` — 1-agent shortcut. | suggestion | **Intentional** (research depth per lens) — add a `### Design Note` block to `/optimise` documenting why, so future `/review` runs do not re-flag |
| B3. `/optimise` Step 1.5 (Focal Points Brief) has no counterpart in `/review`. | `optimise.md:300-329`; `review.md` Step 1 has no analogue. | suggestion | **Intentional** (`/optimise`'s lenses are runtime-specific, `/review`'s are language-agnostic) — add a `### Design Note` block to `/review` |
| B4. `/optimise-apply` lacks a critical-finding gate. `/review-apply` Step 5 halts if `severity=critical ∧ category∈{security,db}` is being dismissed. | `review-apply.md` Step 5 (critical-finding gate prose); `optimise-apply.md` — absent (verified via Grep). | warning | **Accidental** — port with category set `{memory, query, concurrency}` |
| B5. `/optimise-apply` lacks a secret-pattern pre-write scan. `/review-apply` scans the ops payload. | `review-apply.md` Step 5 scan; `optimise-apply.md` — absent (verified via Grep). | warning | **Accidental** — port verbatim |
| B6. `/optimise-apply` lacks diff-reconciliation of `applied` claims. `/review-apply` Step 5 unions `git diff --name-only HEAD` + `--cached` + `ls-files --others --exclude-standard` before trusting any `applied R{n}` tag. | `review-apply.md:380-388`; `optimise-apply.md` — absent (verified via Grep). | critical | **Accidental** — port verbatim; closes the same OWASP LLM01:2025 trust gap for `/optimise-apply` |
| B7. `/optimise-apply` deleted-file rule is unconditional (`→ wontapply`). `/review-apply` distinguishes source files (`→ verified-clean`) from auto-generated files (`→ wontfix`). | `optimise-apply.md:293`; `review-apply.md:296-298`. | warning | **Intentional-but-under-documented** — `/optimise` has no `verified-clean` state, so the one-way transition is correct. Port the *generated-file* branch to `/optimise-apply` (→ `wontapply` with richer rationale); leave source-file branch as-is. |
| B8. `/optimise-apply` does not mint partial-apply child items. `/review-apply` mints new R-items with `related = [parent]`. | `review-apply.md:341` partial-apply block; `optimise-apply.md` — absent. | warning | **Accidental** — port with O-prefix analogue |
| B9. `/review-apply` has threat-model narration for `security`/`architecture` categories in Step 2; `/optimise-apply` has no analogue for `concurrency`. | `review-apply.md:300`; `optimise-apply.md:290-297` — absent. | suggestion | **Accidental** — port with categories `{concurrency}` (invariant narration for lock ordering, async boundaries) |

### C. Orchestration primitives underused

| Gap | Evidence | Severity |
|-----|----------|----------|
| C1. No TaskCreate usage in any of the four commands. | `implement.md:136` uses it per-task; the four targets do not. | warning |
| C2. No interim checkpointing between agent-batch return and final ledger render. An interrupted run loses all findings. | `plan-new.md:150, 179` persists to plan file; `optimise.md` / `review.md` Step 3 writes only at the end. | warning |
| C3. Findings-per-agent cap of 10 is conservative for Opus 4.7 1M context. | `optimise.md:360`, `review.md:333`. | suggestion |
| C4. Pre-analysis selector cap of 15 is declared "non-negotiable" but is also tuned for shorter-context models. | `optimise-apply.md:288`; `review-apply.md:291`. | suggestion |
| C5. No truncation-priority rules for agent output, unlike `plan-new.md:147` ("prioritise file paths and interface signatures over narrative"). | `optimise.md` Step 2, `review.md` Step 2 — bare word-count caps only. | suggestion |
| C6. Pre-analysis read-batching in apply commands is not explicitly required to be in a single response message. | `optimise-apply.md:288`, `review-apply.md:291` — "batched in parallel Read tool calls" does not mandate same-message emission. | suggestion |
| C7. Scope classification, CLAUDE.md reads, and ledger-load remain in the main orchestrator thread. `Explore` subagent would reclaim context. | `review.md:279-313`, `optimise.md:286-296` — serial orchestrator work. | suggestion |
| C8. `/optimise` Step 1.5 already uses `subagent_type: "Explore"` — a positive precedent that the others have not followed. | `optimise.md:310`. | n/a (positive example) |

### D. Robustness & failure-mode gaps

| Gap | Evidence | Severity |
|-----|----------|----------|
| D1. Dedup rule (`file AND (symbol OR summary)`) fails under symbol rename, file move, or whitespace-different summaries. Orphan IDs accumulate silently. | Ledger schema line 240 (shared). | warning |
| D2. "Already applied" test compares the finding's recommendation as a literal against the read range. Whitespace / formatting drift defeats it. | `optimise-apply.md:294`, `review-apply.md:299`. | warning |
| D3. No rollback protocol documented when Step 5 verification fails post-byte-write. | `optimise-apply.md` Step 5; `review-apply.md` Step 5 — detection without recovery. | warning |
| D4. Deferred items have a `defer_trigger` field but no mechanism to detect when the trigger has fired. | Ledger schema line 166 (shared); R28 resolution addressed wording only. | suggestion |
| D5. No multi-step dependency orchestration. Clustering is ad-hoc; chains A→B→C get emergent ordering. | `optimise-apply.md:299-305`, `review-apply.md:307-314`. | suggestion |
| D6. No ledger integrity verification (R29 deferred). Accidental out-of-band edits corrupt state silently. | R29 noted as future enhancement in `review-apply.md` prose; `tomlctl` has no `--verify-integrity`. | suggestion |
| D7. No find→apply freshness gate. Code committed between `/review` and `/review-apply` causes apply to run against drifted code. | `review-apply.md` Step 1; `optimise-apply.md` Step 1 — no `last_updated` vs `git log` comparison. | warning |
| D8. Clock-skew validation exists in `/review` (`review.md:306`) but not in `/optimise`. | `optimise.md:274-298`. | suggestion |

### E. tomlctl feature surface gaps

| Gap | Evidence | Severity |
|-----|----------|----------|
| E1. No `tomlctl items find-duplicates` for dedup hygiene. | `tomlctl/src/main.rs` subcommand enum. | suggestion |
| E2. No `tomlctl items orphans` for rename-aftermath cleanup. | `tomlctl/src/main.rs` subcommand enum. | suggestion |
| E3. No `tomlctl --write-integrity` / `--verify-integrity` sidecar. | R29 deferred. | suggestion |
| E4. No `tomlctl items update --unset <key>` for clean field removal on reopen transitions. | Current `items update --json` only sets keys. | suggestion |
| E5. No `tomlctl blocks verify --block "## Flow Context"` for runtime parity checks. | New subcommand — cited as a fallback to the pre-commit hook. | suggestion |
| E6. No compound `items list` queries (e.g. `--status open --category security`). | `tomlctl items list --status` only. | suggestion |

---

## Approach — Staged Recommendations

Three tracks, ordered by risk + dependency. Tracks 1 can land immediately; Track 2 requires coordinated shared-block edits; Track 3 requires new `tomlctl` Rust subcommands.

### Track 1 — Quick Wins (no shared-block or tomlctl changes)

Each item is sized at < 1 hour, textual edits only, no schema changes. Safe to batch into a single PR.

| # | Change | File:line target | Gap addressed |
|---|--------|------------------|---------------|
| T1.1 | Port **staleness pre-check** to `/optimise` Step 1 — byte-identical wording, command name swapped. | `optimise.md` insert after `:282`. | B1 |
| T1.2 | Port **critical-finding gate** to `/optimise-apply` Step 5 with category set `{memory, query, concurrency}`. | `optimise-apply.md` Step 5. | B4 |
| T1.3 | Port **secret-pattern scan** verbatim to `/optimise-apply` Step 5 between `--ops` construction and `tomlctl items apply`. | `optimise-apply.md` Step 5. | B5 |
| T1.4 | Port **diff-reconciliation** block from `/review-apply` Step 5 to `/optimise-apply` Step 5 (adjust `R{n}` → `O{n}`, `fixed` → `applied`, `wontfix` → `wontapply`). | `optimise-apply.md` Step 5. | B6 |
| T1.5 | Port **partial-apply child-item minting** to `/optimise-apply` Step 4/5 with O-prefix. | `optimise-apply.md` Step 4 agent prompt + Step 5 transition table. | B8 |
| T1.6 | Port **auto-generated-file branch** of deleted-file rule to `/optimise-apply` Step 2 (→ `wontapply` with rationale "file is auto-generated"). Keep the source-file branch unchanged. | `optimise-apply.md:293`. | B7 |
| T1.7 | Add **concurrency threat-model narration** requirement to `/optimise-apply` Step 2 for `category=concurrency` findings. | `optimise-apply.md:290-297`. | B9 |
| T1.8 | Add **clock-skew validation** to `/optimise` Step 1 (mirror `review.md:306`). | `optimise.md:288` onward. | D8 |
| T1.9 | Add **`### Design Note: Intentional Asymmetry`** block to `/optimise` documenting why it always runs 5 agents (contrast `review.md:315` small-diff shortcut). Add a mirror note to `/review` explaining why there is no focal-points synthesis counterpart. | `optimise.md` after Step 1; `review.md` after Step 1. | B2, B3 |
| T1.10 | Raise findings-per-agent cap from **10 → 15** (target) with ceiling 20 in `/optimise` and `/review`. Add truncation-priority rule verbatim from `plan-new.md:147` ("prioritise file paths and interface signatures over narrative; preserve `critical`/`warning` over `suggestion`; preserve entries with non-empty `evidence[]`"). | `optimise.md:360`; `review.md:333`. | C3, C5 |
| T1.11 | Add explicit **"emit all of the following tool calls in a single response message"** directive to Step 1 scope-resolution of all four commands. | `optimise.md:286`, `review.md:279`, `optimise-apply.md:263`, `review-apply.md:263`. | C6 |
| T1.12 | Raise pre-analysis selector cap to a **tiered model**: target 25, ceiling 30, warn above 25, abort above 30. Replace the "non-negotiable" language with the tiered rule. | `optimise-apply.md:288`; `review-apply.md:291`. | C4 |
| T1.13 | Add **find→apply freshness gate** to Step 1 of both apply commands using only existing tomlctl + `git log -1 --format=%cI` — no new subcommand required. Prompt: `[p]roceed / [r]e-run /review or /optimise / [a]bort`. | `optimise-apply.md:282`; `review-apply.md:285`. | D7 |
| T1.14 | Update **R32 `wontfix_rationale`** in `.claude/reviews/claude-commands.toml` to reference the parity-enforcement mechanism from T2.1 (below) and the R31 point-fix outcome as precedent. Preserve wontfix status. | `.claude/reviews/claude-commands.toml:518`. | A3 |
| T1.15 | Add an **orphan surfacing** pass to `/review` and `/optimise` Step 1 using `Glob` + `Grep` only — report items whose `file` is missing or `symbol` no longer appears. Do not auto-transition; surface to console only. | `review.md:308`; `optimise.md` post-scope-classification. | D1 (partial) |

### Track 2 — Structural Changes (coordinated shared-block edits + new artefacts)

Each item is a coordinated multi-file edit. Land each as its own commit for reviewability.

| # | Change | File:line target | Gap addressed |
|---|--------|------------------|---------------|
| T2.1 | Introduce **parity-enforcement mechanism**: add `scripts/verify-shared-blocks.sh` (POSIX `sha256sum` with `shasum -a 256` fallback), `scripts/shared-blocks.toml` (manifest listing `[[block]]` entries), `.githooks/pre-commit` (invokes the script when command files are staged), and a CLAUDE.md "Developer setup" note pointing to `git config core.hooksPath .githooks`. | New files. | A1 |
| T2.2 | Wrap **both shared blocks** in all four command files with explicit HTML-comment markers: `<!-- SHARED-BLOCK:flow-context START -->` … `END`; `<!-- SHARED-BLOCK:ledger-schema START -->` … `END`. Markers hash-neutral (script strips them during verification). | Four command files, lines 6 and 92 + lines 93 and 246 each. | A2 |
| T2.3 | Introduce **TaskCreate wrapping** at agent-batch granularity. Mint one task per lens-agent in `/optimise` Step 2 and `/review` Step 2; one task per file-cluster in `/optimise-apply` Step 4 and `/review-apply` Step 4; one task for verification sub-agent in both apply commands. **Do not mint per-finding tasks** — that shadows the ledger. Do not hand tasks from find to apply — tasks are per-run, the ledger is persistent. | `optimise.md:331`; `review.md:317`; `optimise-apply.md:307`; `review-apply.md:315`. | C1 |
| T2.4 | Introduce **two-phase ledger checkpointing**: after agent batches return but before final report render, persist non-risky transitions (new items, `verified-clean` in `/review-apply`, `wontapply` / `wontfix` / `skipped` resolutions) via one `tomlctl items apply --ops -`. Defer `fixed`/`applied` transitions until after verification. Use existing Step 1 idempotency guards for recovery: a re-run sees resolved items and skips them. Defer `set last_updated` to final render. | `optimise.md` between Step 2 and Step 3; `review.md` between Step 2 and Step 3; both apply commands between Step 4 and Step 5. | C2 |
| T2.5 | **Delegate pre-analysis** in apply commands to an `Explore` agent (thoroughness `quick`). Returns a classification table per selector (already-in-place / drifted / fresh) rather than raw read output; orchestrator keeps only the table. Rationale: the raised T1.12 selector cap produces ~300KB of raw reads in the orchestrator; delegating saves that context for verification and report rendering. | `optimise-apply.md:284-297`; `review-apply.md:287-305`. | C7 |
| T2.6 | **Delegate scope classification** in `/review` (when scope > 10 files) to an `Explore` agent — returns file list + classification + relevant CLAUDE.md excerpts. Skip this when the small-diff shortcut fires (≤ 3 files). | `review.md:279-313`. | C7 |
| T2.7 | Add a **rollback protocol** as new Step 5.5 in both apply commands. Triggers: build fail on touched files, test regression for tests outside find-ledger scope, `applied` tag without a matching diff entry. Sequence: `git stash push -u -m "<cmd>-rollback-<timestamp>"` → `git checkout -- <paths>` → narrowly-scoped `git clean -fd -- <paths>` for untracked agent-created files → reverse `tomlctl items apply --ops -` batch that resets `status = "open"` for items touched this run. Emit a `[[rollback_events]]` array-of-tables entry at the ledger root (schema extension — see T2.8). | `optimise-apply.md` after Step 5; `review-apply.md` after Step 5. | D3 |
| T2.8 | **Shared-block schema extensions** — add optional fields to `## Ledger Schema`: `depends_on = ["O7", "R12"]` (array of ledger IDs), `rollback_rationale` (string), `reopen_rationale` (string), `fingerprint` (computed by `tomlctl`, not hand-authored — see T3.3). Also add a new `[[rollback_events]]` top-level table. Coordinated across all four files, validated by T2.1's parity script. | Shared block lines 130–170 in all four files. | D1, D3, D4, D5 (schema half) |
| T2.9 | Replace ad-hoc clustering in Step 3 of both apply commands with **dependency-DAG + Kahn's-algorithm topological sort** over items' `depends_on` (when populated). Fail-fast on cycles with the cycle path printed. Within a round, keep existing file-cluster grouping. | `optimise-apply.md:299-305`; `review-apply.md:307-313`. | D5 |
| T2.10 | **Already-applied test formalisation**: add a per-language whitespace-normalisation rule to Step 2 of both apply commands (Tier 1 = literal match after normalisation; Tier 2 = pass `uncertain_already_applied = true` flag into the agent prompt, which then read-verifies before editing). | `optimise-apply.md:294`; `review-apply.md:299`. | D2 |
| T2.11 | **Deferred-item reopen sweep** in `/review` and `/optimise` Step 1: `tomlctl items list --status deferred` → pattern-match `defer_trigger` against known forms (`after <path> exists`, `after <file>:<symbol> landed`, `when <id> resolves`, `after <branch> merges`, `after <ISO-date>`) → on high-confidence fire, prompt user for one-keystroke reopen confirmation; on free-text fallback, surface only. Never auto-transition silently. | `review.md:306`; `optimise.md` Step 1 analogous location. | D4 |

### Track 3 — Deferred (require new tomlctl features or further evidence)

Each item is blocked on a Rust code change to `tomlctl/src/main.rs` or on accumulating evidence that justifies investment. Recommended implementation order: T3.1 → T3.2 → T3.4 → T3.3 → T3.5 → T3.6.

| # | Change | Prerequisite | Gap addressed |
|---|--------|-------------|---------------|
| T3.1 | **`tomlctl --write-integrity` / `--verify-integrity`** sidecar (`<file>.sha256`, standard `sha256sum` format). Default `--write-integrity` on; `--verify-integrity` opt-in. On mismatch: error with expected/actual hashes; never auto-repair. Wire into `/review` and `/optimise` Step 1 ledger-load with `--verify-integrity`. | `tomlctl` changes. | D6, E3 |
| T3.2 | **`tomlctl blocks verify <file>... --block "## Flow Context" --block "## Ledger Schema"`** subcommand for runtime parity belt-and-braces. Optional Step 0 in each command; skip-if-not-installed. Cost: one cold Rust invocation per command run (~20 ms). | `tomlctl` changes. | A1 (runtime half, complementing T2.1) |
| T3.3 | **`tomlctl items update --unset <key>`** flag for clean field removal during reopen / rollback transitions. Needed by T2.11's reopen flow to drop `defer_reason` / `defer_trigger` when moving `deferred → open`. | `tomlctl` changes. | E4 |
| T3.4 | **`tomlctl items find-duplicates <ledger> [--tier A\|B\|C]`** for dedup hygiene. Tier A = current rule; Tier B = `fingerprint + basename` (suggest-not-auto); Tier C = `file + line ± 10` when symbol missing. Read-only. Wired into `/review` and `/optimise` Step 1. | `tomlctl` changes + T2.8 schema extension. | D1, E1 |
| T3.5 | **`tomlctl items orphans <ledger>`** for rename-aftermath surfacing. Walks items, checks `file` path via `std::fs::exists`, and ripgreps for `symbol` when present. JSON output. | `tomlctl` changes. | D1, E2 |
| T3.6 | **Compound `items list` queries** (`--status open --category security`, `--newer-than <date>`, `--file <path>`). Reduces main-thread Grep/Glob work in freshness gate and orphan surfacing. | `tomlctl` changes. | E6 |
| T3.7 | **Revisit R32** if T2.1's parity script ever graduates from byte-identical block hashing to structural-template verification (i.e. "same section headings, different content per file"). Until then, keep R32 wontfix with the T1.14 refreshed rationale. | Evidence that byte-identical is too strict in practice. | A3 |
| T3.8 | **Tier-2 already-applied test** (agent-adjudicated structural match) — add `uncertain_already_applied` flag to agent prompt contract. This requires a coordinated prompt-schema change across both apply commands' Step 4 instructions. Separable from T2.10 Tier 1. | Evidence that Tier-1 normalisation misses real cases in practice. | D2 |

---

## Dependency Ordering / Batches

**Batch 1 (Track 1 — parallel, independent edits per command file):**
- T1.1 (optimise.md)
- T1.2 + T1.3 + T1.4 + T1.5 + T1.6 + T1.7 (optimise-apply.md — single coordinated edit)
- T1.8 + T1.9 (optimise.md; T1.9 also touches review.md)
- T1.10 + T1.11 (cross-file; batch into one commit per command)
- T1.12 + T1.13 (apply commands; batch into one commit)
- T1.14 (ledger only)
- T1.15 (find commands)

All Track 1 items are safe to land without parity-enforcement infrastructure because they do not touch the shared blocks.

**Batch 2 (Track 2 — must land after Batch 1):**
- T2.1 + T2.2 (parity infrastructure and markers — prerequisite for all further shared-block edits)
- T2.3 + T2.4 (orchestration primitives — independent per command)
- T2.5 + T2.6 (subagent delegation — independent per command)
- T2.7 (rollback — blocked by T2.8 for ledger schema)
- T2.8 (shared-block schema extension — blocked by T2.1 parity check)
- T2.9 (topological-sort — blocked by T2.8)
- T2.10 (already-applied Tier 1 — independent)
- T2.11 (deferred-item reopen — blocked by T2.8 if it needs `reopen_rationale`)

**Batch 3 (Track 3 — requires tomlctl Rust changes, ship independently as each lands):**
- T3.1 → T3.2 → T3.3 → T3.4 → T3.5 → T3.6 → (optionally T3.7 / T3.8)

---

## Verification

### Track 1 verification

Each command file remains syntactically valid markdown. For each change:

- **Small-diff commits**: a follow-up run of `/review claude/commands/optimise-apply.md` (and `review-apply.md`, `optimise.md`, `review.md`) should NOT re-flag the ported gaps as new findings. Use the flow-less fallback ledger at `.claude/reviews/claude-commands.toml` (closed today with `wontfix` / `fixed`) — new items would surface as fresh R-IDs.
- **R32 rationale update**: `grep -n "wontfix_rationale" .claude/reviews/claude-commands.toml` returns the refreshed text.
- **Design Notes (T1.9)**: manual visual check — each file has a `### Design Note: Intentional Asymmetry` block with cross-reference lines.

### Track 2 verification

- **T2.1 / T2.2 (parity script + markers)**:
  - `bash scripts/verify-shared-blocks.sh` exits 0 immediately after marker insertion (block hashes match across all four files).
  - Manually break parity in one file (add a space in the Flow Context block), re-run: script exits non-zero, diagnostic lists the drifting file.
  - Revert; `.githooks/pre-commit` blocks a commit that introduces drift.
- **T2.3 (TaskCreate)**: manual run of `/optimise` produces a task list with five lens-tasks that transition `pending → in_progress → completed` as agents return. Same for the other three commands.
- **T2.4 (checkpointing)**: mid-run abort (Ctrl-C after agent batch returns, before verification) leaves ledger in a consistent state — re-running the command picks up where it left off. Validate by inspecting ledger before/after interrupt.
- **T2.5 / T2.6 (Explore delegation)**: orchestrator context usage drops (observable via `/context` command). Apply-command runs on 25-item selectors complete without orchestrator context pressure.
- **T2.7 (rollback)**: deliberately introduce a failing test under scope, run `/review-apply`, verify rollback sequence executes and ledger items revert to `status = "open"` with `rollback_rationale` set.
- **T2.8 (schema extension)**: all four files have the new optional fields in `## Ledger Schema`. Parity script passes. `tomlctl parse` accepts a ledger containing `depends_on = [...]`.
- **T2.9 (topo sort)**: craft a deliberate A→B→C chain in a test ledger; run `/optimise-apply A,B,C`; verify apply order matches topo sort even when ledger order is reversed.
- **T2.10 (Tier 1 already-applied)**: whitespace-drifted version of a known finding is correctly detected as already-applied.
- **T2.11 (deferred reopen)**: create a deferred item with `defer_trigger = "after <path> exists"`; create the path; run `/review`; prompt fires.

### Track 3 verification

Each tomlctl subcommand gets its own Rust test module; existing `tomlctl/tests/` pattern applies. CLI smoke tests:

- `tomlctl set <file> foo "bar" --write-integrity` creates `<file>.sha256` with valid digest.
- `tomlctl parse <file> --verify-integrity` succeeds; manually perturb the file, re-run: exits non-zero with hash-mismatch diagnostic.
- `tomlctl blocks verify claude/commands/*.md --block "## Flow Context" --block "## Ledger Schema"` exits 0 when parity holds.

### End-to-end verification

- Run `/review` against `claude/commands/` after each track lands. The new run should surface fewer or different gaps (evidence of progress) — if the same gaps recur, the port/copy was incomplete.
- Run `/optimise` against `tomlctl/src/` after Track 3 to verify the new subcommands did not regress the existing performance posture.
- `cargo test` in `tomlctl/` passes (current: 16/16; Track 3 adds more).

### Commands discovered during exploration

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test:  cargo test  --manifest-path tomlctl/Cargo.toml
lint:  cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

No top-level build/test/lint exists for the repo at large — the only compiled artefact is `tomlctl`. Markdown command files have no linter beyond rendering sanity checks.

---

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| **Batch 1 port (T1.2–T1.6) introduces a subtle divergence between `/optimise-apply` and `/review-apply`.** The source text has R-prefix IDs, `fixed`/`wontfix`/`verified-clean` states; the port must swap to O-prefix, `applied`/`wontapply`, and no `verified-clean`. | Medium | Port one block at a time; run a follow-up `/review claude/commands/optimise-apply.md` pass to surface accidental R→O replacements missed in the port. |
| **T2.1 pre-commit hook is bypassed by contributors running `git commit --no-verify` or using editor workflows that skip hooks.** | Medium | Add a parity check to whatever CI pipeline exists (or will exist) as a second gate. Document in CLAUDE.md that `--no-verify` must not be used for these files. |
| **T2.3 TaskCreate adoption creates runtime noise** — task chrome adds visual load on simple runs. | Low | Gate TaskCreate on `scope > 1 file` for `/review` and `/optimise`; for apply commands, always create tasks since file-cluster work is inherently multi-step. |
| **T2.4 checkpointing writes mid-run might race with a concurrent `tomlctl` invocation in another shell.** | Low | `tomlctl`'s 30-second `try_lock_exclusive` handles this; the checkpoint inherits the existing lock discipline. |
| **T2.8 schema extension might silently accept `depends_on = ["R99"]` pointing at an ID that does not exist.** | Medium | T2.9 topo-sort passes restrict the DAG to items in the selected set — forward references to non-existent IDs are harmless in that pass, but ledger hygiene is improved by extending T3.5 (`items orphans`) to also check `depends_on` forward refs. Deferred to T3.5. |
| **T2.7 rollback's `git clean -fd -- <paths>` could remove user-created work-in-progress files if the path list comes from a subverted agent.** | Medium | Narrow scope: `git clean -fd` only for untracked files inside cluster-agent scopes, never bare `git clean`. User confirmation prompt before the clean step (non-interactive mode defaults to skip-and-warn). |
| **Track 3 new tomlctl features grow the binary surface faster than the SKILL.md can track.** | Low | Each Track 3 item ships with a corresponding SKILL.md section in the same commit. R32-style drift is prevented by the same principle applied at T2.1. |
| **Opus 4.7 context caps (T1.10, T1.12) are raised based on first-principles reasoning without a measurement campaign.** If 1M Opus 4.7 has unanticipated degradation at the new caps, findings quality drops silently. | Low-medium | Raise caps incrementally: ship T1.10 as 10→15 first; observe two weeks of usage (new ledger findings against existing projects); only then raise to ceiling 20. Same staging for T1.12 (15→25 then 25→30). |

---

## Critical Files

Read-only references (for implementation phase):

- `claude/commands/optimise.md` (518 lines) — Track 1 & 2 edits
- `claude/commands/review.md` (514 lines) — Track 1 & 2 edits
- `claude/commands/optimise-apply.md` (432 lines) — Track 1 & 2 edits (heaviest)
- `claude/commands/review-apply.md` (498 lines) — Track 2 edits
- `claude/commands/plan-new.md` — reference pattern for checkpoint + truncation-priority
- `claude/commands/implement.md` — reference pattern for TaskCreate, Phase 4.5 structure
- `tomlctl/src/main.rs` — Track 3 Rust changes
- `tomlctl/README.md` + `claude/skills/tomlctl/SKILL.md` — Track 3 companion documentation
- `.claude/reviews/claude-commands.toml` — Track 1 R32 rationale update; closed ledger used as evidence

New files (Track 2):

- `scripts/verify-shared-blocks.sh`
- `scripts/shared-blocks.toml`
- `.githooks/pre-commit`
- `CLAUDE.md` (top-level, may need creation if absent)

---

## Summary

The find-vs-apply split and shared-block normalisation completed today is a solid foundation. The next round splits cleanly into three tracks: low-risk ports of accidental asymmetries into `/optimise-apply` (Track 1, < 1 day), coordinated shared-block + parity-enforcement + orchestration-primitive adoption (Track 2, ~1 week), and strategic `tomlctl` feature additions (Track 3, incremental). The 40-finding ledger's own experience — where shared-block drift surfaced four times in one day — argues strongly for Track 2.1's parity mechanism as the single highest-leverage investment.
