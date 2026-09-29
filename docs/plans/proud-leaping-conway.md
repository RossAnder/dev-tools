# Plan: Tighten the house plan-document format

**Plan path**: `docs/plans/proud-leaping-conway.md`
**Created**: 2026-09-29

## Summary

This plan reshapes the house plan template around its three readers: the approver, the task importer and the implementing agent. It is itself written in the new shape. The approver gets a `## Summary`, a plan-level `## Success Criteria` and an `## After Merge` list, and the research appendix moves to the end. The `**Status**` header is dropped (it disagrees with the flow in 43 of 44 plans). Effort tags are redefined by what they drive, `S` meaning lite-eligible. `Files` gains a `(new)` / `(delete)` vocabulary that `tasks show` exposes, and `/implement`'s rollback uses it to remove a failed task's new files. The acceptance-probing method moves to a reference file. `/implement` gets a defined preamble and runs `success:` commands. Three task-store gaps are fixed along the way. **Review first**: the new section order (Approach, "Template"), the `tasks show --with files` output shape (Approach, "File notes in the store"), and the rollback change in task 12.

## Context

The critique of the house style (this session) made 12 recommendations, and this planning pass tested each against the corpus, the code and outside practice before adopting it (Exploration Notes, Research Notes). Three were corrected along the way:
- The approval content was less buried than claimed (Approach at a median 28% of the file).
- Effort does drive dispatch (`S` gates `implement-lite`).
- The heading-rename trap has never fired in this repo.

The measured problems:
- Status headers are wrong almost everywhere.
- Five of the eight newest plans don't fully verify their own stated goal.
- Post-merge steps are scattered across eight different sections.
- File annotations are invisible to implementers, and a failed task's new files survive rollback.
- 40% of the format skill is a probing method loaded on every Phase-7 read.
- The implementer preamble's "narrative sections" is undefined.

The same exploration also found two latent task-store bugs (a ref rename strands file notes and backlog links) and a stale `CLAUDE.md` claim about the `.github` mirrors.

## Scope
- **In scope**: the plan template and format rules; the acceptance-probing reference; `/plan-new`, `/review-plan`, `/implement`, `/plan-update` and the restructure contract; `tasks show --with files` output; `tasks update --ref` carry-over; a heading-anchor warning; a section-order regression test; the backlog seed stub; the `CLAUDE.md` mirror note.
- **Out of scope**: explicit task ids (User Decisions); a tomlctl lint for acceptance structure; reformatting existing plans; storing a change-kind field (it is derived from the note on read); `/tdd`'s mini-plan shape.
- **Affected areas**: `claude/skills/flow-contract-plan-output-format/`, `claude/skills/flow-contract-plan-restructure/SKILL.md`, `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/tomlctl/references/`, `claude/skills/backlog-capture/SKILL.md`, `claude/commands/plan-new.md`, `claude/commands/review-plan.md`, `claude/commands/implement.md`, `claude/commands/plan-update.md`, `tomlctl/src/tasks/`, `tomlctl/src/cli/types.rs`, `tomlctl/tests/`, `CLAUDE.md`

## User Decisions

1. **Split?** One plan, milestones, with checkpoints at the template / tomlctl / carrier boundaries. *Prompted by*: scope assessment, three independently shippable groups.
2. **Explicit task ids** → drop them; only warn when a heading carries a `{#…}` anchor. *Prompted by*: no post-import rename in any flow with a store, and `open_heading` silently dropping `{#x}` after the effort tag.
3. **Research placement** → add `## Summary` and move Exploration + Research Notes to an appendix after `## After Merge`. *Prompted by*: Approach at a median 28%, notes at 16% of bytes, and KEP/Google putting alternatives last.
4. **Structured Acceptance** → a template convention only (`forward:` / `falsifier:` / `guard:` sub-bullets with `today:` baselines), no parser change. *Prompted by*: sub-bullets already round-trip, only 2% use them, and no outside precedent.
5. **Files change-kind** → vocabulary, the rollback fix, and exposure in `tasks show --with files`. *Prompted by*: `(new)` on 18% of Files fields with nothing reading it, and `implement.md` step 6 lacking a `git clean`.
6. **Implementer preamble** → Summary, Context, Scope, User Decisions, Approach and Risks; never the notes. A task citing design text names its exact `###` Approach sub-heading. *Prompted by*: `implement.md:54,86` never defining "narrative sections", and 11% of tasks citing other sections.
7. **Success criteria** → a `## Success Criteria` section probed at plan time, with runnable ones repeated as `success:` keys that `/implement` Phase 3 runs after `test:`. *Prompted by*: 5 of 8 newest plans not verifying their goal, and the KEP "How will we know that this has succeeded?".

### Phase 5 outcome
Skipped: every answer concerns code already explored, with no new library or API to research.

## Approach

### Template
Section order, with the three rendered sections unchanged in the middle:

`# Plan:` header (`**Plan path**`, `**Created**` only) → `## Summary` → `## Context` → `## Scope` → `## User Decisions` → `## Approach` → `## Success Criteria` → `## Verification Commands` → `## Execution Policy` → `## Tasks` → `## Dependency Graph` → `## Risks` → `## After Merge` → `## Exploration Notes` → `## Research Notes`

