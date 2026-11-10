Verdict: Mechanical: `files` from `file`/`target_file`/`instances` (tomlctl/src/review/fields.rs:538,593,600), a base premise and a forward accept from a cleanup's `old` (fields.rs:589, checked by `check_old` at tomlctl/src/review/add.rs:673), `outcome` and `action` text from `summary` + `failure_scenario`/`fix` or `edit` (fields.rs:545-560,581), cleanup `executor = "lite"` by decision; no source on the finding for a defect's `accept` or anchor premise, a defect's `executor`, `constraints`, `precedent`, `design`, `checkpoint`, or the task-to-finding link.

## Answer

The finding catalogue is `FIELDS` in tomlctl/src/review/fields.rs:469-694. A v2 task is a row (`TaskRow`, tomlctl/src/tasks/schema.rs:325-349) plus a spec (`TaskSpec`, schema.rs:238-248) written through `tasks add-many` (`ROW_FIELDS` and `SPEC_FIELDS`, tomlctl/src/tasks/add.rs:65-93). Field by field:

| Task field | Defect | Cleanup | Mechanical? |
|---|---|---|---|
| `files` | `file` only | `target_file` else `file`, plus `instances` (the existing `edited_paths`, fields.rs:1178) | Yes. For a defect it is a lower bound only: the `fix` prose may name a second file or a test, and nothing structured holds that |
| `premises` | `file` exists. Nothing pins the anchor: `line` drifts, `symbol` is free-form (`a::run`), and the computed `anchor` is an identity key, not content (review/matching.rs:24-31) | `old` present at base in the edit target. `old` is required for `change`/`remove` and for `add` into an existing file (fields.rs:589-592, 853-865) | Cleanup: yes. Defect: no |
| `accept` | None. `failure_scenario`, `counter` and `evidence` are prose, and no field carries a command | `remove`/`change`: `old` is gone, or occurs fewer times than at base. `add`: none, because `old` is the neighbouring anchor and stays, and the added text is only prose in `edit` | `remove`/`change` cleanup only |
| `outcome` | A template over `summary` + `failure_scenario` ("<scenario> no longer happens"). The decision's "the fix to `outcome`" (docs/ideas/v2-pipeline.md:167) puts a how into a what-must-be-true field | `summary` + `edit` | Yes, as a template |
| `constraints` | None | None | No source on the finding. Only a fixed constant per kind is possible, such as v1's behaviour-preservation and stay-in-files rules |
| `executor` | None decided. `effort`, `severity` (after the verdict override, `Store::severity`, review/store.rs:244), the file count and `route` are inputs a rule could use | `lite` (decision, v2-pipeline.md:168) | Cleanup: yes. Defect: needs a rule the plan decides |
| `title` | `summary` (≤160) | `summary` | Yes |
| `action`/`detail` | `fix` (≤800) | `edit`, `direction`, `old`, `target_file`, `instances`. `export-v1`'s `description` already composes this text (review/export.rs:469-503) | Yes |
| `effort` (S/M/L) | `effort` is a size scale (trivial…large). S/M/L names the dispatch tier (flow-contract-plan-output-format/SKILL.md:236-241) | same | Only through a mapping table the plan has to decide |
| `needs` | `depends_on` holds finding ids (fields.rs:654) | same | Yes, when the finding it names is also selected. Otherwise there is no task for the edge to point at |
| `means` | `fix` could go in as `binding = "suggested"` | none | Optional |
| `checkpoint`, `design`, `precedent`, `coupling`, `file_notes` | none | `file_notes` `(new)` only when `target_file` does not exist yet | No source, apart from that one `(new)` |
| finding↔task link | — | — | Neither store has a field. `backlog_links` takes backlog ids only (tasks/schema.rs:222-227), and the finding's companions have no task slot (fields.rs:137-149) |

What blocks a defect is `accept`. `plan/spec-no-accept` and `plan/accept-no-check` are errors (claude/skills/tomlctl/references/plan.md:558-559), so a defect task minted mechanically fails `plan check` unless one of three things supplies the check: a person or agent writes one, the reproduce-check verdict (milestone 3) counts as acceptance, or the class is waived for apply tasks.

## Findings

- **Library/API or file**: tomlctl/src/review/fields.rs:549-579 (defect fields), 581-603 (cleanup fields); tomlctl/src/tasks/add.rs:84-98 (`SPEC_FIELDS`, `EXECUTORS`)
- **Source**: read of both catalogues, 2026-11-10
- **Evidence-grade**: high
- **Finding**: A defect carries no structured field that can become an `accept` check or an anchor premise. A cleanup carries `old`, and a `remove`/`change` cleanup yields both mechanically.
- **Details**: Defect fields: `severity`, `failure_scenario` (≤600), `fix` (≤800), `fix_shape`, `effort`, `counter` (≤300), `grade`, `evidence` (≤5×300), all prose or enums. An `accept` entry needs a `check` that exits non-zero while the behaviour does not hold (tasks/schema.rs:251-262). `accept_entry` refuses a `check` that has no `kind` (add.rs:553-571). A premise needs `at = base | after:<id>` (add.rs:593-607). A premise holds on rc=0 (plan-probes.md:74).
- **Counter**: The finding would supply them if the verifier's verdict `citation` (fields.rs:688-690, a `path:line` or command-output string) were held to a runnable command. Today it is not: `PathForm::Citation` accepts any text containing whitespace (fields.rs:942).
- **Impact on plan / report**: Defect tasks need an authored or derived `accept`. The plan should decide whether the reproduce-check verdict stands in for it, or the minting verb leaves `accept` for the orchestrator. That choice decides whether apply tasks pass `plan check`.

