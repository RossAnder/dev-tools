# Plan: review-v2 harness repairs from run U1

**Plan path**: `docs/plans/review-v2-first-run-repairs.md`
**Created**: 2026-11-10

## Summary

Repairs the 19 defects the first real `/review-v2` run (U1, on `jiggly-exploring-robin`) found in the harness itself: 22 `tomlctl` tasks (a `head` bound on runs, a `review run show` read verb, structured rule-outs, duplicate candidates with a strict export fold, a proposal that stays inside its ceiling, an explicit `--flow` miss that fails), 2 Workflow-script tasks (batched cleanup verification, escalation text and an agent count in the index, with the plan-review Workflow brought onto the same rules), 17 prose tasks, one per file group, across the review agents and the `review-v2-engine`, `review-v2-method` and `review-v2-lenses` skills, and one repair of the v1 ledger rows run U1 folded wrongly. Scrutinise first: **the size** — 42 tasks in four checkpoints (`tomlctl tasks list --slug review-v2-first-run-repairs --count` and `--pluck files` give the counts), roughly double the estimate given when one plan for all 19 was chosen, driven by the richer option taken at each design question (Risks, first bullet); then Approach, "Run bounds" and "Duplicates and rule-outs", which add the only new stored fields and change what `export-v1` folds; then Approach, "Explicit flow miss", which turns a warning every carrier tolerated into a hard error.

## Context

Run U1 reviewed 74 files with 39 agents and produced 35 findings, and along the way exposed 19 gaps in the review-v2 harness, captured as backlog items and promoted to the seed flow `review-v2-first-run-repairs`. They fall into five groups:

- **Wrong scope.** A flow-bound run on `main` has no scope rule, so U1 improvised a base and a third of its findings belonged to a later flow's commits (B-827b9763). `flow resolve --flow <slug>` silently binds another flow when the slug matches nothing (B-80c20e25).
- **Wasted coverage.** `review plan --propose` exceeded its own ceiling with per-file correctness cells and cut every signal-matched lens (B-39b9e713); the sweeper sealed `thin` on large slices with no definition of what sweeping a file means (B-d9cfd36e); a finder escalated a grep-provable cleanup instead of writing it (B-30ba0a0a).
- **Lost information.** Duplicate findings across cells stay unlinked and `export-v1` folded two different defects into one v1 row (B-5d5bf09e); a rule-out in one cell's note never meets another cell's finding on the same site (B-dc721e5c); escalation text is dropped from the Workflow index (B-bc4a0080); a needs-plan defect and a parked advisory land nowhere after close (B-ce9ebb4d).
- **Unmeasured verification.** Verifiers confirmed 30 of 30 rows, the audit recorded nothing for rows it upheld, and `review stats` therefore read `audited 0` (B-fc4d7f8b, B-1e1be4cf).
- **Friction and drift.** Signals and baselines cannot be read back (B-99efd715); baselines go out one dispatch at a time (B-ab53208e); each verifier-routed cleanup row gets its own Opus verifier (B-e684aed8); seven agents were refused at the 600-character cap after writing (B-00ae0571); the surveyor skips project checks (B-989439ec); the context-free finder is not context-free (B-75341a1d); run bookkeeping is hand-kept (B-27f5ba35); two reference lines are stale (B-0bdfc2dc).

The intended outcome is that the next `/review-v2` run needs none of the workarounds those items record.

## Scope