- **Summary**: one paragraph covering the change, the key decisions, and what the approver should scrutinise, by section name. Written last in Phase 7 but placed first.
- **Success Criteria**: falsifiable checks of the Context's stated goal, labelled and probed like acceptance. Each runnable one is repeated as a `success:` key, written to exit non-zero when the criterion does not hold (`test "$(grep -c … file)" -eq 4`, `… | grep -q …`), because the `verification` agent judges a command by its exit code alone. Only `/implement` Phase 3 runs `success:` keys; the apply flows do not.
- **After Merge**: the steps someone must take once the work lands (install a binary, resync a cache, restart sessions, update memory), or the single line `_None._`.
- **Exploration Notes / Research Notes**: an appendix. `/plan-new` appends each at the file's end as its recovery checkpoint, so the approval content sits above them. `/plan-update reformat` moves both to `RESEARCH-NOTES.md`.
- **Header**: drop `**Status**`, because the flow's `context.toml` owns status (as KEP keeps it in `kep.yaml`).
- **Execution Policy**: stop authoring `- **Checkpoint after**:`. The markers are the source and `render` derives the bullet; `/plan-new` Phase 9 runs `tomlctl tasks render --slug <slug>` once after a clean import (task 10), so the derived bullet lands before `/review-plan` and `/implement` read the plan. An authored bullet is still compared (`checkpoint/marker-mismatch`).
- **Effort by dispatch meaning**:
  - `S` is lite-eligible: ≤2 files, fully specified, no cross-file refactor, not security-sensitive (the `implement.md` gate).
  - `M` is one concern for `implement-deep`, ≤3 files.
  - `L` is cross-cutting or 4+ inseparable files.
  - Drop the minute ranges.
- **Files vocabulary**: a note opening with `(new)` marks a file the task creates and `(delete)` one it removes (classification rule: "File notes in the store"). Anything else is an edit, including an addition to an existing file such as `` (new `Shape` enum) ``. The note follows the backticked path. Only a regular-file path takes `(new)`; a directory entry never counts as new.
- **Acceptance convention**: one sub-bullet per criterion, opening `forward:`, `falsifier:` or `guard:`, with `today: <value>` on a forward criterion. A single criterion may stay inline. `guard:` is the `(regression guard)` tag's new spelling: the verdict table and both probe sweeps (`/plan-new` Phase 7, `/review-plan` Step 2.6) accept either spelling, and a guard still needs a `forward:` or `falsifier:` beside it (tasks 1, 10, 11).
- **Citing design text**: a task relying on Approach names its exact `###` sub-heading in quotes. Implementers receive Approach in the preamble; the notes appendix is never sent.
- **Probing method**: the two-control rule's full method, the probe helper and the verdict table move to `references/acceptance-probing.md`. The Format rules keep a short statement of the rule and the link, so `/review-plan`'s `FORMAT CONTRACT` still carries the rule.

### File notes in the store
There is no new stored field. The kind is derived on read, and only from a note that opens with a parenthetical: that parenthetical's first word, case-insensitive, is `new` (→ new) or `delete`/`deleted` (→ delete), and the word either closes the parenthetical or is followed by `,`, `;`, `:` or a ` —` dash. `(NEW)`, `(new — generated)` and `(new, generated)` count as new. `(new thread)` and `` (new `Shape` enum …) `` describe an addition to an existing file and do not: the corpus holds 9 such notes against 293 genuine new-file notes. A `FileKind` helper goes in `tomlctl/src/tasks/schema.rs` beside `FileNote`, written with string operations rather than a regex (`every_pattern_compiles` covers only `parse_tasks.rs` and `parse_policy.rs`).

`tasks show --with files` then emits, directly after the unchanged `files` array and in this order:
- `file_notes`: an object mapping each annotated path to its note; `{}` when the row has none.
- `new_files` and `deleted_files`: path arrays in the row's `files` order, always present and possibly empty.

Implementers thereby see the plan's per-file notes for the first time.

### Rollback of created files
Today's step 6 runs `git restore -- <files>` over every declared file. `git restore` exits 1 on any path git does not track (an untracked new file, or a declared file never created) and then restores *nothing*. So whenever a task declares a `(new)` path, today's step leaves the whole task unrestored, tracked edits included (measured on git 2.55.0).

The new step decides from git state, never from note text alone:
1. **At dispatch**, and again whenever a cross-cut escalation widens the row's `files`, the orchestrator records which declared paths already exist on disk.
2. **On rollback**, it fetches `new_files` itself (`tomlctl tasks show <id> --slug <slug> --with files`). A missing key, from an installed `tomlctl` older than task 5, reads as an empty array.
3. **It stashes first.** `git stash push -u -m implement-rollback-<ref> -- <paths>` over the declared paths that exist now, passed as literal pathspecs (`git --literal-pathspecs`), existing paths only, because a missing path aborts the stash. It reports the stash ref.
4. **It restores the tracked set** with `git restore -- <tracked>`, where `<tracked>` is `git ls-files -- <files>`. The restore set comes from git, never from the note.
5. **It removes a declared path only if all of these hold**: the path is untracked now, was absent at dispatch, is a regular file, and is in `new_files` or reported `(new)` by the agent. Removal uses `git --literal-pathspecs clean -f -- <path>`, one path per call.

