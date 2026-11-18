Verdict: Settle needs an `open` (verified) code/perf defect or cleanup with `blocked_on = "evidence"`, `blocked_missing` and no `proposal` (schedule.rs:409-435), attributed to a cell whose concerns hold its `lens` (add.rs:403); an escalation can reach that by reusing `--convert`'s mint-into-the-cell path keyed on the escalating cell (triage.rs:1069), and a multi-file cleanup already fits the cleanup row (one `old` at the anchor plus `instances`/`fix_files`); it is refuted only because the checker, verifier and audit bound the edit to `file`/`target_file`/`instances` and never `fix_files` (review-checker.md:31, review-verifier.md:41, fields.rs:550).

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 409
claim = "blocked() admits only an open code/perf defect or cleanup; settle_refusal gives a settle task only to an evidence block with no proposal and a settle checkpoint"

[[source]]
path = "tomlctl/src/review/add.rs"
line = 403
claim = "review add refuses a lens the cell does not cover, except a cleanup carrying old under a mechanical lens"

[[source]]
path = "tomlctl/src/review/add.rs"
line = 283
claim = "review add refuses a sealed cell"

[[source]]
path = "tomlctl/src/review/add.rs"
line = 731
claim = "check_old: old must occur in the edit target within OLD_WINDOW lines of line; instances and fix_files are not checked"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 672
claim = "old is needed for change/remove cleanup (Need::OldText); fix_files is allowed on defect and cleanup, max 8"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 550
claim = "FIX_FILES visibility: the checker column is N, so a checker never sees fix_files"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1417
claim = "route() reads file, target_file and instances via edited_paths, not fix_files"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 694
claim = "blocked_on with its required blocked_* detail fields"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 969
claim = "replacement_row defaults lens/file/line/symbol from the advisory and checks lens against the advisory's cell through add::field_refusals"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 1048
claim = "finish_row mints the replacement into the advisory's cell (sealed) and the open run, status unverified"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 917
claim = "target_run: the replacement needs an open run of the advisory's family"

[[source]]
path = "claude/workflows/review-v2.js"
line = 367
claim = "escalations exist only in the Workflow index as {cell, text}, text cut to 300 characters"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 173
claim = "three escalation dispositions: follow-up cell, mechanical row for the next cell, close-summary item"

[[source]]
path = "claude/skills/review-v2-engine/references/triage-and-close.md"
line = 140
claim = "Convert section lists cleanup fields as edit with direction, old, effort, target_file only; lens must be a concern of the advisory's cell"

[[source]]
path = "claude/skills/review-v2-engine/references/triage-and-close.md"
line = 35
claim = "audit overturns a checked cleanup whose edit leaves file, target_file and instances"

[[source]]
path = "claude/skills/review-v2-engine/references/triage-and-close.md"
line = 201
claim = "unhandled escalations are only offered as backlog captures; the rest drop with the run"

[[source]]
path = "claude/agents/review-checker.md"
line = 31
claim = "checker predicate 4: every path the edit names is file, target_file or instances"

[[source]]
path = "claude/agents/review-verifier.md"
line = 41
claim = "verifier cleanup rule: edit stays within file, target_file and instances"

[[source]]
path = "claude/agents/review-sweeper.md"
line = 33
claim = "sweeper escalation line format: file:line — summary — why it needs judgement"

[[source]]
path = "claude/skills/review-v2-method/SKILL.md"
line = 78
claim = "method defines cleanup fix_files as files the edit touches beyond anchor, target_file and instances"

[[source]]
path = "claude/skills/tomlctl/references/review-apply.md"
line = 24
claim = "schedule's settle rule: evidence block, no proposal, --settle-checkpoint; settle task runs deep and writes one note"