- **In scope**: the 19 seed backlog items, delivered in full, for the code and perf families and for the plan-review family alike (`claude/workflows/plan-v2-review.js`, `claude/agents/plan-review-lead.md`, `claude/commands/review-plan-v2.md`); a `tomlctl` minor version bump (0.21.0 → 0.22.0) with its capability entries and reference docs; the repair of the two v1 ledger folds B-5d5bf09e records (task 40).
- **Out of scope**: in-run planted negatives (a hidden `seeded` finding field); a per-agent path-restriction hook for the context-free finder; a reproduction-evidence rule on verifier confirms; the open backlog items B-e922f5cb, B-466a423b, B-d1d11150, B-55a13775 and B-edb20f98; any change to the v1 review commands; applying U1's own exported findings (R22–R48) — except the repair B-5d5bf09e requires before any of them is applied: F4 split out of R22 into its own row and R31 folded into R26, done by task 40.
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/cli/types/`, `tomlctl/src/flow/resolve.rs`, `tomlctl/src/capabilities.rs`, `tomlctl/tests/`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/README.md`, `glimpse/Cargo.lock`, `claude/workflows/`, `claude/agents/`, `claude/commands/review-v2.md`, `claude/commands/optimise-v2.md`, `claude/commands/review-plan-v2.md`, `claude/commands/tdd.md`, `.claude/flows/jiggly-exploring-robin/review-ledger.toml`, `claude/skills/review-v2-engine/`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/`, `claude/skills/tomlctl/`

## User Decisions

1. **Plan scope** — *One plan, all 19.* Prompted by the Phase 1 estimate of about 35 files against the ~25-file guide.
2. **Flow scope rule (B-827b9763)** — *Stamped commits bound it*, with the user's additions: "I would want to use this for flowless too where the orchestrator would have more flexibilty in determining scope. Beyond the commit bounds, I would expect ripple to pick up affected files that were not touched that might be worth including too". Prompted by `claude/skills/review-v2-engine/SKILL.md:92` (HEAD~1 on the default branch; `--base` the only bound).
3. **Duplicates (B-5d5bf09e)** — *Candidates + strict fold.* Prompted by `tomlctl/src/review/export.rs` `decide` folding on file + symbol, and the prior-art finding that no established tool folds on location without the defect class.
4. **Rule-outs (B-dc721e5c)** — *Structured, joined.* Prompted by rule-outs living only in a 600-character free-text cover note while `review digest` reported `contested = []`.
5. **Landing place (B-ce9ebb4d)** — *Backlog + stage 0*: a needs-plan defect becomes a backlog row; parked and still-open needs-plan rows are listed at the next run's stage 0. Prompted by `claude/skills/review-v2-engine/references/triage-and-close.md:94` (`plan F5`: no store write).
6. **Verifier leniency (B-1e1be4cf, B-fc4d7f8b)** — *Full recommended set*: record every sampled audit row; fill empty refuted sample slots and sample plausible defects; a low-side `suspect` refute band and a 30-row floor; an offline seeded-precision probe. Prompted by `tomlctl/src/review/stats.rs` `overturn` already counting an upheld audit verdict.
7. **600-character cap (B-00ae0571)** — *Aim low, keep cap*: prose states a 450-character target, 600 stays the hard limit. Prompted by 7 of 39 U1 agents refused at 609–950 characters.
8. **Checkpoint cadence** — *milestones.* Prompted by the early scope check (over 10 files).
9. **Backlog fold-in** — *None beyond the seed.* Folded ids: B-80c20e25, B-827b9763, B-d9cfd36e, B-39b9e713, B-5d5bf09e, B-bc4a0080, B-99efd715, B-989439ec, B-0bdfc2dc, B-fc4d7f8b, B-1e1be4cf, B-dc721e5c, B-30ba0a0a, B-00ae0571, B-e684aed8, B-ab53208e, B-75341a1d, B-ce9ebb4d, B-27f5ba35.

### Phase 5 outcome

Skipped: every answer names a topic already in the Research Notes (Workflow index, finding identity, audit and calibration, subagent frontmatter). The user's ripple addition is a codebase rule, settled in Approach, "Run bounds".

## Approach

### Explicit flow miss

`tomlctl flow resolve --flow <slug>` with a slug no flow carries returns a `kind=not_found` error and exits non-zero, with or without `--strict-read`. Today only `--strict-read` errors (`tomlctl/src/flow/resolve.rs`, `resolve`, step 1) and the default path pushes a warning and falls through to scope, branch and active-latest. `claude/agents/flow-bootstrap.md` already sets `ok: false` on a non-zero resolve, so no carrier that resolves through it needs a change. The one direct caller is `claude/commands/tdd.md`, whose parent re-resolve halts on `resolved == false`; task 4 makes it halt on the non-zero exit. A carrier that probes for a possibly-absent flow passes no `--flow`. Rejected: keeping the fall-through and having `flow-bootstrap` compare slugs — every other caller of `flow resolve` would keep the trap.

### Run bounds

A run is bounded at both ends. `review run start` gains an optional `--head <REV>`, stored as its sha in the run's `head`; when absent the run measures to `HEAD` as today (`changed_lines`, the survey's `load_diff` and `project_layer_changes` all diff `base..HEAD`, so uncommitted edits are invisible to them; only a finder's `git diff <base>` reads the working tree). `changed_lines`, `project_layer_changes`, the survey's diff detectors and the finder assignment all read `base..head` when `head` is set. Agents read files from the working tree whatever the bounds are, so when `head` is behind `HEAD` the orchestrator names in the run's opening line every scope file a later commit changed (`git diff --name-only <head> HEAD -- <scope entries>`).

The engine states one rule for choosing them, in `claude/skills/review-v2-engine/SKILL.md` stage 1:

- **Flow-bound run whose task rows carry `commit` stamps**: the commit list is those stamps; `base` is the first parent of the stamp every other stamp descends from (`git merge-base --is-ancestor`; committer dates tie on rebased work), `head` is the stamp that descends from all others — the orchestrator asks when the stamps are not on one ancestry line — and `scope` is the union of the files those commits touch (`git show --name-only` per stamp), not the files changed across the range — an interleaved commit from another flow contributes no file.
- **Flow-less run, or a flow with no stamps**: the orchestrator chooses `--base`, `--head` and the scope list from the user's argument (a range, a commit list, paths), and states its choice in the run's opening line. On a feature branch with no argument the default stays merge-base to `HEAD`.
- **Scope lists are staged outside the repository** and passed as `--set-json 'scope=@<path>'`, which `review run start` already accepts.

**Ripple reaches past the scope.** The scope bounds what cells *read as changed*; it does not bound where a `ripple` finding may be anchored. A cell working the `ripple` lens greps the whole tree for references to names the diff removed or renamed, and a stale reference in an untouched file is a finding on that file. `review add` checks a finding's lens against its cell, not its file against the run's scope, so no CLI change is needed — task 7's test pins that.

### Proposal ceiling and sweeper slices

`review plan --propose` never emits more cells than `ceiling(run)` minus the cells already planned, counting check cells. Before any signal-tier lens is cut for `ceiling`, the floor lens's slices are merged: adjacent small slices (single files, single directories) combine until the floor lens fits in at most half the budget. A lens that fits an existing cell's bundle is added to it rather than marked `ceiling` because an earlier lens missed. Hotspots are limited to files in the run's scope at the source (the survey's `churn-hotspot` evidence). A sweeper cell's slice is capped by file count — the cap is one named constant, 40 files — and a larger scope yields several sweeper cells, each counted against the ceiling.

The ceiling is absolute, and these rules settle the rest:

- **Adjacent** slices are slices sharing a parent directory, merged in the order `slices_for` yields them.
- **Half the budget** counts cells after `repeats` has doubled them, as the budget test does today.
- **A whole-scope slice** (a `slice = "whole-scope"` lens such as `ripple`) is split by expanding its entries to the scope's files and chunking them in path order.
- **When the budget cannot hold the cells** — the ceiling can be as low as 3 — slices keep merging past the half-budget target and past the sweeper cap, down to one whole-scope cell per lens. The proposal lists each cell that exceeds the cap under `over_cap`; it never emits more cells than the ceiling.

A sweeper has swept a slice when every name the slice's diff removed or renamed has been grepped tree-wide and every hit read at its line; `covered` means that and `thin` means some name went ungrepped. The definition lives in `claude/skills/review-v2-method/SKILL.md` and the sweeper cites it.

### Duplicates and rule-outs

Three joins the store does not make today, built on one matcher in `tomlctl/src/review/matching.rs`: two anchors match when they share a file and either a non-empty symbol or lines within the window `matching::candidates` already uses.

- **Duplicate candidates.** `review digest` gains `duplicate_candidates`: pairs of this run's non-superseded findings from different cells whose anchors match and neither of which names the other in `same_as`. A candidate is a question, never a fold. The orchestrator settles each pair at stage 5: `review resolve --run <run> --same-as <dup>=<target>` (repeatable) sets `same_as` and resolves the pair in the same call through the existing `Outcome::for_target` table, unchanged — the duplicate becomes `merged` into the target with `outcome` repeat, contested, reopened or suppressed, or stays open as a regression — and a pair it judges distinct is left alone.
- **Strict export fold.** `export-v1` reuses an open v1 row only when the row already carries the finding's `v2_ref`, or when file, symbol and summary are all equal and the row's own `v2_ref` names a finding in this store with the same lens (a v1 row carries `category`, never `lens`, so a row with no resolvable `v2_ref` never matches on that clause). Two findings of one export call never fold into each other unless one is `merged` into the other. A dismissed, deferred or fixed v1 row keeps today's loose match (same file, and same non-empty symbol or exact summary), so a wontfix or defer decision still suppresses a reworded re-find and a fixed row still flags a regression. Everything else mints a row.
- **Rule-outs.** `review cover` gains `--ruled-out <SRC>` (NDJSON or a `.toml` record file): entries of `file`, optional `symbol` or `line`, and `reason` (capped at 200 characters), stored on the cell as `ruled_out`. `review digest` gains `ruled_out_hits`: each finding from another cell whose anchor matches a rule-out, with the ruling cell and its reason. The verifier and audit projections carry the matching rule-outs on each finding, so a verifier confirms or refutes knowing another cell judged the site deliberate.

Rejected: an automatic fold at `resolve` on file + symbol + lens — the lens differs across the context-free and context-bearing cells that produced U1's true duplicates.

### Escalations and out-of-lens cleanup

A cell agent that sees a mechanical cleanup outside its bound lenses writes it: a `cleanup` row whose `lens` is a sweeper-executed lens (`ripple` or `hygiene`) is accepted from any cell, provided its `old` text is exact. It keeps the route `route_of` computes for any cleanup — the checker for a non-executable edit, the verifier otherwise — and a checker may refute it as it refutes a sweeper's row: the checker's `refuted` is allowed on a row a sweeper cell wrote or on a cleanup row whose lens the catalogue marks `mode = "mechanical"` (`written_by_sweeper` in `tomlctl/src/review/verdict.rs`, the checker prompt in `claude/workflows/review-v2.js` and `claude/agents/review-checker.md` all widen together). `review add` keeps refusing every other out-of-cell lens. What cannot be written that way is escalated, and the Workflow index returns each escalation's text (see Approach, "Workflow index"). An escalation the orchestrator does not turn into a follow-up cell — out of budget, or anchored outside the scope and not a ripple row — is named in the close summary and offered as a backlog capture through the `backlog check` gate.

### Workflow index

`claude/workflows/review-v2.js` returns `{run, wave, cells, verified, agents, errors, script_version, escalations}`. Each cell entry keeps `escalated` (the count); the texts go in the top-level `escalations` array of `{cell, text}` (each cut to 300 characters), placed as the index's last key so a result cut at 8,000 characters loses escalation text before any cell's `status`, `verified` or `errors`. `agents` counts the script's own `agent()` calls. Verifier-routed cleanup rows are batched per cell exactly as minor defects are (`verifyBatch`), at every depth. The script takes an optional `mechanical_lenses` argument (the ids of the catalogue's `mode = "mechanical"` lenses, from `tomlctl review catalogue`) and passes it to the checker as a `mechanical lenses:` line. `SCRIPT_VERSION` moves to `'2'`.

`claude/workflows/plan-v2-review.js` follows the same rules: it counts every `agent()` call of its own and sums its waves' `agents` into its index; its surveyor prompt carries the run id and no signals; its baselines ride one `verification` dispatch with `keep_going: true`; it passes `mechanical_lenses` through to the pipeline; and it hands each wave's `escalations` to the lead's `settle` phase, which also links duplicate candidates and returns the rule-out hits that need re-verification under `reverify`, which the Workflow dispatches. Its `SCRIPT_VERSION` moves to `'2'` too. The calling conversation sees at most 8,000 characters of a Workflow result; the engine reads the notification's output file when the result says it was truncated.

### Reading a run back

`review run show <RUN>` prints one run: `base`, `head`, `scope`, `depth`, `signals` (each signal's `roots` limited to roots under a scope entry, with `roots_outside_scope` as a count), `baselines`, `waves`, `agents` and `audit`. It writes nothing. The surveyor reads signals from it by run id and the orchestrator reads baseline keys from it, so nothing is copied into a prompt by hand. `waves` is derived from the run's cells wherever it is reported (`review stats --by run`, `review run show`, `review run list` and the digest's run line, which `review report` prints), through one `pub(crate)` helper task 5 adds to `tomlctl/src/review/stats.rs`; tasks 10, 15 and 19 call it and depend on 5. `review catalogue` prints each check's `executor` and resolved `command`, so a project check reads as a verification check with its command.

### Landing places

- `plan <id>` at triage: the orchestrator mints a backlog row for the needs-plan defect (through `backlog check`, context carrying the `review show <id> --for implementer` command and the store flag) and stamps the finding with `review triage <id> --backlog-ref <B-id>`.
- Stage 0 of every run lists what earlier runs left: `review list --status parked` and `review list --status open --fix-shape needs-plan` over the store, reported in the run's opening lines. `--fix-shape` is a new `review list` filter.

### Audit and measurement

Prose and one digest function; `review stats` is unchanged because it already counts an upheld audit verdict.

- Stage 6 records a `--by audit` verdict for **every** sampled row, `confirmed` or `refuted` only, in one `--ndjson` batch, each with a citation and a one-line rationale. The run's `audit.sampled` is the batch's row count.
- `audit_sample` fills each empty refuted slot with one more standing defect chosen by the same index rule, and its standing group is confirmed-or-plausible.
- `claude/skills/review-v2-engine/references/measurement.md`: a pooled verifier refute rate under half the matching v1 vet-drop over at least 30 verified rows reads `suspect` and triggers the seeded-precision probe; audit-overturn bands read `insufficient` below 30 audited rows per class and are read pooled per model.
- `claude/skills/review-v2-engine/references/measurement.md` documents the offline seeded-precision probe: a scratch store, three false seeds drawn from v1 `verified-clean` items and two true ones, one verifier dispatch, scored as a 95% lower bound on the verifier's true-negative rate, `0.05**(1/n)` over n false seeds all refuted and pooled across probes: one probe's 3 of 3 shows only at least 37%, so a single pass rules out gross leniency and nothing more; the two true seeds are scored as a confirm rate; each probe records its seed ids in its scratch run so reuse can be checked.

### Agent prose rules

One rule per concern, stated once in `review-v2-method` and cited by the agents:

- **Cap target**: notes and rationales aim for 450 characters; 600 is the limit the tool enforces.
- **Context-free finder**: `claude/agents/review-finder-cf.md` preloads `review-v2-method` only. The shared body's sentence about `research-methods` is reworded to hold for both carriers (preloaded where the frontmatter lists it; a context-free cell works from `review-v2-method`'s bar alone and does not load it). A context-free cell reads nothing under `.claude/flows/` or `docs/flows/`. The description says what `omitClaudeMd` omits and that path-scoped rules still load.
- **Surveyor**: lists every project check under `verification:` with the command `review catalogue` prints; cites only scope files as candidate evidence; writes `profile.md` and `pack.md` whole with Write; reads signals with `review run show`.
- **Baselines**: one `verification` dispatch carries every baseline command with `keep_going: true`, each with its own `timeout`; every key is recorded from that one return.

## Success Criteria

- forward: `grep -c 'A row you agree with gets no verdict' claude/skills/review-v2-engine/references/triage-and-close.md` prints `0` (today: `1`).
- forward: `grep -c 'only copy of the signals' claude/skills/review-v2-engine/references/survey.md` prints `0` (today: `1`).
- forward: `grep -c 'escalations' claude/workflows/review-v2.js` prints at least `1` (today: `0`).
- forward: `grep -c '^version = "0.22.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
- forward: `sed -n '1,12p' claude/agents/review-finder-cf.md | grep -c 'research-methods'` prints `0` (today: `1`).
- guard: `bash scripts/verify-shared-blocks.sh` exits 0.
- forward: `grep -c 'HEAD~1' claude/skills/review-v2-engine/SKILL.md` prints `0` (today: `1`).
- forward: `grep -c 'no store write' claude/skills/review-v2-engine/references/triage-and-close.md` prints `0` (today: `1`).
- forward: `grep -c 'one at a time' claude/commands/optimise-v2.md` prints `0` (today: `1`).
- forward: `grep -c 'fn propose_never_exceeds_ceiling' tomlctl/tests/review_cells.rs` prints `1` (today: `0`); the `test:` run then shows it passes.
- forward, **predicted, unverified** (builds the binary): `cargo run -q --manifest-path tomlctl/Cargo.toml -- --error-format json flow resolve --flow no-such-flow-zz` reports `"kind":"not_found"` (today, on the installed 0.21.0: exits 0 and binds the active-latest flow), and `cargo run -q --manifest-path tomlctl/Cargo.toml -- review run show --help` exits 0 (today: unrecognised subcommand).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: test "$(grep -c 'A row you agree with gets no verdict' claude/skills/review-v2-engine/references/triage-and-close.md)" -eq 0
success: test "$(grep -c 'only copy of the signals' claude/skills/review-v2-engine/references/survey.md)" -eq 0
success: grep -q 'escalations' claude/workflows/review-v2.js
success: grep -q '^version = "0.22.0"' tomlctl/Cargo.toml
success: test "$(sed -n '1,12p' claude/agents/review-finder-cf.md | grep -c 'research-methods')" -eq 0
success: bash scripts/verify-shared-blocks.sh
success: cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
success: cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items
success: test "$(grep -c 'HEAD~1' claude/skills/review-v2-engine/SKILL.md)" -eq 0
success: test "$(grep -c 'no store write' claude/skills/review-v2-engine/references/triage-and-close.md)" -eq 0
success: test "$(grep -c 'one at a time' claude/commands/optimise-v2.md)" -eq 0
success: grep -q 'fn propose_never_exceeds_ceiling' tomlctl/tests/review_cells.rs
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- --error-format json flow resolve --flow no-such-flow-zz 2>&1 | grep -q '"kind":"not_found"'
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- review run show --help
install: cargo install --path tomlctl
install: cargo install --path glimpse
```

Prefix the full-verification cargo commands with `CARGO_INCREMENTAL=0`. The `test:` run includes `cli::dispatch::tests` (`command_lint`, `flag_table_lint`, `workflow_scripts_name_real_agents`, the skill line ceilings), which is what checks the prose tasks' `tomlctl` invocations against the new CLI. The two predicted criteria run as `success:` keys through `cargo run`, so they need no installed binary.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 1, 2, 3, 4, 5, 6, 7, 8, 9, 21, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 44
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### Phase 1: CLI foundations and independent repairs

### 1. Split the review clap types into per-verb modules [L]
- **Files**: `tomlctl/src/cli/types/review.rs` (delete), `tomlctl/src/cli/types/review/mod.rs` (new), `tomlctl/src/cli/types/review/run.rs` (new), `tomlctl/src/cli/types/review/cells.rs` (new), `tomlctl/src/cli/types/review/verdicts.rs` (new), `tomlctl/src/cli/types/review/read.rs` (new), `tomlctl/src/cli/types/review/triage.rs` (new), `tomlctl/src/cli/types/review/export.rs` (new)
- **Depends on**: —
- **Action**: Move the contents of `tomlctl/src/cli/types/review.rs` into a `review/` module directory with no behaviour change.
- **Detail**: `mod.rs` keeps `ReviewOp`, `ReviewRunOp`, `ReviewStore`, `ReviewStatsStore` and re-exports every `*Args` type so no path outside the module changes. `run.rs` takes the four `ReviewRun*Args`; `cells.rs` takes `ReviewSurveyArgs`, `ReviewPlanArgs`, `ReviewCoverArgs`, `ReviewAddArgs`; `verdicts.rs` takes `ReviewVerdictArgs`, `ReviewResolveArgs`; `read.rs` takes `ReviewListArgs`, `ReviewShowArgs`, `ReviewDigestArgs`, `ReviewReportArgs`, `ReviewStatsArgs`; `triage.rs` takes `ReviewTriageArgs`, `ReviewTriageMode`, `ReviewApplyArgs`, `NOT_DEFER`, `NOT_CONVERT`; `export.rs` takes `ReviewSyncArgs`, `ReviewExportArgs`, `ReviewCatalogueArgs`, `ReviewSchemaArgs`. Help text, flag names and group attributes move byte-for-byte. This split exists so the later flag tasks touch different files.
- **Acceptance**:
  - forward: `test -f tomlctl/src/cli/types/review/mod.rs && test ! -f tomlctl/src/cli/types/review.rs && echo split` prints `split` (today: prints nothing).
  - guard, **predicted, unverified**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` reports no error; `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(command_lint)'` passes, which fails if any flag was dropped in the move.

### 2. Move `review cover` into its own module [L]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/src/review/cover.rs` (new), `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs`
- **Depends on**: —
- **Action**: Move the `cover` verb and `coverage_accounting` out of `tomlctl/src/review/cells.rs` into `tomlctl/src/review/cover.rs`, with no behaviour change.
- **Detail**: Helpers both halves use stay in `tomlctl/src/review/cells.rs` as `pub(super)`. Keep `coverage_accounting` and `Accounting` reachable as `cells::…` through `pub(crate) use super::cover::{coverage_accounting, Accounting};` in `tomlctl/src/review/cells.rs`, so `tomlctl/src/review/digest.rs` and `tomlctl/src/review/runs.rs` stay untouched. `newest_wave` stays in `tomlctl/src/review/cells.rs`. Update the module list and the dispatch arm. The four files are inseparable: the move does not compile without the module and dispatch edits.
- **Acceptance**:
  - forward: `grep -c 'fn cover' tomlctl/src/review/cover.rs` prints at least `1` (today: no such file).
  - forward: `grep -c 'pub(crate) fn cover' tomlctl/src/review/cells.rs` prints `0` (today: `2`).
  - guard, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_cells)'` passes.