This follows steps 2–4 of the `flow-contract-apply-rollback-protocol` skill: stash first, and git evidence AND declaration, never agent text alone. A git-ignored new file survives `git clean -f`; the report names it rather than deleting it.

### Store fixes
- `tasks update --ref` re-keys the row's `[[file_notes]]` and `[[backlog_links]]` entries along with `import_overrides` (`update.rs:176-189`). Without that, a render before the next import drops both. This reverses a documented rule ("`tasks update --ref` leave[s] it alone"), so tasks 9 and 14 rewrite every sentence that states it.
- `parse_tasks` raises `plan/heading-anchor` (warning) once per numbered heading containing `{#`. A ref derives from the title, so the anchor is dropped after the effort tag or becomes part of the ref before it.

## Success Criteria

- forward: the template carries the four new sections — `grep -cE '^## (Summary|Success Criteria|After Merge|Exploration Notes)$' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `4` (today: `0`).
- forward: no status header — `grep -c '^\*\*Status\*\*' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `0` (today: `1`).
- forward: the probing method left the Phase-7 load — `grep -c 'p(){' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `0` (today: `1`).
- forward: implementers are told which sections they get — `grep -c 'Summary, Context, Scope, User Decisions, Approach' claude/commands/implement.md` prints at least `1` (today: `0`).
- forward: end to end, `cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 1 --slug proud-leaping-conway --with files` lists `claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md` under `new_files` (today: no `new_files` key) — predicted, unverified (needs the Phase-9 store).
- guard: this plan, written in the new order, imports under `tomlctl tasks import-plan --plan docs/plans/proud-leaping-conway.md --dry-run` with no error-class finding.

This run's `/implement` predates the `success:` key it adds (task 12), so run these by hand after Phase 3.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo test --manifest-path tomlctl/Cargo.toml -- --exact --test-threads=1 {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
checkpoint: cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl --no-fail-fast -- tasks:: cli::dispatch::tests
checkpoint: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast --test tasks_read --test tasks_render --test tasks_import
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: test "$(grep -cE '^## (Summary|Success Criteria|After Merge|Exploration Notes)$' claude/skills/flow-contract-plan-output-format/SKILL.md)" -eq 4
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 1 --slug proud-leaping-conway --with files | tr -d ' \r\n' | grep -qF '"new_files":["claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md"]'
```

Also run `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored` in the final pass. The pre-commit hook runs it whenever a `docs/plans/` file is staged, and this plan is one.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 2, 3, 4, 8, 9, 10, 11, 12, 13, 14
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Move the acceptance-probing method into a reference file [M]
- **Files**: `claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md` (new)
- **Depends on**: —
- **Action**: Create the reference file holding the probing method now in `claude/skills/flow-contract-plan-output-format/SKILL.md`. Move it verbatim; task 2 deletes the originals.
- **Detail**: Carry over these parts of the "Two-control rule" bullet and what follows it: the polarity paragraph, the negative and positive controls, the named-test falsifier paragraph, the traps list, the **Acceptance probe helper** with its quoting notes, and the verdict table. Add a short opening section on the acceptance sub-bullet convention (Approach, "Template": `forward:` / `falsifier:` / `guard:`, `today:`), and amend the moved verdict table's **vacuous** row so a `guard:` line counts as the `(regression guard)` tag. Open with a two-sentence purpose line and keep the file under the 600-line cap (`tomlctl/src/cli/dispatch/tests/skills.rs`). Name no finding class the code does not define.
- **Acceptance**:
  - forward: `cat claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md 2>/dev/null | grep -c 'p(){'` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests::skills` passes.

### 2. Rewrite the plan template and format rules [L]
- **Files**: `claude/skills/flow-contract-plan-output-format/SKILL.md`
- **Depends on**: 1
- **Action**: Rewrite the template and Format rules to the shape in Approach, "Template", and replace the moved probing text with a short statement of the two-control rule plus a relative link to `references/acceptance-probing.md`.
- **Detail**:
  - Header without `**Status**`, and the full section order with guidance for `## Summary`, `## Success Criteria`, `## After Merge`, `## Exploration Notes` and `## Research Notes` as the appendix.
  - A `success:` entry in the optional-keys list under `## Verification Commands`. State that it is judged by exit status alone, so each one is written to exit non-zero when its criterion fails (`test "$(…)" -eq N`, `… | grep -q …`), never as a bare count, and that only `/implement` Phase 3 runs it.
  - Keep the Files-line closure rule's `plan/files-malformed` sentence word for word.
  - The `Checkpoint after` bullet removed from the authored Execution Policy template, with a note that `render` derives it.
  - Effort redefined by dispatch meaning; the `(new)` / `(delete)` Files vocabulary, stating that `tasks show --with files` reports it as `new_files` / `deleted_files`.
  - The acceptance sub-bullet convention and the rule for citing an Approach sub-heading.
  - Keep `## Verification Commands` and "Affected areas" spelled as they are, since both are parsed.
  - Rewrite the frontmatter `description` to name the new sections while staying at or under 1024 characters (today 957), and keep the file under 500 lines.
