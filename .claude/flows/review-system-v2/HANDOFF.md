# Handoff: review-system-v2 `/implement` run, paused 2026-10-12

## Where it stands

- 51 of 62 tasks are `done` in `tasks.toml`; 11 are `pending`; none are in flight (`[tasks].in_progress = 0`).
- **Nothing is committed.** Every completed task's work is uncommitted in the working tree. The tree also holds the pre-run uncommitted planning edits (`docs/plans/review-system-v2.md`, the flow's TOML files, `.claude/flows/review-system-v2/archive/`). Never `git add -A`; the commit train stages per-group files only.
- Four tasks were minted mid-run (each recorded as a `deviation` in the execution record). The plan has been re-rendered and re-imported, so `tasks check` is clean and `tasks render --check` shows no drift:
  - 59: workflow batch prompts. `batch:` and `sweeper rows:` lines, and `{verdicts:[…]}` batch returns.
  - 60: `bind_cell` accepts the 3-check `plan-checks` cell.
  - 61: consolidate duplicated helpers (pending).
  - 62: panel tally counts each seat's latest vote once.
- `execution-record.toml` holds every completion and 63 deviations; `PROGRESS-LOG.md` is regenerated.

## Resume

Run `/implement` and pick `review-system-v2` when asked to choose between fresh flows. Step 0.5's `import-plan --reconcile-record` is idempotent here, and the skip-list comes from the store.

Remaining DAG, in this order:
1. **Ready now:** 61 (helper consolidation) and 21 (tomlctl skill `review.md` docs).
2. **Then:** 20 (output-option cases and help entries). It is coupled behind 61.
3. **Then:** 22 (bump to 0.16.0 and refresh `glimpse/Cargo.lock`). This completes **checkpoint A**.
4. **Checkpoint A** (members 1–22 and 60–62):
   - Run `tasks closure --checkpoint A`, then drain.
   - Dispatch the verification gate: build plus `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`, then glimpse `cargo clippy --locked`.
   - Run the commit train **unnarrowed** (`tomlctl tasks train --slug review-system-v2 --lines`). It will also carry the checkpoint-B rows that are already done.
5. **Then:** 46 (catalogue gate and lint scan set), 57 (pre-commit trigger), 58 (vocabulary fixture tests). This completes **checkpoint B**.
6. **Then:** 53 `/review-v2` (C); 54, 55, 56 (D); Phase 3; Phase 4.

**Before the first commit:** the crate-wide `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` currently fails, at least on `tomlctl/src/review/digest.rs`. The pre-commit hook blocks on it, so run `cargo fmt` as part of checkpoint A's fix cycle.

## Dispatch notes for the remaining tasks

This is orchestrator context that no store field carries.

- **61:** its Action has been widened to also fold together:
  - `query.rs` `View` and `digest.rs` `Rows`;
  - `stats.rs`'s copy of `cells.rs` `PathSet`/`within`;
  - the counts rules duplicated between `runs.rs` `counts` and `digest.rs` `counts_line`/`report_rows`/`live`.

  The catalogue-rebuild helper is copied in survey, cells, add, triage, query and export, and possibly others. Find them with `grep -n 'fn run_catalogue' tomlctl/src/review/*.rs`, plus survey's own differently named copy.
- **21:** document the verbs as built, not just as planned. Differences from the plan:
  - `cover --note-file`, `verdict --rationale-file` and `run finish --incomplete-file` twins exist.
  - `verdict --citation` is optional at parse time and refused in validation.
  - On `plan`, `--accept-signals`, `--lens` and `--out` conflict with `--ndjson`.
  - `review list --for` defaults to `orchestrator`, and `list` hides `superseded` rows unless asked.
  - `export-v1 --all` scopes by store and ledger family, because there is no `--run` flag.
  - `review report` prints `{path, counts}`.
  - The proposal's last NDJSON line is a `{"proposal":{…}}` summary.
  - Bash fences must follow command_lint: no spaced `<run directory>` placeholders, and no `a|b` alternation inside values.
- **20:** `every_leaf_command_has_a_case` in the `output_options` binary is red until this task lands. That is expected.
- **46:** the shipped catalogue already validates (`tomlctl/target/debug/tomlctl.exe review catalogue --builtin claude/skills/review-v2-lenses --project .claude/review-lenses --check` gives `ok`).
- **53–55 (carriers):**
  - The context-free correctness cell has no marker field, so the carrier prompt must withhold `pack.md` itself.
  - Keep the `plan-checks` cell exactly as `--propose` emits it.
  - Verification-check baselines can resolve several commands, keyed `<id>`, `<id>-2` and so on (see `checks/lint-baseline.md`).
- **Shared preamble used for every dispatch:** point at the plan's narrative by line range and do not paste it. Use `docs/plans/review-system-v2.md` lines 1–533 and the Risks/After Merge range. The plan was re-rendered, so check the current line numbers of `## Tasks` and `## Risks` with `grep -n '^## ' docs/plans/review-system-v2.md`. Constraints:
  - v1 files are untouchable.
  - Only the task's own Files may be touched.
  - Agents use read-only task verbs.
  - Narrow builds only.
  - No git stash, reset, checkout or restore.
  - Falsifier check on every new or changed assertion.
  - `.claude/rules/documentation.md` applies.
  - `DEV SERVER: none`.

## Phase-4 harvest candidates (agent TANGENTIALs not yet acted on)

- `fields.rs` lacks a run `incomplete` field, which `run finish` writes into `rest`.
- `sync.rs`'s disposition-companion list omits `reopen_reason`.
- `review plan --ndjson` is not idempotent per wave: a retry after `os error 5` duplicates cells.
- v1 `v2_ref` is a single string, so a second export matching an already-linked open v1 row overwrites the link. This is a design question.
- Per-agent-model audit overturn rates in `measurement.md` have no producing command.
- `survey.md` §6 and `review-surveyor.md` say "one command per verification check". Align them with the `<id>-<n>` keying.
- The agent bodies (`review-*.md`) use the `--note - < file` stdin idiom, while `review-v2-method` prefers the `--*-file` twins. Both work.
- The finder body asks for more in the coverage note than `cell.note` (≤600 characters) allows.
- The vocabulary has no concern tag for supply-chain execution risk (install or build scripts).
- Candidate lenses test-gaps, seams, idioms, architecture and duplication have no `apply_when.signals`, so a skipped one needs no waiver.
- The catalogue validator checks neither a facet's `### For <lens>` sections against `attaches_to` nor the experimental-rework ownership rule. `owned_tags` reads the first backtick span of every Owns paragraph.
- The `backlog-cluster` detector counts literal `status = "open"` lines.

## Orchestrator deviations to report in Phase 4

- Dispatch prompts referenced the plan's narrative sections by line range instead of embedding them verbatim, to avoid pasting about 35k tokens into each of 60+ prompts.
- The orchestrator made small inline fixes rather than dispatching:
  - a stale doc comment in `review/catalogue_cmd.rs`;
  - the checker-return sentence in `agent-waves.md`;
  - spaced placeholders and convert-payload fields in `triage-and-close.md`;
  - STORE CHECK ownership and foreign-format wording in `lenses/plan-structure.md`.
- One `cargo nextest … -E 'test(/command_lint|guidance_lint/)'` ran in the main conversation. That breaks the inline-build rule.