### 3. Add the `head` run field and the `ruled_out` cell field [M]
- **Files**: `tomlctl/src/review/fields.rs`, `tomlctl/src/review/store.rs`
- **Depends on**: —
- **Backlog**: B-827b9763, B-dc721e5c
- **Action**: Declare `head` on runs and `ruled_out` on cells in the field catalogue and the typed store.
- **Detail**: Approach, "Run bounds" and "Duplicates and rule-outs". In `FIELDS`: `field(Run, "head", Ty::Str, RECORD_STATE)` beside `base`; `field(Cell, "ruled_out", Ty::TableList, RECORD_STATE).computed()` beside `note` — orchestrator and report only. A verifier or audit column would make `role_sees` (`tomlctl/src/review/query.rs`) let `review show <cell> --for verifier` through, which `show_cell_carries_bound_catalogue_paths` in `tomlctl/tests/review_query.rs` refuses; task 16 brings rule-outs to those roles on findings. Entry keys are `file` (required, checked with `path_problem`), `symbol`, `line`, `reason` (required, 200-character cap); an unknown key is refused. `check_value` accepts any object for `Ty::TableList`, so follow the `Waiver` pattern: a typed `RuledOut` struct in `tomlctl/src/review/store.rs` with its `_from_toml`/`_to_toml` pair, plus `pub(crate) fn ruled_out_refusals(entry: &Map<String, JsonValue>) -> Vec<String>` in `tomlctl/src/review/fields.rs`, which task 13's `cover` calls. Add `head: Option<String>` to `Run` and `ruled_out` to `Cell` so both round-trip; a store with neither writes back byte-identical. Add unit tests `run_head_round_trips` and `ruled_out_entry_refuses_unknown_key` in the `fields.rs` test module.
- **Acceptance**:
  - forward: `grep -c 'field(Run, "head"' tomlctl/src/review/fields.rs` prints `1` (today: `0`).
  - forward: `grep -c 'field(Cell, "ruled_out"' tomlctl/src/review/fields.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(ruled_out_entry_refuses_unknown_key) | test(run_head_round_trips)'` passes.

### 4. Fail `flow resolve` on an explicit `--flow` miss [M]
- **Files**: `tomlctl/src/flow/resolve.rs`, `tomlctl/tests/flow_resolve.rs`, `claude/commands/tdd.md`
- **Depends on**: —
- **Backlog**: B-80c20e25
- **Action**: Make `resolve` return the `ErrorKind::NotFound` error for an explicit slug that matches no flow, whether or not `--strict-read` is set.
- **Detail**: Approach, "Explicit flow miss". Remove the fall-through warning branch in step 1 of `resolve`, and correct the module comment at the top of `tomlctl/src/flow/resolve.rs` that describes the error as strict-read-only. In `claude/commands/tdd.md`, the bootstrap-missing fallback's parent re-resolve (`tomlctl flow resolve --flow <parent-slug>`) halts on a non-zero exit instead of on `resolved == false`. Replace the test `explicit_flag_missing_falls_through_to_none_by_default` with `explicit_flag_missing_errors_with_not_found`, asserting a non-zero exit and the `not_found` kind under `--error-format json`; keep `explicit_flag_missing_strict_read_errors_with_not_found`. Security-adjacent (it decides which flow's stores a carrier writes), so not lite.
- **Acceptance**:
  - falsifier: `grep -c 'explicit_flag_missing_falls_through_to_none_by_default' tomlctl/tests/flow_resolve.rs` prints `1` today — the test that pins the old behaviour exists and must go.
  - forward: `grep -c 'fn explicit_flag_missing_errors_with_not_found' tomlctl/tests/flow_resolve.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(flow_resolve)'` passes.

### 5. Derive `waves` from a run's cells in `review stats` [S]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/tests/review_stats.rs`
- **Depends on**: —
- **Backlog**: B-27f5ba35
- **Action**: Report `waves` in `review stats --by run` as the highest `wave` among the run's cells instead of the never-written `run.waves`.
- **Detail**: Add one `pub(crate)` helper in `tomlctl/src/review/stats.rs` that returns the highest `wave` among a run's cells (`0` for a run with none) and call it from `by_run`; tasks 10, 15 and 19 call the same helper (Approach, "Reading a run back"). Add the test `stats_by_run_reports_waves_from_cells` with a two-wave fixture asserting `waves` is `2`.
- **Acceptance**:
  - forward: `grep -c 'fn stats_by_run_reports_waves_from_cells' tomlctl/tests/review_stats.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(stats_by_run_reports_waves_from_cells)'` passes; it fails on today's code, which prints `null`.

### 6. Fold `export-v1` rows only on a strict key [M]
- **Files**: `tomlctl/src/review/export.rs`, `tomlctl/tests/review_export.rs`
- **Depends on**: —
- **Backlog**: B-5d5bf09e
- **Action**: Restrict `decide`'s reuse of an existing v1 row to the strict key and stop two findings of one export call folding into each other.
- **Detail**: Approach, "Duplicates and rule-outs", strict export fold. Reuse when the row already carries the finding's `v2_ref`, or when `file`, `symbol` and `summary` are all equal and the row's own `v2_ref` names a finding in this store whose `lens` equals this finding's. A v1 row carries no `lens`, so a row with no resolvable `v2_ref` never matches on this second clause; additionally a finding `merged` into another exports onto that other's row. Today's loose match (same file and same non-empty symbol or exact summary) still decides `Settled` and `Regression`, so a dismissed, deferred or fixed v1 row keeps suppressing or flagging a re-found defect; a loose match on an open row that fails the strict key mints. Anything else mints. `export_regression_on_fixed_v1_match` must pass unchanged. Add `export_mints_separate_rows_for_different_defects_on_one_symbol` (two findings, same file and symbol, different summaries → two rows, each with its own text) and `export_does_not_fold_within_one_call`. Update `reuse_keeps_both_v2_refs` and `export_reuses_open_v1_id_and_bumps_rounds_once` to the strict key rather than deleting them; `export_is_idempotent_on_v2_ref` must pass unchanged.
- **Acceptance**:
  - forward: `grep -c 'fn export_mints_separate_rows_for_different_defects_on_one_symbol' tomlctl/tests/review_export.rs` prints `1` (today: `0`).
  - guard: `grep -c 'fn export_is_idempotent_on_v2_ref' tomlctl/tests/review_export.rs` prints `1`.
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_export)'` passes; the new test fails on today's `decide`, which folds on file + symbol.

### 7. Accept a mechanical cleanup row from any cell [M]
- **Files**: `tomlctl/src/review/add.rs`, `tomlctl/tests/review_add.rs`
- **Depends on**: —
- **Backlog**: B-30ba0a0a
- **Action**: Let `review add` accept a `cleanup` row whose `lens` is a sweeper-executed lens the cell does not bind, and pin that a finding's file may lie outside the run's scope.
- **Detail**: Approach, "Escalations and out-of-lens cleanup" and "Run bounds". The lens check in `field_refusals` (the `` `lens` is …; cell … covers … `` refusal) gains one exception: kind `cleanup`, exact `old` present, and a lens the merged catalogue marks sweeper-executed (`ripple`, `hygiene` — the marker is the lens file's `mode = "mechanical"`; read it from the catalogue, do not hardcode the ids). Such a row keeps the route `route_of` computes for any cleanup (checker for a non-executable edit, verifier otherwise). The exception applies to `review add` only: keep `field_refusals`' signature and add the allowance beside it in `tomlctl/src/review/add.rs` (for example a `field_refusals_with` that `field_refusals` delegates to), so its second caller, the advisory convert in `tomlctl/src/review/triage.rs`, is untouched and gets no exception. Add `add_accepts_mechanical_cleanup_outside_cell_lenses`, `add_refuses_defect_outside_cell_lenses` and `add_accepts_ripple_finding_outside_run_scope`.
- **Acceptance**:
  - forward: `grep -c 'fn add_accepts_mechanical_cleanup_outside_cell_lenses' tomlctl/tests/review_add.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_add)'` passes; the first new test fails on today's lens check. If `add_accepts_ripple_finding_outside_run_scope` fails on unchanged code, stop and report: Approach, "Run bounds" assumes no scope check exists.

### 8. Print each check's executor and command in `review catalogue` [M]
- **Files**: `tomlctl/src/review/catalogue.rs`, `tomlctl/src/review/catalogue_cmd.rs`, `tomlctl/tests/review_catalogue.rs`
- **Depends on**: —
- **Backlog**: B-989439ec
- **Action**: Add `executor` and `command` to every check entry `review catalogue` prints.
- **Detail**: Approach, "Reading a run back". A project `[[checks]]` entry prints `executor = "verification"` (already assigned in the catalogue loader) and its `command`; a built-in check prints its frontmatter executor and its command when it declares one. Add `catalogue_prints_project_check_executor_and_command`.
- **Acceptance**:
  - forward: `grep -c 'fn catalogue_prints_project_check_executor_and_command' tomlctl/tests/review_catalogue.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_catalogue)'` passes.

### 9. Fill the audit sample when nothing was refuted [M]
- **Files**: `tomlctl/src/review/digest.rs`, `tomlctl/tests/review_digest.rs`
- **Depends on**: —
- **Backlog**: B-1e1be4cf
- **Action**: Change `audit_sample` so empty refuted slots are filled with extra standing defects and plausible defects are sampled.
- **Detail**: Approach, "Audit and measurement". The standing group is defects whose deciding verdict is `confirmed` or `plausible`. Pick `SAMPLE_SIZE` from it as today; then for each refuted slot left empty, pick one more standing defect not already chosen, by the same `spread` rule over the remainder. Output keys stay `confirmed_defects`, `refuted_defects`, `checked_cleanup`; the extra picks land in `confirmed_defects`. Add `digest_audit_sample_fills_empty_refuted_slots` and `digest_audit_sample_includes_plausible_defects`; `digest_audit_sample_is_chosen_by_index` must still pass on a fixture with two refuted rows.
- **Acceptance**:
  - forward: `grep -c 'fn digest_audit_sample_fills_empty_refuted_slots' tomlctl/tests/review_digest.rs` prints `1` (today: `0`).
  - guard: `grep -c 'fn digest_audit_sample_is_chosen_by_index' tomlctl/tests/review_digest.rs` prints `1`.
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_digest)'` passes.

### Phase 2: CLI features on the foundations

### 10. Bound a run with `review run start --head` [M]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/src/cli/types/review/run.rs`, `tomlctl/tests/review_runs.rs`
- **Depends on**: 1, 3, 5
- **Backlog**: B-827b9763
- **Action**: Add the optional `--head <REV>` flag, store its sha on the run, and measure `changed_lines` over `base..head` when it is set.
- **Detail**: Approach, "Run bounds" and "Reading a run back". Resolve `--head` exactly as `resolve_base` resolves `--base`; refuse a head that is not a descendant of the base. `project_layer_changes` also reads `base..head`, so a lens-layer commit after `head` does not refuse the run. `run start`'s envelope prints `head` (null when absent). `run list` rows carry `head`, and their `waves` comes from task 5's helper in `tomlctl/src/review/stats.rs` instead of the never-written `run.waves`. Add `run_start_records_head_and_measures_to_it` (a commit after head does not change `changed_lines`), `run_start_refuses_head_not_descended_from_base` and `run_list_reports_waves_from_cells`.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) head' tomlctl/src/cli/types/review/run.rs` prints `1` (today: no such file).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_runs)'` passes.

### 11. Read the survey's diff to `head` and keep hotspots in scope [M]
- **Files**: `tomlctl/src/review/survey.rs`, `tomlctl/tests/review_survey.rs`
- **Depends on**: 3, 10
- **Backlog**: B-827b9763, B-39b9e713
- **Action**: Make the diff detectors read `base..head` when the run has a `head`, and record `churn-hotspot` evidence only for files in the run's scope.
- **Detail**: Approach, "Run bounds" and "Proposal ceiling and sweeper slices". `commits_per_file` still counts repo-wide history, but the evidence list and roots written for `churn-hotspot` keep only scope files. Add `survey_diff_stops_at_head` and `survey_hotspots_are_limited_to_scope`. Depends on 10 because the black-box harness in `tomlctl/tests/review_survey.rs` creates runs through `review run start`, and only `--head` yields a run with a `head`.
- **Acceptance**:
  - forward: `grep -c 'fn survey_hotspots_are_limited_to_scope' tomlctl/tests/review_survey.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_survey)'` passes.