- **Acceptance**:
  - forward: `grep -cE '^## (Summary|Success Criteria|After Merge|Exploration Notes)$' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `4` (today: `0`).
  - forward: `grep -c '^\*\*Status\*\*' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `0` (today: `1`).
  - forward: `grep -c 'p(){' claude/skills/flow-contract-plan-output-format/SKILL.md` prints `0` (today: `1`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes (description cap, line cap, link resolution, required-skill invocation).
  - falsifier: a description over 1024 characters fails the `skills.rs` description-cap test — predicted, unverified.

### 3. Drop the status line from the backlog seed stub [S]
- **Files**: `claude/skills/backlog-capture/SKILL.md`
- **Depends on**: —
- **Action**: Remove the `**Status**: Draft` line from the seed-flow stub example. Keep the `<!-- backlog-seed -->` marker line, which seed detection reads.
- **Acceptance**: forward: `grep -c '^\*\*Status\*\*' claude/skills/backlog-capture/SKILL.md` prints `0` (today: `1`).

### 4. Correct the mirror note in CLAUDE.md [S]
- **Files**: `CLAUDE.md`
- **Depends on**: —
- **Action**: In the Build & test bullet on the full suite, replace "tracked copies of `claude/` synced by hand" with the truth. `.github/agents` and `.github/skills/*` are git symlinks (mode 120000) into `claude/`, shown as junctions on disk, and `core.symlinks=false` makes git report them as deleted. Say to edit `claude/` only and never stage those deletions.
- **Acceptance**: forward: `grep -c 'synced by hand' CLAUDE.md` prints `0` (today: `1`).

### 5. Report file notes and change kinds from tasks show [M]
- **Files**: `tomlctl/src/tasks/schema.rs`, `tomlctl/src/tasks/show.rs`, `tomlctl/tests/tasks_read.rs` — extend the key list in `the_fetch_by_id_projection_carries_the_body_the_files_and_a_summary_per_dep`
- **Depends on**: —
- **Action**: Add a `FileKind { New, Delete }` helper derived from a note, and extend `show`'s `ShowPart::Files` output with `file_notes`, `new_files` and `deleted_files`, per Approach, "File notes in the store".
- **Detail**: Put the classifier beside `FileNote` in `tomlctl/src/tasks/schema.rs`, written with string operations, not a regex. It classifies only a note that opens with `(`, by the parenthetical's first word, case-insensitively (`new`; `delete` / `deleted`), and only when that word closes the parenthetical or is followed by `,`, `;`, `:` or ` —`. Read notes through `Store::file_note`. Insert the three keys directly after `files`, in the order `file_notes` (`{}` when empty), `new_files`, `deleted_files`; the last two keep the row's `files` order. `tomlctl/tests/tasks_read.rs` asserts the exact top-level key order of `show 3 --with body,files,deps` (serde_json `preserve_order`), so add the three keys to its expected list. Add a test `files_part_reports_notes_and_change_kinds` in `show.rs` covering `(new)`, `(NEW)`, `(new, generated)`, `(delete)`, a negative `(new thread)`, a plain ` — note` and an unannotated path.
- **Acceptance**:
  - forward: `grep -c 'new_files' tomlctl/src/tasks/show.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- tasks::show::` passes, including `files_part_reports_notes_and_change_kinds`.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_read` passes.
  - falsifier: an always-empty `new_files`, or `(new thread)` classified as new, fails that test — predicted, unverified.

### 6. Carry file notes and backlog links through a ref rename [S]
- **Files**: `tomlctl/src/tasks/update.rs`
- **Depends on**: —
- **Action**: Where `tasks update --ref` re-keys `import_overrides`, re-key the row's `store.file_notes` and `store.backlog_links` entries from the old ref to the new one as well.
- **Detail**: Add a test `a_ref_rename_carries_file_notes_and_backlog_links`: a row with a file note and a backlog link, renamed with `--ref`, keeps both under the new ref and leaves none under the old one.
- **Acceptance**:
  - forward: `grep -c 'file_notes' tomlctl/src/tasks/update.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- tasks::update::` passes, including the new test.
  - falsifier: deleting the new re-key lines fails it — predicted, unverified.

### 7. Warn on an id anchor in a task heading [M]
- **Files**: `tomlctl/src/tasks/parse_tasks.rs`, `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/tomlctl/references/tasks.md`
- **Depends on**: —
- **Action**: Raise a `plan/heading-anchor` WARNING finding once per numbered task heading whose text contains `{#`. Its `ids` name the task. Its `detail` gives the plan line and says a ref derives from the title, so the anchor is dropped (after the effort tag) or becomes part of the ref (before it). In the same change, add its row to both finding-class tables, next to `plan/heading-too-deep`.
- **Detail**: Follow `deep_heading`'s shape for building the finding, and push it in the `Some(task)` arm of the `open_heading` match in `parse_tasks_at` (numbered headings land there; the phase-label arm is the other one). Add a test `an_id_anchor_in_a_heading_warns` covering both placements and a heading without an anchor (no finding). The table rows ship with the code because `documented_finding_classes_match_the_source` (`tomlctl/src/cli/dispatch/tests/finding_classes.rs`) fails on an emitted class with no row, which would turn every sibling's `cli::dispatch::tests` guard red until the docs landed.
- **Acceptance**:
  - forward: `grep -c 'plan/heading-anchor' tomlctl/src/tasks/parse_tasks.rs` prints at least `1` (today: `0`).
  - forward: `grep -c 'plan/heading-anchor' claude/skills/flow-contract-task-store/SKILL.md claude/skills/tomlctl/references/tasks.md` prints `<path>:N` for each file with N ≥ 1 (today: both `:0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- tasks::parse_tasks::` passes, including the new test.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes (finding-class gate).

### 8. Pin the full section order through import and render [M]
- **Files**: `tomlctl/tests/tasks_render.rs`, `tomlctl/tests/fixtures/tasks/house-plan-sections.md` (new)
- **Depends on**: —
- **Action**: Add a fixture plan in the new section order (Approach, "Template"), with a non-empty body in every section, and an integration test `a_plan_in_the_full_section_order_renders_unchanged`. It imports the fixture into a sandbox store, renders, and asserts the rendered plan is byte-identical to the fixture and that the import envelope carries no `plan/text-unstored` finding.
- **Detail**: Stage with `stage_tasks_flow(&root, SLUG, PLAN_REL, None)` (no seeded store) and import then render as `a_files_annotation_survives_the_plan_to_plan_round_trip` in `tomlctl/tests/tasks_import.rs` does; the `stage()` helper in `tomlctl/tests/tasks_render.rs` seeds the house-plan store and must not be reused. Author the fixture's rendered sections exactly as `render` writes them — the house `—` markers with a `dependency closure:` clause — so the byte comparison holds.
- **Acceptance**:
  - forward: `grep -c 'fn a_plan_in_the_full_section_order' tomlctl/tests/tasks_render.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_render` passes.
  - falsifier: moving `## After Merge` above `## Risks` in the rendered output (a `reorder_owned_sections` change) fails the byte comparison — predicted, unverified.

### 9. Document the show fields and rename carry-over [M]
- **Files**: `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/tomlctl/references/tasks.md`, `claude/skills/tomlctl/references/tasks-write.md`
- **Depends on**: 5, 6, 7
- **Action**: Add `file_notes`, `new_files` and `deleted_files` to the `tasks show` output description in `claude/skills/tomlctl/references/tasks.md` and to §5 `show` in `claude/skills/flow-contract-task-store/SKILL.md`. Rewrite, rather than append to, every sentence task 6 falsifies, so each says `tasks update --ref` re-keys the stamp, file notes and backlog links:
  - `claude/skills/flow-contract-task-store/SKILL.md`, the §1 **Backlog links** paragraph ("`tasks remove` and `tasks update --ref` leave it alone"), narrowed to `tasks remove`.
  - `claude/skills/flow-contract-task-store/SKILL.md`, §5a ("`--ref` carries the stamp with the row"), plus the side-table sentence "`tasks show` reports the first and the third".
  - `claude/skills/tomlctl/references/tasks.md`, the `backlog/*` paragraph ("`tasks remove` and `tasks update --ref` leave links in place until the next import").
  - `claude/skills/tomlctl/references/tasks-write.md`, the `--unlock-import-fields` section ("`--ref` carries the stamp with the row").
- **Acceptance**:
  - forward: `grep -c 'new_files' claude/skills/tomlctl/references/tasks.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'update --ref. leave' claude/skills/flow-contract-task-store/SKILL.md claude/skills/tomlctl/references/tasks.md` prints `<path>:0` for each file (today: `SKILL.md:1`, `tasks.md:1`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes (finding-class and flag-table gates).

### 10. Teach /plan-new the new sections [M]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 2
- **Action**: Update the phases to the new template.
- **Detail**:
  - Phase 2 appends `## Exploration Notes` at the file's end. Phase 3 appends `## Research Notes` after it.
  - Phase 4 inserts `## User Decisions` above `## Exploration Notes`, not at the file's end.
  - Phase 6 step 5 stops recording a `Checkpoint after` list. It drafts `## Success Criteria` from the Context's goal and `## After Merge`.
  - Phase 7 lists the new section order (`:109`), writes the document in that order with both notes sections kept verbatim at the end, and writes `## Summary` last but places it first. The acceptance probe sweep also covers `## Success Criteria` (whose `success:` lines must exit non-zero on failure), points at the skill's `references/acceptance-probing.md`, and accepts `guard:` as the `(regression guard)` tag.
  - Phase 9 runs `tomlctl tasks render --slug <slug>` once after a clean import, so the derived `Checkpoint after` bullet and marker closures land before review.
  - Leave the Phase 9 "Affected areas" derivation, and the "the `flow-contract-plan-output-format` skill" invocation phrase that `carrier_invokes_required_skills` requires, intact.
- **Acceptance**:
  - forward: `grep -c 'Success Criteria' claude/commands/plan-new.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'references/acceptance-probing.md' claude/commands/plan-new.md` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes.

### 11. Teach /review-plan the new sections [M]
- **Files**: `claude/commands/review-plan.md`
- **Depends on**: 2
- **Action**: Point Step 2.6 check 4 at `references/acceptance-probing.md` for the probe helper, extend that sweep to `## Success Criteria`, and let the **vacuous** verdict's exemption accept a `guard:` line as well as the `(regression guard)` tag. In Agent 3's lens, add two checks: a house-format plan carries `## Summary` and `## Success Criteria`, and the criteria verify the goal the Context states. A plan that still carries a `**Status**` header line predates this format, and the absence is judged minor for it; write that test into the lens text.
- **Detail**: The `FORMAT CONTRACT` fence keeps embedding the Format rules block, which now carries the short two-control rule. Keep house-format detection (`## Tasks` / `## Dependency Graph`) and the skill-invocation phrase unchanged.
- **Acceptance**:
  - forward: `grep -c 'references/acceptance-probing.md' claude/commands/review-plan.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'Success Criteria' claude/commands/review-plan.md` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes.

### 12. Define the implementer preamble and run success and rollback steps in /implement [L]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 2, 5
- **Action**: Four changes:
  - Define "the plan's narrative sections" (`:54`, `:86`) as Summary, Context, Scope, User Decisions, Approach and Risks, excluding the notes appendix.
  - In Phase 3, run the plan's `success:` commands after `test:` and `e2e:` in the verification list.
  - In step 6, roll back per Approach, "Rollback of created files".
  - In the Phase 4 report, list the plan's `## After Merge` steps verbatim under their own heading.
- **Detail**: Step 6 fetches `new_files` itself with `tomlctl tasks show <id> --slug <slug> --with files`, because the dispatch-time fetch is the sub-agent's and its output never reaches the orchestrator. It reads a missing key, from an installed `tomlctl` older than task 5, as an empty array. The dispatch step, and the cross-cut widening path, record which declared paths exist before the agent runs, which the rollback's removal test needs. A plan with no `success:` key or no `## After Merge` section runs as before. Keep the lite-eligibility gate's wording consistent with the effort definitions in the skill.
- **Acceptance**:
  - forward: `grep -c 'Summary, Context, Scope, User Decisions, Approach' claude/commands/implement.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'literal-pathspecs' claude/commands/implement.md` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes (`command_lint` scans `implement.md`).
  - forward: `grep -c 'new_files' claude/commands/implement.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'success:' claude/commands/implement.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'After Merge' claude/commands/implement.md` prints at least `1` (today: `0`).

### 13. Carry the new sections through plan restructuring [M]
- **Files**: `claude/commands/plan-update.md`, `claude/skills/flow-contract-plan-restructure/SKILL.md`
- **Depends on**: 2
- **Action**: Two changes:
  - In the restructure contract, the outline keeps `## Summary`, `## Success Criteria` and `## After Merge`, while `## Exploration Notes` moves to `RESEARCH-NOTES.md` alongside the research findings, under its own topic heading.
  - In `claude/commands/plan-update.md` Step 3 item 3, scope the "Last updated" refresh to `RESEARCH-NOTES.md`, the one document whose format carries that line.
  - In the same file's reformat Agent 1 inventory, add `## Summary`, `## Success Criteria`, `## After Merge` and `## Exploration Notes` to the enumerated content classes.
- **Acceptance**:
  - forward: `grep -c 'Exploration Notes' claude/skills/flow-contract-plan-restructure/SKILL.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'After Merge' claude/skills/flow-contract-plan-restructure/SKILL.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'Last updated.*RESEARCH-NOTES' claude/commands/plan-update.md` prints `1` (today: `0`).

### 14. Correct the remaining rename and show-field references [M]
- **Files**: `claude/skills/tomlctl/references/tasks-store.md`, `claude/skills/tomlctl/references/backlog-reconcile.md`, `tomlctl/src/cli/types.rs`
- **Depends on**: 5, 6
- **Action**: Rewrite the sentences tasks 5 and 6 falsify outside the two files task 9 owns:
  - In `claude/skills/tomlctl/references/tasks-store.md`, narrow the side-table passage that says `tasks update --ref` leaves the tables alone to `tasks remove`.
  - In `claude/skills/tomlctl/references/backlog-reconcile.md`, the dangling-link sentence ("left behind by `tasks remove` or `tasks update --ref`") becomes `tasks remove` only.
  - In `tomlctl/src/cli/types.rs`, the `ShowPart::Files` doc comment ("The row's declared `files` list on its own") names the `file_notes`, `new_files` and `deleted_files` keys it now carries.
- **Acceptance**:
  - forward: `grep -c 'tasks update --ref' claude/skills/tomlctl/references/backlog-reconcile.md` prints `0` (today: `1`).
  - forward: `grep -c 'new_files' tomlctl/src/cli/types.rs` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes (flag-table gates read the clap help text).

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 2, 3, 4 — dependency closure: 1, 2, 3, 4. The template, its probing reference, and the two prose fixes: one coherent documentation increment.

— CHECKPOINT B after tasks 8, 9, 14 — dependency closure: 5, 6, 7, 8, 9, 14. The tomlctl changes with their tests and reference docs: a buildable increment the pre-commit gates can check on their own.

— CHECKPOINT C after tasks 10, 11, 12, 13 — dependency closure: 1, 2, 5, 10, 11, 12, 13. The four carriers adopting the new template and store fields.

## Risks

- **Uncommitted in-flight work shares this plan's files.** The `plan/files-malformed` change is uncommitted. It spans `tomlctl/src/tasks/parse_tasks.rs`, `tomlctl/src/tasks/parse_policy.rs`, `tomlctl/tests/tasks_corpus.rs` and one line in each of the three skill docs that tasks 2, 7 and 9 edit. Left as it is:
  - Per-task staging would split it across checkpoints A and B.
  - The two files no task claims would halt the commit train.
  - Step 6's `git restore` would discard it.

  Mitigation: commit it as its own change before `/implement` starts. Task 2 keeps its Files-line `plan/files-malformed` sentence, and the `.github/*` symlink deletions are never staged.
- **A rollback deletes a file the task did not create.** `git clean` is unrecoverable. Mitigation: task 12 follows Approach, "Rollback of created files" (stash first, remove only paths absent at dispatch, literal pathspecs), and the pre-change `implement.md` this run executes under still has no `git clean` at all.
- **Carrier prose drifts from the template.** Four carriers restate parts of it. Mitigation: tasks 10–13 depend on task 2, so they are written against the finished template, and each names the Approach sub-heading it implements.
- **The skill description overflows its 1024-character cap** (957 today) once the new sections are named. Mitigation: task 2 rewrites it rather than appending, and the `cli::dispatch::tests` guard fails on overflow.
- **`/review-plan` flags every legacy plan for a missing Summary or Success Criteria.** Mitigation: task 11 judges the absence minor for a plan predating this format.
- **This run's `/implement` uses the old instructions.** It loads `implement.md` at start, so this run neither runs `success:` keys nor cleans new files on rollback. Mitigation: run the Success Criteria by hand (After Merge).
- **A new-file classifier that is too loose.** A note such as `(newline handling)` would match a prefix test. Mitigation: task 5 classifies the parenthetical's first *word*, and its test pins the accepted spellings.

## After Merge

- `cargo install --path tomlctl`: the carriers call the installed binary, which needs the new `tasks show` fields and the anchor warning.
- Restart open Claude Code sessions so the updated skills and commands load.
- Update the memory note `acceptance_two_control_probe.md` and its `MEMORY.md` line to say the probe helper now lives in `claude/skills/flow-contract-plan-output-format/references/acceptance-probing.md`.
- Run the `## Success Criteria` checks by hand, including `tasks show 1 --slug proud-leaping-conway --with files` against the installed binary.

## Exploration Notes

**Template and carriers.** The format lives only in `claude/skills/flow-contract-plan-output-format/SKILL.md`: 226 lines, no `references/` directory, and a description of 957 characters against the 1024 cap in `tomlctl/src/cli/dispatch/tests/skills.rs` (`:799`). Other gates on skills: 500 lines per SKILL.md (`:654`), 600 per `references/*.md` (`:704`), every relative link must resolve (`:1024`), and `carrier_invokes_required_skills` (`:1193`) needs `plan-new.md` and `review-plan.md` to invoke the skill. `finding_classes.rs:22-26` checks finding-class names in the prose files it lists in `PROSE`.
- `claude/commands/plan-new.md`: Phase 2 writes `## Exploration Notes` with a `**Backlog**` sub-heading (`:60-62`); Phase 3 appends `## Research Notes` (`:72`); Phase 5 writes `### Phase 5 outcome` and `### Directed research additions` (`:86-88`); Phase 7 lists the section order (`:109`) and runs the probe sweep (`:113`); Phase 9 derives `scope` from the **Affected areas** field (`:141-144`).
- `claude/commands/implement.md`: "the plan's narrative sections" go verbatim into every Phase-2 agent's preamble (`:54`, `:86`), but "narrative" is never defined. The per-task body comes from `tomlctl tasks show <id> --with body,files,deps` (`:92-100`). Step 6 rollback runs `git restore -- <files>` on the failed task's declared files and has no scoped `git clean`, so a failed task's new untracked files survive (`:148`). Effort drives tier: `S` is lite-eligible, `M`/`L`/untagged go deep (`:104`).
- `claude/commands/review-plan.md`: detects the house format from `## Tasks` + `## Dependency Graph` (`:69`); Agent 3 gets the **Format rules** block verbatim as `FORMAT CONTRACT` (`:69`, `:89`); Step 2.6 runs the probe helper (`:105`).
- `claude/commands/plan-update.md:201` refreshes a "Last updated" outline line the template does not have. `claude/skills/flow-contract-plan-restructure/SKILL.md:77` needs User Decisions "adjacent to `## Approach`".
- `**Status**:` is read and written by no command; `/plan-update status|complete` write `context.toml` only. It appears in the template (`SKILL.md:38`) and the backlog seed stub (`claude/skills/backlog-capture/SKILL.md:238`).
- `.github/skills/*` and `.github/agents` are git symlinks (mode 120000) that show as junctions on disk; `core.symlinks=false` makes them read as deleted. Editing `claude/skills/**` is enough. `CLAUDE.md:57` wrongly calls them "tracked copies synced by hand".

**tomlctl task store** (`tomlctl/src/tasks/`).
- File notes are stored verbatim in `[[file_notes]]` and nothing interprets them; `show`/`snapshot`/`list` do not emit them. `tasks update --ref` moves `import_overrides` but not `file_notes` (`update.rs:176-189`), so a render before the next import drops that row's notes — a latent bug.
- Acceptance is `join_prose` text; nested sub-bullets already survive import → render → import (`render.rs::nested_lists_survive_import_and_render`). Nothing interprets `today:`, `Falsifier`, `predicted, unverified` or `(regression guard)`.
- `ref` = `dedupe_refs(derive_ref(title))`. `### 4. Title [L] {#x}` drops the anchor silently; `{#x}` before the tag becomes part of the title and ref.
- `- **Checkpoint after**:` is parsed, compared by `marker_mismatch` (`import_plan.rs:887`, silent when absent), never stored, and always rendered from the groups.
- `render.rs::reorder_owned_sections` (`:452`) leaves unowned sections in place; the fixture tests pin `## Risks` as the first heading after `## Dependency Graph` (`tests/tasks_render.rs:43`, `tests/tasks_import.rs:656`).

**Corpus** (52 plans in `docs/plans/`, 44 with a flow).
- `## Approach` starts at a median 28% of the file (range 13–58%); Exploration + Research Notes are 16% of bytes, Tasks 42%, Approach 15%. Median plan 35.6 KB, largest 130 KB.
- `## Exploration Notes` appears in 18 plans, unlisted in the template; section order breaks in 5 plans.
- `**Status**` disagrees with the flow's status in 43 of 44 flowed plans (38 say draft; 33 of those have every task complete).
- Of the 8 most recent plans, 3 verify their stated goal, 3 partly, 2 not at all (e.g. `woolly-greeting-rainbow.md:9` states 411 → 307 ms; only prose at `:123` checks time).
- 74 of 656 tasks (11%) cite Approach, Research Notes, Exploration Notes or User Decisions, some by sub-heading (`whimsical-hugging-puppy.md` "§Store", `woolly-greeting-rainbow.md` "Approach, **Owner check**").
- `(new)` tags 114 of 646 Files fields; `(delete)` 5, `(rewrite)` 6, `(modify)` 4; `Created:` never.
- Acceptance: 82% single-line, 16% wrapped prose, 2% sub-bulleted; median 221 chars. `today:`/`Falsifier`/`predicted, unverified` are almost all from September 2026 plans.
- Post-merge steps (`cargo install --path` ×28, plugin-cache resync, session restart) are scattered across 8 different sections.
- No heading was renamed after import in any flow with a store; unmatched execution-record refs are tasks minted mid-implementation.
- `## Execution Policy` is in only 8 plans.

**Backlog**: no live rows under `claude/skills`, `claude/commands`, `claude/agents`, `tomlctl/src/tasks` or `docs/plans`.

## Research Notes

**Design-doc conventions** (research-lite; vetted 3 of 9, none dropped).
- A short summary comes first in the Rust RFC template (`rust-lang/rfcs` `0000-template.md`, one paragraph), the KEP template (`kubernetes/enhancements` `keps/NNNN-kep-template/README.md`, Summary before Motivation) and PEP 1 (~200-word Abstract after the preamble). Supports `## Summary`.
- KEP keeps Status in `kep.yaml` for tooling, not in the prose. The Rust RFC template has no status field. PEP 1 keeps one but gives it an owner and freezes the document at resolution ("a historical document rather than a living specification"). Our flow's `context.toml` is the `kep.yaml` analogue, so the plan header should not duplicate status.
- The KEP Goals guidance asks "How will we know that this has succeeded?". Its Production Readiness questionnaire and Upgrade/Downgrade section are the precedent for post-merge steps. A separate Success Criteria section is a minority practice (RiskLedger, via the Pragmatic Engineer survey); most templates put success checks inside Goals.
- KEP puts Drawbacks and Alternatives near the end, and Google design docs put "Alternatives considered" after Design (industrialempathy.com, 2020-07-06, medium grade).
- Searched: KEP template and process doc, Rust RFC template, PEP 1, Oxide RFD 1, industrialempathy.com, Squarespace blog, Pragmatic Engineer survey — fetched 2026-09-29.

**Agent task-spec formats** (research-lite; vetted 3 of 9, none dropped).
- spec-kit `templates/commands/tasks.md`: one line per task, `- [ ] T001 [P] [US1] Description with file path`, sequential ids, with the rule "each task must be specific enough that an LLM can complete it without additional context". Its `implement` command still passes plan.md, the data model and research as shared context. So tasks being self-contained and a shared design preamble go together.
- Kiro specs: EARS acceptance (`WHEN … THE SYSTEM SHALL …`), tasks numbered `1`, `1.1`, traced back with `_Requirements: 1.1_`. Its "Sync Files" rebuilds tasks when requirements change, with no documented identity preservation.
- Anthropic, "Effective harnesses for long-running agents" (2025-11-26): feature lists are JSON with `"passes": false`, because "the model is less likely to inappropriately change or overwrite JSON files compared to Markdown files". Each session runs a basic test first, which is a runtime baseline.
- Claude Code sub-agents docs: a non-fork sub-agent starts with a fresh context, so any design section a task depends on has to be in the dispatch.
- No format surveyed marks files as new or deleted, or records a baseline or falsifier on acceptance. Our `(new)` marker and `today:`/falsifier conventions have no precedent to copy.
- Searched: spec-kit tasks-template, commands/tasks and commands/implement; kiro.dev specs pages; Anthropic engineering posts (2025-06-13, 2025-11-26); code.claude.com sub-agents and best-practices; agents.md — fetched 2026-09-29. Not covered: Cline, Cursor, Aider.