[[source]]
path = "claude/skills/apply-v2-engine/SKILL.md"
line = 167
claim = "apply-v2 adds settle-<n> before apply-<n> when the preview lists settle rows"
```

## Answer

**1. What the settle route needs from a finding.** `review schedule` sends a finding to a settle task only when all of these hold (schedule.rs:409-435; review-apply.md:23-24):
- family `code` or `perf`, kind `defect` or `cleanup`, status `open`. `open` means the finding has a verifier, checker or orchestrator verdict: `unverified` is refused like every status except `open`.
- `blocked_on = "evidence"` with `blocked_missing`, the exact unknown in at most 400 characters (fields.rs:694-700).
- no `proposal`, since a finding gets one settle round.
- `--settle-checkpoint` passed. `/apply-v2` adds `settle-<n>` when its preview lists `settle` rows (SKILL.md:167).

A fix that needs no settle task has to meet the same status, family and kind rule, plus no block, `effort` other than `large`, and not a test removal or a `plausible` cleanup under `--all`. Every row also has to pass `review add`'s checks:
- `lens` must be one of the cell's `concerns`, unless the row is a cleanup carrying `old` under a mechanical lens (add.rs:403-415).
- the cell must be unsealed (add.rs:283), and must sit in the run named.
- `line` is required, and a `change` or `remove` cleanup needs `old` within 20 lines of `line` (add.rs:731).

The route comes from the kind and the edit target: a defect goes to the verifier, and a cleanup on executable code to the verifier, otherwise to the checker (fields.rs:1417).

**2. Persisting an escalation.** Today an escalation exists only as `{cell, text}` in the Workflow index, cut to 300 characters (review-v2.js:367). It gets one of three dispositions (compose.md:173-177), and at close it is either captured to the backlog or dropped (triage-and-close.md:201). `review add` cannot write it later, because the escalating cell is sealed.

`--convert` already does the hard part for an advisory:
- It builds a row and checks its lens against the source finding's cell (triage.rs:1022).
- It mints the row into that cell even though the cell is sealed, inside the open run, as `unverified` (triage.rs:1048-1076).
- It adds an `orchestrator` verdict, after which a one-row verifier or checker dispatch verifies it.

A disposition such as `fix <cell>:<n>` can take the same path with the escalating cell as the source. The orchestrator writes a defect or cleanup row from the escalation text, and the verb checks `lens` against that cell's concerns, mints `unverified`, and routes the row. The verifier then confirms the row, which makes it `open` and schedulable. When the orchestrator cannot write the fix, the row carries `blocked_on = "evidence"` with the escalation's "why it needs judgement" as `blocked_missing`. It then reaches `/apply-v2` as a settle task, unless the verifier unblocks it first.

Two constraints decide the shape:
- **The lens.** A sweeper cell's concerns are `ripple` and `hygiene`, so an escalation that is really a correctness defect must be filed under one of those. The alternative is a cell that sits outside the ceiling with an override recorded, which is the ceiling lens's question.
- **The run.** The run must still be open, which triage satisfies. Escalations from an earlier finished run are gone, so this covers the current run only.

**3. A multi-file cleanup.** The cleanup row can already hold it:
- `direction = "change"`;
- `old` is the one text at the advisory's anchor (for example the signature line), checked only in the edit target within 20 lines (add.rs:731);
- `edit` is the prose;
- the other sites go in `instances` (same pattern) or `fix_files` (other files, up to 8), which `review schedule` adds to the task's `files`.

It looked impossible in run U5 for two reasons:
- The Convert section lists cleanup fields as only `edit`, `direction`, `old`, `effort` and `target_file` (triage-and-close.md:140).
- Every judge bounds a cleanup's edit to `file`, `target_file` and `instances`, never `fix_files`: the checker's predicate 4 (review-checker.md:31), the verifier's cleanup rule (review-verifier.md:41) and the audit rule (triage-and-close.md:35). The checker cannot even see `fix_files` (fields.rs:550, checker column `N`).

As a `cleanup`, the verifier judges "changes nothing observable", which a signature change satisfies. As a `defect`, it needs a `failure_scenario`, which this change lacks. No verdict field can change a finding's kind: a verdict carries only `severity`, `unblocks` and `fix_sketch`. So "let the verifier downgrade a defect to a cleanup" would need new kind-change semantics, plus the cleanup fields the defect row lacks. That is the heavier option.

## Findings

- **Library/API or file**: tomlctl/src/review/schedule.rs:409 `blocked`, :418 `settle_refusal`
- **Source**: file read at HEAD 00ff2113
- **Evidence-grade**: high
- **Finding**: A settle task is minted only for an `open` code or perf defect or cleanup with `blocked_on = "evidence"`, non-empty `blocked_missing`, no `proposal`, and `--settle-checkpoint` given (or `--dry-run`).
- **Details**: `blocked()` returns a block only for status `Open` and kind `Defect | Cleanup`. `settle_refusal` refuses `measurement` and `alternatives` as waiting for trials, and `decision` and `design` by the export rule. `blocked_missing` is required by `Need::WhenBlockedOn(BlockedOn::ALL)` (fields.rs:697-700).
- **Counter**: If `Selection::new` admitted `unverified` rows, a freshly minted escalation could settle unverified; review-apply.md:23 says any status other than `open` is refused.
- **Impact on plan / report**: A persisted escalation must be verified before `/apply-v2` sees it, so the disposition must include the verification dispatch, as convert does.

- **Library/API or file**: tomlctl/src/review/add.rs:403 `field_refusals_with`, :283 `refuse_sealed`
- **Source**: file read at HEAD
- **Evidence-grade**: high
- **Finding**: `review add` cannot carry an escalation after the wave: the escalating cell is sealed, and a lens outside its concerns is refused unless the row is a mechanical cleanup with `old`.
- **Details**: The refusal text is "`lens` is …; cell C covers …", the U5 error. A sweeper cell's concerns are `ripple`/`hygiene`, so a judgement defect from an escalation must take one of those lenses, or get a new cell.
- **Counter**: A third route would refute this: a verb that adds a finding to a sealed cell, other than `review triage --convert`. A grep of `tomlctl/src/review/` for `mint` shows only add.rs and triage.rs mint findings.
- **Impact on plan / report**: Decide lens attribution: either the escalating cell's concern or a new cell outside the ceiling. The second couples to the ceiling-override lens.

- **Library/API or file**: tomlctl/src/review/triage.rs:969 `replacement_row`, :1048 `finish_row`, :917 `target_run`
- **Source**: file read at HEAD
- **Evidence-grade**: high
- **Finding**: Convert already mints a verified-later row into a sealed cell of the open run. An escalation disposition can reuse it with the escalating cell in place of `advisory.cell`, and the escalation text in place of the advisory's anchor defaults.
- **Details**: `finish_row` sets `cell` to the source's cell, `status = unverified`, and `related` to the source plus anchor matches. `convert` writes an `orchestrator` confirmed verdict. `target_run` requires an open run of the same family. The escalation text (`<file>:<line> — <summary> — <why>`, review-sweeper.md:33) supplies the file, line and summary defaults. It needs a new source type, because there is no advisory id to mark `converted`.
- **Counter**: If run finish or the digest refused findings in a sealed cell minted after sealing, this route would fail. Convert does exactly that today, and triage-and-close.md:140-142 documents it as working.
- **Impact on plan / report**: Shapes the triage-disposition task: a new `review triage` mode or verb, the grammar row in triage-and-close.md, and the agent count (SKILL.md:169 already counts verifiers dispatched after a convert). This is `needs-plan` because the verb, the grammar and the lens decision move together.

- **Library/API or file**: claude/agents/review-checker.md:31, claude/agents/review-verifier.md:41, claude/skills/review-v2-engine/references/triage-and-close.md:35, tomlctl/src/review/fields.rs:550 `FIX_FILES`
- **Source**: files read at HEAD
- **Evidence-grade**: high
- **Finding**: A cleanup's `fix_files` is defined, scheduled and shown to the verifier, but every judge bounds the edit to `file`/`target_file`/`instances`, and the checker cannot see `fix_files`. A multi-file cleanup naming its other files in `fix_files` fails the bound. Add `fix_files` to the bound in all three texts, and give the checker read access to `fix_files`.
- **Details**: review-v2-method SKILL.md:78 says `fix_files` is "every file the `edit` edits … beyond the anchor, `target_file` and `instances`". The checker's predicate 4, the verifier's cleanup rule and the audit overturn rule omit it. `FIX_FILES = [R, R, N, R, R, R, N, R]`, with the checker as the third column (role order at fields.rs:96). Also, `route()` reads only `edited_paths` (fields.rs:688 comment), so a cleanup anchored in a non-executable file with executable `fix_files` routes to the checker.
- **Counter**: If the U5 refusal of F164 came from the defect rubric alone, the bound fix would not have saved it as a defect. The backlog item records the verifier's words as "a cleanup, not a defect", so the bound matters only once the row is converted as a cleanup.
- **Impact on plan / report**: Files touched: review-checker.md, review-verifier.md, triage-and-close.md (lines 35 and 140), fields.rs (`FIX_FILES` checker column, and the `MATRIX` row at fields.rs:1795), and possibly `route` to read `fix_files`.

- **Library/API or file**: claude/skills/review-v2-engine/references/triage-and-close.md:140 (Convert)
- **Source**: file read at HEAD; tomlctl/src/review/add.rs:731 `check_old`
- **Evidence-grade**: high
- **Finding**: The Convert section lists only `edit`, `direction`, `old`, `effort` and `target_file` for a cleanup. It should name `instances` and `fix_files` and say that `old` is only the anchor text, so a signature change across six files converts as `cleanup`, not `defect`.
- **Details**: `check_old` checks `old` only in the edit target, within `OLD_WINDOW` lines of `line`. A cleanup also needs `line`, which convert defaults from the advisory, while the section mentions `line` only for a defect. Schedule makes the task `deep` once it holds more than two files (review-apply.md:26).
- **Counter**: If the reproduce gate could not judge a `change` cleanup, it would hang at gate time. It does not: a cleanup that is not a `remove` gets the shared `reproduce-status --expect gone` accept check, which a verifier decides.
- **Impact on plan / report**: Makes "a convert shape for a multi-file cleanup" a docs-plus-bound change rather than a new row shape. A verifier kind-downgrade is the alternative and needs new verdict semantics.

Coverage: Read in full: apply-v2-engine/SKILL.md, triage-and-close.md, review-sweeper.md, verifying.md, finding-kinds.md. Read in part: add.rs (339-448, 672-754), fields.rs (560-720, 975-1084, 1415-1452, projection matrix), triage.rs (765-1077), schedule.rs (337-471), review-apply.md (schedule and settle), compose.md (Escalations and Follow-up), review-v2-method SKILL.md (25-114). Grepped review-v2.js and the agents for escalation, and review-checker.md and review-verifier.md for the bound. Ruled out: a verdict-based kind change, because the verdict fields are `severity`, `unblocks` and `fix_sketch` only. The cell ceiling and the propose slicing are left to sibling lenses. The U5 store and the backlog evidence directories are absent locally, so the U5 escalation texts were not seen.