### 12. Keep `review plan --propose` inside its ceiling [L]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/tests/review_cells.rs`
- **Depends on**: 2
- **Backlog**: B-39b9e713, B-d9cfd36e
- **Action**: Rework `propose` so the proposal never exceeds the ceiling, merges small floor slices before cutting signal lenses, and caps sweeper slices by file count.
- **Detail**: Approach, "Proposal ceiling and sweeper slices" is the specification; every sentence there is a requirement, including the 40-file sweeper cap, the adjacency and half-budget rules, the whole-scope split and the `over_cap` list for a budget too small to hold the cells. Touch points: the budget in `propose`, the floor-tier admission, `add_check_cells` (must draw on the budget), the `stopped` flag (a lens that fits an existing bundle is not `ceiling`), `slices_for` and `bundle`. Add `propose_never_exceeds_ceiling` (a 74-file, many-directory fixture at `standard`), `propose_merges_small_floor_slices_before_cutting_signal_lenses`, `propose_counts_check_cells_against_ceiling`, `propose_caps_sweeper_slice_by_file_count` and `propose_reports_over_cap_cells_at_a_small_ceiling`. Every existing `propose_*` test (`grep -n 'fn propose_' tomlctl/tests/review_cells.rs`) and `plan_executor_cells_batch_tasks_outside_ceiling` in `tomlctl/tests/review_cells.rs` must pass, adjusted only where they assert the old overshoot.
- **Acceptance**:
  - forward: `grep -c 'fn propose_never_exceeds_ceiling' tomlctl/tests/review_cells.rs` prints `1` (today: `0`).
  - guard: `grep -c 'fn propose_respects_scaled_ceiling' tomlctl/tests/review_cells.rs` prints `1`.
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_cells)'` passes; `propose_never_exceeds_ceiling` fails on today's code, which admits floor and check cells regardless of budget.

### 13. Record structured rule-outs with `review cover --ruled-out` [M]
- **Files**: `tomlctl/src/review/cover.rs`, `tomlctl/src/cli/types/review/cells.rs`, `tomlctl/tests/review_cover.rs` (new), `tomlctl/src/review/fields.rs`
- **Depends on**: 1, 2, 3
- **Backlog**: B-dc721e5c
- **Action**: Add `--ruled-out <SRC>` to `review cover`, validating and storing the entries on the cell.
- **Detail**: Approach, "Duplicates and rule-outs", rule-outs. `SRC` follows the group's convention (`-` for stdin, a path, a leading `@` accepted, a `.toml` record file). Entries validate through `ruled_out_refusals` from task 3 (`tomlctl/src/review/fields.rs`), and an entry whose `file` lies outside the cell's slice is refused unless the cell binds a sweeper-executed lens; a refused entry refuses the whole cover and seals nothing. Re-covering a cell replaces its `ruled_out`. Tests: `cover_stores_ruled_out_entries`, `cover_refuses_ruled_out_entry_without_reason`, `cover_without_ruled_out_leaves_the_cell_unchanged`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_cover.rs && grep -c 'fn cover_stores_ruled_out_entries' tomlctl/tests/review_cover.rs` prints `1` (today: no such file).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_cover)'` passes.

### 14. Carry `head` in the finder assignment [S]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs`
- **Depends on**: 3
- **Backlog**: B-827b9763
- **Action**: Add the run's `head` to the object `review show <cell> --for finder` prints, beside `base`.
- **Detail**: In `assignment`, emit `head` only when the run has one. Add `finder_assignment_carries_head_when_the_run_has_one`.
- **Acceptance**:
  - forward: `grep -c 'fn finder_assignment_carries_head_when_the_run_has_one' tomlctl/tests/review_query.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(finder_assignment_carries_head_when_the_run_has_one)'` passes.

### 15. List duplicate candidates and rule-out hits in `review digest` [L]
- **Files**: `tomlctl/src/review/digest.rs`, `tomlctl/src/review/matching.rs`, `tomlctl/tests/review_digest.rs`
- **Depends on**: 5, 9, 13
- **Backlog**: B-5d5bf09e, B-dc721e5c
- **Action**: Add the `duplicate_candidates` and `ruled_out_hits` keys to the digest, on one shared anchor matcher.
- **Detail**: Approach, "Duplicates and rule-outs" and "Reading a run back". `run_line` reads `waves` from task 5's helper in `tomlctl/src/review/stats.rs`; add `digest_run_line_reports_waves_from_cells`. In `tomlctl/src/review/matching.rs` add a `pub(crate)` anchor matcher (same file, and same non-empty symbol or lines within the existing candidate window) and use it for both joins. `duplicate_candidates` rows: `{a, b, file, symbol, cells}` with `a` the lower id; a pair already linked by `same_as` or `merged_into` is omitted. `ruled_out_hits` rows: `{finding, cell, ruled_out_by, reason}`; a cell's own findings never hit its own rule-outs. Both honour `--since-wave`. Tests: `digest_lists_duplicate_candidates_across_cells`, `digest_omits_linked_pairs_from_duplicate_candidates`, `digest_lists_findings_that_hit_another_cells_rule_out`. Depends on 13 because the black-box fixtures write rule-outs through `review cover`.
- **Acceptance**:
  - forward: `grep -c 'fn digest_lists_duplicate_candidates_across_cells' tomlctl/tests/review_digest.rs` prints `1` (today: `0`).
  - forward: `grep -c 'ruled_out_hits' tomlctl/src/review/digest.rs` prints at least `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_digest)'` passes.

### 16. Show matching rule-outs in the verifier and audit projections [M]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs`
- **Depends on**: 14, 15
- **Backlog**: B-dc721e5c
- **Action**: Add a `ruled_out` array to each finding printed `--for verifier` and `--for audit`, holding the other cells' rule-outs that match its anchor.
- **Detail**: Reuse the matcher task 15 adds to `tomlctl/src/review/matching.rs`. Each element is `{cell, reason}`. The key is absent when nothing matches, and never appears in the finder projection. Tests: `verifier_projection_carries_matching_rule_outs`, `finder_projection_never_carries_rule_outs`.
- **Acceptance**:
  - forward: `grep -c 'fn verifier_projection_carries_matching_rule_outs' tomlctl/tests/review_query.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_query)'` passes.

### 17. Filter `review list` by `--fix-shape` [S]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/src/cli/types/review/read.rs`, `tomlctl/tests/review_query.rs`
- **Depends on**: 1, 16
- **Backlog**: B-ce9ebb4d
- **Action**: Add `--fix-shape <SHAPE>` to `review list`, keeping findings whose `fix_shape` equals it.
- **Detail**: Approach, "Landing places". Validate the value against the field catalogue's `fix_shape` vocabulary and refuse an unknown one with `kind=validation`. Test: `list_filters_by_fix_shape_across_runs` (two runs, one `needs-plan` defect each, no `--run`).
- **Acceptance**:
  - forward: `grep -c 'pub(crate) fix_shape' tomlctl/src/cli/types/review/read.rs` prints `1` (today: no such file).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(list_filters_by_fix_shape_across_runs)'` passes.

### 18. Link duplicates with `review resolve --same-as` [L]
- **Files**: `tomlctl/src/review/resolve.rs`, `tomlctl/src/cli/types/review/verdicts.rs`, `tomlctl/tests/review_resolve.rs`
- **Depends on**: 1
- **Backlog**: B-5d5bf09e
- **Action**: Add the repeatable `--same-as <DUP>=<TARGET>` flag, which sets `same_as` on the duplicate and resolves the pair in the same call.
- **Detail**: Approach, "Duplicates and rule-outs", duplicate candidates. Both ids must be findings of `--run`, of the same family, and neither superseded; a self-link, a cycle or a duplicate already merged elsewhere is refused before anything is written. The link applies to rows that are already verified, which today's pass skips: a linked duplicate resolves through the existing `apply` path and `Outcome::for_target` table unchanged: it becomes `merged` into the target with `outcome` repeat, contested, reopened or suppressed (or stays open as a regression), exactly as a row carrying `same_as` from `review add` would. Report linked pairs in the envelope under `linked`. Tests: `resolve_same_as_merges_a_verified_pair`, `resolve_same_as_marks_differing_verdicts_contested`, `resolve_same_as_refuses_a_cycle`.
- **Acceptance**:
  - forward: `grep -c 'fn resolve_same_as_merges_a_verified_pair' tomlctl/tests/review_resolve.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_resolve)'` passes.

### 19. Add the `review run show` read verb [L]
- **Files**: `tomlctl/src/review/run_show.rs` (new), `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs`, `tomlctl/src/cli/types/review/run.rs`, `tomlctl/src/cli/types/review/mod.rs`, `tomlctl/src/cli/mod.rs`, `tomlctl/tests/output_options.rs`, `tomlctl/tests/review_run_show.rs` (new)
- **Depends on**: 1, 2, 5, 10
- **Backlog**: B-99efd715
- **Action**: Add `review run show <RUN>`, printing one run's bounds, signals, baselines, waves, agents and audit.
- **Detail**: Approach, "Reading a run back" gives the output. A `ReviewRunOp::Show` leaf taking the run id, the store group and the read-integrity bundle. An unknown run is `kind=not_found`. The `ReviewRunOp::Show` variant goes in `tomlctl/src/cli/types/review/mod.rs`, where task 1 keeps the enum, and the new args type joins the explicit `Review*Args` re-export list in `tomlctl/src/cli/mod.rs`. `waves` comes from task 5's helper in `tomlctl/src/review/stats.rs`. The files are inseparable (a verb needs its clap leaf, enum variant, re-export, dispatch arm, module entry and implementation). Depends on 2 and 10 because they edit `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs` and `tomlctl/src/cli/types/review/run.rs`. Tests: `run_show_prints_signals_with_roots_limited_to_scope`, `run_show_prints_recorded_baselines`, `run_show_derives_waves_from_cells`. Add a `review run show` read case beside the `review run list` one in `tomlctl/tests/output_options.rs`: `every_leaf_command_has_a_case` fails on a leaf with no case, and print through the shared output seam as `runs::list` does.
- **Acceptance**:
  - forward: `test -f tomlctl/src/review/run_show.rs && echo present` prints `present` (today: prints nothing).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_run_show)'` passes.

### 20. Bump tomlctl to 0.22.0 and register the new surface [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md`, `tomlctl/src/capabilities.rs`, `tomlctl/src/cli/types/mod.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 4, 5, 6, 7, 8, 10, 11, 12, 13, 15, 16, 17, 18, 19
- **Action**: Set the version to 0.22.0 and register `review run show` and the new flags as capabilities.
- **Detail**: Follow the shape of the 0.21.0 bump (`git show 6454bc72 --stat`). Add `review run show` to `MUTEX_GROUPS` with the review store group and to the read-verb list in `tomlctl/tests/capabilities.rs`. Add `FEATURES` entries `review_run_head`, `review_run_show`, `review_ruled_out`, `review_same_as_link`, `review_list_fix_shape`. Refresh `glimpse/Cargo.lock` with `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null`. In `tomlctl/README.md`, set the capabilities sample's `"version"` to `0.22.0`, add the five names to its `"features"` array and one "Feature meanings" row each (`readme_feature_transcriptions_match_capabilities_features` checks both), and correct the `--strict-read` paragraph's `flow resolve --flow` clause, which task 4 makes unconditional. In `tomlctl/tests/capabilities.rs`, add the five names to the expected array of `capabilities_features_contains_every_plan_feature` and change the `"0.21.0"` literal in `capabilities_version_matches_cargo_toml`. The seven files are inseparable: the pre-commit hook's `cargo clippy --manifest-path glimpse/Cargo.toml --locked` fails on a version bump without the refreshed lock.
- **Acceptance**:
  - forward: `grep -c '^version = "0.22.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `grep -c 'review_run_show' tomlctl/src/cli/types/mod.rs` prints at least `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(capabilities)'` and `cargo clippy --manifest-path glimpse/Cargo.toml --locked` pass.

### 21. Document the new review surface in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/references/review.md`, `claude/skills/tomlctl/references/review-runs.md` (new), `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 20
- **Action**: Move the `review run` section into a new reference and document every flag, verb, field and output key this plan adds.
- **Detail**: `claude/skills/tomlctl/references/review.md` is 583 lines against the 600-line reference ceiling (`skill_references_under_line_ceiling`), so the `review run` verbs move to `claude/skills/tomlctl/references/review-runs.md`, which gains `--head` and `run show`. `claude/skills/tomlctl/references/review.md` gains `cover --ruled-out`, the `ruled_out` cell field, `resolve --same-as`, `list --fix-shape`, the digest's `duplicate_candidates` and `ruled_out_hits`, the audit-sample fill, the strict export fold, the mechanical-cleanup lens exception and the checker's widened `refuted` rule, the catalogue's `executor` / `command`, the finder projection's `head`, the verifier and audit projections' `ruled_out`, the survey's `base..head` diff and scope-limited hotspots, `run list`'s `head` and derived `waves`, the proposal's `over_cap` list, and `resolve`'s `linked` key. State the 450-character target beside each 600 cap. Add the new file to the References list in `claude/skills/tomlctl/SKILL.md` and extend its Quick Reference `review` row with `run show`.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/review-runs.md && echo present` prints `present` (today: prints nothing).
  - forward: `grep -c -- '--ruled-out' claude/skills/tomlctl/references/review.md` prints at least `1` (today: `0`).
  - guard: `test "$(wc -l < claude/skills/tomlctl/references/review.md)" -le 600 && echo ok` prints `ok`.
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(flag_table_lint) | test(command_lint) | test(skill_references_under_line_ceiling) | test(skill_markdown_links_resolve)'` passes.

