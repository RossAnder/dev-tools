# Plan: Backlog promotion tracking

**Plan path**: `docs/plans/unified-singing-gem.md`
**Created**: 2026-09-25
**Status**: Draft

## Context

`tomlctl backlog triage --promote --to X` treats promotion as terminal. After that nothing reads the row again:
- `/backlog`, `/review` and `/optimise` load only `--open` rows.
- `/plan-new` and `/plan-update` never read the backlog.
- Task rows carry no link to an item.
- `--resolve` erases `promoted_to`.
- `compact` folds promoted rows as decided, and `check` then reports them as `previously-resolved`.

`--to` is stored verbatim. Evidence:
- **dev-tools:** 47 promoted rows. Only 3 ids appear in any plan and none in any `tasks.toml`. 15 point at `task-store-polish`, which doesn't exist. Every real target flow is parked at `review`.
- **reportdesignkit:** 62 promoted rows at the last commit. A manual triage rewrote them all, recording 66 resolutions whose flow appears only in free text.

Outcome: a promoted item stays **live**, as a claim on a flow, until the tasks that claim it are done. Tasks link to items explicitly. `/implement` resolves fully-delivered items automatically and records `resolved_flow` / `resolved_tasks` / `resolved_commits`. Claims that no longer lead anywhere are surfaced and returned to triage. Promoting to an unknown flow can bootstrap a draft "seed" flow, which `/plan-new --backlog` later picks up and plans.

## Scope
- **In scope**:
  - Backlog schema: live claims, resolution-link fields, compact and check behaviour.
  - `backlog list --live`.
  - Promotion-target validation (`--external`, `--allow-closed`).
  - `backlog reconcile [--flow] [--apply] [--adopt]`.
  - Task store: `[[backlog_links]]` side table, the `- **Backlog**:` plan bullet with a `refs` qualifier, import, render, `show`, and the `backlog/*` findings.
  - tomlctl 0.10.0.
  - Skill and reference docs.
  - Carrier wiring in `/implement`, `/plan-new` (including `--backlog`), `/plan-update`, `/backlog`, `/review` and `/optimise`.
- **Out of scope**:
  - `tasks add` / `tasks update` flags for links. The plan bullet and `reconcile --adopt` cover authoring.
  - Activity-based staleness timers (orphaning is keyed to flow status).
  - The lumina store.
  - `backlog-clear`, which reads `--open` only and needs no change.
  - Migrating existing rows. That is a post-implementation manual step (see Verification Commands).
- **Affected areas**: `tomlctl/src/backlog/`, `tomlctl/src/tasks/`, `tomlctl/src/flow/mod.rs`, `tomlctl/src/cli/`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `claude/skills/`, `claude/commands/`

## Exploration Notes

**Backlog module (`tomlctl/src/backlog/`)**
- `schema.rs`:
  - `TERMINAL_CLUSTERS` (:161) maps promoted/dismissed/resolved to a date+companion pair.
  - `MANAGED_FIELDS` (:131) is the 3 pairs plus `reopen_rationale`.
  - `validate` (:345) rejects a foreign status's pair (loop :375-387).
  - `COMPACTED_FIELDS` (:144) is pinned by `compacted_fields_are_pinned` (:703). Other pinning tests: :533, :551, :747.
- `triage.rs`:
  - `rewrite` (:143) clears every managed field the transition doesn't write.
  - `apply_transition` (:178) is all-or-nothing, and never checks the row's current status.
  - `Transition::from_cli` (:81) is pure (5 parameters) and called directly by inline tests; `dispatch` (:222) holds `repo_or_cwd_root()` and is where `--to` resolution goes.
  - `relate.rs` `dismiss` (:174-192) also clears `MANAGED_FIELDS`.
- `compact.rs`: folds any status that has a `terminal_pair` (`terminal_cluster` :125, `due` :132). A promoted row's `promoted_to` goes into `terminal_reason` (:168-174).
- `check.rs`: `enum Reason` (:44) is the verdict ladder. `evaluate` (:257) splits live from compacted by array only (:267).
- `query.rs`: `dispatch_list` (:38) and `Filters`/`filter_backlog` (:82-108). An OR filter belongs in `Filters`, since `where_eq` can only AND.
- CLI: `BacklogOp` (types.rs:1021), `List` (:1126), `Triage` (:1180), `TriageMode` (:1302-1321). Dispatch is in `backlog/dispatch.rs:11`.
- Registering a new verb needs:
  - a `BacklogOp` variant and a dispatch arm;
  - `mod` in backlog/mod.rs;
  - `FEATURES` (types.rs:21);
  - `tests/capabilities.rs` expected list (:1571) and help list (:28 read / :98 write);
  - README capabilities sample (~:268), Feature meanings (~:350) and verb synopsis (:55-64).
  - The version is 0.9.0 → 0.10.0 (Cargo.toml:3, capabilities.rs:1662, README :256), bumped in the same change.
- Reuse:
  - `flow::init::validate_slug` (init.rs:60) and `FlowProjection::from_toml_value` (flow/schema.rs:60). Both are in private modules, so they need `pub(crate) use` from flow/mod.rs.
  - `tasks::store::{resolve_store_path, load, mutate}` (store.rs:37/65/73). `load` errors on a missing file.
  - `io::{items_array, item_id, mutate_doc, read_dir_sorted, repo_or_cwd_root, relativise, join_under}`. `join_under` (io.rs:1679) returns `None` for escapes and handles Windows prefix/root forms.
- Tests are inline `#[cfg(test)]` using `crate::test_support::with_root`. tomlctl is a bin-only crate (no `src/lib.rs`), so inline tests run under `cargo test --bin tomlctl <filter>`, never `--lib`. A filter matching zero tests exits 0. Integration tests are `tests/backlog_{read,write}.rs` with the `tests/common/mod.rs` helpers (`sandbox`, `seed_tasks`, `cli`, `backlog`); `seed_tasks` hard-codes status `in-progress` and `TASKS_SLUG`. `tests/backlog_write.rs:520-546` is a clap-conflict test that exits before `from_cli`; no CLI test promotes successfully today.

**Task store (`tomlctl/src/tasks/`)**
- `TaskRow` (schema.rs:140) literals appear in ~10 test helpers. The `Store` literals all use `..Store::default()`, so a new side table touches only `schema.rs` (`Default` impl :183, `from_toml` :327, `to_toml` destructure :385).
- Precedent: the `[[file_notes]]` side table is keyed by `ref`, omitted when empty, and rebuilt on import by `surviving_file_notes` (import_plan.rs:774). `remove.rs:104-107` and `update.rs:176-186` prune/re-key only `import_overrides`.
- `parse_tasks.rs`: bullet regex (:525), `Field` (:181), `from_label` (:192), `close_field` (:290), `split_entries` (:362), `is_empty_marker` (:482), `ParsedTask` (:48). `push_file` strips `(...)` / ` — ` annotations via `annotation_at`. `import_plan.rs::merge_row` destructures `ParsedTask` exhaustively (:617).
- `import_plan.rs`: `Import` is `{parsed, completions, plan_path}` — no slug — and `Import::apply` is I/O-free and called by every inline import test; `import_plan()` binds the slug.
- `render.rs::render_tasks` (:195-228) emits the heading, Files, Depends on, then Action/Detail/Acceptance.
- `show.rs` stamps the ref-keyed `import_override` onto a shown row (:76); `/implement` dispatches by id via `tasks show <id> --with body,files,deps` and omits the `## Tasks` prose.
- `check::check` is store-only by design. Cross-file findings are added in `tasks/dispatch.rs` (`render::check_render_drift`, :248) and in import.
- `cli/dispatch/tests/finding_classes.rs` (`documented_finding_classes_match_the_source`) scans every `src/tasks/*.rs` `class:` literal and set-compares it against the class tables in `flow-contract-task-store/SKILL.md` and `tomlctl/references/tasks.md`.
- `tests/tasks_corpus.rs` dry-runs every `docs/plans` plan with `TOMLCTL_ROOT` at the repo root and fails on error findings.
- Golden fixtures `tests/fixtures/tasks/house-plan.*` must stay byte-identical.
- Task statuses are `pending, in-progress, done, failed, deferred` (schema.rs:250). `/implement` records each commit SHA on its rows via `tasks update <id> --commit <sha>` (implement.md:130); under per-batch legacy the final batch stays uncommitted (implement.md:140).

