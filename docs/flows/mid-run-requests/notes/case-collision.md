Verdict: yes — a created file is one whose `[[file_notes]]` note reads `FileKind::New` (tomlctl/src/tasks/schema.rs:222), not `authored_files` or a spec field, and nothing checks existence at HEAD; the warning belongs as a new `plan/file-case-collision` warning built in tomlctl/src/plan/check.rs `findings()` (line 148) outside the before-approval block at line 188, documented in claude/skills/tomlctl/references/plan.md and tested in tomlctl/tests/plan_check.rs.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = []

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 222
claim = "FileKind::of_note reads a (new)/(create)/(created) note as a new file and (delete)/(remove) as a deletion; anything else is an edit"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 574
claim = "Store::file_note looks up a row's note on one path, keyed by ref and file"

[[source]]
path = "tomlctl/src/tasks/show.rs"
line = 212
claim = "tasks show --with files derives new_files from file_notes through FileKind::of_note"

[[source]]
path = "tomlctl/src/tasks/show.rs"
line = 91
claim = "tasks show --with absent tests existence with root.join(file).exists() against the working tree, not HEAD"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 723
claim = "the worker view labels each file new, delete or edit from the same note reading"

[[source]]
path = "tomlctl/src/review/tier.rs"
line = 408
claim = "review apply already combines working-tree existence with a declared (new) note to decide a path is a planned new file"

[[source]]
path = "tomlctl/src/plan/authored.rs"
line = 210
claim = "authored_files lists the flow's own brief, design and note documents under docs/flows/<slug>/, not the files tasks create"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 148
claim = "findings() assembles every plan check class; the before-approval-only classes sit in the block at line 188"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 5
claim = "module header lists the warning and info classes; every other class is an error"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 518
claim = "the only existence test in plan check is bind_known's working-tree root.join(..).exists()"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 653
claim = "a warning class is built as a Finding literal with WARNING; errors go through the error helper"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 452
claim = "documented_plan_classes_match_the_source holds the plan.md class table equal to the classes src/plan emits"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 570
claim = "the plan check class table, where warnings and info classes sit"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 244
claim = "plan stage blocks prove only on error classes and plan/probe-absent, so a new warning needs no remedy entry"

[[source]]
path = "tomlctl/tests/plan_check.rs"
line = 184
claim = "assert_reports_warning and the seeded Flow fixture with a writable root are what a new warning case uses"

[[source]]
path = "claude/skills/flow-contract-task-store/SKILL.md"
line = 249
claim = "the contract states how a note marks a created or removed file"