### Phase 3: Workflow scripts

### 22. Batch cleanup verification and return escalations and an agent count [M]
- **Files**: `claude/workflows/review-v2.js`
- **Depends on**: —
- **Backlog**: B-e684aed8, B-bc4a0080, B-27f5ba35
- **Action**: Change `routeCell` and `index` in `claude/workflows/review-v2.js` to the shape Approach, "Workflow index" gives.
- **Detail**: In `routeCell`, a `verifier`-routed finding of kind `cleanup` joins a per-cell batch dispatched through `verifyBatch` (label `verify <cell> cleanup xN`, batch `verify-<cell>-cleanup`); a single row still goes through `verifyOne`. Collect each cell's `escalate` strings, cut to 300 characters, into the top-level `escalations` array of `{cell, text}`, the index's last key; each cell entry keeps its `escalated` count. Count each dispatch at its call site: either `agents++` beside each `agent(` call, or a thunk wrapper such as `counted(() => agent(prompt, { agentType: … }))` that keeps the literal options object on the `agent(` call. Never use a pass-through `(...args) => agent(...args)`, which `workflow_scripts_name_real_agents` refuses as an `agent()` call with no `agentType`. Return the total as `agents`; the early-return `index([],0)` reports `agents: 0`. Read the optional `mechanical_lenses` argument (an array of lens ids) and add a `mechanical lenses: <ids or none>` line beside the checker prompt's `sweeper rows:` line, per Approach, "Escalations and out-of-lens cleanup". Set `SCRIPT_VERSION` to `'2'` and correct the header comment. No `model` or `effort` override and no new `agentType` (`workflow_scripts_name_real_agents`).
- **Acceptance**:
  - forward: `grep -c 'escalations' claude/workflows/review-v2.js` prints at least `1` (today: `0`).
  - forward: `grep -c "SCRIPT_VERSION = '2'" claude/workflows/review-v2.js` prints `1` (today: `0`).
  - guard, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(workflow_scripts_name_real_agents) | test(command_lint)'` passes.

### 23. Sum the agent count in the plan-review Workflow [M]
- **Files**: `claude/workflows/plan-v2-review.js`
- **Depends on**: 21, 22
- **Backlog**: B-27f5ba35, B-99efd715, B-ab53208e, B-bc4a0080, B-dc721e5c
- **Action**: Bring `claude/workflows/plan-v2-review.js` onto the rules Approach, "Workflow index" gives it: a full agent count, a signal-free surveyor prompt, one baseline dispatch, and escalations and re-verification through the lead's settle phase.
- **Detail**: Read `ran.agents` beside `ran.verified`, defaulting to 0 when it is not an integer. Count every `agent()` call the script makes (the surveyor, the baseline dispatch, the lead's compose, settle and audit, and each re-verification) at its call site, keeping each call's literal `agentType`, and return the total with the waves' sums as `agents`; set `SCRIPT_VERSION` to `'2'`. Survey phase: drop the `signals:` lines from the surveyor prompt — the surveyor reads `tomlctl review run show <run>` by the run id the prompt already carries. Baselines: replace the per-command loop with one `verification` dispatch listing every resolved command, each with its `timeout`, and `keep_going: true`; widen `BASELINE_SCHEMA` to one `{key, outcome, log}` entry per command and build `baselineLines` from that return, keeping today's `none — …` line for a command with no log. Waves: pass the optional `mechanical_lenses` argument through to `review-v2-pipeline`; hand the wave's `ran.escalations` to the lead's `settle` prompt as an `escalations:` block (`none` when empty); add an optional `reverify` array of finding ids to the settle schema and dispatch one `review-verifier` per id beside the existing mop-up of `unverified`. Depends on 21 because the script then names `review run show`, which `command_lint` checks against the CLI.
- **Acceptance**:
  - forward: `grep -c 'ran.agents' claude/workflows/plan-v2-review.js` prints at least `1` (today: `0`).
  - forward: `grep -c 'One at a time' claude/workflows/plan-v2-review.js` prints `0` (today: `1`).
  - forward: `grep -c 'reverify' claude/workflows/plan-v2-review.js` prints at least `1` (today: `0`).
  - guard, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(workflow_scripts_name_real_agents)'` passes.

### Phase 4: Agent and skill prose

### 24. State run bounds, stage-0 carry-over and the new index in the engine skill [M]
- **Files**: `claude/skills/review-v2-engine/SKILL.md`
- **Depends on**: 21, 22
- **Backlog**: B-827b9763, B-ce9ebb4d, B-bc4a0080, B-27f5ba35, B-ab53208e
- **Action**: Rewrite the stage 0, stage 1, stage 3 baseline, stage 4 index and stage 5 escalation passages of `claude/skills/review-v2-engine/SKILL.md`.
- **Detail**: Stage 0: add the two `review list` reads from Approach, "Landing places", and add `review_run_head`, `review_run_show`, `review_ruled_out`, `review_same_as_link` and `review_list_fix_shape` to the pre-flight `.features` gate. Stage 1: replace the "against `HEAD~1` on the default branch" sentence with the three-case rule in Approach, "Run bounds", including `--head`, the scratch-staged `scope=@<path>` and the ripple sentence. Baselines: one dispatch, per Approach, "Agent prose rules". Stage 4: the index shape from Approach, "Workflow index"; `agents` is the index's `agents` summed over waves plus the orchestrator's own dispatches (bootstrap, surveyor, baselines), recorded with one `review run update`; the 8,000-character note. Stage 5: read `escalations` from the index, and `duplicate_candidates` and `ruled_out_hits` from the digest. Also correct three sentences the other tasks make stale: the project-layer refusal reads `base..head` when the run has a head; the routing sentence and the depth-table row that describe per-cell batching for minor defects only now cover verifier-routed cleanup rows too; and the Workflow launch passes `mechanical_lenses`, the ids `tomlctl review catalogue` lists with `mode = "mechanical"`. Keep the file under 500 lines.
- **Acceptance**:
  - forward: `grep -c 'HEAD~1' claude/skills/review-v2-engine/SKILL.md` prints `0` (today: `1`).
  - forward: `grep -c -- '--head' claude/skills/review-v2-engine/SKILL.md` prints at least `1` (today: `0`).

### 25. Read signals from the store and run baselines in one dispatch [M]
- **Files**: `claude/skills/review-v2-engine/references/survey.md`
- **Depends on**: 21
- **Backlog**: B-99efd715, B-ab53208e, B-989439ec, B-827b9763
- **Action**: Rewrite the signals hand-off, the project-check rule and the baselines section of `claude/skills/review-v2-engine/references/survey.md`.
- **Detail**: Replace "Its output is your only copy of the signals" and the instruction to copy signals into the surveyor prompt: the surveyor prompt carries the run id and reads `tomlctl review run show <run>`. The diff reads `base..head` when the run has a head. Project checks: the surveyor takes executor and command from `tomlctl review catalogue`. Section 6: one `verification` dispatch with every baseline command and `keep_going: true`, each command's `timeout`, all keys recorded from the one return and read back with `review run show`. Remove every "one at a time" and "once per resolved command" sentence.
- **Acceptance**:
  - forward: `grep -c 'only copy of the signals' claude/skills/review-v2-engine/references/survey.md` prints `0` (today: `1`).
  - forward: `grep -c 'keep_going' claude/skills/review-v2-engine/references/survey.md` prints at least `1` (today: `0`).

### 26. Settle duplicates, rule-out hits and escalations at compose [M]
- **Files**: `claude/skills/review-v2-engine/references/compose.md`
- **Depends on**: 21, 22
- **Backlog**: B-5d5bf09e, B-bc4a0080, B-ab53208e, B-dc721e5c
- **Action**: Add the stage-5 settling steps and correct the baseline and sweeper-slice lines of `claude/skills/review-v2-engine/references/compose.md`.
- **Detail**: Stage-5 decisions, in order: each `duplicate_candidates` pair is linked with `review resolve --same-as` or left as distinct, one line of reasoning each in the run directory; each `ruled_out_hits` row is checked against its verdict and re-verified when the verifier's rationale does not address the rule-out; each `escalations` entry becomes a follow-up cell, a mechanical row the orchestrator asks the next cell to write, or a close-summary item (Approach, "Escalations and out-of-lens cleanup"). Judged-signal candidates citing a file outside the scope are rejected. The baseline line points at the single dispatch. Note that the proposal now slices sweeper cells itself.
- **Acceptance**:
  - forward: `grep -c 'duplicate_candidates' claude/skills/review-v2-engine/references/compose.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'ruled_out_hits' claude/skills/review-v2-engine/references/compose.md` prints at least `1` (today: `0`).

### 27. Bring the Agent-wave fallback in line with the Workflow [M]
- **Files**: `claude/skills/review-v2-engine/references/agent-waves.md`
- **Depends on**: 21, 22
- **Backlog**: B-e684aed8, B-bc4a0080, B-27f5ba35, B-99efd715
- **Action**: Update the routing table, the wave index, the signals lines and the agent total in `claude/skills/review-v2-engine/references/agent-waves.md`.
- **Detail**: Routing table: add the per-cell batch for verifier-routed cleanup rows, and the checker prompt's `mechanical lenses:` line beside `sweeper rows:`. Wave index: `escalations` and `agents` as in Approach, "Workflow index". The surveyor prompt lines stop copying signals (`review run show`). The agent total is counted once, from the wave indexes plus the orchestrator's own dispatches.
- **Acceptance**:
  - forward: `grep -c 'escalations' claude/skills/review-v2-engine/references/agent-waves.md` prints at least `1` (today: `0`).

### 28. Record every audited row and give plan and escalation dispositions a landing place [M]
- **Files**: `claude/skills/review-v2-engine/references/triage-and-close.md`
- **Depends on**: 21
- **Backlog**: B-fc4d7f8b, B-1e1be4cf, B-ce9ebb4d, B-bc4a0080
- **Action**: Rewrite the stage-6 sample and recording passages and the `plan` grammar row of `claude/skills/review-v2-engine/references/triage-and-close.md`, and add unhandled escalations to the close summary.
- **Detail**: Approach, "Audit and measurement" and "Landing places". The sample line describes the fill. Recording: one staged NDJSON batch of `--by audit` verdicts covering every sampled row, `confirmed` or `refuted` only, with the reason a `plausible` audit verdict is not used; `audit.sampled` equals the batch size. The `plan F5` row: mint the backlog row through the `backlog-capture` gate, then `review triage F5 --backlog-ref <B-id> --expect-status open`. Close summary: each escalation not turned into a cell, offered as a backlog capture.
- **Acceptance**:
  - forward: `grep -c 'A row you agree with gets no verdict' claude/skills/review-v2-engine/references/triage-and-close.md` prints `0` (today: `1`).
  - forward: `grep -c 'no store write' claude/skills/review-v2-engine/references/triage-and-close.md` prints `0` (today: `1`).

### 29. Add the leniency readings and the seeded-precision probe to measurement [M]
- **Files**: `claude/skills/review-v2-engine/references/measurement.md`
- **Depends on**: 21
- **Backlog**: B-1e1be4cf, B-0bdfc2dc
- **Action**: Add the low-side refute band, the audit-band floor and the seeded-precision probe section to `claude/skills/review-v2-engine/references/measurement.md`, and correct its two stale lines.
- **Detail**: Approach, "Audit and measurement", third and fourth bullets, written out as a Thresholds row, a floor sentence and a "Seeded precision check" section beside the existing seeded-recall one, using only existing verbs (`review run start --scope`, `review plan --ndjson`, `review add`, one unnamed `review-verifier` dispatch). Record as watch items, with the command that reads each: in-run planted negatives, and the share of verifier confirms citing only the finding's own anchor (11 of 22 in run U1, measured 2026-11-10 with `tomlctl review list --slug jiggly-exploring-robin --run U1 --for audit`). In the "Plan seeded recall" section of `claude/skills/review-v2-engine/references/measurement.md`, replace the sentence "No v2 plan has run under an execution engine yet…" and the "Until one does," clause of the next sentence (leave the "Contract note for the execution engine" section untouched) with the fact that `/implement-v2` has run `human-only-handovers`.
- **Acceptance**:
  - forward: `grep -c 'No v2 plan has run under an execution engine yet' claude/skills/review-v2-engine/references/measurement.md` prints `0` (today: `1`).
  - forward: `grep -ci 'seeded precision' claude/skills/review-v2-engine/references/measurement.md` prints at least `1` (today: `0`).

### 30. Update the plan-flow reference for scope staging and the index [S]
- **Files**: `claude/skills/review-v2-engine/references/plan-flow.md`
- **Depends on**: 21, 23
- **Backlog**: B-27f5ba35, B-99efd715, B-ab53208e
- **Action**: Update the scope lines, the pre-flight gate, the signals and baseline passages, the documented Workflow index, the agents line and the escalations line of `claude/skills/review-v2-engine/references/plan-flow.md`.
- **Detail**: Scope lists are staged in the scratch directory and passed as `scope=@<path>`. Section 1's `.features` gate gains `review_run_head`, `review_run_show`, `review_ruled_out`, `review_same_as_link` and `review_list_fix_shape`. Replace the sentence saying no read verb returns the signals: `tomlctl review run show <run>` does, and the surveyor reads it. The baseline passage describes one `verification` dispatch with `keep_going: true`. The `plan-v2-review` index gains `agents`; `agents` on the run is taken from it. Each wave's index carries the top-level `escalations` array, which the Workflow hands to the lead's `settle` phase, and the lead's settle return gains `reverify` (Approach, "Workflow index"). The launch passes `mechanical_lenses`.
- **Acceptance**: forward: `grep -c 'scope=@' claude/skills/review-v2-engine/references/plan-flow.md` prints at least `1` (today: `0`).

### 31. State the shared agent rules in the method skill [M]
- **Files**: `claude/skills/review-v2-method/SKILL.md`
- **Depends on**: 21
- **Backlog**: B-00ae0571, B-d9cfd36e, B-30ba0a0a, B-dc721e5c
- **Action**: Add the cap target, the swept-slice definition, the mechanical-cleanup lens exception and the structured rule-out to `claude/skills/review-v2-method/SKILL.md`.
- **Detail**: Approach, "Agent prose rules" (cap target), "Proposal ceiling and sweeper slices" (the swept definition, under the coverage statuses), "Escalations and out-of-lens cleanup" (beside the rule that `lens` is one of the cell's concerns) and "Duplicates and rule-outs" (the coverage note's ruled-out part becomes a staged `--ruled-out` file; the note keeps counts only). Show the `review cover` call with `--ruled-out`. Keep the file under 500 lines.
- **Acceptance**:
  - forward: `grep -c '450 characters' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: `0`).
  - forward: `grep -c -- '--ruled-out' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: `0`).