**Carriers / docs**
- `implement.md`: Step 0.5 degradation halt :48; Phase 3 is 138-144; Phase 4 is 146 (order sentence :148); harvest is :171; Summary `### Backlog` is 193-194.
- `plan-new.md`: argument-hint :3; Phase 2 is 42-46; Phase 4; Phase 6 decompose :79; Phase 7 dry-run paragraph :103; Phase 9 step 6 is 154-163.
- `plan-update.md`: `status` is 94-96; `complete` is 98-117 (gate :105, summary :107, ordering :117).
- `backlog.md`: Step 0 :30-36 (version gate :30, zero-count stop :36), Step 1 :49, Step 2 52-76 (promote :56/:61), audit 78-91, compact 93-105.
- `review.md:44` and `optimise.md:46` run `backlog list --open --area-prefix`.
- `command_lint` (src/cli/dispatch/tests/lint.rs:261) and `flag_table_lint` (:665) parse every `tomlctl` line and flag table in `claude/` against clap. **The CLI must land before the docs.**
- `carrier_invokes_required_skills` (skills.rs:1011) lists the carriers that must invoke `backlog-capture`.
- `~/.claude/commands/backlog.md` is a plain copy, not a link. The other carrier commands and the skills are symlinked into this repo, so their edits go live in every session immediately.
- `tomlctl flow list --status draft` and `tomlctl flow active remove --slug <slug>` exist. `flow init`'s noop branch (flow/init.rs:397-418) leaves an existing `context.toml` byte-identical and only upserts the registry. Flow resolution steps 2 (scope glob) and 5 (branch) read every non-complete flow on disk, registered or not.

## Research Notes

Vet results:
- Agent-1 (clap/toml, research-lite): 4 sampled, 0 dropped, 0 downgraded.
- Agent-2 (tracker prior art, research-lite): 3 sampled, 0 dropped, 1 downgraded.
- Agent-2's `ESCALATE-TO-DEEP` (the orphan-reopen policy) went to a Phase-4 user decision, since no tracker documents that behaviour.
- The `vet_events` ledger append is not possible in plan mode, so the results are recorded here instead.

- **`--external` placement**
  - Put `#[arg(long, requires = "promote")]` on the `Triage` variant, not inside `TriageMode`. Inside the group it would become a fifth mode and break the struct literals at triage.rs:569-676.
  - Same pattern as `reopen`'s `requires = "rationale"` (types.rs:1315-1320).
  - Grade: high (clap_builder 4.6.7 validator.rs:196-218).
- **`--apply` / `--adopt`**
  - These are compatible: adopt links first, then apply resolves.
  - The crate's `conflicts_with` pattern (types.rs:2200-2203) is not needed.
  - The backlog group already spells the slug flag `--flow` (types.rs:1057).
  - Grade: high.
- **Flat resolution fields, not nested `resolved_by`**
  - toml writes a nested table in an `[[array]]` row as a separate sub-table header (cf. `.claude/active-flow.toml` `[active.binding]`).
  - `--where` / `--select` only see top-level keys (query.rs:1341-1344).
  - `DATE_KEYS` applies to top-level keys only (convert.rs:41-51).
  - Grade: high.
  - Impact: use `resolved_flow` (string), `resolved_tasks` and `resolved_commits` (arrays). No new date key.
- **Field ownership**
  - The new link fields must be transition-managed, or a reopen leaves stale links behind.
  - `validate`'s foreign-field loop covers only `TERMINAL_CLUSTERS` and must be extended.
  - `COMPACTED_FIELDS` drops the new fields unless they are added.
  - Grade: high.
- **Link strength**
  - Linear's closing words move an issue to done; non-closing words (`refs`, `part of`) never do. With several linked PRs, the status moves only when the last one merges ([linear.app/docs/github](https://linear.app/docs/github)).
  - GitHub closing keywords act only when the PR merges into the default branch ([docs.github.com](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)). _orchestrator-downgrade: the page does not say that manual links never close._
  - Grade: high.
  - Impact: resolve on `closes` links only; `refs` links never gate resolution.