- **Library/API or file**: tomlctl/src/review/add.rs:673-696 `check_old`; add.rs:378-386 `text_refusals`
- **Source**: read 2026-11-10
- **Evidence-grade**: high
- **Finding**: Build the cleanup premise and accept on `check_old`'s semantics through a tomlctl verb, not by emitting `grep -qF '<old>'`.
- **Details**: `old` is up to 400 characters (fields.rs:589-591) and may span lines. `grep -F` reads each line of a multi-line pattern as a separate pattern, so the check would pass on a partial match. `old` also needs shell quoting. `check_old` normalises CRLF and matches the whole text near `line`. It checks the `target_file` when that exists, else `file` (add.rs:381-383). Its only caller is `text_refusals` (add.rs:384). An accept of "old is gone" is unsatisfiable when `old` occurs more than once in the file, since `review add` checks only within the 20-line window (`OLD_WINDOW`, add.rs:36). A count below the count at base avoids that.
- **Counter**: Every stored `old` is one line with no quote characters. A sweep of existing `review-v2.toml` stores would show this; I did not run one.
- **Impact on plan / report**: A small verb (an anchor check on a finding id, with an exit code) gives probe-able premises and accepts and reuses `check_old`. It also meets the success criterion "a finding whose anchor no longer matches the code is caught before a worker is dispatched", for cleanups only.

- **Library/API or file**: tomlctl/src/review/fields.rs:600-603 `instances`
- **Source**: read 2026-11-10; claude/skills/review-v2-method/SKILL.md:77, references/finding-kinds.md:53-56
- **Evidence-grade**: high
- **Finding**: `instances` are bare paths with no per-site `old` or line, and `review add` checks `old` only at the edit target. A premise or accept per instance cannot be assumed mechanical.
- **Details**: The method's example uses the same `old` at every instance, but the schema does not require it. A per-instance premise run on base can fail spuriously. The run-milestone procedure then withholds the task to the replan (implement-v2-engine/references/run-milestone.md:129).
- **Counter**: The finder procedure requires `old` to occur verbatim at every instance. I found no such rule in review-v2-method.
- **Impact on plan / report**: Mint premises and accepts for the edit target only, and leave instances to `files` and the action text, or extend `check_old` to instances behind a stated rule.

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:93,106-107; implement-v2-engine/references/run-milestone.md:146
- **Source**: read 2026-11-10
- **Evidence-grade**: high
- **Finding**: The engine needs `executor` ∈ {lite, deep} and a non-empty `files` on every launched task. Every finding has a non-empty `file` (required, fields.rs:538-540), so `files` is always fillable. `executor` is fillable only for cleanups, by the decision.
- **Details**: The Workflow refuses the whole launch on a bad executor or empty files. The engine drops a row with no spec, or with a `tool`/`clerk` executor, to `needs_human`.
- **Counter**: The plan decides one rule for defects as well, for example `deep` for every defect. That is a decision, not a field on the finding.
- **Impact on plan / report**: The minting verb needs a defect-executor rule, chosen in the plan. Its inputs on the finding are `effort`, `severity` after the verdict override, the file count and `route`.

- **Library/API or file**: tomlctl/src/tasks/schema.rs:222-227 `BacklogLink`; tomlctl/src/review/fields.rs:137-149 `companions`, 230 `LINK_FIELDS`
- **Source**: read 2026-11-10
- **Evidence-grade**: high
- **Finding**: Neither store has a field linking a task to its findings, which success criterion 2 needs (one task, several findings).
- **Details**: `backlog_links` keys backlog ids by `ref`. The finding side has `exported_as`/`export_ledger` for v1 rows and `converted_to` for conversions, and none of these names a task. `depends_on` (fields.rs:654) maps to `needs` only after the fold assigns each finding to a task.
- **Counter**: Some field already carries a finding id on a task row. A search of `tasks/schema.rs` for `finding` found only the diagnostic type in `tasks/finding.rs`.
- **Impact on plan / report**: A new link field is needed on one side, or both. The task-home and export-v1-exit lenses own where it lives.

Coverage: Read in full: tomlctl/src/review/fields.rs:1-1450 (the catalogue and its matrix), tasks/schema.rs:20-360, tasks/add.rs:60-460 and 545-790, review/export.rs:300-580, review/add.rs:365-394 and 640-696, review/matching.rs:1-48, docs/ideas/v2-pipeline.md:29-218, docs/flows/v2-apply/brief.md. Checks run: `plan.md` check classes; `plan-probes.md` verdicts; the human-only-handovers worker view (`tomlctl plan view --for worker --task 1`) for the shape of a real spec; Workflow input validation. Ruled out: the computed `anchor` as a content premise (it is the identity string `family|file|symbol`); perf-only fields (the perf family has none of its own); triage as a source (it writes dispositions, not spec content, triage.rs). Outside this lens: selection at triage leaves no field on the finding, which belongs to the task-home and export-v1-exit lenses.