### 32. Correct the method references [S]
- **Files**: `claude/skills/review-v2-method/references/finding-kinds.md`, `claude/skills/review-v2-method/references/verifying.md`, `claude/skills/review-v2-method/references/plan-records.md`
- **Depends on**: 21
- **Backlog**: B-00ae0571, B-0bdfc2dc
- **Action**: State the 450-character target where each reference states the 600 cap, and replace the stale handover field list.
- **Detail**: In `claude/skills/review-v2-method/references/plan-records.md` the handover fields are `reason`, `action`, `when`, `done_when`, `source_task`, `done`; `command` is gone. In `claude/skills/review-v2-method/references/verifying.md`, add one worked verdict that weighs a `ruled_out` entry from the projection.
- **Acceptance**:
  - forward: `grep -c 'done_when' claude/skills/review-v2-method/references/plan-records.md` prints at least `1` (today: `0`).
  - forward: `grep -c '450 characters' claude/skills/review-v2-method/references/verifying.md` prints at least `1` (today: `0`).

### 33. Update the finder body and make the context-free finder context-free [M]
- **Files**: `claude/agents/review-finder.md`, `claude/agents/review-finder-cf.md`
- **Depends on**: 21
- **Backlog**: B-75341a1d, B-30ba0a0a, B-00ae0571, B-dc721e5c, B-827b9763
- **Action**: Edit the shared `review-finder-body` block identically in both files, and change `claude/agents/review-finder-cf.md`'s frontmatter.
- **Detail**: Approach, "Agent prose rules", context-free finder. Shared body: the `research-methods` sentence reworded to hold for both carriers; the slice diff reads `git diff <base> <head> -- <slice entries>` when the assignment carries `head`; a context-free cell reads nothing under `.claude/flows/` or `docs/flows/`; a mechanical cleanup outside the bound lenses is written under `ripple` or `hygiene`, not escalated; ruled-out candidates go in a staged `--ruled-out` file; the note aims for 450 characters. Frontmatter of the cf file only: drop `research-methods` from `skills:` and correct the description ("without the user, project and local CLAUDE.md"; path-scoped rules still load). The two files are inseparable: `scripts/verify-shared-blocks.sh` requires the block byte-identical. Edit nothing between the nested `review-v2-store-writer` and `research-delivering` markers inside the body: those blocks are shared with other agent files (`scripts/shared-blocks.toml`) that this task does not own. A context-free cell may still read a path under `.claude/flows/` or `docs/flows/` that is inside its own slice.
- **Acceptance**:
  - forward: `sed -n '1,12p' claude/agents/review-finder-cf.md | grep -c 'research-methods'` prints `0` (today: `1`).
  - forward: `grep -c -- '--ruled-out' claude/agents/review-finder.md` prints at least `1` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0.

### 34. Define a swept slice for the sweeper and let ripple reach past the scope [M]
- **Files**: `claude/agents/review-sweeper.md`, `claude/skills/review-v2-lenses/methods/sweep.md`, `claude/skills/review-v2-lenses/lenses/ripple.md`
- **Depends on**: 21
- **Backlog**: B-d9cfd36e, B-00ae0571, B-827b9763
- **Action**: Rewrite the sweeper's sweep and seal steps and the two catalogue files they cite.
- **Detail**: Approach, "Proposal ceiling and sweeper slices" (the swept definition; work from the diff's removed and renamed names, not whole files) and "Run bounds" (a stale reference in an untouched file is a `ripple` finding on that file). Tell the sweeper to grep with narrow paths, one directory root per call, never a brace glob from the repository root. The note aims for 450 characters; `claude/skills/review-v2-lenses/methods/sweep.md` drops its "one note line per sweep" instruction, which the method skill overrides.
- **Acceptance**:
  - forward: `grep -c 'could not sweep every file in the slice' claude/agents/review-sweeper.md` prints `0` (today: `1`).
  - forward: `grep -ci 'outside the scope' claude/skills/review-v2-lenses/lenses/ripple.md` prints at least `1` (today: `0`).

### 35. Fix the surveyor's checks, evidence and writes [M]
- **Files**: `claude/agents/review-surveyor.md`
- **Depends on**: 21
- **Backlog**: B-989439ec, B-99efd715
- **Action**: Rewrite the check-listing, candidate-evidence, signal-reading and file-writing steps of `claude/agents/review-surveyor.md`.
- **Detail**: Approach, "Agent prose rules", surveyor. Replace "note its frontmatter `executor`" with reading `executor` and `command` from `tomlctl review catalogue`; every `verification` check, project checks included, is listed under `verification:` with its command. Candidate evidence paths must be scope files; a candidate with none is dropped. Signals come from `tomlctl review run show <run>`. `profile.md` and `pack.md` are each written once, whole, with Write; a correction rewrites the file. Add a line bounding its output: no restating of file contents in its turns.
- **Acceptance**:
  - forward: `grep -c 'review run show' claude/agents/review-surveyor.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'note its frontmatter' claude/agents/review-surveyor.md` prints `0` (today: `1`).

### 36. Tell the verifier about rule-outs, batches and the cap target [S]
- **Files**: `claude/agents/review-verifier.md`
- **Depends on**: 21
- **Backlog**: B-00ae0571, B-dc721e5c, B-e684aed8
- **Action**: Add three rules to `claude/agents/review-verifier.md`.
- **Detail**: A finding's projection may carry `ruled_out`; the rationale must say why the rule-out does or does not defeat the finding. A batch may hold cleanup rows from one cell as well as minor defects. The rationale aims for 450 characters.
- **Acceptance**: forward: `grep -c 'ruled_out' claude/agents/review-verifier.md` prints at least `1` (today: `0`).

### 37. Update the plan executor and the plan review lead [M]
- **Files**: `claude/agents/plan-executor.md`, `claude/agents/plan-review-lead.md`
- **Depends on**: 21, 23
- **Backlog**: B-00ae0571, B-fc4d7f8b, B-5d5bf09e, B-dc721e5c, B-bc4a0080
- **Action**: State the cap target and the staged rule-out file in `claude/agents/plan-executor.md`, and make `claude/agents/plan-review-lead.md`'s audit phase record every sampled row and its settle phase act on duplicates, rule-out hits and escalations.
- **Detail**: The lead's `audit` phase follows Approach, "Audit and measurement", first bullet, in place of recording overturns only. Its `settle` phase follows Approach, "Duplicates and rule-outs" and "Workflow index": after `review resolve` and the digest, it links each `duplicate_candidates` pair it judges the same with `review resolve --run <run> --same-as <dup>=<target>`; it returns under a new `reverify` key the id of each `ruled_out_hits` finding whose verifier rationale does not address the rule-out (`[]` when none) — it cannot dispatch, so the Workflow re-verifies them; and it reads the wave's escalation text from the `escalations:` block of its assignment when deciding the follow-up. The return object becomes `{"next": …, "unverified": […], "reverify": […]}`, the shape task 23 gives the Workflow's settle schema.
- **Acceptance**:
  - forward: `grep -c '450 characters' claude/agents/plan-executor.md` prints at least `1` (today: `0`).
  - forward: `grep -ci 'every sampled row' claude/agents/plan-review-lead.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'reverify' claude/agents/plan-review-lead.md` prints at least `1` (today: `0`).

### 38. Point the code and perf carriers at the new scope and baseline rules [S]
- **Files**: `claude/commands/review-v2.md`, `claude/commands/optimise-v2.md`, `claude/commands/review-plan-v2.md`
- **Depends on**: 21
- **Backlog**: B-827b9763, B-ab53208e
- **Action**: Replace each carrier's restated scope and per-command baseline sentences with a pointer to the engine's stage 1 and survey section 6.
- **Detail**: Carriers stay thin: no restatement of the rule. A scope argument may be a range, a commit list or paths, and each carrier names `--head` as the run's upper bound. Scope lists are staged outside the repository. `claude/commands/optimise-v2.md` drops "once per line, one at a time" for the single `keep_going` dispatch and its "measures `changed_lines` over `base..HEAD`" sentence for `base..head`. `claude/commands/review-plan-v2.md` restates the per-line baseline rule ("once per line, one at a time") and is edited the same way. `claude/commands/review-v2.md`'s sentence that the finder reads the slice as `git diff <base>` gains the `<head>` form for a bounded run.
- **Acceptance**:
  - forward: `grep -c 'one at a time' claude/commands/review-plan-v2.md` prints `0` (today: `1`).
  - forward: `grep -c 'one at a time' claude/commands/optimise-v2.md` prints `0` (today: `1`).
  - forward: `grep -c -- '--head' claude/commands/review-v2.md` prints at least `1` (today: `0`).

### 39. Record mechanical-lens ownership in the lens catalogue [S]
- **Files**: `claude/skills/review-v2-lenses/SKILL.md`, `claude/skills/review-v2-lenses/lenses/hygiene.md`
- **Depends on**: 21
- **Backlog**: B-30ba0a0a
- **Action**: State that a sweeper-executed lens owns its rows but accepts one from any cell.
- **Detail**: Approach, "Escalations and out-of-lens cleanup". In `claude/skills/review-v2-lenses/SKILL.md`, where it says `ripple` and `hygiene` go to `review-sweeper`, and in its Diff detector line, which reads `base..head` when the run has a head; in `claude/skills/review-v2-lenses/lenses/hygiene.md`, beside its ownership section.
- **Acceptance**: forward: `grep -ci 'from any cell' claude/skills/review-v2-lenses/SKILL.md` prints at least `1` (today: `0`).

### Phase 5: Ledger repair and checker refutation

### 40. Repair the two v1 ledger folds run U1 left [S]
- **Files**: `.claude/flows/jiggly-exploring-robin/review-ledger.toml`, `.claude/flows/jiggly-exploring-robin/review-ledger.toml.sha256`
- **Depends on**: —
- **Backlog**: B-5d5bf09e
- **Action**: Give finding F4 its own v1 row instead of riding on R22, and fold R31 into R26, in `.claude/flows/jiggly-exploring-robin/review-ledger.toml`.
- **Detail**: Every write goes through `tomlctl items` (load the `tomlctl` and `flow-contract-ledger-schema` skills first); never line-edit the ledger. R22 carries the `v2_ref` of both F1 and F4, which are different defects on one symbol: set R22's `v2_ref` to F1's alone, then mint a row for F4 with `tomlctl items add … --id-prefix R`, its fields taken from `tomlctl review show F4 --slug jiggly-exploring-robin --for implementer` and its `v2_ref` naming F4. R26 (finding F6) and R31 (finding F14) are one bug with overlapping edits: append F14's `v2_ref` and R31's test-file edit to R26's `v2_ref` and `description`, then close R31 as a duplicate of R26 with the status and companion fields the ledger schema gives a not-applied duplicate. Run no export and apply no finding. The ledger has uncommitted changes from run U1's export; this task's commit stages them with its own.
- **Acceptance**:
  - forward: `tomlctl items list .claude/flows/jiggly-exploring-robin/review-ledger.toml --where-contains 'v2_ref=review-v2.toml#F4' --get id` prints one id that is not `R22` (today: `R22`).
  - forward: `tomlctl items list .claude/flows/jiggly-exploring-robin/review-ledger.toml --where-contains 'v2_ref=review-v2.toml#F14' --get id` prints `R26` (today: `R31`).

### 41. Let the checker refute a mechanical cleanup from any cell [M]
- **Files**: `tomlctl/src/review/verdict.rs`, `tomlctl/tests/review_verdict.rs`
- **Depends on**: —
- **Backlog**: B-30ba0a0a, B-e684aed8
- **Action**: Widen the checker's `refuted` allowance in `tomlctl/src/review/verdict.rs` from rows a sweeper cell wrote to those rows plus any cleanup row whose lens is mechanical.
- **Detail**: Approach, "Escalations and out-of-lens cleanup". Today `written_by_sweeper` allows a checker `refuted` only when the finding's cell agent is `review-sweeper`. Allow it also when the finding's kind is `cleanup` and the merged catalogue marks its `lens` `mode = "mechanical"` (read the marker from the catalogue; do not hardcode `ripple` and `hygiene`). Every other checker `refuted` stays refused with today's message. Add `checker_may_refute_a_mechanical_cleanup_from_a_finder_cell` and `checker_refute_of_a_non_mechanical_finder_row_is_refused`.
- **Acceptance**:
  - forward: `grep -c 'fn checker_may_refute_a_mechanical_cleanup_from_a_finder_cell' tomlctl/tests/review_verdict.rs` prints `1` (today: `0`).
  - forward, **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_verdict)'` passes; the first new test fails on today's `written_by_sweeper`, which refuses the verdict.