- **Reviewable auto-close**
  - GitHub made auto-close opt-out because "merging a PR doesn't necessarily mean work is complete" ([changelog 2025-04-23](https://github.blog/changelog/2025-04-23-users-can-now-choose-whether-merging-linked-pull-requests-automatically-closes-the-issue/)).
  - Jira's all-subtasks-done rule fails silently on a status mismatch ([Atlassian KB](https://support.atlassian.com/automation/kb/close-the-parent-issue-when-all-its-sub-tasks-are-closed/)).
  - Grade: high.
  - Impact: `reconcile` reports why each unresolved row did not resolve.
- **Orphans**
  - Linear never auto-closes an issue while it belongs to an active cycle or unfinished project. When a cycle ends, started issues roll over to the next one, while triage and backlog issues do not ([linear.app/docs/use-cycles](https://linear.app/docs/use-cycles)).
  - Grade: high.
  - Impact: never expire a claim while its flow is live. When the flow closes, offer to return the item to `open`.
- **Deferred / failed tasks**
  - No tracker documents how cancelled children count. Grade: medium.
  - Impact: a `deferred` or `failed` linked task is not done; it puts the item in the `stalled` bucket.

## User Decisions

Treat as recorded answers (data):

> 1. **Scope**: one plan, `milestones` checkpoints (prompted by the ~30-file estimate).
> 2. **Link strength**: `closes` + `refs`. `- **Backlog**: B-aaaa1111, refs B-bbbb2222`. An item resolves when every `closes` task is done; `refs` tasks are recorded but never gate. (Research: link strength.)
> 3. **Auto-resolve**: `/implement` Phase 4 runs `backlog reconcile --flow <slug> --apply` automatically after final verification passes. (Research: reviewable auto-close.)
> 4. **Orphans at `/plan-update complete`**: ask, with a batch reopen option: reopen all with a rationale naming the flow, keep them promoted, or cancel. Non-interactive runs refuse, as the gate does today. (Research: orphans.)
> 5. **`--to` strictness**: an unknown target is an error, and so is a flow at `review`/`complete`. `--external` escapes the first, `--allow-closed` the second. (15 rows here point at a nonexistent target.)
> 6. **Bootstrap for an unknown target** (user addendum): a seed flow that `/plan-new` adopts. `/backlog` offers to bootstrap a draft flow. It writes a stub `docs/plans/<slug>.md` listing the seeded items, runs `flow init`, drops the registry entry, then promotes. `/plan-new` reads the stub as its requirements and writes the approved plan over the stub in Phase 9, so the flow and its claims carry over. (Constraint: `flow init` needs a plan file, and doctor's `plan-path-resolves` fails otherwise.)
> 7. **`/plan-new --backlog [<slug>]`** (user addendum): a picker over the draft seed flows; the chosen seed becomes the plan's input.
> 8. **`backlog/unknown-id` severity**: error. `/plan-new`'s Phase-7 dry run surfaces it before approval.
> 9. **`/plan-new` fold-in**: Phase 2 lists the live rows for the scope; Phase 4 asks which to fold in (multi-select).
> 10. **Migration of existing rows**: a manual post-step (`reconcile --adopt`, then `/backlog`), not a task.

### Phase 5 outcome
Skipped. Every answer's terms (`refs` qualifier, `--allow-closed`, seed flow via `flow init`) are covered by Research Notes or by local verbs (`flow init`, `flow list --status`, `flow active remove`).

## Approach

**Backlog row lifecycle.**
- The statuses split into **live** (`open`, `promoted`) and **terminal** (`resolved`, `dismissed`).
- The claim pair (`promoted`, `promoted_to`) is required on `promoted` rows, allowed on `resolved`/`dismissed` rows as history (both or neither), and forbidden on `open`.
- New flat fields `resolved_flow`, `resolved_tasks` and `resolved_commits` are allowed only on `resolved`.
- One helper, `schema::clear_for_transition(table, target_status)`, owns field clearing for `triage`, `relate` and `reconcile`. It keeps the claim pair when moving to `resolved`/`dismissed` and clears everything else the target does not own.
- `reopen` clears the claim; its rationale names the flow.
- `compact` never folds `promoted` rows, and copies `promoted_to` / `resolved_flow` into compacted rows when present.
- `check` ranks a new `in-flight` verdict right after `duplicate` for a fingerprint hit on a live `promoted` row, and reports `promoted_to`.

**Targets.** The new `backlog/target.rs` builds a `Resolver` once per command — one pass over `.claude/flows/*/context.toml`, skipping files that fail to parse — and resolves a `--to` value to one of:
- `Flow{slug, status, plan_path}`: a slug with `.claude/flows/<slug>/context.toml`, or a plan path that some flow's `plan_path` binds. A plan path bound this way is stored as the slug.
- `Plan{path}`: an existing `.md` file with no flow.
- `External(text)`: stored as `external:<text>`.
- `Unknown`, which includes every value `io::join_under` refuses (absolute, prefixed or `..`-bearing). Resolution never errors on a value, so `reconcile` reports a reason per row instead of aborting.

`triage --promote` behaviour:
- Rejects `Unknown` unless `--external` is given. The error names the seed-flow bootstrap.
- Rejects a `Flow` at `review`/`complete` unless `--allow-closed` is given.

`reconcile` uses the same resolver, so it handles both the slug and plan-path forms already in the store.

**Task↔item link.**
- Plan bullet: `- **Backlog**: B-aaaa1111, refs B-bbbb2222`, parsed into `ParsedTask.backlog_closes` / `backlog_refs`. `refs` qualifies its own comma-separated token only, and `(...)` / ` — ` annotations are stripped as on `Files`.
- Stored in a store-level `[[backlog_links]]` side table: `ref`, `closes`, `refs`, following the `[[file_notes]]` precedent. It is omitted when empty, so the golden fixtures and existing stores round-trip unchanged. `TaskRow` does not change.
- The links are plan-owned: import rebuilds them from the plan. `tasks remove` / `tasks update --ref` leave them alone, so a link can name a ref no row carries until the next import; `reconcile` reports such a link rather than joining it.
- `render` emits the bullet after `Depends on`, only when a link is present. `tasks show` emits `backlog: {closes, refs}` on a linked row, so an implementer dispatched by id sees the item its task closes.
- A new `tasks/backlog_refs.rs` computes the findings from the store plus `.claude/backlog.toml`. `import-plan` and `tasks check` both call it, and load the backlog only when the store holds links; `check::check` stays store-only. The classes:

| Class | Severity | Raised when |
|---|---|---|
| `backlog/unknown-id` | ERROR | the id is in neither the live nor the compacted array |
| `backlog/unpromoted` | WARNING | a `closes` id's item is still `open` |
| `backlog/claimed-elsewhere` | WARNING | a `closes` id's `promoted_to` is neither this flow's slug nor its plan path (not raised when no flow slug is known, as in a plan-mode dry run) |
| `backlog/closed` | INFO | the item is resolved, dismissed or compacted |

**`backlog reconcile [--flow <slug>] [--apply] [--adopt]`.** For each promoted row (filtered to rows whose target resolves to `--flow` when given):
- Resolve the target.
- Load that flow's store (no store file means no links).
- Join `backlog_links` to row statuses. It judges by task-row status, never by the flow's `status`, except to detect a closed flow.

Buckets:

| Bucket | Condition |
|---|---|
| `ready` | ≥1 `closes` task, and all of them are `done` |
| `in-progress` | some `closes` task is still pending or in progress, and the flow is live |
| `stalled` | a `closes` task is `deferred` or `failed`, and the flow is live |
| `unlinked` | no `closes` task, and the flow is live (a refs-only link, or a link naming no task, is noted) |
| `orphaned` | the flow is at `review`/`complete` and the row is not ready |
| `dangling` | the target is `Unknown`, or a `Plan` with no flow |
| `external` | the target is `External` |

- Each entry carries `id`, `summary`, `target`, `flow_status`, `closes`, `refs` and a `reason`; `closes` and `refs` are task `ref` slugs.
- `--adopt` runs first. For link-less `unlinked`/`orphaned` rows, it looks for the id as a whole token in the target store's row title/action/detail/acceptance and adds it to `closes` via `tasks::store::mutate`. It reports `adopted` and `render_needed` slugs (the caller must `tasks render` them, or the next import drops the links). Bucketing then reads the stores as `--adopt` left them.
- `--apply` resolves `ready` rows in one `mutate_doc`, re-checking each row inside the lock and skipping (with a reason) any row no longer `promoted` to the same target:
  - status → `resolved`;
  - generated `resolution` text;
  - claim pair kept;
  - `resolved_flow`, `resolved_tasks` (the `closes` and `refs` task refs);
  - `resolved_commits` (the distinct non-empty `commit` values of the linked rows; may be empty when the last batch is uncommitted).
- With neither flag, `reconcile` is read-only.

**Seed flows.**
- A promotion stub is a draft flow whose plan carries the marker line `<!-- backlog-seed -->`. The `backlog-capture` skill defines it.
- `/backlog` writes the stub, then runs `tomlctl flow init` without `--scope`/`--branch` and `tomlctl flow active remove --slug <slug>`. A seed is then neither the active-latest nor a scope/branch match for any resolution. Then it runs `triage --promote`.
- `/plan-new --backlog [<slug>]` lists the seeds (`tomlctl flow list --status draft`, grep for the marker) with their promoted items. It uses the chosen seed as its requirements and pre-folds those ids.
- `/plan-new` Phase 9 copies the approved plan over the seed's `plan_path`, removes the plan-mode file, and runs `flow init` with the seed's slug. That call's noop branch keeps the flow's `created` date and its claims but leaves `context.toml` untouched, so Phase 9 writes the derived scope and branch explicitly.

**Carriers.**
- `/implement` Phase 4: `reconcile --flow <slug> --apply` after green verification. The Summary reports what resolved and what is left. Phase 1 warns on this flow's `unlinked` rows.
- `/plan-new`:
  - live rows for the scope in Exploration Notes;
  - a fold-in question in Phase 4;
  - `Backlog` bullets in Phase 7;
  - Phase 9 seed adoption, promotion of the folded `closes` ids that are still `open`, and `tasks check`.
- `/plan-update complete`:
  - invokes `backlog-capture`, then runs `reconcile --flow <slug> --apply` first;
  - the remaining live rows join the warn gate, with batch reopen / keep / cancel;
  - `status` prints the reconcile counts.
- `/backlog`:
  - a new Promotions step (reconcile dry run → apply / adopt+render / reopen / re-promote);
  - the promote option validates the target and offers the seed bootstrap.
- `/review` and `/optimise` switch to `--live`, and send a claim that leads to no live flow to `/backlog` re-triage.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

Also run `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` and `bash scripts/verify-shared-blocks.sh`. The fmt check is the pre-commit gate. No shared block is edited, so the second is a regression guard.

After the final pass:
1. Confirm `tomlctl --version` reports 0.10.0. The orchestrator installs it at checkpoint F (see Dependency Graph); rerun `cargo install --path tomlctl` only if a later task touched Rust source.
2. Re-copy `claude/commands/backlog.md` over `~/.claude/commands/backlog.md`. It is a plain copy, not a link.
3. **Manual migration of existing data (not a task):**
   - In dev-tools, per flow that has claims: run `tomlctl tasks render --slug <slug> --check` first; only when it reports no pre-existing drift, run `tomlctl backlog reconcile --flow <slug> --adopt` then `tomlctl tasks render --slug <slug>`. (Checking after the adopt always reports drift, because the adopted link is itself a plan change. `whimsical-hugging-puppy` already reports `render/drift`, so skip it.) `/backlog`'s Promotions step does this for you.
   - The rows promoted to `tomlctl-followups` and `tomlctl-followups-round-2` have no `tasks.toml` for `--adopt` to link; send them straight to `/backlog` triage.
   - Run `/backlog` to triage the `dangling` rows (e.g. the 15 that point at `task-store-polish`) and the `orphaned` ones.
   - Repeat in `~/dev/reportdesignkit` if any promoted rows are re-created there.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 2, 3, 4, 5, 6, 8, 10, 11, 15, 18, 20, 22, 23, 25, 26, 27, 28
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### Phase A — backlog lifecycle (tomlctl/src/backlog)

### 1. Make promotion a live claim in the backlog schema [M]
- **Files**: `tomlctl/src/backlog/schema.rs`
- **Depends on**: —
- **Action**: Add the live/terminal split, the resolution-link fields, the claim-retention rules to `validate`, and a shared `clear_for_transition` helper.
- **Detail**: - Add `FIELD_RESOLVED_FLOW`, `FIELD_RESOLVED_TASKS`, `FIELD_RESOLVED_COMMITS`.
  - Add `LIVE_STATUSES = [open, promoted]` and `is_live(status)` (task 5 consumes it).
  - Add `CLAIM_FIELDS = [promoted, promoted_to]` and `RESOLUTION_LINK_FIELDS`, and append the link fields to `MANAGED_FIELDS`.
  - Add `pub(crate) struct ResolutionLink { flow: String, tasks: Vec<String>, commits: Vec<String> }` with `fn write_to(&self, table)`.
  - Add `pub(crate) fn clear_for_transition(table: &mut toml::Table, target: &str)`. It removes every `MANAGED_FIELDS` entry except the target's own pair, and keeps `CLAIM_FIELDS` when the target is `resolved` or `dismissed`.
  - In `validate` (:345-389):
    - keep rejecting foreign pairs, but allow the claim pair on `resolved`/`dismissed`;
    - add a `BacklogError::PartialClaim` (exactly one of the pair present);
    - reject the link fields on any status other than `resolved`;
    - `open` still rejects everything.
  - Add `pub(crate) const COMPACTED_OPTIONAL_FIELDS: &[&str] = &[FIELD_PROMOTED_TO, FIELD_RESOLVED_FLOW]`. Leave `COMPACTED_FIELDS` and its pin test unchanged.
  - Update the doc comments this falsifies (:128-130, :248-250, :327-332), which describe promotion as terminal.
  - Update the inline tests `a_terminal_status_rejects_another_status_cluster` (:551) and `every_terminal_status_owns_a_pair_and_open_owns_none` (:747) for the new rule. Add tests:
    - a resolved row with a claim validates;
    - a partial claim is rejected;
    - link fields on `dismissed`/`open` are rejected;
    - `clear_for_transition` keeps the claim for resolve and dismiss and clears it for reopen.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::schema` passes. Falsifier: today `validate` rejects a `resolved` row carrying `promoted`/`promoted_to` with `ForeignTerminalField` (schema.rs:375-387), so the new "resolved row with a claim validates" test is red on the current tree — predicted, unverified.

### 2. Route triage and relate transitions through the shared clear helper [S]
- **Files**: `tomlctl/src/backlog/triage.rs`, `tomlctl/src/backlog/relate.rs`
- **Depends on**: 1
- **Action**: Replace the ad-hoc `MANAGED_FIELDS` loops with `schema::clear_for_transition`, and expose a resolve-with-link entry point for `reconcile`.
- **Detail**: - `triage::rewrite` (:143) calls `clear_for_transition(table, t.status())`, then writes status, date and companion as today.
  - `relate::dismiss` (:174-192) does the same with `STATUS_DISMISSED`.
  - Add `pub(crate) fn resolve_with_link(row: &mut TomlValue, resolution: &str, link: &schema::ResolutionLink, today: Datetime) -> Result<()>`. It calls `rewrite` for `Transition::Resolve`, writes the link fields, then calls `schema::validate`.
  - Update the module docs this falsifies (triage.rs:4-8, relate.rs:168-173).
  - Inline tests:
    - promote → resolve keeps `promoted`/`promoted_to`;
    - promote → reopen clears them;
    - promote → dismiss keeps them;
    - a resolved row re-dismissed drops the link fields;
    - relate's duplicate-of dismissal of a promoted row keeps the claim.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- backlog::triage backlog::relate` passes. Falsifier: `rewrite` today removes `promoted_to` on resolve (triage.rs:152-157), so the keep-claim test is red before this task — predicted, unverified.

### 3. Stop compact folding claimed rows [S]
- **Files**: `tomlctl/src/backlog/compact.rs`
- **Depends on**: 1
- **Action**: Make `promoted` rows ineligible for folding, and carry the optional claim/resolution fields into compacted rows.
- **Detail**: - In `due` (:132), return `None` when the status is `promoted`.
  - In `compacted_row` (:163-180), copy each `schema::COMPACTED_OPTIONAL_FIELDS` entry when it is present and non-empty.
  - Update the doc (:159-162) and the name of the test at :354, which describe promoted rows as foldable.
  - Inline tests:
    - an aged promoted row is left in place;
    - an aged resolved row with a claim folds and keeps `promoted_to` and `resolved_flow`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::compact` passes. Falsifier: `due` folds any status with a `terminal_pair` (compact.rs:125-135), so the aged-promoted test is red today — predicted, unverified.

### 4. Add the in-flight verdict to backlog check [S]
- **Files**: `tomlctl/src/backlog/check.rs`
- **Depends on**: —
- **Action**: Rank a fingerprint hit on a live `promoted` row as `in-flight` and report its target.
- **Detail**: - Add `Reason::InFlight` between `DedupId` and `Compacted` (enum order is the ladder, :44). `as_str` is `"in-flight"`; `verdict` is `"in-flight"`.
  - In `evaluate` (:257-273), choose `InFlight` when the live row's status is `promoted`.
  - Add `promoted_to` (skipped when absent) to the candidate's JSON.
  - Update the doc at :162-163.
  - Inline test: a dedup hit on a promoted row gives the verdict `in-flight`, and the candidate carries `promoted_to`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::check` passes. `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_read` still passes (regression guard; it pins the existing verdict strings). Falsifier: today a promoted live dedup hit yields `duplicate` — predicted, unverified.

### 5. Add backlog list --live [M]
- **Files**: `tomlctl/src/backlog/query.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/backlog/dispatch.rs`
- **Depends on**: 1
- **Action**: Add a `--live` filter that keeps `open` and `promoted` rows.
- **Detail**: - `BacklogOp::List` (types.rs:1126): `#[arg(long, conflicts_with_all = ["open", "status"], help = "Only live rows: open or promoted")] live: bool`.
  - Thread it through the dispatch arm (dispatch.rs:65) and the `dispatch_list` parameters.
  - Add `live` to `Filters` (query.rs:82) and check it in `filter_backlog` with `schema::is_live`.
  - Inline tests in the query.rs `mod tests` (:284): a store with one row per status gives two rows under `--live`; `Cli::try_parse_from(["tomlctl","backlog","list","--live"])` parses and `--live --open` yields `ArgumentConflict` (precedent: triage.rs `two_mode_flags_conflict_at_the_parser`).
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::query` passes, including the parser test. Falsifier: the flag does not exist today (clap `UnknownArgument`) — predicted, unverified.

### 6. Add the promotion-target resolver [M]
- **Files**: `tomlctl/src/backlog/target.rs` (new), `tomlctl/src/backlog/mod.rs`, `tomlctl/src/flow/mod.rs`
- **Depends on**: —
- **Action**: Create `target.rs`, which resolves a `promoted_to` / `--to` value to a flow, plan, external or unknown target.
- **Detail**: - `pub(crate) enum Target { Flow { slug, status, plan_path }, Plan { path }, External(String), Unknown(String) }`, plus `fn is_closed(&self)` (a flow at `review`/`complete`) and `fn stored_value(&self) -> String` (slug, path, `external:<text>`).
  - `pub(crate) struct Resolver`, built by `Resolver::new(root: &Path) -> Result<Resolver>`: one pass over `.claude/flows/*/context.toml` (`io::read_dir_sorted`) that indexes each flow's slug, status and normalised `plan_path`, skipping files that fail to parse, as `enumerate_flows` does. A missing `.claude/flows` gives an empty index.
  - `pub(crate) fn resolve(&self, raw: &str) -> Target`, infallible:
    - an `external:` prefix → `External`;
    - a slug-shaped value (`flow::validate_slug`) in the index → `Flow`, via `flow::FlowProjection`;
    - otherwise normalise `\`→`/` and look the value up by `plan_path` → `Flow`;
    - else join through `io::join_under`; a value it refuses (absolute, prefixed or `..`-bearing) → `Unknown`, never `Err`;
    - else an existing repo-relative `.md` file → `Plan`;
    - else `Unknown`.
  - `flow/mod.rs`: add `pub(crate) use init::validate_slug; pub(crate) use schema::FlowProjection;`.
  - `backlog/mod.rs`: add `mod target;` and change `mod schema;` to `pub(crate) mod schema;` (task 13 reads it).
  - Inline tests with `crate::test_support::with_root`: slug, bound plan path, unbound plan, external, unknown, closed flow, a refused `..`/absolute value → `Unknown`, and a malformed `context.toml` skipped without failing `Resolver::new`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::target` passes and reports at least 8 tests run (a filter matching none exits 0, so read the count). Falsifier: the module does not exist today, so the filter runs 0 tests — predicted, unverified.

### 7. Validate promotion targets in triage [M]
- **Files**: `tomlctl/src/backlog/triage.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/backlog/dispatch.rs`
- **Depends on**: 2, 5, 6
- **Action**: Make `triage --promote --to` resolve its target, add `--external` and `--allow-closed`, and store the canonical value.
- **Detail**: - `BacklogOp::Triage` (types.rs:1180) gets `#[arg(long, requires = "promote")] external: bool` and `#[arg(long, requires = "promote")] allow_closed: bool` on the variant, not in `TriageMode`. Update the `to` doc (:1185), which currently says "stored verbatim". Thread both through the dispatch arm in `backlog/dispatch.rs`.
  - `dispatch` (triage.rs:222) resolves a `Transition::Promote` companion via `target::Resolver::new(&repo_or_cwd_root()?)?.resolve(..)` after `Transition::from_cli` (and its missing-companion check) and before `mutate_doc`, keeping `from_cli` pure:
    - `Unknown` → a `NotFound` error: ``no flow or plan `X` — bootstrap a draft seed flow (see /backlog) or pass --external``;
    - `--external` → `Target::External`;
    - a closed `Flow` without `--allow-closed` → a `Validation` error naming its status.
  - The stored value is `stored_value()`, so a bound plan path is stored as the slug. The output JSON adds `"to"`.
  - Update any inline test that promotes through `dispatch` to seed a flow directory or pass `external`. The `from_cli` tests (:566, :679) keep expecting kind validation on a missing `--to`.
  - New inline dispatch tests: unknown target → error; `external` stores `external:…`; a closed flow → error, and `allow_closed` passes; a plan path is stored as the slug.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::triage` passes, including the four new dispatch tests. Falsifier: `--to nonexistent` succeeds today (triage.rs `companion` only rejects blank values), so the unknown-target test is red before this task — predicted, unverified.

### 8. Integration-test validated promotion targets [S]
- **Files**: `tomlctl/tests/backlog_write.rs`
- **Depends on**: 7
- **Action**: Add CLI cases for `triage --promote` target validation through the real binary.
- **Detail**: - Cases: unknown target → error naming `--external`; `--external` stores `external:…`; a closed flow → error, and `--allow-closed` passes; a plan path is stored as the slug.
  - Seed flows with `seed_tasks`; for the closed-flow case, overwrite `.claude/flows/<TASKS_SLUG>/context.toml` inline with a `review` status after `seed_tasks`. Do not edit `tomlctl/tests/common/mod.rs`.
  - The clap-conflict test at :520-546 exits before `from_cli` and stays as it is.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_write` passes. Falsifier: `backlog_write` has no unknown-target case today, and on the pre-task-7 binary `--to nonexistent` succeeds — predicted, unverified.

### Phase B — task↔item link (tomlctl/src/tasks)

### 9. Add the backlog_links side table to the task store [S]
- **Files**: `tomlctl/src/tasks/schema.rs`
- **Depends on**: —
- **Action**: Add a `[[backlog_links]]` side table keyed by task `ref`.
- **Detail**: - `pub(crate) struct BacklogLink { pub(crate) r#ref: String, pub(crate) closes: Vec<String>, pub(crate) refs: Vec<String> }`.
  - Add `Store.backlog_links: Vec<BacklogLink>`, defaulting to empty (`Default` impl :183).
  - Read it in `from_toml` (:327) like `file_notes`, dropping entries with both lists empty.
  - Write it in `to_toml` (:385), omitted when empty (:419-439 pattern).
  - Add `pub(crate) fn links_for(&self, r#ref: &str) -> Option<&BacklogLink>`.
  - `tasks remove` / `tasks update --ref` do not touch `backlog_links`; a link naming a ref no row carries survives until the next import, and task 16 reports it.
  - Inline tests: round trip; an empty table is omitted.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::schema` passes. `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_import` still passes (regression guard; golden `house-plan.tasks.toml`) — predicted, unverified.

### 10. Parse the Backlog plan bullet [S]
- **Files**: `tomlctl/src/tasks/parse_tasks.rs`, `tomlctl/src/tasks/import_plan.rs`
- **Depends on**: —
- **Action**: Accept `- **Backlog**: B-aaaa1111, refs B-bbbb2222` on a task.
- **Detail**: - Add `Backlog` to the bullet regex (:525), a `Field::Backlog` variant and its `from_label` arm (:192).
  - Add `backlog_closes: Vec<String>` and `backlog_refs: Vec<String>` to `ParsedTask` (:48).
  - In `close_field` (:290): `split_entries(.., ',')`, strip backticks and whitespace, skip `is_empty_marker`. Strip `(...)` / ` — ` annotations as `push_file` does (`annotation_at`). A token starting `refs ` goes to `backlog_refs`, with the remainder trimmed; others go to `backlog_closes`. `refs` covers its own token only.
  - In import_plan.rs, add `backlog_closes: _, backlog_refs: _` to the exhaustive `ParsedTask` destructure in `merge_row` (:617) only.
  - Inline tests: mixed closes/refs, backticked ids, `none`, an annotated id (`B-1 (partial)` → `B-1`), and `refs B-1, B-2` → refs=[B-1], closes=[B-2].
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::parse_tasks` passes. Falsifier: the regex (:525) has no `Backlog` label today, so the bullet is ignored — predicted, unverified.

### 11. Import backlog links from the plan [S]
- **Files**: `tomlctl/src/tasks/import_plan.rs`
- **Depends on**: 9, 10
- **Action**: Rebuild `store.backlog_links` from the parsed tasks on every import, as plan-owned state.
- **Detail**: - Beside `surviving_file_notes` (:272/:774), set `store.backlog_links` to one entry per parsed task with a non-empty closes or refs list, keyed by the task's final `ref`.
  - Removed tasks lose their links.
  - `--dry-run` computes the links but writes nothing, as today.
  - Inline test: import a plan with one linked task; the store holds one `BacklogLink`, and re-importing without the bullet clears it.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::import_plan` passes. Falsifier: before this task the import leaves `store.backlog_links` empty, so the one-link assertion is red — predicted, unverified.

### 12. Render the Backlog bullet [S]
- **Files**: `tomlctl/src/tasks/render.rs`, `tomlctl/src/tasks/show.rs`
- **Depends on**: 9, 10
- **Action**: Emit `- **Backlog**: …` after `Depends on` for tasks that have links, and show the link on a fetched row.
- **Detail**: - In `render_tasks` (:195-228), after the `Depends on` line, look up `store.links_for(&row.r#ref)`.
  - Emit it only when a link exists: closes ids first, then each refs id prefixed `refs `, comma-separated.
  - In `show.rs`, beside the ref-keyed `import_override` stamp (:76), emit `backlog: {closes, refs}` for a linked row, so `/implement`'s dispatch-by-id shows the implementer the item its task closes.
  - Inline tests: render a fixture with a link, parse it with `parse_tasks`, and get equal lists; `show` of a linked row carries `backlog`, and of an unlinked row omits it.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- tasks::render tasks::show` passes. `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_render` still passes (regression guard; golden `house-plan.rendered.md`) — predicted, unverified.

### 13. Add backlog link findings [M]
- **Files**: `tomlctl/src/tasks/backlog_refs.rs` (new), `tomlctl/src/tasks/mod.rs`
- **Depends on**: 6, 9
- **Action**: Compute the `backlog/*` findings from a store and the backlog document, and expose the store API that `reconcile` needs.
- **Detail**: - `pub(crate) fn backlog_findings(store: &Store, backlog: &TomlValue, flow: Option<&str>) -> Vec<Finding>`. For each link id, find the row by id in `backlog` then `compacted` (`crate::backlog::schema::{ARRAY_BACKLOG, ARRAY_COMPACTED, …}`). The classes:
    - `backlog/unknown-id` (`ERROR`): the id is not found.
    - `backlog/unpromoted` (`WARNING`): a `closes` id's item is `open`.
    - `backlog/claimed-elsewhere` (`WARNING`): a `closes` id's item is `promoted` with a `promoted_to` that is neither `flow` nor `store.plan_path`. Compare after `\`→`/` normalisation. Not raised when `flow` is `None` (the plan-mode dry run cannot know the slug).
    - `backlog/closed` (`INFO`): the item is resolved or dismissed, or is in `compacted`.
  - Build each `Finding` with a literal `class: "backlog/..."` field and its `severity:` within the next four lines — the only shape `finding_classes.rs` reads.
  - Findings carry the linked task ids.
  - `pub(crate) fn load_backlog() -> Result<TomlValue>` wraps `crate::backlog::schema::read_store` with default integrity args. A missing store gives an empty table.
  - tasks/mod.rs: add `mod backlog_refs;` — the sibling modules reach it as `super::backlog_refs`, so no re-export. Add `pub(crate) use store::{load as load_store, mutate as mutate_store, resolve_store_path}` and `pub(crate) use schema::{BacklogLink, Status, Store}` for `backlog::reconcile`.
  - Inline tests: one per class, a `refs` id on an `open` item raising nothing, and `flow = None` raising no `claimed-elsewhere`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::backlog_refs` passes and reports at least 6 tests run. Falsifier: the module does not exist today, so the filter runs 0 tests — predicted, unverified.

### 14. Surface backlog findings in import and check [M]
- **Files**: `tomlctl/src/tasks/import_plan.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/tests/tasks_corpus.rs`
- **Depends on**: 11, 13
- **Action**: Add the `backlog_findings` output to `tasks import-plan` (dry-run and real) and to `tasks check`, without making a link-free store read the backlog.
- **Detail**: - In `import_plan()` (which binds `slug`), after each `import.apply(..)` call and before `refuse_on_errors`, append `backlog_findings(store, &backlog, slug)`, loading the backlog only when `store.backlog_links` is non-empty, so `backlog/unknown-id` refuses a real import and `Import::apply` stays I/O-free.
  - The plan-mode `--plan` dry run passes `None` for the slug.
  - In tasks/dispatch.rs `Check` arm (near :248), append the same under the same non-empty guard, with the `--slug` value (or `None` for `--file`).
  - `tests/tasks_corpus.rs`: ignore `backlog/*` classes when collecting error findings (near :167) — the corpus checks plan structure, not the local backlog store.
  - Inline tests: a plan naming an id absent from `.claude/backlog.toml` makes a real import refuse with `backlog/unknown-id`; a plan with no links imports cleanly when `.claude/backlog.toml` is malformed.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::import_plan` runs both new tests and passes; `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus` still passes. Falsifier: today a plan naming an unknown backlog id imports cleanly, so the refusal test is red before this task — predicted, unverified. Integration coverage is task 15.

### 15. Test the backlog link round trip end to end [S]
- **Files**: `tomlctl/tests/tasks_import.rs`
- **Depends on**: 12, 14
- **Action**: Add integration tests for bullet → store → findings → render.
- **Detail**: Use `tests/common/mod.rs` `sandbox`/`cli`. Seed `.claude/backlog.toml` with one open item and one promoted item (targeting the test flow), plus a plan with a `Backlog` bullet naming both as `closes` ids and an unknown id. Assert:
  - `import-plan --plan … --dry-run` reports `backlog/unknown-id` (ERROR) and `backlog/unpromoted` (WARNING);
  - after fixing the unknown id, a real import writes `[[backlog_links]]`;
  - `tasks render --slug … --check` exits 0;
  - `tasks check --slug …` reports no `backlog/claimed-elsewhere`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_import` passes — predicted, unverified.

### Phase C — reconcile verb and release

### 16. Implement backlog reconcile [L]
- **Files**: `tomlctl/src/backlog/reconcile.rs` (new), `tomlctl/src/backlog/mod.rs`
- **Depends on**: 2, 6, 13
- **Action**: Implement `pub(crate) fn dispatch(flow: Option<String>, apply: bool, adopt: bool, integrity: WriteIntegrityArgs) -> Result<()>`, which buckets promoted rows and optionally adopts links and resolves ready rows.
- **Detail**: The steps, as in the Approach:
  1. Read the store (`schema::read_store`) and select the `promoted` rows.
  2. Build one `target::Resolver`, resolve each row, and filter by `--flow` against the resolved slug.
  3. Cache one store per flow: `tasks::load_store(tasks::resolve_store_path(Some(slug), None)?)` when the file exists, and an empty store (no links) when it does not — `load` errors on a missing file.
  4. If `adopt`: for link-less rows whose target is a `Flow`, find task rows whose `title`/`action`/`detail`/`acceptance` contain the id as a whole token (followed by end-of-text or a non-hex character — widened ids extend shorter ones). Add them to `closes` via `tasks::mutate_store`, and replace the cached store with the store `mutate_store` returns. Record `adopted: [{id, flow, task_refs}]` and `render_needed: [slug]`.
  5. Bucket: `ready`/`in-progress`/`stalled`/`unlinked`/`orphaned`/`dangling`/`external`. Decide by `Status::Done` / `Deferred` / `Failed` and `Target::is_closed()`. A `closes` ref no row carries is left out of the join and named in `reason` (`link names no task: <ref>`); a row left with no live `closes` task is `unlinked`. Each entry carries `id`, `summary`, `target`, `flow_status`, `closes`, `refs` and `reason`.
  6. If `apply`: in one `io::mutate_doc`, re-find each `ready` row by id, skip (and report) any no longer `promoted` or whose `promoted_to` changed, and call `triage::resolve_with_link` on a clone, swapping it in only on success (a failure is reported under `skipped`, and the other rows still resolve), with:
     - resolution ``resolved by flow `<slug>` (tasks <refs>)`` — it names tasks, never commits;
     - `ResolutionLink { flow, tasks: the closes and refs task refs, commits: distinct non-empty row.commit }` — `commits` may be empty, since under per-batch legacy the final batch is uncommitted when Phase 4 reconciles;
     - `last_updated` stamped.
  7. Print `{ok, flow, buckets: {<bucket>: [entry...]} (all seven keys, empty arrays kept), adopted: [{id, flow, task_refs}], applied: [id...], skipped: [{id, reason}], render_needed: [slug...]}`; an entry's `closes`/`refs` are task `ref` slugs.
  - Without flags the verb is read-only.
  - `mod reconcile;` in backlog/mod.rs.
  - Inline tests (`with_root`, seeding `.claude/flows/<slug>/{context,tasks}.toml` and `.claude/backlog.toml`): one per bucket; `--apply` writes the link fields and keeps `promoted_to`; `--adopt` links a prose-referenced id; `--adopt --apply` resolves a row adopted in the same run; `--adopt` does not link `B-1a2b3c4d` from text naming `B-1a2b3c4d5e`; a flow with no `tasks.toml` buckets as `unlinked`; `--flow` filters.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl backlog::reconcile` passes and reports at least 13 tests run. Falsifier: the module does not exist today, so the filter runs 0 tests — predicted, unverified.

### 17. Register the reconcile verb and release 0.10.0 [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`
- **Depends on**: 7, 16
- **Action**: Wire `backlog reconcile` into the CLI and bump the crate to 0.10.0. These edits are inseparable: the parity tests compare `FEATURES`, the capabilities test and the README together, and the version appears in all of them.
- **Detail**: - `BacklogOp::Reconcile { #[arg(long)] flow: Option<String>, #[arg(long)] apply: bool, #[arg(long)] adopt: bool, #[command(flatten)] integrity: WriteIntegrityArgs }` with help text.
  - Add a dispatch arm.
  - Add `"backlog_reconcile"` to `FEATURES` (types.rs:21).
  - In `tests/capabilities.rs`, add it to the expected list (:1571) and `&["backlog","reconcile","--help"]` to the write help list (:98), and bump the version literal (:1662).
  - In the README, update the capabilities sample (~:256-268), the Feature meanings table (~:340-350, including the `backlog_compact`/`backlog_list` rows) and the verb synopsis (:55-64: add `reconcile` and `--live`; :60 promotes to the closed flow `tomlctl-backlog-capture`; :64 says open items never move); update the `Compact` doc (types.rs:1229-1230).
  - Cargo.toml `version = "0.10.0"`; update tomlctl's own entry in Cargo.lock.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes. Falsifier: `readme_sample_version_matches_cargo_toml` and `capabilities_version_matches_cargo_toml` go red if only Cargo.toml is bumped — predicted, unverified.

### 18. Integration-test backlog reconcile [S]
- **Files**: `tomlctl/tests/backlog_reconcile.rs` (new)
- **Depends on**: 17
- **Action**: Add assert_cmd tests for `backlog reconcile` through the real binary.
- **Detail**: Use `tests/common/mod.rs` (`sandbox`, `cli`, `backlog`, `seed_tasks`). For the `review`-status flow, overwrite `.claude/flows/<TASKS_SLUG>/context.toml` inline after `seed_tasks`; do not edit `tomlctl/tests/common/mod.rs`. Scenarios:
  - ready + `--apply`: the row becomes `resolved`, with `resolved_flow`/`resolved_tasks`/`resolved_commits` set and `promoted_to` kept;
  - an `unlinked` row that `--adopt` links, reported under `render_needed`;
  - `orphaned` (flow `review`, pending task);
  - `stalled` (a deferred closes task);
  - `dangling` (a target that doesn't exist);
  - `external`;
  - a refs-only link stays `unlinked`;
  - `--flow` filters;
  - the default run leaves `.claude/backlog.toml` byte-identical.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_reconcile` passes — predicted, unverified.

### Phase D — skills, references and carriers

### 19. Document the backlog lifecycle in the tomlctl reference [M]
- **Files**: `claude/skills/tomlctl/references/backlog.md`, `claude/skills/tomlctl/SKILL.md`, `claude/skills/tomlctl/references/backlog-reconcile.md`
- **Depends on**: 18
- **Action**: Document `--live`, the target validation (`--external`, `--allow-closed`), the claim retention, compact's rule for claimed rows, the new store fields, the `in-flight` verdict, and a new `backlog reconcile` section.
- **Detail**: - references/backlog.md:
    - Contents (10-27): add the reconcile entry.
    - `list` table (131-139): `--live`.
    - `triage` table (195-205): the `--to` resolution rules, `--external`, `--allow-closed`. Fix the example at :192, which promotes to a target that is not a flow.
    - `compact` (263-290).
    - Store shape table (368-378), status invariant (379-382) and compacted shape (384-389): the live/terminal split, claim pair, link fields and the optional compacted fields.
    - Verdict ladder (421-441): `in-flight`.
    - New `## backlog reconcile` section: flags table, bucket definitions, the output shape exactly as task 16 step 7 prints it, and the `render_needed` duty.
  - SKILL.md: the verb list (:3), the backlog routing row (:37) and the version sample (:93, 0.9.0 → 0.10.0).
  - Bash examples must be real flags, because `command_lint` and `flag_table_lint` parse them.
- **Acceptance**: `grep -c "backlog reconcile" claude/skills/tomlctl/references/backlog.md` ≥ 3 (today: 0). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl lint` passes (predicted, unverified).

### 20. Document the task-store backlog link [M]
- **Files**: `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/tomlctl/references/tasks.md`, `claude/skills/tomlctl/references/tasks-store.md`
- **Depends on**: 14
- **Action**: Document the `- **Backlog**:` bullet (with `refs`), the `[[backlog_links]]` side table (plan-owned, rebuilt on import, rendered after `Depends on`, shown by `tasks show`), and the four `backlog/*` finding classes.
- **Detail**: - task-store SKILL: the side table near the store schema (40-90) and the "two side tables" sentence (:168), the classes table (255-281), the render order (:300). State that `refs` qualifies its own token only.
  - tasks.md: the class table (491-520).
  - tasks-store.md: add `[[backlog_links]]` beside `[[import_overrides]]` and `[[file_notes]]` in the optional side tables.
- **Acceptance**: `grep -l "backlog_links" claude/skills/flow-contract-task-store/SKILL.md claude/skills/tomlctl/references/tasks.md claude/skills/tomlctl/references/tasks-store.md | wc -l` = 3 (today: 0). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl finding_classes` passes. Falsifier: with task 13's classes in the source and this task undone, `documented_finding_classes_match_the_source` is red — predicted, unverified.

### 21. Update the backlog-capture skill for live promotion and seed flows [S]
- **Files**: `claude/skills/backlog-capture/SKILL.md`
- **Depends on**: 18
- **Action**: - Redefine `promoted` as a live claim.
  - Add the `in-flight` verdict to the gate.
  - Describe reconcile-driven resolution.
  - Define seed flows.
- **Detail**: - Description (:3): add `in-flight` to the verdict list. Fix :44 to match.
  - Status vocab (66-74): `promoted` is live until reconcile or triage resolves it; resolution keeps the claim and adds `resolved_flow`/`resolved_tasks`/`resolved_commits`.
  - Verdict table: `in-flight` → do not mint. Name the claiming flow; if the discovery shows the claim is wrong or incomplete, surface it to that flow instead.
  - "After the mint" (205-211): `triage --promote` validates its target, `reconcile` resolves.
  - New "Seed flows" subsection:
    - the marker line `<!-- backlog-seed -->`;
    - the stub layout (`# Plan: <title>`, `**Status**: Draft`, `## Context` listing each id + summary);
    - the bootstrap sequence (`tomlctl flow init --slug <slug> --plan <plans_dir>/<slug>.md`, with no `--scope`/`--branch` — resolution steps 2 and 5 read every non-complete flow on disk, registered or not — then `tomlctl flow active remove --slug <slug>`, then `tomlctl backlog triage <ids> --promote --to <slug>`);
    - `/plan-new --backlog` as the way to pick a seed up.
  - Idioms (275/281): `list --live`; fix :281, whose `--to lumina-pty-hardening` names no flow.
- **Acceptance**: `grep -c "backlog-seed" claude/skills/backlog-capture/SKILL.md` ≥ 1 and `grep -c "in-flight" claude/skills/backlog-capture/SKILL.md` ≥ 1 (both today: 0). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl lint` passes (predicted, unverified).

### 22. Wire reconcile into /implement [S]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 19
- **Action**: - Phase 4 resolves delivered items.
  - Phase 1 warns about claims the plan never links.
- **Detail**: - Phase 4: update the order sentence (:148) to "render the plan, reconcile the backlog, harvest the backlog, then output the Summary". Add a **Backlog reconcile** paragraph before the harvest (:171): run `tomlctl backlog reconcile --flow <slug> --apply` after the green final pass, only when the pass was green.
  - The Summary `### Backlog` block (193-194) lists the resolved ids and the counts of the remaining buckets.
  - Phase 1: run `tomlctl backlog reconcile --flow <slug>` and surface `unlinked` ids as `N backlog item(s) promoted to <slug> are claimed by no task`.
  - Step 0.5 degradation halt (:48): add the `backlog/unknown-id` recovery — sync `.claude/backlog.toml` (e.g. a worktree cut before the mint), or drop the id from the task's `Backlog` bullet — beside the stale-binary and revert recoveries.
- **Acceptance**: `grep -c "backlog reconcile --flow" claude/commands/implement.md` ≥ 2 (today: 0). `grep -c "backlog/unknown-id" claude/commands/implement.md` ≥ 1 (today: 0).

### 23. Wire the backlog into /plan-new with --backlog seed adoption [M]
- **Files**: `claude/commands/plan-new.md`, `tomlctl/src/cli/dispatch/tests/skills.rs`, `claude/skills/flow-contract-plan-output-format/SKILL.md`
- **Depends on**: 20, 21, 24
- **Action**: Add backlog awareness to `/plan-new`: a `--backlog [<slug>]` seed picker, live-row exploration, a fold-in question, `Backlog` bullets, and Phase-9 seed adoption plus promotion; document the bullet in the plan format.
- **Detail**: - Frontmatter `argument-hint` (:3): add `[--backlog [seed-slug]]`.
  - Phase 1: `--backlog`. List the seeds with `tomlctl flow list --status draft`, keeping flows whose plan contains `<!-- backlog-seed -->`, each with `tomlctl backlog list --where status=promoted --where promoted_to=<slug> --select id,summary`. Pick via `AskUserQuestion` (or use the slug given). Read the stub as the requirements, and bind `seed_slug` and `seed_plan_path`.
  - Phase 2: invoke the `backlog-capture` skill, run `tomlctl backlog list --live --area-prefix <dir>` per affected area, and record the rows in Exploration Notes.
  - Phase 4: a multi-select fold-in question over those rows, with the seed's items pre-selected.
  - Phase 6/7: folded ids go on their tasks' `- **Backlog**:` line (`refs` for partial coverage). Amend Phase 7's dry-run paragraph (plan-new.md:103): `backlog/unpromoted` on a folded id is expected until Phase 9 step 7 — report it, do not disposition it.
  - Phase 9:
    - when `seed_slug` is bound: write the approved plan over `seed_plan_path`, delete the plan-mode file, and use the seed slug for steps 1-6; `flow init`'s noop branch leaves the seed's `context.toml` untouched, so afterwards write the derived scope (`tomlctl set-json .claude/flows/<slug>/context.toml scope --json '[...]'`) and branch (`tomlctl set .claude/flows/<slug>/context.toml branch <b>`) explicitly;
    - new step 7: collect the `closes` ids from the store's `[[backlog_links]]` whose backlog status is `open` (never `refs`, resolved, dismissed, compacted or claimed-elsewhere ids — report those) and run `tomlctl backlog triage <ids> --promote --to <slug>`, then `tomlctl tasks check --slug <slug>` (expect no `backlog/unpromoted`).
  - skills.rs `carrier_invokes_required_skills` (:1011): add `backlog-capture` to the skills required of `plan-new.md` and `plan-update.md` (task 24 adds the invocation there).
  - plan-output-format SKILL: an optional `- **Backlog**:` line in the task template (105-119), and a format rule saying the bullet names backlog ids the task closes or `refs`, with `refs` qualifying its own token only.
- **Acceptance**: `grep -c "backlog-seed\|--backlog" claude/commands/plan-new.md` ≥ 3 (today: 0). `grep -c '\*\*Backlog\*\*' claude/skills/flow-contract-plan-output-format/SKILL.md` ≥ 1 (today: 0). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- carrier_invokes_required_skills lint` passes (predicted, unverified).

### 24. Gate /plan-update complete on live promotions [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 19
- **Action**: - `complete` resolves ready items, then gates on the rest with a batch reopen.
  - `status` prints the reconcile counts.
- **Detail**: - Invoke the `backlog-capture` skill before `complete` step 4.
  - `complete` step 4 (:105): first `tomlctl backlog reconcile --flow <slug> --apply`. Then count the remaining rows (`in-progress`, `stalled`, `unlinked`, `orphaned`) and add them to the prompt: `… <b_count> backlog (<b_list>)`. The options:
    - `Reopen them and complete`: `tomlctl backlog triage <ids> --reopen --rationale "flow <slug> completed without resolving it"`;
    - `Complete, keep them promoted`;
    - `Cancel`.
  - Non-interactive runs keep the existing refusal. Update step 6's summary (:107) and the ordering note (:117).
  - `status` (94-96): print `backlog: N ready, M in-progress, …` from a dry run.
- **Acceptance**: `grep -c "backlog reconcile --flow" claude/commands/plan-update.md` ≥ 2 (today: 0). `grep -c "backlog-capture" claude/commands/plan-update.md` ≥ 1 (task 23's `carrier_invokes_required_skills` enforces it).

### 25. Add promotion triage and seed bootstrap to /backlog [M]
- **Files**: `claude/commands/backlog.md`
- **Depends on**: 19, 21
- **Action**: - Add a Promotions step.
  - Make the promote disposition validate its target and offer the seed bootstrap.
- **Detail**: - Step 2 promote option (:56/:61): the `--to` target must resolve; fix :61, which promotes to a closed flow.
    - On `Unknown`, ask: bootstrap a draft seed flow `<slug>` (per the backlog-capture skill's Seed flows sequence, with plans dir from `tomlctl json get .claude/settings.json plansDirectory`, default `docs/plans/`), pick an existing flow, or `--external`.
    - On a closed flow, ask: `--allow-closed` or seed a new flow.
  - New step between 2 and 3, "Promotions":
    - run `tomlctl backlog reconcile`;
    - `ready` → offer `--apply`;
    - `unlinked` → offer `--adopt`, then for each `render_needed` run `tomlctl tasks render --slug <slug> --check` first and render only when it reports no pre-existing drift (otherwise report and stop);
    - `orphaned`/`stalled`/`dangling` → reopen (`triage --reopen --rationale`) or re-promote;
    - `external` → listed only.
  - Step 0 (:30-36): stop only when the `--live` count is 0 (not the `--open` count), so promoted-only stores still reach Promotions; raise the version gate from 0.6 to 0.10 (`reconcile`, `--live`).
  - Compact (:105): promoted rows no longer fold.
- **Acceptance**: `grep -c "backlog reconcile\|backlog-seed" claude/commands/backlog.md` ≥ 3 (today: 0). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl lint` passes (predicted, unverified).

### 26. Show live backlog rows to /review and /optimise [S]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`
- **Depends on**: 18
- **Action**: Switch the backlog context load from `--open` to `--live`, and tell lenses that a `promoted` row is already claimed by a flow.
- **Detail**: - `review.md:44` and `optimise.md:46`: `tomlctl backlog list --live --area-prefix <scope-dir>`; update the lead-ins (review.md:41, optimise.md:43) that describe the load as open rows.
  - Add one sentence to each: a promoted row names its claiming flow in `promoted_to`; do not re-raise it as a new finding unless that flow is at `review`/`complete` or does not exist — then point the user at `/backlog` re-triage.
- **Acceptance**: `grep -c "backlog list --live" claude/commands/review.md claude/commands/optimise.md` gives 1 per file (today: 0 each).

### 27. Cover the envelope enums in the capabilities parity test [S]
- **Files**: `tomlctl/src/capabilities.rs`, `tomlctl/src/flow/envelope.rs`, `tomlctl/src/flow/mod.rs`
- **Depends on**: 17
- **Action**: Add command and require_artifact branches to enum_values_match_value_enum_variants, comparing ENUM_VALUES against the envelope's VALID_COMMANDS / VALID_ARTIFACTS
- **Acceptance**: cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl capabilities passes

### 28. Make the duplicate-number rename hint a runnable command [S]
- **Files**: `tomlctl/src/tasks/import_plan.rs`
- **Depends on**: 14
- **Action**: Rewrite the name_duplicate_rows hint so tasks update/remove put the id first and the store target as a flag, matching the CLI and the two pinned tests
- **Acceptance**: cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::import_plan passes with no failures

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 2, 5, 6 — dependency closure: 1, 2, 5, 6. schema, transitions, `--live` and the target resolver; committed before task 7 so a failed 7 cannot restore `triage.rs`, `cli/types.rs` or `backlog/dispatch.rs` over finished work

— CHECKPOINT B after tasks 3, 4, 8 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8. backlog lifecycle complete (tasks 1-8) — compact, check verdict, validated triage and its CLI cases; crate builds and backlog tests pass

— CHECKPOINT C after tasks 10 — dependency closure: 10. Backlog bullet parse; committed before task 11 because both edit `import_plan.rs`

— CHECKPOINT D after tasks 11 — dependency closure: 9, 10, 11. side table and link import; committed before task 14, the third `import_plan.rs` editor

— CHECKPOINT E after tasks 15, 20 — dependency closure: 6, 9, 10, 11, 12, 13, 14, 15, 20. task↔item link (tasks 9-15) — render and show, findings wired into import and check — plus task 20, which documents the `backlog/*` classes the `finding_classes` parity test requires alongside task 13

— CHECKPOINT F after tasks 18 — dependency closure: 1, 2, 5, 6, 7, 9, 13, 16, 17, 18. `backlog reconcile` and the 0.10.0 release (tasks 16-18); the orchestrator runs `cargo install --path tomlctl` at this checkpoint, before any of tasks 19, 21-26 dispatches (carrier commands and skills are symlinked into `~/.claude`, so their edits go live in every session immediately)

— CHECKPOINT G after tasks 22, 23, 25, 26, 27, 28 — dependency closure: 1, 2, 5, 6, 7, 9, 10, 11, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28. skills, references and carrier wiring (tasks 19, 21-26); `command_lint` / `flag_table_lint` pass against the new flags

## Risks
- **Existing promoted rows become `dangling`/`orphaned` noise.** 47 rows here, most of them unlinked. Mitigation: the manual `reconcile --adopt` + `/backlog` migration step. `reconcile` reports a reason per row rather than failing.
- **Old `--promote --to` usages break under strict validation** (tests, scripts). Mitigation: task 7 updates the inline tests and task 8 adds the CLI cases; the docs tasks run `command_lint`; the error message names `--external` / `--allow-closed`.
- **Links written by `--adopt` are erased by the next import** because they are plan-owned. Mitigation: `reconcile` returns `render_needed`, `/backlog` runs `tasks render` for each slug (after a `--check` for pre-existing drift), and the reference documents the duty.
- **A seed flow becomes active-latest and hijacks resolution.** Mitigation: the bootstrap runs `flow init` with no `--scope`/`--branch` (resolution steps 2 and 5 read every non-complete flow on disk, registered or not), then `flow active remove --slug <slug>`; `/plan-new` Phase 9 re-registers it and writes scope and branch.
- **`/plan-new` seed adoption deletes the plan-mode file.** Mitigation: it deletes only after the content is written to the seed's `plan_path`, and only when `seed_slug` is bound.
- **Premature auto-resolve** (a task marked done that didn't fully fix the item). Mitigation: `refs` links never gate; `tasks show` puts the linked item in front of the implementer; `/implement` applies only after green verification; the resolution is recorded with its links, and `triage --reopen` reverses it.
- **Carrier and skill edits go live globally before the new binary exists.** They are symlinked into `~/.claude`, and the installed tomlctl is 0.9.0. Mitigation: every task that calls a new verb or flag (19, 21-26) depends on task 18, and the orchestrator installs 0.10.0 at checkpoint F; task 20's docs land earlier but name no new CLI surface.
- **File count is ~40, above the ~25 guidance.** The user chose one plan. Mitigation: seven milestone checkpoints, each a buildable increment, and ≤3 files per task except the inseparable release task 17.
- **`~/.claude/commands/backlog.md` is a stale copy.** Mitigation: the re-copy step under Verification Commands.