[[source]]
path = "docs/flows/mid-run-requests/brief.md"
line = 40
claim = "success criterion: plan check warns in every project on a created file whose stem differs only by case from an existing or planned file in the same directory"
```

**Answer.** The store already says which files a task creates: a row's `[[file_notes]]` entry whose note opens `(new)`, `(create)` or `(created)` (any case, with `,` `;` `:` `—` allowed after the word) is a new file by `FileKind::of_note`. `tasks show --with files`, the worker view and `review apply`'s tiering all read it that way. The `authored_files` feature is not the right tool: it lists the flow's brief, design and note documents. No spec field marks a file as created. Nothing compares against HEAD; the only existence tests (`bind_known`, `tasks show --with absent`, `review` tier) use `root.join(p).exists()` on the working tree. The real collision would have been caught by notes alone. In the package-design-tools store the original row carried `(new)` on `ColourField.tsx`, `colourField.ts` and their tests, all in one row and one directory. The audited `file_notes` change that renamed the module records this.

**Where it sits.** Add a `case_collision_findings(&tasks, &root)` to `plan/check.rs` and call it from `findings()` beside `ui_scope_findings`, outside the `plan.approvals.is_empty()` block. Rows added after approval (replans, `/apply-v2` fix rows, granted widenings) can create files too, and a warning never moves the exit code. Build it as a `Finding` literal with `WARNING`, `ids` naming the creating row and any planned row it collides with, so `--milestone` scoping keeps it. Then:
- add the class to the module header (`check.rs:5-8`) and add a row to `plan.md`'s class table; `documented_plan_classes_match_the_source` fails otherwise;
- add no `stage.rs` remedy, because `plan stage` blocks prove only on errors and `plan/probe-absent`. An unknown class falls back to `plan check`;
- add cases to `tomlctl/tests/plan_check.rs` with `assert_reports_warning`. The base fixture's `src/a.rs` is absent and has no sibling, so the clean case stays clean.

**Design points the planner must settle.**
- Stem rule. `Path::file_stem` turns `colourField.test.ts` into `colourField.test`. That still catches the real case through `ColourField.tsx` against `colourField.ts`, but it misses a case-variant test file whose sibling has no non-test form. Comparing stems up to the first dot is the other choice.
- Case-folding filesystems. On Windows and default macOS, `exists()` folds case, so `ColourField.ts` reads as existing when only `colourField.ts` is on disk. List the parent directory with `read_dir`, or use `git ls-files` if tracked-only is wanted, and compare exact names. Never probe with `exists()`.
- Exclusions. Skip a `files` entry that is a directory or ends in `/`, compare against other rows' planned paths through `normal_path`, and do not count a sibling that a row marks `(delete)` as existing.
- Unannotated new files. Should an absent, unannotated path also count as created? The plan-v2 Write step has no rule telling authors to annotate new files, though v2 stores do so in practice. Counting absence too would catch more, but a typo would then read as a creation.

- **Library/API or file**: tomlctl/src/tasks/schema.rs:222 `FileKind::of_note`
- **Source**: tomlctl/src/tasks/schema.rs:211-236; tomlctl/src/tasks/show.rs:212-229; tomlctl/src/plan/view.rs:723
- **Evidence-grade**: high
- **Finding**: Created files are already decidable from `[[file_notes]]` `(new)` notes. The check should reuse `FileKind::of_note` and `Store::file_note` and add no new spec field.
- **Details**: `Store::file_note(ref, file)` (schema.rs:574) returns the note, and `of_note` classifies it. `tasks add-many` and `update --ndjson` accept `file_notes` (tasks/add.rs:81), so v2 rows carry them, and every v2 flow store in `.claude/flows/` has `[[file_notes]]` entries.
- **Counter**: A plan whose author left new files unannotated would make the note-only check silent. The plan-v2 engine has no rule requiring the note (`rg -i "new file|file_notes" claude/skills/plan-v2-engine` finds none).
- **Impact on plan / report**: Key "created" on `FileKind::New`, and decide whether to also count absent unannotated paths.

- **Library/API or file**: tomlctl/src/plan/check.rs:148 `findings`
- **Source**: tomlctl/src/plan/check.rs:148-195, 5-8, 653-661; tomlctl/src/cli/dispatch/tests/finding_classes.rs:452
- **Evidence-grade**: high
- **Finding**: The new class goes in as a warning `Finding` literal in `plan/check.rs`, called outside the before-approval block at line 188, with a `plan.md` table row and a header-comment entry to satisfy the parity gate.
- **Details**: `file_chain_findings`, `milestone_width_findings` and `milestone_order_findings` run only while `plan.approvals` is empty. A case-collision warning should run always, because post-approval rows also create files. `the_plan_scan_reads_both_construction_sites_and_no_tasks_class` shows that a `Finding { class, severity: WARNING }` literal is a shape the gate parses.
- **Counter**: If the planner wants the warning only before approval, it belongs inside the block at check.rs:188 instead. Nothing in the code forces either placement.
- **Impact on plan / report**: Declare `tomlctl/src/plan/check.rs`, `claude/skills/tomlctl/references/plan.md` and `tomlctl/tests/plan_check.rs`. `stage.rs` needs no change.

- **Library/API or file**: tomlctl/src/plan/check.rs:518 `bind_known`; tomlctl/src/tasks/show.rs:91
- **Source**: both call `root.join(path).exists()`; tomlctl/src/review/tier.rs:408
- **Evidence-grade**: medium (a filesystem-semantics inference, not run on this machine)
- **Finding**: Existing-file detection in tomlctl is working-tree `exists()`, which folds case on Windows and macOS. The collision check must list the parent directory and compare exact names.
- **Details**: HEAD is never consulted for file existence. `plan/drift.rs` and `plan/approve.rs` shell out to git only for logs, status and rev-parse. Use `fs::read_dir(parent)` names, or `git ls-files -- <dir>` for tracked-only, and fold with `to_lowercase` for the comparison.
- **Counter**: On a case-sensitive filesystem (Linux) `exists()` would behave, so the risk is platform-specific. A test creating `colourField.ts` and planning `ColourField.ts` on Windows would settle it.
- **Impact on plan / report**: The task's accept should include a same-name, case-only-variant case as well as the different-extension case.

Coverage: read in full tomlctl/src/plan/authored.rs. Read in part: plan/check.rs (findings, spec, bind and header), tasks/schema.rs (FileNote, FileKind, file_note, normal_path), tasks/show.rs (files and absent parts), plan/view.rs file_lines, review/tier.rs new-file test, plan/stage.rs remedy and prove, cli/dispatch/tests/finding_classes.rs, plan.md's class table, plan_check.rs fixture and helpers, and the brief. Checks run: `rg` for authored_files, file_notes, FileKind, exists and git calls in src/plan; the `[[file_notes]]` count across `.claude/flows/*/tasks.toml`; the package-design-tools store's audited `file_notes` change. Ruled out: `authored_files` (lists flow documents, not task outputs); a spec field (none exists); a HEAD check (no code path reads HEAD trees); a `stage.rs` remedy (warnings do not block prove).