### 42. Tell the checker which rows it may refute [S]
- **Files**: `claude/agents/review-checker.md`
- **Depends on**: 21, 22, 41
- **Backlog**: B-30ba0a0a
- **Action**: Widen the `refuted` rule in `claude/agents/review-checker.md` to cover mechanical-lens cleanup rows.
- **Detail**: Approach, "Escalations and out-of-lens cleanup". The rule that allows `refuted` only on a row the assignment's `sweeper rows:` line names also allows it on a cleanup row whose `lens` the assignment's `mechanical lenses:` line names; the conditions (a predicate is false and a grep shows it) are unchanged. Say that the assignment carries both lines. Edit nothing inside the `review-v2-store-writer` shared block.
- **Acceptance**: forward: `grep -c 'mechanical lenses:' claude/agents/review-checker.md` prints at least `1` (today: `0`).

### 43. Share one mechanical-lens helper between `review add` and `review verdict` [S]
- **Files**: `tomlctl/src/review/catalogue.rs`, `tomlctl/src/review/add.rs`, `tomlctl/src/review/verdict.rs`, `tomlctl/src/review/cover.rs`
- **Depends on**: 7, 8, 13, 41
- **Action**: Replace the private copies of the mechanical-lens test — `swept_lenses` in `tomlctl/src/review/add.rs`, `mechanical_lenses` in `tomlctl/src/review/verdict.rs`, and the equivalent check `review cover` makes for an out-of-slice rule-out in `tomlctl/src/review/cover.rs` — with one `pub(crate)` helper in `tomlctl/src/review/catalogue.rs` that all three call, with no behaviour change. The helper returns the ids of the lenses the merged catalogue marks `mode = "mechanical"` that list the given family and are not disabled; where a caller today applies a narrower or wider filter than that, keep its behaviour and say so in the report rather than changing it. Keep each caller's error handling for an unreadable catalogue, and when it loads the catalogue, as they are today. Add no test; the existing `review_add`, `review_verdict` and `review_cover` suites are the guard.
- **Acceptance**:
  - forward: `grep -c 'fn swept_lenses' tomlctl/src/review/add.rs` prints `0` and `grep -c 'fn mechanical_lenses' tomlctl/src/review/verdict.rs` prints `0` (today: `1` each).
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_add) | binary(review_verdict) | binary(review_cover)'` passes.

### 44. Use the catalogue's root-overlap rule in `review run show` [S]
- **Files**: `tomlctl/src/review/catalogue.rs`, `tomlctl/src/review/run_show.rs`
- **Depends on**: 19, 43
- **Action**: `tomlctl/src/review/run_show.rs` carries its own copy of the path-overlap rule `tomlctl/src/review/catalogue.rs` keeps private (`meets` / `within`: a signal root against a slice or scope entry). Widen the catalogue's functions to `pub(super)`, call them from `run_show.rs`, and delete the copy, with no behaviour change. If the two copies differ in any case, keep `review run show`'s current output and report the difference rather than changing either. Add no test; the existing `review_run_show` and `review_catalogue` suites are the guard.
- **Acceptance**:
  - forward: `grep -c 'fn within' tomlctl/src/review/run_show.rs` prints `0` (today: `1`).
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(review_run_show) | binary(review_catalogue)'` passes.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 1, 2, 3, 4, 5, 6, 7, 8, 9, 40, 41 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 40, 41. Foundations (type and module splits, new fields) and the repairs that need nothing else; builds and passes the suite alone

— CHECKPOINT B after tasks 21, 44 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 41, 43, 44. Every CLI feature, the 0.22.0 bump and its reference docs; the binary the prose is written against

— CHECKPOINT C after tasks 23 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23. Both Workflow scripts on the new index

— CHECKPOINT D after tasks 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 42 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 41, 42. Agent and skill prose, gated by the dispatch lint suite against the new CLI

## Risks

- **Size.** 42 tasks over about 80 files, against the ~35 estimated when one plan was chosen. Mitigation: four checkpoints, each a buildable increment; checkpoint A and the CLI half of B can ship and be used without C and D; `/review-plan` before `/implement`. If the plan is to be cut, the natural seam is after checkpoint B, with tasks 22–39 and 42 as a second plan.
- **`flow resolve` is now strict.** Any script or carrier that passed `--flow` speculatively starts failing. Mitigation: swept at plan review — every `--flow` reaches `flow resolve` through `flow-bootstrap`, which maps a non-zero resolve to `ok: false`, except `claude/commands/tdd.md`'s parent re-resolve, which task 4 edits.
- **Task 40 commits another flow's ledger.** `.claude/flows/jiggly-exploring-robin/review-ledger.toml` is already modified in the working tree by run U1's export. Mitigation: the task's Detail says its commit carries those changes; nothing else in this plan touches that flow.
- **The strict export fold mints more v1 rows.** A true duplicate nobody linked now exports twice. Mitigation: `duplicate_candidates` in the digest and the stage-5 linking step come in the same plan; a duplicate row costs one `wontfix`, a wrong fold loses a defect.
- **Out-of-scope ripple anchors assume `review add` has no scope check** (verified by reading `field_refusals`, not by running). Mitigation: task 7's `add_accepts_ripple_finding_outside_run_scope` is stop-and-report if it fails on unchanged code.
- **`resolve --same-as` over verified rows** extends a pass written for unverified ones. Mitigation: task 18 is deep-tier with three tests, including the cycle refusal; the existing `review_resolve` suite is its guard.
- **Shared-block drift.** `review-finder-body` must stay byte-identical across two files. Mitigation: task 33 owns both; `bash scripts/verify-shared-blocks.sh` is its guard and the pre-commit hook's.
- **Reference line ceilings.** `claude/skills/tomlctl/references/review.md` sits at 583 of 600 lines. Mitigation: task 21 moves the `review run` section out before adding anything.
- **Workflow result truncation.** A wave with many long escalations can pass 8,000 characters. Mitigation: 300-character cut per entry, and the engine reads the output file on truncation.
- **Parallel prose tasks quoting flags.** A prose task citing a flag that does not exist fails `command_lint` for every sibling. Mitigation: all prose tasks depend on 21, so the CLI is complete first.
- **Installed binary lag.** Carriers shell out to the installed `tomlctl`. Mitigation: the orchestrator installs 0.22.0 (`cargo install --path tomlctl`, then `--path glimpse`, through the `verification` agent) once checkpoint B's gate passes, before task 24 edits the first prose file — prose under `claude/` is live through the config dir's junctions and symlinks as soon as it is edited, not when committed; the Phase 3 `install:` keys reinstall the final build.

## After Merge

- **restart**: Restart any Claude Code session that will run `/review-v2`. **When:** after the `install:` commands have run. **Done when:** a new session's `review-finder-cf` agent starts without `research-methods` preloaded.

## Exploration Notes

Shorthand: ENG = `claude/skills/review-v2-engine/`, METH = `claude/skills/review-v2-method/`, LENS = `claude/skills/review-v2-lenses/`, AG = `claude/agents/`, REV = `tomlctl/src/review/`.

### tomlctl review module (REV)

- Layout: `store.rs` (1519; `Store`, `Run` :95, `Signal`, `Cell` :133, `Finding`, `Verdict`, locked `load`/`mutate`), `fields.rs` (1692; `FIELDS`, `.cap(n)`, role visibility :431-454, `validate_row`, `project`, `path_problem` :954, `cap(record,name)` :695), `cells.rs` (1383; `plan`/`propose` :727, `cover` :1294, `newest_wave` :1150), `runs.rs` (787; start :31, update :369, finish :597, list :670, `UPDATE_FIELDS` :26), `survey.rs` (915; `commits_per_file` :797), `add.rs` (1060; `matching::candidates` call :774), `verdict.rs` (356; `apply_one` :164), `resolve.rs` (342), `query.rs` (480; list :48, `assignment` :234), `digest.rs` (736; `run_line` :463), `triage.rs` (721; dispositions :55-91, `judge` :262), `stats.rs` (1362; `overturn` :292, `verifier_call` :275, tally :359, rate :390, `by_model` :461, `by_run` :595), `export.rs` (897; `decide` :733, `place` :610, `with_v2_ref` :793), `sync.rs` (473), `matching.rs` (160), `catalogue.rs` (3121; project check executor hardcoded `Verification` :1973; `review catalogue` prints neither executor nor command :790-810), `schema_out.rs` (435; `cell_return` :230, `verdict_return` :275).
- Clap: `tomlctl/src/cli/types/review.rs` (`ReviewOp` :15, `ReviewRunOp` :81). Registration: `tomlctl/src/capabilities.rs` `MUTEX_GROUPS` :16 (review verbs :21-39), `FEATURES` in `tomlctl/src/cli/types/mod.rs`, hand-kept verb lists in `tomlctl/tests/capabilities.rs` (:26 read, :120 write).
- **flow resolve** (`tomlctl/src/flow/resolve.rs`): `resolve` :136, step 1 :148-176, fall-through warning :173, hard error only under `--strict-read` :163. Tests `tomlctl/tests/flow_resolve.rs`: `explicit_flag_missing_falls_through_to_none_by_default` :206 pins the warning, `explicit_flag_missing_strict_read_errors_with_not_found` :225. `AG/flow-bootstrap.md` :68 sets `ok:false` only on non-zero exit; :90 warnings never flip `ok`.
- **propose** (`cells.rs`): `depth_ceiling` :67 (6/16/32), budget :747-753. Overshoot causes: floor lenses admitted regardless of budget :811; `add_check_cells` :928 unbudgeted (:777, :819); `stopped` marks every later lens "ceiling" once one misses, even ones that fit an existing cell (:792-795, :814-815). `slices_for` :457 splits per subsystem/dir and one slice per in-scope hotspot; `bundle` :535 merges only identical slices. Hotspots: `Signals::of` :235 top decile of `churn-hotspot` evidence from a repo-wide `git log` ignoring scope; summary emits unfiltered list :896. Tests `tomlctl/tests/review_cells.rs` :474, :524, :677, :714.
- **resolve / export-v1 / sync-v1**: `resolve.rs` only acts on rows that already carry `same_as` (:141-152); no duplicate discovery. `export.rs` `decide` matches a v1 row on `v2_ref`, else same `file` + (same non-empty `symbol` OR exact `summary`) :765-767, reading the doc as it is written, so two findings in one export fold; `Reuse` branch :624-633 keeps the first's text and appends a `v2_ref`. `sync.rs` `run` :226 applies a row's status to every finding its `v2_ref` names. Tests: `review_export.rs` `reuse_keeps_both_v2_refs` :752, :571, :598; `review_sync.rs` :421; `review_resolve.rs` :212-488.
- **Runs**: signals at `Run.signals`; baselines, audit, counts in `run.rest`. Signals are read back only by `propose`; baselines only by `review show <cell> --for finder`; `run list` shows neither. Scope goes in as `--set-json scope=[…]` or `scope=@file` (`src/fields.rs` `parse_set_json` :100; no stdin). `waves` is `.computed()` (`fields.rs` :483) but never written → `stats by_run` null; each cell stores `wave`.
- **Stats**: `audited`/`overturned` counted only from `by=="audit"` verdicts; `run.rest["audit"]` is read only by `digest.rs` `run_line`. Tests `stats_refute_and_audit_overturn_rates_per_lens` :446, `overturns_group_by_model` :909.
- **600 cap**: `.cap(600)` on cell `note` (`fields.rs` :510), `failure_scenario` :544, verdict `rationale` :681. `cover` refuses at `cells.rs` :1314-1319; verdict via `validate_row`→`check_text` :923 (single and `--ndjson`; batch lists the row under `refused`). No test pins note/rationale caps.
- **Digest**: `contested` = rows with `outcome == "contested"`, set only by `review resolve`. `Cell` has no escalate field.
- **Triage**: dismiss, defer, reopen, park, resolve, backlog-ref, convert. No "plan" disposition; park is advisory-only. `review list --status parked` already spans runs (`--run` optional); no `fix_shape` filter.
- Doc gates are one-directional: `command_lint` (`cli/dispatch/tests/lint.rs` :433) and `flag_table_lint` :2039 only require that documented flags exist. Reference: `claude/skills/tomlctl/references/review.md` (cap text :107, :276, :317; scope :161).

### Workflow (`claude/workflows/review-v2.js`, 190 lines)

- Args :27-41; `index(cells, verified)` :28 returns `{run, wave, cells:[{id,status,findings,escalated}], verified, errors, script_version}` — `escalated` is a count, entries only logged (:155-156); no agent count.
- `routeCell` :150-181: `verifier` route → `minors` (minor defect, depth ≠ deep → `verifyBatch` :106, one batch per cell) or `singles` (:169; every verifier-routed cleanup gets its own `verifyOne` :96). Checker pool `poolChecker` :135, `CHECKER_BATCH=20`.
- `claude/workflows/plan-v2-review.js` :279-298 calls the pipeline, reads `errors`, `cells`, `verified`, copies `cells` through; index documented at ENG `references/plan-flow.md:113`.
- Gates: `workflow_scripts_name_real_agents` (lint.rs :1091), `command_lint` parses backtick `tomlctl` spans in JS. No JS-level tests exist.
- Schemas from `tomlctl review schema cell-return|verdict-return`; `cell_return.escalate?: string[]`.

### Harness prose

- **Scope rule**: ENG `SKILL.md:92-93` (HEAD~1 on default branch; `--base` only bound). Restated: ENG `references/survey.md:23`, `AG/review-finder.md:30`, `-cf.md:31`, `claude/commands/review-v2.md:50`, `optimise-v2.md:35-44`. Task rows carry `commit`.
- **Sweeper thin**: `AG/review-sweeper.md:38`, step 6 :29; METH `SKILL.md:153-163`; LENS `methods/sweep.md:20,28`.
- **Surveyor** (`AG/review-surveyor.md`; haiku, medium, tools without Edit): :26 reads each check file's frontmatter `executor` — a project check's path is `config.toml` with no frontmatter, hence "no executor"; :27 evidence "inside the scope" is not stated as a hard rule; :34-35 Write; caps :34, :40. Rule broken: ENG `references/survey.md:90`.
- **Signals copy**: `survey.md:28` "only copy of the signals"; restated `agent-waves.md:99,113`, `survey.md:32`. **Baselines** one dispatch per command: `survey.md:8,51,66,108,118,133-146`, ENG `SKILL.md:105`, `compose.md:90`, `commands/review-v2.md:40`, `optimise-v2.md:50`. `AG/verification.md:21` `keep_going`.
- **Escalations**: ENG `SKILL.md:149,171`, `compose.md:148,152`, `agent-waves.md:134,141`, `plan-flow.md:330`; METH `SKILL.md:224-225`.
- **Finder out-of-lens cleanup**: `AG/review-finder.md:36,54` (same in `-cf.md`, inside shared block `review-finder-body`), METH `SKILL.md:41`, `compose.md:58`, LENS `lenses/ripple.md`, `hygiene.md`.
- **600 cap prose**: METH `SKILL.md:153,159,161,175`, METH `references/finding-kinds.md:85`, `verifying.md:5`, `AG/review-finder.md:48`, `-cf.md:49`, `review-sweeper.md:38`, `plan-executor.md:42`, `review-verifier.md:42`.
- **finder-cf** frontmatter :1-12: skills `review-v2-method`, `research-methods`; `omitClaudeMd: true`; body shared via `review-finder-body` (`scripts/shared-blocks.toml:172-177`); no `.claude/flows` rule.
- **Verifier/audit**: `AG/review-verifier.md:28` reproduce; ENG `references/triage-and-close.md:19` (two REFUTED), :38-44 (upheld row gets no verdict), :91 park, :94 `plan F5` no store write, :152 close summary; `measurement.md:123-127` bands, :154 stale line, :182; METH `references/plan-records.md:49` stale `command` (real keys: `reason, action, when, done_when, source_task, done`).
- **Bookkeeping**: agents count ENG `SKILL.md:153-157`, `agent-waves.md:145-149`, `plan-flow.md:155`; scope inline `SKILL.md:98`.
- Stage 0 (ENG `SKILL.md:69-86`) lists nothing from earlier runs.
- Shared blocks touching review agents: `research-delivering`, `review-v2-store-writer`, `review-finder-body`.
- Project lens config `.claude/review-lenses/config.toml`: one `[[checks]] id="harness-gates"`.

### Verification commands

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`; `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`; `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`; `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items`; `cargo clippy --manifest-path glimpse/Cargo.toml --locked`; `bash scripts/verify-shared-blocks.sh`; hook subset `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`.
- Version: `tomlctl` 0.21.0 installed and in `Cargo.toml`. Convention: minor bump for new verbs/flags touches `tomlctl/Cargo.toml`, both lock files, `tomlctl/README.md`, `capabilities.rs`, `FEATURES`, `tests/capabilities.rs`; then `cargo install --path tomlctl` and `--path glimpse`.
- Agents, skills and Workflows under `~/.claude` are live links into the repo; only a new file needs `scripts/link-v2-review.ps1`.

**Backlog**

- Seed (`seed_ids`, promoted to `review-v2-first-run-repairs`): B-80c20e25, B-827b9763, B-d9cfd36e, B-39b9e713, B-5d5bf09e, B-bc4a0080, B-99efd715, B-989439ec, B-0bdfc2dc, B-fc4d7f8b, B-1e1be4cf, B-dc721e5c, B-30ba0a0a, B-00ae0571, B-e684aed8, B-ab53208e, B-75341a1d, B-ce9ebb4d, B-27f5ba35.
- Open under touched directories (fold-in candidates): B-e922f5cb (catalogue validator skips facet For-lens / experimental-rework checks), B-466a423b (`review apply` symlink re-check under lock), B-d1d11150 (`review apply` writes the applying mark for a stale unmarked prose row), B-55a13775 (question: prose finding guidance on `expect` recovery), B-edb20f98 (suite wall time bounded by process starts).
- Related rows are all resolved or dismissed (context only).

**Early scope check**: ~35-40 unique files — over the ~25 guide; the user chose one plan for all 19 in Phase 1.

## Research Notes

### Workflow return and agent count (Claude Code 2.1.322)

- A Workflow's return value may be any JSON-serialisable value; the result the calling conversation sees is cut at 8,000 characters, with the full text in the task output file the notification names. Arrays crossing the script VM boundary cap at 4,096 items. Source: installed `claude.exe` bundle (undocumented at code.claude.com/docs/en/workflows, fetched 2026-11-10). **Impact**: per-cell `escalate` arrays fit one wave; cap each entry's length so the index stays under 8,000 characters, and have the engine fall back to the output file when the notification says it was truncated.
- The script has no dispatched-agent counter (hooks: `agent`, `parallel`, `pipeline`, `phase`, `log`, `args`, `budget`, `workflow`); the completion notification carries `<usage><agent_count>` outside `<result>`, and a nested `workflow()` shares the parent's counter. **Impact**: `claude/workflows/review-v2.js` counts its own `agent()` calls and returns the count; `claude/workflows/plan-v2-review.js` sums its children's.
- A nested `workflow()` returns the child's value as a full JSON-safe copy, uncut; a non-serialisable value throws in the parent (already wrapped at `claude/workflows/plan-v2-review.js:279`).
- Searched: `workflow-authoring` skill; code.claude.com/docs/en/workflows and /agent-sdk/typescript (2026-11-10); 2.1.322 binary.

### Subagent frontmatter

- `skills:` injects each listed skill's full `SKILL.md` at startup and does not limit which skills the agent may invoke (code.claude.com/docs/en/sub-agents, fetched 2026-11-10). `claude/agents/review-finder-cf.md:9-11` preloads `review-v2-method` and `research-methods`; the sentence describing the `research-methods` preload sits inside the shared block `review-finder-body` (`:20`), so removing the preload from one carrier needs the shared sentence reworded to hold for both.
- `omitClaudeMd` omits user, project and local CLAUDE.md only: path-scoped rules (`.claude/rules/documentation.md` matches `**/*.rs`, `**/*.md` and more) and a subdirectory's CLAUDE.md still load when the agent reads files there. **Impact**: the cf description's "without CLAUDE.md, rules or memory" overstates; correct it.
- No per-agent path restriction exists in frontmatter (`disallowedTools` removes whole tools; `permissions.deny` is session-wide). A per-agent `hooks` PreToolUse entry could enforce one but is undocumented for path checks. **Impact**: "do not read `.claude/flows/` or `docs/flows/`" stays a prose rule in this plan.
- Searched: code.claude.com/docs/en/sub-agents (2026-11-10; last ~17,000 characters unread).

### Verification agent

- One dispatch with `keep_going: true` returns one block per attempted command, with a per-command `timeout` (default 540 s), commands in series (`claude/agents/verification.md:21,117,144`). **Impact**: all baseline commands ride one dispatch.

### Finding identity (prior art)

- SARIF 2.1.0 §3.27.16-17: a fingerprint is the same for logically identical results and different for logically distinct ones; it should survive line moves; the algorithm is left to the consumer (docs.oasis-open.org/sarif/sarif/v2.1.0/errata01, fetched 2026-11-10; Appendix B unread).
- GitHub code scanning: `primaryLocationLineHash` hashes the first 100 non-space characters from the flagged line plus an occurrence counter; rule id is matched separately (github/codeql-action `src/fingerprints.ts`, vetted 2026-11-10).
- SonarQube `Tracker.java`: five of six matching passes key on the rule; the sixth is a block-move detector (_orchestrator-downgrade: the lite pass said "every pass"_). DefectDojo's hash keys all include a rule id, title or CWE.
- **Impact**: a safe automatic fold needs same file + same anchor + same defect class. Same file and symbol with a different class or summary is a *candidate* duplicate needing a judgement, never a fold. That covers both U1 failures (F4 folded into F1's row; three true duplicates missed).
- Searched: SARIF errata01; codeql-action; GitHub SARIF docs; Sonar Tracker.java; DefectDojo settings.dist.py; CodeClimate SPEC.md (all 2026-11-10).

### Verifier leniency (lite pass, then `research-deep`)

- LLM judges of code feedback show true-positive rate ~96% and true-negative rate <25% (arXiv 2510.11822, vetted 2026-11-10; _orchestrator-downgrade: the per-model "14 models" attribution is unconfirmed_). A 30/30 confirm rate is what that bias predicts; only planted negatives measure it. Seeds must resemble real findings and never repeat (Mills 1972 via secondary sources; Checco et al., IJCAI 2019).
- 0 overturns of 4 is consistent with a true rate up to 53% (`1-0.05**(1/n)`); about 30 audited rows with none are needed to claim under 10%.
- **The CLI already counts an upheld audit verdict.** `tomlctl/src/review/stats.rs` `overturn` (:292-305, vetted) counts any non-`unsettled` `by=audit` verdict as audited and an overturn only when its polarity differs from the verifier's. The gap is `claude/skills/review-v2-engine/references/triage-and-close.md:44` ("A row you agree with gets no verdict", vetted) and the same instruction in `claude/agents/plan-review-lead.md:104-110`. **Impact**: B-fc4d7f8b is a prose fix — record every sampled row as a `--by audit` verdict (`confirmed` or `refuted` only; a `plausible` audit verdict would flip cleanup export eligibility at `tomlctl/src/review/export.rs:258`). A `stats` fallback to `run.audit.sampled` would double-count afterwards and is not built.
- `tomlctl/src/review/digest.rs` `audit_sample` (:216-283, vetted) picks two `confirmed` and two `refuted` non-critical defects; with no refuted rows the sample shrinks, and a `plausible` defect is never sampled. **Impact**: fill empty refuted slots with extra standing defects and widen the confirmed group to confirmed-or-plausible (one function, one test, `triage-and-close.md:19`).
- `claude/skills/review-v2-engine/references/measurement.md:115` scores refute rate one-sidedly (0% reads "better"); :109 and :123-127 give the audit bands no minimum sample. **Impact**: add a low-side `suspect` reading and a 30-row pooled floor (prose).
- In-run planted negatives need a hidden `seeded` field excluded from export, digest, report, finish counts and stats in step — a subsystem. An offline seeded-precision probe in a scratch store uses existing verbs only (`review run start --scope`, `review plan --ndjson`, `review add`, one verifier dispatch), seeded from v1 `verified-clean` items; evidence grade medium (sequence not run end to end).
- Not built: requiring reproduction evidence on confirms (the citation check is form-only at `tomlctl/src/review/fields.rs:928-937`, so the rule is gameable). Watch item: share of verifier confirms citing only the finding's own anchor (11 of 22 in U1).
- Searched: arXiv 2510.11822; IJCAI 2019/850; Wikipedia "Bebugging" (2026-11-10); tree reads for the rest.
