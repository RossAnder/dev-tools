# Plan: tomlctl agent ergonomics — prose payloads, batch writes, consistent reads

**Plan path**: `docs/plans/federated-beaming-hollerith.md`
**Created**: 2026-10-21

## Summary
Agents still work around tomlctl in the ways a transcript survey of 7,189 calls measured. They stage one scratch file per prose field (481 commands since 10-09), loop per id where each id needs a different value, and loop over `items get` because it takes one id. They also guess at `backlog show`'s `item` nesting, re-format prose JSON through python, and grep store files by hand. Sub-agents never load the skill. This plan fixes the tool, and moves the carrier recipes that produce those workarounds onto the new forms. Changes:
- The shared payload readers accept a **TOML record file** (`[[record]]` rows with `'''` literal strings), so one Write-tool file carries any number of prose-bearing records to every existing `--ndjson` / `--json '@…'` / `--ops '@…'` flag.
- Batch forms: `tasks update --ndjson`, `backlog triage --ndjson`, per-row `task` in `flow record`, and `backlog edit --ndjson`.
- Read additions: `items get <ids>`, a row-only `backlog get`, and `--grep` on the three list verbs.
- Output changes: a `--text-view` block view and template escapes; `--rows`/`--header`/`--template --select` refusals relaxed where the flag is redundant.
- Regex Unicode `(?i)`/`\d`/`\w`; a `counts` map on `backlog reconcile`.
- Generic writers refuse the five verb-owned stores without `--raw-store`.
- A shared `tomlctl-for-agents` block in the four flow agents, read one-liners in the carriers' Explore and `general-purpose` prompts, and the carriers' write recipes moved to record files and batch forms.

Scrutinise first: Approach, "TOML record files" (the shape rule and string normalisation every verb inherits) and Approach, "Owned-store guard" (it refuses writes that work today).

## Context
Backlog item `B-7138d8d7` (filed 2026-10-16) surveyed backlog workarounds. A wider survey on 2026-10-21 covered every tomlctl call in 123 sessions across 9 repos. The id lists, global output options and `flow record` that shipped 2026-10-07..09 cut python pipes 20× and heredocs to zero, which shows agents adopt a capability once it exists. What remains is mostly capability gaps:
- **Prose transport is the dominant cost.** Commands of the form `printf … > f.txt` then `--set-file summary=f.txt`, three files per deviation record, average 1.7k characters.
- **Per-id write loops are flat at about 40 per 1k calls**, because existing id lists apply one value to every id.
- **The read verbs disagree with each other**: `items get` takes a single id, and `backlog show` nests its row.
- **Sub-agents use the tool without learning it**: 0 of about 1,400 sub-agent transcripts loaded the skill, and they ran 149 `2>/dev/null` and 62 `--help` lookups.

The goal is that the common agent jobs are each a single documented tomlctl call: write N prose records, update N tasks with per-task values, find a row by text, read one row or a few, and print prose legibly.

## Scope
- **In scope**: tomlctl CLI features and fixes listed in Summary; the docs and lint-gated examples that describe them (`claude/skills/tomlctl/**`, `tomlctl/README.md`, the execution-record schema skill); every statement that the build has no Unicode classes (tomlctl source and test headers, the review-v2 lenses, the research method); the four flow-agent files' shared block; the Explore dispatch prompts in `/plan-new`, `/review`, `/optimise` and the apply pre-analysis, and `/plan-update`'s `general-purpose` reconcile and catchup prompts; `plan-update.md`'s reconcile summary line; the write recipes in `implement.md` (3b deviation records) and `plan-update.md` (deviation and deferral records) moved to TOML record files; `tasks update --ndjson` semantics in `flow-contract-task-store`; `backlog-capture`'s `backlog show … {item.*}` reads moved to `backlog get`; capabilities features.
- **Out of scope**: a version bump (capabilities features carry the gating); changing `backlog show`'s shape; guarding `agents.toml` or `context.toml`; making agents load the tomlctl skill; the review-v2 writer agents (`review-finder`, `review-finder-cf`, `review-verifier`, `plan-executor`), which write prose through `review add --ndjson` and could adopt TOML record files in a follow-up; `implement.md`'s per-group commit-train `tasks update --commit` and per-task `task-completion` records, which are one call per entry for crash safety; lumina; the other open backlog rows under `tomlctl/` (B-edb20f98, B-e4c1a22a, B-e922f5cb, B-ef306556, B-771b57e7, B-b9b7e4c1).
- **Affected areas**: `tomlctl/Cargo.toml`, `tomlctl/src/`, `tomlctl/tests/`, `tomlctl/README.md`, `claude/skills/tomlctl/`, `claude/skills/flow-contract-execution-record-schema/SKILL.md`, `claude/skills/flow-contract-apply-pipeline/references/pre-analysis.md`, `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/backlog-capture/SKILL.md`, `claude/skills/review-v2-lenses/`, `claude/skills/research-methods/references/codebase-review.md`, `claude/agents/`, `claude/commands/plan-new.md`, `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/plan-update.md`, `claude/commands/implement.md`, `scripts/shared-blocks.toml`

## User Decisions

Answers recorded as data:

> 1. **Scope** — one plan, milestone checkpoints (asked at Phase 1 scope check: 10 concerns, 40+ files).
> 2. **Payloads** — TOML on existing flags: shared readers accept a `.toml` file wherever `--ndjson <path>`, `--json '@path'` or `--ops '@path'` take one; `[[record]]` rows with `'''` literals, CRLF→LF, one trailing newline trimmed; JSON stays. (Prompted by: 481 `--set-file` staging commands; Research Notes › TOML payload files; `io.rs:345 read_ndjson_source`, `:319 read_json_arg`.)
> 3. **Show shape** — add `backlog get <ids>` row-only verb; `show` keeps `{item, evidence, neighbours}`. (Prompted by: `backlog/query.rs:189 build_show_resolved`; `['item']` guesses.)
> 4. **Raw guard** — all verb-owned stores: generic writers refuse `backlog.toml`, `tasks.toml`, `execution-record.toml`, `inputs.toml`, `review-v2.toml` without an override flag. (Prompted by: `doc.rs:82`, `dispatch/items.rs:534`, `dedup.rs:33`.) Audit: the only generic write against an owned store in `claude/**` is `backlog-recipes.md:46` (`items update … --array backlog`).
> 5. **Search** — `--grep <regex>` on backlog/tasks/items list, case-insensitive, matches if ANY of a per-store default text-field set matches; `--grep-field <f>` overrides; ANDs with `--where*`; items ledgers default summary/description/file/symbol. (Prompted by: 36 list|grep + 80 raw greps; `query.rs:162 Predicate`.)
> 6. **Prose view** — template escapes `\n` `\t` `\\` plus a global `--text` block render (`key: value`, multi-line prose indented; honours `--select`/`--omit`/`--max-chars`). (Prompted by: `template.rs:34`; python/node reformatting of `tasks show --with body`.)
> 7. **Learnability** — shared `tomlctl-for-agents` block in implement-deep/lite + research-deep/lite; read one-liners in carrier Explore prompts (plan-new, review, optimise, apply pre-analysis); error/help hints naming the right verb or flag. Not chosen: making agents load the skill. (Prompted by: 0/~1,400 skill loads; Exploration Notes › Docs, agents, gates.)
> 8. **Batch writes** — `tasks update --ndjson` (rows `{ids, status?, agent?, commit?, set?}`), `backlog triage --ndjson` (rows `{id, resolve|dismiss|promote|reopen: text, rationale?, expect_status?}`), per-row `task` in `flow record --ndjson`, and `backlog edit --ndjson`. All accept the TOML record files. (Prompted by: per-id loop rate 37→45/k; `update.rs:94`, `triage.rs:211`, `record.rs:191`.)
> 9. **Backlog** — fold in `B-7138d8d7`, closed by the delivering tasks.
> 10. **Prose-view flag name** (asked at `/review-plan` round 1) — `--text-view`, matching the `text_view` feature. `--text` is already `inputs answer --text <TEXT>`, and clap 4.6.7 refuses a global sharing that long name. (Prompted by: `cli/types/inputs.rs` `InputsOp::Answer`.)

### Phase 5 outcome
Skipped: every answer's library terms (TOML `DeTable`, `'''` literals, regex case-insensitivity) are covered by Research Notes; the remaining answers are in-crate design with no library surface.

## Approach

Fix the capability gap in the tool, reuse the crate's existing seams, and change shapes only additively. `backlog show` keeps its shape. Refusals are relaxed only where the flag is redundant, never where it would be ambiguous.

### TOML record files
A **record file** is any file path ending `.toml` (case-insensitive) given to a payload flag that already takes a file. One new module, `tomlctl/src/payload.rs`, converts it to the JSON text the existing parsers already consume, so every downstream verb inherits it unchanged.

Hooks in `tomlctl/src/io.rs`:
- `read_ndjson_source` (both the `@path` and the bare-path branch) feeds every `--ndjson`.
- The `@path` branches of both `read_json_arg` (`--ops '@…'`, `backlog add --json`, `items add`'s text path) and `read_json_value_from_arg` (`set-json`, `array-append --json`, `items add`/`update`'s value path, `--defaults-json`, `flow record --json`, `inputs add`). The second reads `@path` itself through `read_at_file`, not via the first, so both are hooked.
- Emit compact JSON: `parse_apply_ops` reads a payload starting with `{` as NDJSON. File payloads carry no size cap (the 32 MiB cap guards stdin only).

Literals, `-` (stdin) and non-`.toml` paths behave exactly as today.

- **Shape rule**: parse with `toml::de::DeTable::parse`.
  - When the document has a top-level `record` key holding an array of tables (`[[record]]`), the payload is that array of rows.
  - The rows key is reserved as `record`: a `record` array of tables beside any other top-level key is refused (`kind=validation`, naming the stray keys and pointing at `--defaults-json` / `--set` for shared values) rather than folded into one object.
  - Otherwise the whole table is one object.
  - For an NDJSON consumer each row becomes one compact JSON line, so a downstream `line N` names the Nth record. A single object becomes one line.
  - For a `--json`/`--ops` consumer the result is the compact JSON array or object text.
- **Strings**: every multi-line string (one whose source token, read through its value span, opens with `'''` or `"""`), recursively, has CRLF normalised to LF and exactly one trailing `\n` removed; single-line strings, whose newlines can only be explicit escapes, are left as written. A `'''` block written with its closing delimiter on its own line therefore reads back as typed, matching `read_flag_file` (`io.rs`). TOML dates and datetimes become strings via `convert.rs` `detable_to_json`.
- **Errors**:
  - TOML syntax errors forward toml's own `Display` (line, column, caret), prefixed with the file path.
  - Shape errors, such as a top-level array of non-tables, a non-object row or a stray key beside `record`, name `record N (line L)` where a row is at fault; L is computed by counting `\n` before the row's span start.
  - A `kind=validation` tag is used throughout.
- **No `null`**: TOML cannot express one, so an `items apply` op's `expect` absent-field precondition (`null`) needs the JSON form.
- **Authoring form** documented in write.md:
  ```toml
  [[record]]
  type = "deviation"
  task = 12
  original_intent = '''Send `commitId` and hand over by "generationId"'''
  rationale = '''
  Any prose — backticks, quotes, $vars, newlines — verbatim.
  '''
  ```
  Prose containing `'''` falls back to `"""…"""`, where `\` is written `\\`.

### Batch write forms
Each batch form is all-or-nothing under one lock, validates every row before writing, refuses a duplicate id across rows, refuses an unknown key while naming the accepted ones, and takes a TOML record file or NDJSON through `--ndjson <src>`. A clap conflict does not satisfy a `required_unless_present` positional or a `required = true` group (clap_builder 4.6.7 `validate_required`), so each verb's id positional gains `required_unless_present("ndjson")` through `#[command(mut_args(..))]` on its own variant (the `ids_help` pattern; the shared `TaskIds`/`ItemIds` stay unchanged), and `backlog triage` relaxes `TriageMode` with `mut_group("TriageMode", |g| g.required(false))` and re-checks the one-mode rule in dispatch for the flag form. The precedent is `inputs.rs` `handle_rows` / `handle_each`.
- **`tasks update --ndjson <src>`**:
  - Row shape: `{ids: [..] | "1,2" | id: N, status?, agent?, commit?, ref?, set?: {KEY: VALUE}}`.
  - `set` keys go through the same `SETTABLE` allowlist as `--set` (`tasks/update.rs`).
  - `--ndjson` conflicts with positional ids and with the per-value flags, `--checkpoint` included.
  - All rows apply in one `store::mutate`; the output is the existing `{ok, rows:[{id, ref, changed}]}` shape.
- **`backlog triage --ndjson <src>`**:
  - Row shape: `{id, resolve|dismiss|promote|reopen: <companion>, rationale?, expect_status?}`.
  - Exactly one mode key per row. Its value is the companion today's flags take: `resolve` → resolution text, `dismiss` → reason, `reopen` → rationale, `promote` → the target flow slug.
  - `--expect-status` on the command line is each row's default.
  - Rows go through the same `Transition` / `apply_transition` path (`backlog/triage.rs`) in one `mutate_doc_conditional`, with stale rows reported under `skipped_stale` exactly as the id-list form reports them.
- **`flow record` per-row `task`**: an `--ndjson` row may carry `task` (a task id). It is resolved to `task_ref` per row by the same lookup `--task` uses (`flow/record.rs` `task_ref_for`), and the `task` key itself is not stored. A row whose `task` disagrees with `--task`, or whose `task_ref` disagrees with its resolved `task`, is refused — the same rule `fill_defaults` applies to `type`.
- **`backlog edit --ndjson`**: see "Backlog get and edit".

### Backlog get and edit
- **`backlog get <ids>`**: prints only the stored row, with no `item` wrapper, `evidence` or `neighbours`. One id gives an object; several give an array in the order given, de-duplicated. An unknown id fails the whole call. It takes the same id-list parsing (`ItemIds`, `cli/types/backlog.rs`) and integrity args as `show`, and reuses `show`'s row lookup in `backlog/query.rs`.
- **`items get <ids>`**: the same multi-id contract, with the `TaskIds`/`ItemIds` parser pattern and the `tasks/show.rs` output pattern (dedupe, build all rows before printing, one id → object, several → `print_report(Array, Rows::Top)`).
- **`backlog edit <id> …` / `backlog edit --ndjson <src>`**:
  - Edits only the in-place fields: `context` (`--context` / `--context-file`), `tags` (`--tags a,b` replaces; `--add-tag` / `--remove-tag` are repeatable), `evidence` (`--evidence` replaces; `--add-evidence` is repeatable), `origin`, `flow` and `base_sha`.
  - Rows carry the same keys plus `id`.
  - Any other field is refused with the verb that owns it. `status` and the triage-managed fields belong to `backlog triage`. `summary`, `kind` and `area` feed `id`/`dedup_id`, so the refusal points to capturing a successor with `backlog add` and `supersedes`. `created`, `last_seen`, `seen_count`, `id` and `dedup_id` are minted.
  - The edited document is validated with `backlog/schema.rs` `validate` before the write, under one lock, and `last_updated` is stamped. `--dry-run` is supported.

### Owned-store guard
A new `tomlctl/src/store_guard.rs` provides `refuse_owned_store(path, raw_store) -> Result<()>`, called by the generic writers before their write. It classifies the target by the trailing components of its resolved path (`canonicalize_for_write` when the file or its parent exists, else the path with `.`/`..` folded lexically), so `..` segments, a junctioned `.claude` or an 8.3 short name cannot slip past:

| Path | Owned by |
|---|---|
| `.claude/backlog.toml` | backlog verbs |
| `.claude/inputs.toml` | `inputs` verbs |
| `.claude/flows/<slug>/tasks.toml` | `tasks` verbs |
| `.claude/flows/<slug>/execution-record.toml` | `flow record` |
| `.claude/flows/<slug>/review-v2.toml`, `.claude/review-v2/<scope>.toml` | `review` verbs |

The generic writers are `set`, `set-json`, `array-append`, and `items add` / `add-many` / `update` / `remove` / `apply` / `backfill-dedup-id`. `items sweep --update` is deliberately left unguarded: it rewrites `instances` only on rows carrying a `sweep` array, which no owned store holds. Reads are untouched, and `json set`/`unset` already refuse `.toml` targets.

- The refusal is `kind=validation`. It leads with the owning verbs and the specific replacement (`backlog edit` for context/tags/evidence/origin, `backlog triage` for status, `tasks update` / `tasks add`, `tasks import-plan` for `[[checkpoints]]`, `flow record`), then says `pass --raw-store to write it generically`.
- `RawStoreArg { --raw-store }` lives in `cli/types/shared.rs` and is flattened into each generic writer.
- The guard runs in the CLI layer only; library callers such as glimpse are unaffected.
- Every agent call already matches `Bash(tomlctl *)`, so an agent can take the advertised bypass without a prompt. `ask` rules for `--raw-store` beside the `--allow-outside` deny rules in `.claude/settings.json` close that; they are a permissions change applied by hand under After Merge, not by a task.

### Search
`--grep <regex>` on `items list`, `backlog list` and `tasks list` adds one predicate, `Predicate::Grep`, to the shared query engine (`query.rs`).
- **Matching**: compiled case-insensitively through `compile_user_regex`. It matches when ANY field in its field set matches; an array field matches when any element does. It ANDs with every `--where*`.
- **Field sets**: each caller passes its default set through `QueryInput`:
  - backlog: `summary`, `context`, `area`, `tags`, `resolution`, `dismiss_reason`, `reopen_rationale` (the `backlog/schema.rs` `FIELD_*` constants)
  - tasks: `title`, `ref`, `action`, `detail`, `acceptance`, `files`
  - items ledgers: `summary`, `description`, `file`, `symbol`
- **Overrides**: `--grep-field <f>` (repeatable) replaces the set.
- **Flags**: both are added to `QueryArgs` (`cli/types/shared.rs`) and carried by `to_query_input`.

### Output view and refusals
- **Template escapes**: `Template::parse` (`output/template.rs`) adds `\n`, `\t` and `\\`. Any other backslash stays literal, so existing templates are unchanged; a scan of `claude/` found no template that contains a backslash.
- **`--text-view`**: a new global output flag (arg id `text_view`), rendered by a new `output/text.rs` and registered in `OutputArgs` (`cli/types/mod.rs`). Not `--text`: `inputs answer` already declares a value-taking `--text <TEXT>`, and clap 4.6.7 propagates globals by id (a same-id bool global panics on a String downcast) and debug-asserts unique long names (`the_command_tree_has_no_duplicate_flags`).
  - Each object prints as `key: value` lines. A string containing a newline prints `key:` and then its lines indented two spaces. An array of scalars is comma-joined. A nested object flattens to dotted keys. An array of objects prints as compact JSON.
  - A row report prints its header block (when there is one), then one block per row, separated by blank lines.
  - It composes with `--select`, `--omit`, `--max-chars`, `--where*`, `--limit`, `--rows` and `--header`. It excludes `--get`, `--template`, `--lines` and `-q`.
  - The list verbs rebuild their options through `output.rs` `query_opts` and stream unless a shaping flag is set, so the flag is carried there and turns streaming off.
- **Refusals relaxed** (`output.rs` `validate` / `emit`):
  - `--rows <PATH>` on a row report whose row field is exactly PATH is a no-op; any other path keeps the refusal.
  - `--header` on a single-object report returns the object.
  - `--template` with `--select` applies the select first, then the template.

### Regex
Add `unicode-perl` and `unicode-case` to the `regex` features in `tomlctl/Cargo.toml`. That is the minimal set for `(?i)`, `\d`, `\w`, `\s` and `\b` on both `Regex` and `bytes::Regex`; it adds no new crate, only const tables. Also:
- Rewrite `REGEX_UNICODE_HINT` (`query.rs`) to cover what stays unsupported (`\p{..}` classes) and the size limit (Unicode `\w`/`\W` repeats above about 20 exceed it), pointing at `(?-u:\w)`.
- Document `(?-u:\b)` for fast `sweep` word boundaries.
- Fix every statement that the build has no Unicode classes: `agents/correlate.rs`, the `sweep` help in `cli/types/cmd.rs`, the module docs and `every_pattern_compiles` comments in `tasks/parse_tasks.rs` / `tasks/parse_policy.rs`, the headers of `tests/sweep.rs` / `tests/tasks_corpus.rs`, and `claude/skills/review-v2-lenses/{vocabulary.toml,SKILL.md,methods/sweep.md}` and `claude/skills/research-methods/references/codebase-review.md`. Patterns stay as written; the `(?-u:…)` forms remain the recommended fast form, but bare forms no longer fail.

Tests already build with full Unicode through the `predicates` dev-dependency. The release build's feature set is therefore checked with `cargo tree -e normal,features`, and with the release binary in the final pass.

### Reconcile counts
The `backlog reconcile` report (`backlog/reconcile.rs` `reconcile`) gains `counts: {ready, in-progress, stalled, unlinked, orphaned}`, filled in the existing `Bucket::ALL` loop. `plan-update.md` then reads the summary line with `--get` / `--template` over `counts` instead of asking for array lengths.

### Agent learnability
- **Shared block**: a new byte-identical shared block `tomlctl-for-agents`, registered in `scripts/shared-blocks.toml` and carried by `claude/agents/{implement-deep,implement-lite,research-deep,research-lite}.md`. It is at most 14 lines:
  - Read with `tasks show <ids> --with body --text-view`, `backlog get <ids>`, `items get <file> <ids>`, and `… list --grep <regex> --text-view`.
  - Shape with `--get` / `--select` / `--template`; never pipe tomlctl into python, jq or grep.
  - Never `2>/dev/null` tomlctl; its stderr is the diagnosis.

  Commands go in a bash fence so `command_lint` checks them against the real CLI. The block carries no write guidance: the research agents hold no Write tool and the implement agents never write stores (both are orchestrator-only).
- **Explore and general-purpose prompts**: the Explore dispatch prompts in `plan-new.md`, `review.md`, `optimise.md` and the apply-pipeline `pre-analysis.md`, and `plan-update.md`'s `general-purpose` reconcile and catchup dispatches, gain one bash-fenced line of the same read one-liners. Built-in agents can't carry a block.
- **Carrier write recipes**: the orchestrators are where 465 of the 481 `--set-file` stagings ran, so `implement.md` 3b and `plan-update.md`'s deviation and deferral records move to one TOML record file through `flow record --ndjson`, `flow-contract-task-store` documents `tasks update --ndjson`, and `backlog-capture` reads with `backlog get`.
- **Error hints**: the NDJSON parse errors in `items.rs` (`parse_ndjson_lines`, `parse_ndjson_objects`) append a hint pointing at a TOML record file. The guard refusal names the replacement verb.

### Capabilities and docs
- **Features**: each new capability gets a `FEATURES` entry (`cli/types/mod.rs`), in one registration task with its lockstep test (`tests/capabilities.rs` expected list, global-flags list) and its README transcription. The names are `regex_unicode`, `template_escapes`, `text_view`, `relaxed_refusals`, `items_get_multi`, `list_grep`, `reconcile_counts`, `toml_records`, `tasks_update_batch`, `backlog_triage_batch`, `flow_record_row_task`, `backlog_get`, `backlog_edit` and `owned_store_guard`.
- **Reference docs**: each change gets its rows, which `flag_table_lint` and `command_lint` gate. `backlog.md` sits 3 lines under its 600-line ceiling, so the `backlog get` and `backlog edit` tables go in a new `references/backlog-edit.md`. `backlog-recipes.md` is rewritten from interim workarounds into the real one-call recipes.

## Success Criteria
- forward: the release dependency graph enables `unicode-case` for regex — `cargo tree --manifest-path tomlctl/Cargo.toml -e normal,features -i regex-syntax --offline --locked | grep -q 'unicode-case'` (today: 0 matches).
- forward: no interim ASCII-regex workaround remains in the tomlctl skill — `! grep -rq '(?i-u)' claude/skills/tomlctl` (today: 2 files, `query.md`, `backlog-recipes.md`).
- forward: the four flow agents carry the shared block and the parity verifier passes — `test "$(grep -l 'SHARED-BLOCK:tomlctl-for-agents START' claude/agents/*.md | wc -l)" -eq 4 && bash scripts/verify-shared-blocks.sh` (today: 0 files).
- forward: `/plan-update` no longer asks agents to count reconcile buckets — `! grep -q 'lengths of those five' claude/commands/plan-update.md` (today: 1 match).
- forward: the carriers stage no per-field prose files for deviation records — `! grep -q 'set-file original_intent' claude/commands/implement.md claude/commands/plan-update.md` (today: 1 match in each).
- forward: the whole tomlctl suite, including the dispatch gates (`command_lint`, `flag_table_lint`, `every_leaf_command_has_a_case`, `capabilities_features_contains_every_plan_feature`), passes — the `test:` command.
- forward, **predicted, unverified** (needs a non-test build of the binary, since test builds enable full Unicode): against `tomlctl/target/release/tomlctl.exe` from the final pass —
  - `tasks list --slug federated-beaming-hollerith --where-regex 'title=(?i)toml' --count` exits 0 (today: refused, "Unicode-aware case insensitivity");
  - `set .claude/backlog.toml last_updated 2026-10-21 --dry-run` exits 1 naming `backlog edit` (today: exits 0);
  - a two-record TOML file with `'''` prose passes `flow record --slug federated-beaming-hollerith --type deviation --ndjson <file> --set agent=probe --dry-run`;
  - `backlog list --grep 'toml' --count` exits 0 (today: unknown argument);
  - `backlog get B-7138d8d7 --get summary` prints a bare string (today: unknown subcommand);
  - `tasks show 1,2 --slug federated-beaming-hollerith --with body --text-view` prints `title:` lines (today: unknown argument);
  - a two-row TOML record file through `tasks update --file .claude/probe/tasks.toml --ndjson <file>`, against a copy of the flow's store, sets two different `commit` values (today: unknown argument).

  Run in the final pass, before merge (see Verification Commands).

## Verification Commands
```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
checkpoint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo clippy --manifest-path glimpse/Cargo.toml --locked
checkpoint: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
checkpoint: bash scripts/verify-shared-blocks.sh
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: cargo tree --manifest-path tomlctl/Cargo.toml -e normal,features -i regex-syntax --offline --locked | grep -q 'unicode-case'
success: bash -c "! grep -rq '(?i-u)' claude/skills/tomlctl"
success: bash -c "test \"\$(grep -l 'SHARED-BLOCK:tomlctl-for-agents START' claude/agents/*.md | wc -l)\" -eq 4 && bash scripts/verify-shared-blocks.sh"
success: bash -c "! grep -q 'lengths of those five' claude/commands/plan-update.md"
success: bash -c "! grep -q 'set-file original_intent' claude/commands/implement.md claude/commands/plan-update.md"
```

Run every command from the repo root. Final pass also runs `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items` (rustdoc deny lints) and `cargo audit --file tomlctl/Cargo.lock`. Measure the release-binary size delta of the regex feature change (`cargo build --release --manifest-path tomlctl/Cargo.toml` before/after, `ls -l tomlctl/target/release/tomlctl.exe`) and record it in the final report; growth over ~200 KB is stop-and-report. With that release exe, run the predicted Success Criteria probes as `tomlctl/target/release/tomlctl.exe …` before merge: all are read-only or `--dry-run` except the `tasks update` probe, which writes only a copy of the store under `.claude/probe/` that is deleted afterwards. A debug binary from the suite run is not a substitute, because a build that includes test targets unifies the `predicates` dev-dependency's full Unicode features.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 3, 4, 7, 8, 10, 20, 21, 22, 24, 25, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### Wave A — reads and output

### 1. Enable regex Unicode case folding and Perl classes [S]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/src/query.rs`, `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Backlog**: B-7138d8d7
- **Action**: Add `unicode-perl` and `unicode-case` to the `regex` dependency's `features` in `tomlctl/Cargo.toml` and update its comment; rewrite `REGEX_UNICODE_HINT` in `tomlctl/src/query.rs` and its pinning unit test; fix the comment in `tomlctl/src/agents/correlate.rs` that says the crate is built without Unicode classes.
- **Detail**: Approach, "Regex". The new feature list is `["std", "perf", "unicode-perl", "unicode-case"]` — not the `unicode` umbrella. The hint (used by `compile_user_regex` and `compile_user_bytes_regex`) should now say what is still unsupported (`\p{..}` Unicode property classes), that Unicode `\w`/`\W` repeats above about 20 (`\w{21}`) exceed the 1 MiB compile-size limit (use `(?-u:\w)`), and that `(?-u:\b)` keeps `sweep` on the fast DFA. Update `compile_user_bytes_regex_error_carries_the_unicode_hint` (`query.rs`) to the new text with patterns that fail in every build, test builds included (the `predicates` dev-dependency compiles `\p{Greek}` there): keep `(` for the syntax path and add `\w{100}` for the size-limit path (Unicode `\w{21}` already exceeds the 1 MiB limit; `\w{20}` compiles). Leave `REGEX_COMPILE_SIZE_LIMIT` unchanged. In `correlate.rs`, keep the `[0-9]` pattern; only the comment's claim changes. Unit tests cannot prove the release feature set (the `predicates` dev-dependency enables full Unicode in test builds) — the `cargo tree` check below is the proof.
- **Acceptance**:
  - forward: `cargo tree --manifest-path tomlctl/Cargo.toml -e normal,features -i regex-syntax --offline --locked | grep -c 'unicode-case'` prints ≥1 (today: 0)
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/unicode_hint/)'` passes

### 2. Relax redundant output-flag refusals [M]
- **Files**: `tomlctl/src/output.rs`, `tomlctl/tests/output_nesting.rs`, `tomlctl/tests/output_options.rs` — its `read_options` single-object `--header` probe flips from refused to returns-the-object; keep the `--where a=b` probe refused
- **Depends on**: —
- **Backlog**: B-7138d8d7
- **Action**: In `tomlctl/src/output.rs`, make `--rows <PATH>` a no-op on a row report whose row field is exactly PATH, make `--header` on a single-object report return the object, and allow `--template` with `--select` (select applies first); update the unit tests, `tomlctl/tests/output_nesting.rs` and the single-object `--header` probe in `tomlctl/tests/output_options.rs` to the new contract.
- **Detail**: Approach, "Output view and refusals". The refusals live in `OutputOpts::validate` (the `` `--template` cannot be combined with `--select` `` arm) and in `emit` / `emit_one` (`rows_on_row_report`, `HEADER_ON_ONE`). Keep `HEADER_ON_TOP` (bare-array report), `--rows` naming a different path, `--rows` with `--header`, and every list-verb refusal (`refuse_row_flags_on_list`). The redundant `--rows` case is what `backlog check --ndjson --rows results` hits (`backlog/check.rs` declares `Rows::Field("results")`); `--header` on one object is what `tasks render --check --header` and `backlog reconcile --header` hit. `--template` + `--omit` stays refused. Rename and rewrite `rows_and_header_are_refused_on_list_verbs_and_row_reports` in `output_nesting.rs` so it asserts the refusals that remain and adds positive cases for the three relaxations, and update the unit tests in `output.rs` that pin the removed messages (`header_is_refused_on_a_bare_row_array_and_a_single_report` among them). In `output_options.rs`, the shape harness's single-object arm loops `for probe in [&["--header"][..], &["--where", "a=b"]]` asserting refusal: make the `--header` probe assert the object comes back unchanged and leave the `--where` probe as it is; change nothing else in that file.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test output_nesting` passes with a case running `backlog check --ndjson <file> --rows results` successfully — predicted, unverified (the case is new); falsifier: restoring the `rows_on_row_report` refusal for a matching field turns it red
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/^output::/)'` passes
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test output_options` passes

### 3. Template escape sequences [S]
- **Files**: `tomlctl/src/output/template.rs`
- **Depends on**: —
- **Backlog**: B-7138d8d7
- **Action**: Teach `Template::parse` in `tomlctl/src/output/template.rs` the escapes `\n`, `\t` and `\\` in literal template text, leaving any other backslash sequence literal; add unit tests whose names contain `escape`.
- **Detail**: Approach, "Output view and refusals". Escapes apply only to literal segments, never inside `{path}` placeholders or to values. `{{` / `}}` keep working. Tests: `'{id}\n{title}'` renders two lines; `'a\\nb'` renders `a\nb` (literal backslash-n); `'\x'` renders `\x`.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/^output::template::.*escape/)'` passes including a test asserting `\n` becomes a newline (today: fails, no tests match) — predicted, unverified; falsifier: removing the escape branch turns that test red

### 4. Global --text-view block view [L]
- **Files**: `tomlctl/src/output/text.rs` (new), `tomlctl/src/output.rs`, `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/dispatch.rs` — `output_opts()` builds `OutputOpts` field by field
- **Depends on**: 2
- **Action**: Add a global `--text-view` output flag (arg id `text_view`): declare it in `OutputArgs` (`tomlctl/src/cli/types/mod.rs`), copy it into `OutputOpts` in `output_opts()` (`tomlctl/src/cli/dispatch.rs`), carry it in `OutputOpts` (`tomlctl/src/output.rs`) with its exclusions in `validate`, and render objects and row reports as `key: value` blocks in the new `tomlctl/src/output/text.rs`.
- **Detail**: Approach, "Output view and refusals". Do not name it `--text` and do not give it the id `text`: `inputs answer` already declares a local `--text <TEXT>` (`tomlctl/src/cli/types/inputs.rs` `InputsOp::Answer`), and either form breaks clap's global propagation. It plugs in where `--get` / `--template` render in the option order (after `--omit`/`--select` and `--max-chars`). Declare `mod text;` beside `mod template;` in `output.rs`. Carry the field through `query_opts` (the list verbs' option whitelist), make `streaming_allowed` false under it, and add it to `OutputOpts::shaping_flags`; otherwise `items/backlog/tasks list --text-view` prints JSON and text-output verbs ignore it. Rendering rules are in the Approach; strings print bare, null/missing keys are skipped, and output always ends with a newline. Excludes `--get`, `--template`, `--lines`, `-q` (each a `kind=validation` refusal through the existing `validate` pattern). Write envelopes (`print_json_compact`) honour `--text-view` like any object. Unit tests in `text.rs` cover: scalar object, multi-line string indentation, array-of-scalars join, nested object dotted keys, row report with header. Do not touch `tests/capabilities.rs`; task 23 registers the flag there.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/^output::text::/)'` passes including a multi-line indentation case (today: fails, no tests match) — predicted, unverified; falsifier: rendering strings with `{:?}` turns it red
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test inputs` passes (`inputs answer --text` still parses)

### 5. items get takes a list of ids [S]
- **Files**: `tomlctl/src/cli/types/items.rs`, `tomlctl/src/cli/dispatch/items.rs`, `tomlctl/tests/items_get_ids.rs` (new)
- **Depends on**: —
- **Action**: Change `items get`'s positional `id: String` to a comma- or space-separated id list and print one object for one id or an array in the order given for several, failing the whole call on an unknown id; update the `items get` about text in `tomlctl/src/cli/types/items.rs` ("Get a single item by its `id` field.") to the list form.
- **Detail**: Approach, "Backlog get and edit". Follow `TaskIds` (`tomlctl/src/cli/types/tasks.rs`, help from `ids_help`) / `ItemIds` (`tomlctl/src/cli/types/backlog.rs`) for parsing, and `tomlctl/src/tasks/show.rs` for the output pattern (BTreeSet de-dupe, build every row before printing, one → `print_json`, several → `print_report(Array, Rows::Top)`). The handler is the `ItemsOp::Get` arm in `cli/dispatch/items.rs` calling `items_get_from` / `items_get_from_json` (`tomlctl/src/items.rs`) once per id. The new test covers: single id unchanged shape; `R1,R3` and `R1 R3` arrays in order; duplicate collapsed; unknown id fails naming it (keep `items_get_from`'s untagged `no item with id = …` error — `tests/capabilities.rs` `error_format_json_untagged_fallback_kind_other` pins `kind=other` with a null `file`); `--array` still honoured.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test items_get_ids` passes — predicted, unverified; falsifier: reverting to the single `id: String` makes the two-id case a clap error
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test output_options` passes (the single-id `items get` case stays a One shape)

### 6. --grep predicate in the query engine [M]
- **Files**: `tomlctl/src/query.rs`, `tomlctl/src/cli/types/shared.rs`
- **Depends on**: 1
- **Action**: Add `--grep <REGEX>` and repeatable `--grep-field <FIELD>` to `QueryArgs` (`tomlctl/src/cli/types/shared.rs`) and a `Predicate::Grep { fields, regex }` to `tomlctl/src/query.rs` that matches case-insensitively when any field in its set matches, ANDed with the other predicates.
- **Detail**: Approach, "Search". Extend `QueryInput` with the grep pattern, the override fields and a caller-supplied default field list (`&'static [&'static str]`, empty meaning "`--grep` refused: this list verb has no default fields; pass `--grep-field`"). Compile once in `PreparedPredicates::new` via `compile_user_regex` with `(?i)` prepended (task 1 makes that legal in the release build). Array fields match when any string element matches; non-string scalars are matched on their rendered text; missing fields don't match. Give it a `predicate_cost` above the string predicates. `to_query_input` carries both flags and sets the default field list to `&[]`; leave its signature and every caller unchanged — task 7 assigns the field on the returned `QueryInput` at each list verb's call site. Unit tests in `query.rs`: two-field OR, case-insensitivity, AND with `--where`, array element match, `--grep-field` override.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/grep/)'` passes — predicted, unverified; falsifier: making the predicate require every field turns the two-field OR test red

### 7. Wire --grep default fields into the list verbs [L]
- **Files**: `tomlctl/src/backlog/query.rs`, `tomlctl/src/tasks/list.rs`, `tomlctl/src/cli/dispatch/items.rs`, `tomlctl/tests/list_grep.rs` (new)
- **Depends on**: 5, 6
- **Backlog**: B-7138d8d7
- **Action**: Pass each list verb's default grep field set into its `QueryInput` — `backlog list` in `tomlctl/src/backlog/query.rs` (`dispatch_list`), `tasks list` in `tomlctl/src/tasks/list.rs`, `items list` in `tomlctl/src/cli/dispatch/items.rs` — and add an integration test over all three.
- **Detail**: Approach, "Search" (field sets listed there; backlog's come from the `backlog/schema.rs` `FIELD_*` constants). Assign the default-field list on the `QueryInput` that `to_query_input` returns: `backlog/query.rs` already binds `let mut input`; `tasks/list.rs` and `cli/dispatch/items.rs` build it inline and need a `let mut` binding. The three call-site edits are small and land together so the integration test can cover every verb. The test seeds a backlog, a tasks store and a review ledger and asserts: a word only in `context` finds the backlog row; a word only in `dismiss_reason` finds a dismissed row; `(?i)` upper-case pattern matches lower-case text; `--grep` ANDs with `--where status=open`; `--grep-field summary` excludes a context-only hit; `--template '{id}'` output is the matching ids.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test list_grep` passes — predicted, unverified; falsifier: passing an empty default list in `backlog/query.rs` makes the context-only case return nothing

### 8. Counts map on backlog reconcile [S]
- **Files**: `tomlctl/src/backlog/reconcile.rs`, `tomlctl/tests/backlog_reconcile.rs`, `claude/commands/plan-update.md`
- **Depends on**: —
- **Backlog**: B-7138d8d7
- **Action**: Add `counts: {ready, in-progress, stalled, unlinked, orphaned}` to the `backlog reconcile` report, filled in the existing `Bucket::ALL` loop in `reconcile()`, assert it in `tomlctl/tests/backlog_reconcile.rs`, and change `claude/commands/plan-update.md` to print the summary line from `counts` instead of asking for array lengths.
- **Detail**: Approach, "Reconcile counts". Keys are the bucket names exactly as `buckets` keys them; every key is present (zero when empty). In `plan-update.md` (the backlog step that runs `tomlctl backlog reconcile --flow <slug>` and prints `backlog: N ready, …`), replace the "lengths of those five `buckets` arrays" sentence with a bash-fenced `tomlctl backlog reconcile --flow <slug> --template 'backlog: {counts.ready} ready, {counts.in-progress} in-progress, {counts.stalled} stalled, {counts.unlinked} unlinked, {counts.orphaned} orphaned'`.
- **Acceptance**:
  - forward: `grep -c 'lengths of those five' claude/commands/plan-update.md` prints 0 (today: 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test backlog_reconcile` passes with a `counts` assertion — predicted, unverified; falsifier: omitting an empty bucket's key turns it red

### Wave B — writes

### 9. TOML record file reader [M]
- **Files**: `tomlctl/src/payload.rs` (new), `tomlctl/src/io.rs`, `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/payload.rs` converting a `.toml` record file into JSON text per the shape rule, declare it in `tomlctl/src/lib.rs`, and route `.toml` file paths through it from `read_ndjson_source` (bare and `@` branches) and from the `@path` branches of both `read_json_arg` and `read_json_value_from_arg` in `tomlctl/src/io.rs`.
- **Detail**: Approach, "TOML record files" — implement exactly its shape rule (the reserved `record` key), string normalisation and error forms. `read_json_value_from_arg` reads `@path` through `read_at_file` itself, not via `read_json_arg`; it feeds `items add/update --json`, `--defaults-json`, `set-json`, `array-append --json`, `flow record --json`, `inputs add` and `agents record`, so both hooks are needed for one `.toml` file to behave the same on every flag. API sketch: `pub(crate) fn toml_payload(path: &Path, text: &str, as_ndjson: bool) -> Result<String>`. Parse with `toml::de::DeTable::parse` (keeps per-row spans; `DeArray` derefs to `[Spanned<DeValue>]`; a `DeValue::String`'s span covers its raw token, delimiters included, which is how a multi-line string is told apart), convert rows with `crate::convert::detable_to_json`, then normalise multi-line strings recursively. Emit compact JSON in every form. Detection is by extension on file paths only; literal JSON, `-` stdin, and other extensions are unchanged. File reads carry no size cap today (only stdin is capped) and this task does not add one. Unit tests in `payload.rs`: `[[record]]` two rows → two NDJSON lines; single table → one object; `'''` prose with backticks, quotes, `$` and a trailing newline round-trips minus the one trailing newline; a single-line basic string ending in `\n` keeps it; CRLF input → LF; a top-level key beside `[[record]]` is refused naming the key; a syntax error carries `line`/`column`; a non-table array entry names `record N (line L)`.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/^payload::/)'` passes (today: fails, no tests match) — predicted, unverified; falsifier: dropping the trailing-newline trim turns the round-trip test red

### 10. NDJSON parse errors point at TOML record files [S]
- **Files**: `tomlctl/src/items.rs`
- **Depends on**: 9
- **Action**: Append a one-sentence hint to the JSON parse failures in `parse_ndjson_lines` and `parse_ndjson_objects` (`tomlctl/src/items.rs`) saying that prose-heavy rows can be written as a TOML record file (`[[record]]` tables with `'''` strings) passed to the same flag.
- **Detail**: Approach, "Agent learnability". Only the serde_json parse-failure arms gain the hint, not the "got JSON <type>" arm. Keep the existing `line N` prefix and messages intact (tests pin them); append after them. Add a unit test named `parse_ndjson_error_carries_toml_hint` near `parse_ndjson`'s existing error tests asserting the hint.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/ndjson.*toml_hint/)'` passes (today: fails, no tests match) — predicted, unverified; falsifier: removing the hint text turns it red

### 11. flow record accepts a per-row task [M]
- **Files**: `tomlctl/src/flow/record.rs`, `tomlctl/tests/flow_record.rs`
- **Depends on**: 9
- **Action**: Let each `flow record --ndjson` row carry `task` (a task id) resolved per row to `task_ref` in `tomlctl/src/flow/record.rs`, refusing a row whose `task` disagrees with `--task` or whose `task_ref` disagrees with its resolved task; add tests including a TOML record file of two tasks' deviations.
- **Detail**: Approach, "Batch write forms". Today `build_entries` shallow-extends each row over a base built from `--json` + field flags and `task_ref_for` resolves only the single `--task`. Resolve each row's `task` with the same lookup (load `tasks.toml` once per call), then remove `task` from the stored entry. Error text names the row's line. The TOML test writes `[[record]]` rows with `type = "deviation"`, distinct `task` ids, and `'''` `original_intent` / `rationale` / `summary`, and asserts two entries with the right `task_ref`s and verbatim prose.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test flow_record` passes including the per-row task and TOML cases — predicted, unverified; falsifier: applying `--task`'s ref to every row turns the two-task case red

### 12. tasks update --ndjson batch form [L]
- **Files**: `tomlctl/src/cli/types/tasks.rs`, `tomlctl/src/tasks/update.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/tests/tasks_write.rs`
- **Depends on**: 9
- **Backlog**: refs B-7138d8d7
- **Action**: Add `--ndjson <SRC>` to `tasks update` (`tomlctl/src/cli/types/tasks.rs`) accepting rows `{ids | id, status?, agent?, commit?, ref?, set?}`, apply every row in one `store::mutate` from `tomlctl/src/tasks/update.rs`, wire the arm in `tomlctl/src/tasks/dispatch.rs`, and test it in `tomlctl/tests/tasks_write.rs`.
- **Detail**: Approach, "Batch write forms". `--ndjson` conflicts with the positional ids and with `--status`/`--agent`/`--commit`/`--checkpoint`/`--ref`/`--set` (clap `conflicts_with_all`), and the `Update` variant adds `#[command(mut_args(..))]` making the shared `TaskIds` positional `required_unless_present("ndjson")` — `TaskIds` itself stays unchanged for the other verbs, and a conflict alone does not lift the requirement. `--unlock-import-fields`/`--relock-import-fields` keep applying to every row. `ids` accepts an array of integers or a comma-separated string; `ref` is legal only on a single-id row (today's `--ref` rule). `set` keys go through `SETTABLE`. Refuse duplicate ids across rows and unknown keys (naming the accepted ones). Reuse `update_many`'s per-row validation by building one `UpdateFields` per row; the envelope stays `{ok, rows:[{id, ref, changed}]}`. Tests: `--ndjson <file>` with no positional ids parses; a two-row commit train (different `commit` per row); a status+agent sweep; duplicate-id refusal; nothing written on a bad row; and the same train from a `.toml` record file.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test tasks_write` passes including the commit-train case — predicted, unverified; falsifier: writing rows one `mutate` at a time lets the bad-row case persist the first row, turning it red

### 13. backlog triage --ndjson batch form [L]
- **Files**: `tomlctl/src/cli/types/backlog.rs`, `tomlctl/src/backlog/triage.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/tests/backlog_write.rs`
- **Depends on**: 9
- **Backlog**: B-7138d8d7
- **Action**: Add `--ndjson <SRC>` to `backlog triage` taking rows `{id, resolve|dismiss|promote|reopen: <companion>, rationale?, expect_status?}`, building one `Transition` per row and applying all of them in one `mutate_doc_conditional`; test in `tomlctl/tests/backlog_write.rs`.
- **Detail**: Approach, "Batch write forms" (companion mapping there). `--ndjson` conflicts with the id list, the mode group and the companion flags, and the `Triage` variant relaxes both requirements on its own (`mut_args` making `ItemIds.ids` `required_unless_present("ndjson")`, `mut_group("TriageMode", |g| g.required(false))`), with the one-mode rule re-checked in dispatch for the flag form; the shared `ItemIds` stays unchanged for `show` and the other verbs. `--expect-status` on the line is each row's default; `--dry-run` previews. Build on `Transition::from_cli` (`backlog/triage.rs`) so per-row validation is identical, and on `apply_transition`'s stage-all-then-store path so one bad row writes nothing. Stale rows (status ≠ expected) go to `skipped_stale` as the id-list form reports them. Output shape: the id-list form's envelope. Tests: `--ndjson <file>` with no id or mode flag parses; the flag form with no mode is still refused; three ids with three different resolution texts in one call; mixed resolve+dismiss; a row with two mode keys refused; duplicate id refused; stale row skipped; a `.toml` record file with `'''` resolution prose.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test backlog_write` passes including the three-texts case — predicted, unverified; falsifier: applying the first row's companion to all ids turns it red

### 14. backlog get row-only read [L]
- **Files**: `tomlctl/src/cli/types/backlog.rs`, `tomlctl/src/backlog/query.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/tests/backlog_read.rs`
- **Depends on**: 7, 13
- **Backlog**: B-7138d8d7
- **Action**: Add `backlog get <ids>` printing only the stored row(s): a `Get` variant in `BacklogOp` (`tomlctl/src/cli/types/backlog.rs`), a `dispatch_get` beside `dispatch_show` in `tomlctl/src/backlog/query.rs` reusing its id resolution, the arm in `tomlctl/src/backlog/dispatch.rs`, and tests in `tomlctl/tests/backlog_read.rs`.
- **Detail**: Approach, "Backlog get and edit". Same `ItemIds` and `ReadIntegrityArgs` as `Show`; same unknown-id failure; one id → object, several → array (`print_report(Array, Rows::Top)`), de-duplicated in given order. Resolve ids exactly as `build_show`/`build_show_resolved` do (prefix/alias handling included) but return the row alone. Tests: `--get summary` prints the bare summary (no `item.` prefix); two ids → array; unknown id fails; output equals `show`'s `item` for the same id.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test backlog_read` passes including the `--get summary` case — predicted, unverified; falsifier: returning `build_show`'s wrapper makes `--get summary` fail with the `item.summary` suggestion

### 15. backlog edit core [M]
- **Files**: `tomlctl/src/backlog/edit.rs` (new), `tomlctl/src/backlog/mod.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/backlog/edit.rs` with the field policy (editable: `context`, `tags`, `evidence`, `origin`, `flow`, `base_sha`; refused with the owning verb named: everything else) and a pure `apply_edits(doc: &mut TomlValue, edits: &[Edit]) -> Result<Vec<EditOutcome>>` that applies replace/add/remove operations per id and validates the result; declare `mod edit;` in `tomlctl/src/backlog/mod.rs`.
- **Detail**: Approach, "Backlog get and edit" (refusal wording there). `Edit { id, context: Option<String>, tags: Option<Vec<String>>, add_tags, remove_tags, evidence: Option<Vec<String>>, add_evidence, origin, flow, base_sha }`. `backlog::schema::validate` is per-row over `&JsonValue`: validate each edited row as `schema::validate(&toml_to_json(&row))` and the document with `validate_ids_unique`, the idiom `backlog/relate.rs` uses. Unknown id → error naming it; duplicate id across edits → error; an edit that changes nothing reports `changed: false`. Tags keep their stored order with additions appended and duplicates dropped. Unit tests cover each field, each refusal message, unknown id, and validation failure leaving `doc` untouched (apply to a clone, swap on success). Nothing calls it until task 16 wires it (both land in checkpoint C); add no dead-code attribute — a transient `dead_code` warning between the two tasks is expected and denied by no lint.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/backlog::edit/)'` passes — predicted, unverified; falsifier: allowing `summary` turns the refusal test red

### 16. backlog edit CLI [M]
- **Files**: `tomlctl/src/cli/types/backlog.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/tests/backlog_edit.rs` (new)
- **Depends on**: 9, 14, 15
- **Backlog**: B-7138d8d7
- **Action**: Add `backlog edit <id>` with `--context`/`--context-file`/`--tags`/`--add-tag`/`--remove-tag`/`--evidence`/`--add-evidence`/`--origin`/`--flow`/`--base-sha` and `--ndjson <SRC>` (rows `{id, context?, tags?, evidence?, origin?, flow?, base_sha?}`), dispatching to `backlog::edit::apply_edits` under one `mutate_doc_conditional` with `last_updated` stamped and `--dry-run`; integration-test it in `tomlctl/tests/backlog_edit.rs`.
- **Detail**: Approach, "Backlog get and edit". Use `dispatch.rs`'s `literal_or_file` for `--context`/`--context-file` (as `triage` does for its companions). `--ndjson` conflicts with the positional id and every field flag, and the positional `id` is `required_unless_present("ndjson")` (a conflict alone does not lift a requirement); a row key outside the editable set is refused by the core policy. Output: `{ok, path, rows:[{id, changed}]}` through `print_rows_compact` with `Rows::Field("rows")`. Write through the backlog target helpers the other backlog writers use (`backlog/target.rs`), with the same integrity args. Tests: context replace from a file with backticks/quotes; add/remove tag; `--ndjson` two rows with no positional id; a `.toml` record file; `summary` refused naming `backlog add`; `status` refused naming `backlog triage`; `--dry-run` writes nothing.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test backlog_edit` passes — predicted, unverified; falsifier: skipping `schema::validate` lets an invalid edited row persist in the validation case

### 17. Owned-store guard module [M]
- **Files**: `tomlctl/src/store_guard.rs` (new), `tomlctl/src/lib.rs`, `tomlctl/src/cli/types/shared.rs`
- **Depends on**: 6, 9
- **Backlog**: B-7138d8d7
- **Action**: Create `tomlctl/src/store_guard.rs` with `refuse_owned_store(path: &Path, raw_store: bool) -> Result<()>` classifying the five owned store paths, declare it in `tomlctl/src/lib.rs`, and add the flattenable `RawStoreArg { --raw-store }` to `tomlctl/src/cli/types/shared.rs`.
- **Detail**: Approach, "Owned-store guard" (path table and message there). Classify on the trailing components of the resolved path — `canonicalize_for_write` (`tomlctl/src/io.rs`) when the file or its parent exists, else the path with `.`/`..` folded lexically — after normalising separators, case-insensitively on Windows; the file need not exist (auto-create targets must be caught too). Return `tagged_err(ErrorKind::Validation, Some(path), msg)`. The message leads with the owning group and the concrete replacement for that store: backlog → `backlog edit` (context/tags/evidence/origin), `backlog triage` (status); tasks → `tasks update` / `tasks add`, and `tasks import-plan` for `[[checkpoints]]`, which no other verb writes; execution record → `flow record`; inputs → the `inputs` verbs; review → the `review` verbs. Then `pass --raw-store to write it generically`. `--raw-store` help text: "Write a verb-owned store (backlog, tasks, execution record, inputs, review-v2) with this generic writer, bypassing its validator". Unit tests: each owned path refused; a `.claude/flows/x/../x/tasks.toml` path refused; `--raw-store` allows; `.claude/flows/x/context.toml`, `.claude/flows/x/review-ledger.toml` and `.claude/execution-record.toml` (no slug) allowed.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/store_guard/)'` passes — predicted, unverified; falsifier: matching on basename alone refuses `.claude/execution-record.toml`, turning the no-slug case red

### 18. Guard set, set-json and array-append [S]
- **Files**: `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch/doc.rs`
- **Depends on**: 17
- **Backlog**: refs B-7138d8d7
- **Action**: Flatten `RawStoreArg` into `Set`, `SetJson` and `ArrayAppend` (`tomlctl/src/cli/types/cmd.rs`) and call `store_guard::refuse_owned_store` before the write in `set`, `set_json` and `array_append` (`tomlctl/src/cli/dispatch/doc.rs`).
- **Detail**: Approach, "Owned-store guard". Call it after the target path is resolved and before any read-modify-write or `--dry-run` preview, so a refused call never touches the lock. No other behaviour changes.
- **Acceptance**:
  - forward: `grep -c 'refuse_owned_store' tomlctl/src/cli/dispatch/doc.rs` prints ≥3 (today: 0)
  - guard: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` reports no error in either file; the behaviour is tested in task 20

### 19. Guard the items writers [S]
- **Files**: `tomlctl/src/cli/types/items.rs`, `tomlctl/src/cli/dispatch/items.rs`
- **Depends on**: 7, 17
- **Backlog**: refs B-7138d8d7
- **Action**: Flatten `RawStoreArg` into `items add`, `add-many`, `update`, `remove`, `apply` and `backfill-dedup-id` (`tomlctl/src/cli/types/items.rs`) and call `store_guard::refuse_owned_store` in each arm in `tomlctl/src/cli/dispatch/items.rs` before the write.
- **Detail**: Approach, "Owned-store guard". Reads (`get`, `list`, `next-id`, `find-duplicates`, `orphans`, `sweep`, `clusters`) are not guarded; `sweep --update` is deliberately left unguarded too — it only rewrites `instances` on rows carrying a `sweep` array, which no owned store holds.
- **Acceptance**:
  - forward: `grep -c 'refuse_owned_store' tomlctl/src/cli/dispatch/items.rs` prints ≥6 (today: 0)
  - guard: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` reports no error in either file; the behaviour is tested in task 20

### 20. Owned-store guard tests [M]
- **Files**: `tomlctl/tests/store_guard.rs` (new), `tomlctl/tests/integration.rs`, `tomlctl/tests/review_runs.rs`
- **Depends on**: 18, 19
- **Backlog**: refs B-7138d8d7
- **Action**: Add `tomlctl/tests/store_guard.rs` exercising the guard through the CLI for every generic writer and owned store, and add `--raw-store` to the two existing tests that write an owned store generically: `write_under_claude_auto_creates_missing_parent_dirs` in `tomlctl/tests/integration.rs` and `run_ids_never_reuse_a_removed_id` in `tomlctl/tests/review_runs.rs`.
- **Detail**: Approach, "Owned-store guard". The new test asserts, for each of `set`, `set-json`, `array-append`, `items add|update|remove|apply|add-many|backfill-dedup-id`: refused on `.claude/backlog.toml` and `.claude/flows/s/tasks.toml` (exit 1, stderr names the replacement verb and `--raw-store`, file bytes unchanged); allowed with `--raw-store`; allowed on `.claude/flows/s/review-ledger.toml`. Also assert `items get`/`list` on an owned store still work. The two existing cases are the only owned-store generic writes in `tomlctl/tests/`: `write_under_claude_auto_creates_missing_parent_dirs` runs `set` on `.claude/flows/hashed-kindling-engelbart/execution-record.toml`, and `run_ids_never_reuse_a_removed_id` runs `items remove` on `.claude/review-v2/<scope>.toml` (`repo.store()`). The `integrity_refresh_*` tests in `integration.rs` only read and refresh and stay unchanged, and `.claude/execution-record.toml` (no slug) cases are not owned. Do not run the full suite to look for more — the checkpoint gate does.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test store_guard` passes — predicted, unverified; falsifier: removing the guard call from `cli/dispatch/doc.rs` turns the `set` case red
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test integration --test review_runs` passes

### 21. TOML record files end to end [M]
- **Files**: `tomlctl/tests/payload_toml.rs` (new)
- **Depends on**: 11, 12, 13, 16
- **Action**: Add `tomlctl/tests/payload_toml.rs` driving TOML record files through `items add-many --ndjson`, `items apply --ops '@ops.toml'`, `items add --json '@one.toml'`, `items add-many --defaults-json '@defaults.toml'`, `backlog add-many --ndjson`, `backlog check --ndjson`, `flow record --ndjson`, `tasks update --ndjson`, `backlog triage --ndjson` and `backlog edit --ndjson`, asserting prose with backticks, quotes, `$`, `\` and newlines lands verbatim.
- **Detail**: Approach, "TOML record files". Use `'''` strings with the closing delimiter on its own line (asserting one trailing newline is trimmed) and one `"""` string containing `'''`. Include a CRLF-encoded file (written with explicit `\r\n` bytes), a malformed file whose error names `line`/`column`, and a file with a top-level key beside `[[record]]` that is refused. Seed stores the way neighbouring test files do (`tests/common/mod.rs` helpers). Generic `items` writers here target a plain ledger, not an owned store.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test payload_toml` passes — predicted, unverified; falsifier: hooking only `read_ndjson_source` and `read_json_arg` (not `read_json_value_from_arg`) turns the `--json` and `--defaults-json` cases red

### 22. Output-options cases for new and widened verbs [S]
- **Files**: `tomlctl/tests/output_options.rs`
- **Depends on**: 4, 5, 14, 16, 20
- **Action**: Add `cases()` entries in `tomlctl/tests/output_options.rs` for `backlog get` (One and Top), `backlog edit` (write envelope with rows), the multi-id `items get` (Top), and one `--text-view` case per shape including one list verb (`tasks list … --text-view`), so `every_leaf_command_has_a_case` and the shape assertions cover them.
- **Detail**: Approach, "Backlog get and edit" and "Output view and refusals". Follow the neighbouring backlog cases' seeding. The list-verb `--text-view` case covers the `query_opts` path task 4 wires. The file's existing generic-writer cases target `.claude/review-ledger.toml`, which the guard leaves alone; edit no existing case.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test output_options` passes — predicted, unverified; falsifier: deleting the `backlog get` case fails `every_leaf_command_has_a_case`

### Wave C — registration, docs, agents

### 23. Register capabilities features [M]
- **Files**: `tomlctl/src/cli/types/mod.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 1, 2, 3, 4, 7, 8, 11, 12, 13, 14, 16, 20
- **Action**: Add the fourteen feature names from Approach, "Capabilities and docs" to `FEATURES` (`tomlctl/src/cli/types/mod.rs`); extend the expected lists in `tomlctl/tests/capabilities.rs` (features, and the global-flags list for `--text-view`); transcribe them into `tomlctl/README.md`'s features sample and "Feature meanings" table; update the README lines describing single-id `items get`, `backlog show`, and the `rows_header` refusals; add synopsis lines for `backlog get`, `backlog edit`, `tasks update --ndjson` and `backlog triage --ndjson`, `--text-view` to the output-options comment and `--grep` to the `items list` query-surface sentence; and update the `--template` help in `tomlctl/src/cli/types/mod.rs` to name the `\n` `\t` `\\` escapes.
- **Detail**: Approach, "Capabilities and docs". Give `items_get_multi` its own `FEATURES` entry and README table row (`readme_feature_transcriptions_match_capabilities_features` needs one row per advertised feature, and `capabilities_features_contains_every_plan_feature` asserts an exact count); the `multi_id` row may also mention `items get`. `SUBCOMMANDS` lists only top-level commands, so it needs no edit. Keep feature order consistent with how the list is appended today. `command_lint` does not scan the README, so check its synopsis against the shipped flags by reading the clap definitions.
- **Acceptance**: forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test capabilities` passes with `text_view` in the expected list — predicted, unverified; falsifier: omitting `text_view` from `FEATURES` fails `capabilities_features_contains_every_plan_feature`

### 24. tomlctl SKILL.md: output options and guidance canon [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 2, 3, 4, 9, 12, 17, 18, 19, 26
- **Backlog**: refs B-7138d8d7
- **Action**: Update `claude/skills/tomlctl/SKILL.md`: `--text-view` and template escapes in the Output options table; the relaxed `--rows`/`--header`/`--template --select` rules in the rows-versus-header and Combinations paragraphs; Guidance canon bullets for TOML record files (several prose fields → one record file; `--set-file` stays for one field) and the owned-store guard; Quick Reference rows for `backlog get`, `backlog edit`, `items get <ids>`, `--grep`, `tasks update --ndjson`, `backlog triage --ndjson`; and a References entry for `references/backlog-edit.md`.
- **Detail**: Approach, "Capabilities and docs". Bash-fenced commands are parsed by `command_lint`, so every example must match the shipped flags. Stay well under the 500-line ceiling (today 285).
- **Acceptance**:
  - forward: `grep -c -- '--text-view' claude/skills/tomlctl/SKILL.md` prints ≥1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 25. query.md: --grep, regex, items get [S]
- **Files**: `claude/skills/tomlctl/references/query.md`
- **Depends on**: 1, 5, 7
- **Backlog**: B-7138d8d7
- **Action**: In `claude/skills/tomlctl/references/query.md`, add `--grep` / `--grep-field` rows to the list-verb filter flag table with the per-store default field sets, replace the ASCII-only regex notes (`--where-regex` row and the sweep-pattern section) with the Unicode behaviour plus the `(?-u:\b)` speed note, the `\p{..}` exclusion and the size limit on Unicode `\w`/`\W` repeats above about 20, and document `items get <ids>`.
- **Detail**: Approach, "Search" and "Regex". `flag_table_lint` requires each table flag to exist on the heading's subcommand.
- **Acceptance**:
  - forward: `grep -c '(?i-u)' claude/skills/tomlctl/references/query.md` prints 0 (today: 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 26. Backlog references: get, edit, triage batch [M]
- **Files**: `claude/skills/tomlctl/references/backlog.md`, `claude/skills/tomlctl/references/backlog-edit.md` (new)
- **Depends on**: 13, 14, 16
- **Backlog**: refs B-7138d8d7
- **Action**: Create `claude/skills/tomlctl/references/backlog-edit.md` with flag tables under the headings `` ## `backlog get` `` and `` ## `backlog edit` `` (editable fields, refusals, `--ndjson` row shape) and the `backlog triage --ndjson` row shape; in `backlog.md` add the triage `--ndjson` row, drop the now-stale "never with `--rows results`" note, and link `backlog-edit.md`.
- **Detail**: Approach, "Backlog get and edit" and "Batch write forms". `backlog.md` is at 597 of 600 lines — net its line count to ≤600 (the removed note frees lines). The new file opens with a `## Contents` list like its siblings.
- **Acceptance**:
  - forward: `grep -c 'rows results' claude/skills/tomlctl/references/backlog.md` prints 0 (today: 1)
  - forward: `test -f claude/skills/tomlctl/references/backlog-edit.md` exits 0 (today: absent)
  - guard: `wc -l < claude/skills/tomlctl/references/backlog.md` prints ≤600 (today: 597)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 27. Backlog recipes and reconcile references [S]
- **Files**: `claude/skills/tomlctl/references/backlog-recipes.md`, `claude/skills/tomlctl/references/backlog-reconcile.md`
- **Depends on**: 4, 7, 8, 13, 14, 16
- **Backlog**: B-7138d8d7
- **Action**: Rewrite `claude/skills/tomlctl/references/backlog-recipes.md` from interim workarounds into one-call recipes (`backlog get`, `backlog list --grep`, `backlog edit`, `backlog triage --ndjson` with a TOML record file, `--text-view` reading), and document the `counts` map in `claude/skills/tomlctl/references/backlog-reconcile.md`'s output shape.
- **Detail**: Approach, "Search", "Backlog get and edit", "Reconcile counts". Remove the `(?i-u)` recipe, the per-id triage shell loop, the two-call summary-or-context search and the `items update … --array backlog` edit recipe.
- **Acceptance**:
  - forward: `grep -c 'items update .claude/backlog.toml' claude/skills/tomlctl/references/backlog-recipes.md` prints 0 (today: 1)
  - forward: `grep -c '(?i-u)' claude/skills/tomlctl/references/backlog-recipes.md` prints 0 (today: 2)
  - forward: `grep -c 'counts' claude/skills/tomlctl/references/backlog-reconcile.md` prints ≥1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 28. write.md: TOML record files and the store guard [M]
- **Files**: `claude/skills/tomlctl/references/write.md`
- **Depends on**: 9, 12, 13, 16, 17, 18, 19
- **Backlog**: refs B-7138d8d7
- **Action**: Add a "TOML record files" section to `claude/skills/tomlctl/references/write.md` (where they are accepted, the shape rule and the reserved `record` key, the authoring example, `'''` and `"""` rules, CRLF and trailing-newline normalisation of multi-line strings, `line N` = record N, and that TOML has no `null`, so an `items apply` op's `expect` absent-field precondition needs the JSON form) and an "Owned stores" section (the five paths, the refusal, `--raw-store`).
- **Detail**: Approach, "TOML record files" and "Owned-store guard". Put the example in a ```` ```toml ```` fence (not bash) and any command in a bash fence so `command_lint` checks it. Stay under the 600-line reference ceiling (today 475).
- **Acceptance**:
  - forward: `grep -c 'raw-store' claude/skills/tomlctl/references/write.md` prints ≥1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 29. tasks and flow references: batch update and per-row task [S]
- **Files**: `claude/skills/tomlctl/references/tasks-write.md`, `claude/skills/tomlctl/references/flow.md`, `claude/skills/flow-contract-execution-record-schema/SKILL.md`
- **Depends on**: 11, 12
- **Action**: Document `tasks update --ndjson` (flag row, row shape, conflicts) in `claude/skills/tomlctl/references/tasks-write.md`, and the per-row `task` key of `flow record --ndjson` in `claude/skills/tomlctl/references/flow.md` and in the `flow record` write contract of `claude/skills/flow-contract-execution-record-schema/SKILL.md` (one call for N tasks' records, TOML record file preferred for prose), each with a bash-fenced `--ndjson` example.
- **Detail**: Approach, "Batch write forms".
- **Acceptance**:
  - forward: `grep -c -- '--ndjson' claude/skills/tomlctl/references/tasks-write.md` prints ≥3 (today: 2)
  - forward: `grep -c -- '--ndjson' claude/skills/tomlctl/references/flow.md` prints ≥5 (today: 4)
  - forward: `grep -c -- '--ndjson' claude/skills/flow-contract-execution-record-schema/SKILL.md` prints ≥3 (today: 2)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 30. Shared tomlctl-for-agents block [L]
- **Files**: `scripts/shared-blocks.toml`, `claude/agents/implement-deep.md`, `claude/agents/implement-lite.md`, `claude/agents/research-deep.md`, `claude/agents/research-lite.md`
- **Depends on**: 4, 7, 9, 14
- **Backlog**: B-7138d8d7
- **Action**: Register a `tomlctl-for-agents` block in `scripts/shared-blocks.toml` and insert the byte-identical block (between `<!-- SHARED-BLOCK:tomlctl-for-agents START -->` / `END -->` markers) into the four agent files.
- **Detail**: Approach, "Agent learnability" (content there; ≤14 lines, one bash fence of read one-liners, no write guidance). Place it after the `backlog-candidates` block in the implement pair and after `research-read-only` in the research pair. Keep `tasks show <ids> --slug <slug>` literal where it already appears: `agents/correlate.rs` matches that form in dispatch prompts. Byte identity is checked by `scripts/verify-shared-blocks.sh` (needs gawk).
- **Acceptance**:
  - forward: `grep -l 'SHARED-BLOCK:tomlctl-for-agents START' claude/agents/*.md | wc -l` prints 4 (today: 0)
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 31. Carrier Explore prompts carry the read one-liners [S]
- **Files**: `claude/commands/plan-new.md`, `claude/commands/review.md`, `claude/commands/optimise.md`
- **Depends on**: 4, 7, 14
- **Action**: In each command's Explore dispatch instructions (`claude/commands/plan-new.md` Phase 2, `claude/commands/review.md`, `claude/commands/optimise.md`), add one sentence plus a bash fence telling Explore agents to read flow stores with `tomlctl backlog get`, `tomlctl tasks show <ids> --slug <slug> --with body --text-view` and `tomlctl backlog list --grep <regex> --text-view` rather than grepping `.claude/*.toml`.
- **Detail**: Approach, "Agent learnability". Built-in Explore agents cannot carry a shared block, so the carriers' prompt text is the only lever.
- **Acceptance**:
  - forward: `grep -L 'tomlctl backlog get' claude/commands/plan-new.md claude/commands/review.md claude/commands/optimise.md` prints nothing (today: all three)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 32. Apply pre-analysis Explore prompt [S]
- **Files**: `claude/skills/flow-contract-apply-pipeline/references/pre-analysis.md`
- **Depends on**: 4, 7, 14
- **Action**: Add the same read one-liners (sentence + bash fence as in task 31) to the Explore dispatch in `claude/skills/flow-contract-apply-pipeline/references/pre-analysis.md`, plus `tomlctl items get <ledger> <ids> --text-view` for ledger rows.
- **Detail**: Approach, "Agent learnability".
- **Acceptance**:
  - forward: `grep -c 'items get' claude/skills/flow-contract-apply-pipeline/references/pre-analysis.md` prints ≥1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 33. Correct the no-Unicode statements in tomlctl source [S]
- **Files**: `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/tasks/parse_tasks.rs`, `tomlctl/src/tasks/parse_policy.rs`
- **Depends on**: 1, 18
- **Backlog**: refs B-7138d8d7
- **Action**: Rewrite the statements that the binary has no Unicode classes: the `Sweep` doc comment in `tomlctl/src/cli/types/cmd.rs` (it is the `sweep --help` text), and the module docs and `every_pattern_compiles` comments in `tomlctl/src/tasks/parse_tasks.rs` and `tomlctl/src/tasks/parse_policy.rs`.
- **Detail**: Approach, "Regex". After task 1 the release build compiles `(?i)`, `\d`, `\w`, `\s` and `\b`, but not `\p{..}` classes, and Unicode `\w`/`\W` repeats above about 20 exceed the compile-size limit. Keep `(?-u:\b)` as the documented fast form for `sweep`. Change prose only — leave every regex pattern and every test as it is. The `every_pattern_compiles` scans keep running; restate why (test builds unify a different regex feature set from the shipped binary) rather than deleting them.
- **Acceptance**:
  - forward: `grep -c 'no Unicode tables' tomlctl/src/cli/types/cmd.rs` prints 0 (today: 1)
  - forward: `grep -l 'without its unicode features' tomlctl/src/tasks/parse_tasks.rs tomlctl/src/tasks/parse_policy.rs` prints nothing (today: both)
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/every_pattern_compiles/)'` passes

### 34. Correct the no-Unicode statements in test headers and the research method [S]
- **Files**: `tomlctl/tests/sweep.rs`, `tomlctl/tests/tasks_corpus.rs`, `claude/skills/research-methods/references/codebase-review.md`
- **Depends on**: 1
- **Backlog**: refs B-7138d8d7
- **Action**: Rewrite the header comment of `tomlctl/tests/sweep.rs`, the hand-rolled-pattern doc comment in `tomlctl/tests/tasks_corpus.rs`, and the sweep paragraph of `claude/skills/research-methods/references/codebase-review.md` so none claims the shipped binary has no Unicode tables or refuses a bare `\b`.
- **Detail**: Approach, "Regex". The tests keep their `(?-u:…)` patterns and their reasoning that test builds unify a wider regex feature set than the shipped binary; only the claim about what the binary refuses changes. In `codebase-review.md`, keep recommending `(?-u:\b)` / `(?-u:\w)` / `(?-u:\d)` as the fast forms and the 512-byte cap.
- **Acceptance**:
  - forward: `grep -l 'no Unicode tables\|the binary refuses\|without its unicode features' tomlctl/tests/sweep.rs tomlctl/tests/tasks_corpus.rs claude/skills/research-methods/references/codebase-review.md` prints nothing (today: all three)
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --test sweep` passes

### 35. Correct the no-Unicode statements in the review-v2 lenses [S]
- **Files**: `claude/skills/review-v2-lenses/vocabulary.toml`, `claude/skills/review-v2-lenses/SKILL.md`, `claude/skills/review-v2-lenses/methods/sweep.md`
- **Depends on**: 1
- **Backlog**: refs B-7138d8d7
- **Action**: Rewrite the header comment of `claude/skills/review-v2-lenses/vocabulary.toml`, the `contains` detector bullet in `claude/skills/review-v2-lenses/SKILL.md` and step 4 of `claude/skills/review-v2-lenses/methods/sweep.md` so they no longer say the regex build has no Unicode tables or forbid a bare `(?i)`.
- **Detail**: Approach, "Regex". Keep `(?-u:\b)` and `(?i-u:…)` as the recommended forms (a Unicode `\b` drops the lazy DFA to the PikeVM) and the 512-byte cap; change no pattern in `vocabulary.toml` — only the comment.
- **Acceptance**:
  - forward: `grep -l 'Unicode tables\|compiled ASCII-only' claude/skills/review-v2-lenses/vocabulary.toml claude/skills/review-v2-lenses/SKILL.md claude/skills/review-v2-lenses/methods/sweep.md` prints nothing (today: all three)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 36. Carrier write recipes use record files and batch forms [M]
- **Files**: `claude/commands/implement.md`, `claude/skills/flow-contract-task-store/SKILL.md`, `claude/skills/backlog-capture/SKILL.md`
- **Depends on**: 11, 12, 14
- **Backlog**: refs B-7138d8d7
- **Action**: In `claude/commands/implement.md`, replace the 3b deviation recipe's three `--set-file` stagings with one TOML record file passed to `tomlctl flow record --slug <slug> --type deviation --set agent=implement --ndjson <file>` (one `[[record]]` per deviation, carrying `task`); in `claude/skills/flow-contract-task-store/SKILL.md` add the `--ndjson` batch form to the `update` verb semantics; in `claude/skills/backlog-capture/SKILL.md` switch the `backlog show … {item.*}` reads to `backlog get`.
- **Detail**: Approach, "Agent learnability" and "Batch write forms". Keep `implement.md`'s per-group commit-train `tasks update <ids> --commit <sha>` and its per-task `task-completion` records (5b) one call per entry: each is recorded right after the event it records, for crash safety. Every bash-fenced command is parsed by `command_lint` against the shipped CLI.
- **Acceptance**:
  - forward: `grep -c 'set-file original_intent' claude/commands/implement.md` prints 0 (today: 1)
  - forward: `grep -c 'item\.' claude/skills/backlog-capture/SKILL.md` prints 0 (today: 1)
  - forward: `grep -c -- '--ndjson' claude/skills/flow-contract-task-store/SKILL.md` prints ≥4 (today: 3)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 37. plan-update recipes and reconcile-agent read one-liners [S]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 4, 7, 8, 11, 14
- **Backlog**: refs B-7138d8d7
- **Action**: In `claude/commands/plan-update.md`, move the deviation and deferral `flow record` recipes (three `--set-file` stagings each) to one TOML record file through `flow record --ndjson`, and add the read one-liners sentence + bash fence (as in task 31) to the `general-purpose` reconcile and catchup agent dispatches.
- **Detail**: Approach, "Agent learnability". Task 8 edits the same file's reconcile summary line first; keep its change.
- **Acceptance**:
  - forward: `grep -c 'set-file original_intent' claude/commands/plan-update.md` prints 0 (today: 1)
  - forward: `grep -c 'tomlctl backlog get' claude/commands/plan-update.md` prints ≥1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes

### 38. Escape the --set-json doc comment placeholder for rustdoc [S]
- **Files**: `tomlctl/src/fields.rs`
- **Depends on**: 23
- **Action**: In tomlctl/src/fields.rs, wrap KEY=@<path> in the --set-json doc comment in backticks so rustdoc no longer reads <path> as an unclosed HTML tag.
- **Acceptance**: cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items exits 0

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 3, 4, 7, 8 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8. Unicode regex, relaxed refusals, template escapes, `--text-view`, multi-id `items get`, `--grep`, reconcile counts — the read/output layer, buildable and tested on its own

— CHECKPOINT B after tasks 20 — dependency closure: 1, 5, 6, 7, 9, 17, 18, 19, 20. The owned-store guard with its tests and the two existing tests it re-flags — the one change that refuses writes that work today, landed as its own verified, revertable tip

— CHECKPOINT C after tasks 10, 21, 22 — dependency closure: 1, 2, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22. TOML record files, the batch write forms, `backlog get`/`edit` and their test registries — every remaining write-side change plus the leaf-case registry that must land with the new verbs

— CHECKPOINT D after tasks 24, 25, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38. Capabilities registration, reference docs, the no-Unicode corrections and the agent/carrier learnability edits, gated by `command_lint`/`flag_table_lint` against the shipped CLI

## Risks
- **The guard breaks a caller nobody listed.** Before the plan, `claude/**`, `scripts/**`, `.githooks/**` and `glimpse/src/**` were grepped; only `backlog-recipes.md:46` writes an owned store generically, and task 27 removes it. The transcript survey agrees: since `flow record` shipped, the only owned-store generic writes are `items update .claude/backlog.toml`; one earlier `array-append tasks.toml checkpoints` has no owning verb, so the tasks refusal also names `tasks import-plan`. In `tomlctl/tests/`, the two generic writes on owned stores are named in task 20. Mitigation: the refusal names `--raw-store` and the replacement verb, and the guard has its own checkpoint (B).
- **The TOML shape rule misreads a payload.** A single-object payload whose only key is `record`, holding an array of tables, is read as rows. Mitigation: the rule is documented, `record` beside any other key is refused rather than folded, and no verb's payload has a `record` field.
- **Trailing-newline trimming changes a value.** Every multi-line string loses one trailing `\n`. Mitigation: this mirrors `--set-file` (`read_flag_file`), which agents already rely on, and single-line strings are left as written.
- **Unicode `\b` slows `sweep`.** Mitigation: the docs point at `(?-u:\b)`, and `anchor.rs` already uses it. Measure if sweep timings regress.
- **Binary growth from the Unicode tables.** Mitigation: measured +139,264 bytes on a minimal LTO+strip binary (regex 1.13.1, these two features, 2026-10-21); final verification measures the release exe size, and growth over about 200 KB is stop-and-report.
- **`--raw-store` is reachable without a prompt** under `Bash(tomlctl *)`. Mitigation: the After Merge `ask` rules.
- **`tasks update --ndjson` and `ref` edits.** A row's `ref` rename would be applied inside a batch. Mitigation: `ref` is legal only on single-id rows, as `--ref` is today.

## After Merge
- `cargo install --path tomlctl` (the carriers shell out to the installed binary).
- `pwsh -File scripts/deploy-claude.ps1 -Check` to confirm the agent and skill links still resolve; restart open Claude Code sessions so the agents load the new shared block.
- Reinstall glimpse if it is installed from source, since it links tomlctl's library.
- Add `ask` permission rules `Bash(tomlctl * --raw-store)` and `Bash(tomlctl * --raw-store *)` to `.claude/settings.json` beside the `--allow-outside` deny rules, so an agent cannot take the guard's bypass without a prompt. This is a permissions change, made by hand.
- Update the memory notes `tomlctl_tool.md` and `feedback_heredoc_windows.md` to name TOML record files as the preferred multi-field payload.

## Exploration Notes

### Origin evidence (transcript survey, 2026-10-21)
- 7,189 tomlctl invocations, 123 sessions, 2026-09-01..10-21; only 145 (~2%) failed — cost is workaround volume.
- Features shipped 10-07/09 (id lists, global output options, `flow record`) collapsed python pipes 123→7 per 1k, heredocs 123→0, `items next-id` 65→0.
- Still live since 10-09 (~2.6k invocations): 481 `--set-file` staging commands (465 main; fields summary 628, rationale 292, original_intent 262; verbs flow record 259, tasks update 214) averaging 1.7k chars; per-id write loops flat (37→45/k: commit trains, per-task flow record, per-id triage); `items get` loops rising (0→24/k); `backlog show` `['item']` shape guessing; tasks-show prose re-formatting via python/node; 36 `list | grep -i` searches + 80 raw `.claude/*.toml` greps by sub-agents; 0/~1,400 sub-agent transcripts loaded the tomlctl skill, 62 `--help` lookups, 149 `2>/dev/null`; 4 regex `(?i)` refusals; ~6 `--rows`/`--header` refusals.
- Raw data: `C:\Users\rossa\AppData\Local\Temp\claude\C--Users-rossa-dev-dev-tools\28f7fae3-a80f-4d63-a47a-3005fa339f21\scratchpad\` (`calls.ndjson`, `report2.txt`, `trend.py`).

### Output layer (`tomlctl/src/output.rs`, `output/template.rs`)
- `OutputOpts` :34, `validate()` :148 (combination refusals :174-204 incl. ``"`--template` cannot be combined with `--select`"`` :186), `configure()` :214; `Shape {One, Rows}` :229; `Rows {Top, Field(&'static str)}` :1109.
- Emitters decide shape: `print_json` :805 / `print_json_line` :815 / `print_json_compact` :1080 (single object; write envelopes lenient via `compact_lenient` :828); `print_report(report, Rows)` :1141, `print_rows_compact` :1095; list verbs `print_query_list` :969, options rebuilt by `query_opts` :899 (field whitelist) with `streaming_allowed` :888.
- Refusals: `rows_on_row_report` :330-338 raised :567 (backlog check `--ndjson` declares `Rows::Field("results")` at `backlog/check.rs:680`; single probe `Rows::Field("candidates")` :531); `HEADER_ON_ONE` :323 raised :483 (`tasks render --check` uses `print_json` at `tasks/dispatch.rs:323` though it has `findings`; `tasks check` uses `Rows::Field("findings")` :295; `backlog reconcile` `print_json_line/compact` at `backlog/reconcile.rs:127-130`); list-verb refusal `refuse_row_flags_on_list` :914/:923. Unit test pins: `rows_on_row_report` :2119, `header_is_refused_on_a_bare_row_array_and_a_single_report` :2199-2206, list-verb ~2279. `tests/output_options.rs` `read_options` (:1532-1540) also probes `--header` refusal on every single-object case.
- Template: `Template::parse` (`template.rs:34`) knows `{path}`, `{path:N}`, `{{`, `}}` only — no backslash escapes; `value_text` :161; `push_line` :306.
- Global flags declared `cli/types/mod.rs` (`OutputArgs` :213, `GlobalWhereArgs` :305, `FEATURES` :38), wired `cli/dispatch.rs` `output_opts()` :124 (exhaustive `OutputOpts` literal), `run()` :154. `inputs answer` declares a local `--text <TEXT>` (`cli/types/inputs.rs:110-111`).

### Regex
- `tomlctl/Cargo.toml:56` `regex = { default-features=false, features=["std","perf"] }` (regex 1.13.1, regex-syntax 0.8.11). User patterns compiled at `query.rs:126 compile_user_regex` (`--where-regex`, :1010) and `query.rs:139 compile_user_bytes_regex` (sweep `sweep.rs:144`, `review/survey.rs:263`, `review/catalogue.rs:1092`). Errors carry `REGEX_UNICODE_HINT` `query.rs:95` (test ~:3394). `agents/correlate.rs:192` comment relies on no-Unicode classes.
- Dev-dep `predicates` turns on `regex/unicode` in tests → tests accept patterns release rejects; a release-binary check is the only proof.
- glimpse gets regex only via tomlctl; the feature change flows to glimpse. No `glimpse/Cargo.lock` refresh is needed: features never change a lockfile, and `cargo tree … --locked` succeeds on both manifests.

### Read verbs
- `items get` single id: `cli/types/items.rs:72` `Get{file,id:String,array,integrity}`, handler `cli/dispatch/items.rs:172-187` → `items_get_from` (`items.rs:206`) / `_from_json` :231.
- Multi-id parsers to reuse: `ItemIds` (`cli/types/backlog.rs:436`), `TaskIds` (`cli/types/tasks.rs:38`, `ids_help` :30); output pattern dedupe via BTreeSet, build all, one id → object else `print_report(Array, Rows::Top)` (`backlog/query.rs:67-83`, `tasks/show.rs:27-45`).
- `backlog show` builds `{item, evidence, neighbours}` in `build_show_resolved` (`backlog/query.rs:189-240`); no row-only mode.
- One query engine: `query.rs` (`Predicate` :162, `predicate_cost` :188, `PreparedPredicates::matches` :1071 ANDs, `eval_predicate` :1181, `WhereFilters` :1920, `Query::from_query_input` :2122); `json_rows::filter` :15 for global `--where*`. `items list` (dispatch/items.rs:149), `tasks list` (`tasks/list.rs:20`), `backlog list` (`backlog/query.rs:39`, pre-filter `filter_backlog` :94). `QueryArgs` `cli/types/shared.rs:121`, `to_query_input` :289 (the only `QueryInput` construction site). Task fields `TaskRow` `tasks/schema.rs:187`; backlog field consts `backlog/schema.rs:41-81` (`dismiss_reason`, `reopen_rationale`); item ledgers schemaless.
- `backlog reconcile` report built `backlog/reconcile.rs:134-184`, bucket loop :147-156 over `Bucket::ALL` :39.

### Write surface
- Field flags `fields.rs:17 FieldArgs{set,set_json,set_file}`, `build()` :158; `--set-file` via `io.rs:390 read_flag_file` (strips BOM + one trailing newline); accepted by items add/update, array-append, flow record, review run start/update. `tasks update` has plain `--set` (`types/tasks.rs:270`, `update.rs:175`, `SETTABLE` `update.rs:31`) and NO `--set-file`/`--type`/`--task`.
- Payload readers `io.rs`: `at_file_path` :276, `read_at_file` :282 (uncapped), `read_json_arg` :319, `read_ndjson_source` :345, `read_text_arg` :361, `read_prose` :400, `read_json_value_from_arg` :469 (own `@path` branch via `read_at_file`, :478-480); `claim_stdin` :263 (one stdin consumer), 32 MiB cap :231 (stdin only). NDJSON parsers `items.rs:1366 parse_ndjson`, `:1419 parse_ndjson_objects -> Vec<(line, Map)>`, `:1377 parse_apply_ops` (a payload starting with `{` is read as NDJSON).
- `--ndjson` today: flow record, backlog add-many, backlog check, items add-many, tasks add-many, array-append, inputs handle, review cells. Not: tasks update, backlog triage.
- `flow record` (`flow/record.rs:56`): `build_entries` :212 (base = --json + field flags; NDJSON rows shallow-extended), `fill_defaults` :275, `task_ref_for` :191 (single `--task` for every row; rows may carry `task_ref` string but not a task id), one `mutate_doc_conditional` in `append` :327; caps `record_schema.rs:128` (truncate, not refuse); deviation needs original_intent + rationale (:80).
- `tasks update` `update.rs:94 update_many`: one value set over N ids in one `store::mutate` (`store.rs:62`); no flow record. Its ids come from the shared `TaskIds` (`required_unless_present = "id"`).
- `backlog triage` (`types/backlog.rs:231`, `TriageMode` `#[group(required = true, multiple = false)]` :469, `triage.rs:85 Transition::from_cli`, `:211 apply_transition` stages all then stores, `:340 triage_value` one `mutate_doc_conditional`, `--expect-status` skips stale); one companion text for all ids; `-file` twins via `dispatch.rs:17 literal_or_file`.
- Per-id prose batch precedent: `inputs.rs:326 handle_rows` (`{id, note}`), `:296 handle_each` refuses duplicate ids, one write.
- Backlog validator `backlog/schema.rs:438 validate` (per row, over `&JsonValue`; callers use `validate(&toml_to_json(&row))`, `backlog/relate.rs:125`), `:509 validate_ids_unique` — only backlog verbs call them. `set` (`cli/dispatch/doc.rs:82`) and `items update --array backlog` (`dispatch/items.rs:534`) bypass; `items update` recomputes `dedup_id` with the review-ledger fingerprint (`dedup.rs:33`, hook `items.rs:108`) when the patch touches a fingerprinted field — wrong for backlog (`backlog/ids.rs:32` kind+area+summary). Safe in-place fields: context, tags, evidence, origin, flow, base_sha; derived: summary, kind, area; triage-owned: status + MANAGED_FIELDS; minted: created, last_seen, seen_count.
- No per-store write guard; analogues `doc.rs:29 refuse_json_extension_for_toml_writers`, `json.rs:65`, `io.rs:769 SCHEMA_SEEDED_FLOW_FILES`/`:791 is_schema_seeded`, `:921 stamped_at`, `:1737 canonicalize_for_write`.
- All-or-nothing batch pattern: validate outside lock, mutate staged copy inside `mutate_doc_conditional` (`io.rs:1171`) / `mutate_doc_plan` (:1225).
- No TOML payload format exists; building blocks `convert.rs:491 toml_to_json`, `:532 detable_to_json`; toml 1.1.8, serde_json 1.0.154, clap 4.6.7.

### Docs, agents, gates
- Docs carrying the limitations: `claude/skills/tomlctl/SKILL.md` (:97, :102, :117, :121, :123, :125, :181), `references/query.md` (:97, :209, :215-228, :329-334), `backlog.md` (:157-159, :196-217; 597/600 lines — no headroom), `backlog-recipes.md` (all interim), `backlog-reconcile.md:85-114`, `write.md`, `flow.md:36`, `tomlctl/README.md` (:31, :63, :439, :472, features block + "Feature meanings" table).
- Gates: `cli/dispatch/tests/lint.rs` (`command_lint` :396 parses every bash-fenced `tomlctl` line under `claude/`; `flag_table_lint` :1315 one-way; `guidance_lint` rules pipe-to-text-tool, heredoc-into-tomlctl, stderr-redirect, repeated-set, items-next-id), `skills.rs` (line ceilings 500/600, links), `output_gate.rs` (no duplicate flags; stdout only via output.rs). `tests/output_options.rs` `cases()` :372 + `every_leaf_command_has_a_case` :1660 (items get One :382, backlog show :560-561, reconcile :585). New feature → `FEATURES` + `tests/capabilities.rs:1632` expected list (exact count) + README sample + Feature meanings table (`:2440` parity). The global-flags test checks membership only.
- Sub-agents: implement-deep/lite mention tomlctl only inside shared blocks `file-edits`, `forbidden-working-tree-ops`, `backlog-candidates`; research-deep/lite none (`research-read-only` block candidate). Shared blocks in `scripts/shared-blocks.toml` (byte-identical, verified by `scripts/verify-shared-blocks.sh`). Explore is built-in: carriers' prompts only (`plan-new.md:54`, `optimise.md:59`, `review.md:39`, `apply-pipeline/references/pre-analysis.md:9`); `plan-update.md` dispatches built-in `general-purpose` agents (:168, :187, :195). `agents/correlate.rs:194` matches literal `tasks show <ids> --slug <slug>` in dispatch prompts — keep it.
- `plan-update.md:101-107` asks for bucket lengths from `backlog reconcile` (drives jq `map_values(length)`).
- Commands: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`; `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`; `cargo clippy --manifest-path glimpse/Cargo.toml --locked`; `cargo doc … --lib --no-deps --document-private-items`; `bash scripts/verify-shared-blocks.sh`; `cargo install --path tomlctl`. Version 0.16.0 (lockstep: Cargo.toml, `capabilities_version_matches_cargo_toml`, README sample, SKILL.md:204, both lockfiles). No lint denies warnings (`[lints.clippy]` denies only `manual_filter` and `unnecessary_map_or`).
- Working tree clean at `/review-plan` round 1 (2026-10-21); baseline fmt, clippy (tomlctl, glimpse `--locked`), nextest (2102/2102) and shared-block parity all green.

### Backlog
- `B-7138d8d7` [open] direction tomlctl/src/backlog — this plan's origin (fold-in candidate).
- Context only (other areas, not in scope): B-edb20f98, B-e4c1a22a, B-e922f5cb, B-ef306556, B-771b57e7, B-b9b7e4c1.

## Research Notes

Vet: Agent-1 (regex-features, lite) 3 sampled / 0 dropped / 0 downgraded; Agent-2 (toml-payload, lite) 3 sampled / 0 dropped / 0 downgraded. Agent-2's `ESCALATE-TO-DEEP` named the TOML-vs-JSON choice as a design call depending on payload plumbing — the plumbing is surveyed in Exploration Notes (Write surface), so the choice is taken by the orchestrator in Phase 6 / Phase 4 rather than re-dispatched. No `[[vet_events]]` append: plan mode forbids writes and no ledger exists yet.

### Regex Unicode features (regex 1.13.1 / regex-syntax 0.8.11 / regex-automata 0.4.18)
Searched: installed registry sources only (`regex-1.13.1/Cargo.toml` [features], `src/lib.rs`, `CHANGELOG.md`; `regex-syntax-0.8.11/src/hir/translate.rs`, `src/unicode_tables/`; `regex-automata-0.4.18/src/hybrid/dfa.rs`), 2026-10-21.
- **Minimal set** (high): `unicode-perl` + `unicode-case` makes `(?i)`, `\d`, `\w`, `\s`, `\b` compile in default Unicode mode on both `Regex` and `bytes::Regex`; `unicode-perl` also enables `regex-automata/unicode-word-boundary`. Enabling features only admits more patterns, never changes match semantics. Impact: change `tomlctl/Cargo.toml:56`; rewrite `REGEX_UNICODE_HINT` (`query.rs:95`) for what stays unsupported (`\p{..}`).
- **Size** (medium — source size as proxy): tables `perl_word.rs` 18 KB, `case_folding_simple.rs` 67.6 KB, decimal/space ~2.5 KB; umbrella `unicode` ≈790 KB source. Impact: prefer the two features; measure release exe size delta at verification. `/review-plan` round 1 measured +139,264 bytes on a minimal LTO+strip binary.
- **Perf** (high mechanism / medium impact): a Unicode `\b` makes the lazy DFA quit on the first non-ASCII byte → PikeVM fallback; `(?-u:\b)` avoids it. Patterns without `\b` keep the DFA. Impact: document `(?-u:\b)` for `sweep`.
- **No-feature route rejected** (high): `unicode(false)` on a `&str` regex refuses `.`, `[^x]`, `\W` ("pattern can match invalid UTF-8") and non-ASCII class members.
- **Tests can't see it** (high): dev-dep `predicates` enables full `regex/unicode` in test builds (`cargo tree -e features`), so acceptance must use `cargo tree -e normal,features -i regex-syntax` or a release binary.
- **Size limit** (medium, measured at `/review-plan` round 1): with the two features, Unicode `\w{20}` compiles and `\w{21}` exceeds the 1 MiB `REGEX_COMPILE_SIZE_LIMIT`; keep the limit, document `(?-u:\w)`.
- No feature changes in 1.11–1.13.1 changelogs; no version bump needed.

### TOML payload files (toml 1.1.8+spec-1.1.0, toml_parser 1.1.5, clap 4.6.7)
Searched: installed sources (`toml-1.1.8+spec-1.1.0/src/de/parser/{detable,dearray,document}.rs`, `src/de/error.rs`; `toml_parser-1.1.5+spec-1.1.0/src/decoder/string.rs`, `src/lexer/mod.rs`), https://toml.io/en/v1.1.0, https://aider.chat/2024/08/14/code-in-json.html — fetched 2026-10-21.
- **Parser** (high): `toml::de::DeTable::parse(&str) -> Result<Spanned<DeTable>, toml::de::Error>` keeps a byte span per `[[record]]` row (`DeArray` derefs to `[Spanned<DeValue>]`); `toml::from_str::<Table>` drops spans. Row line = count `\n` before `span.start` (toml's `translate_position` is private). Convert rows with existing `convert.rs` `detable_to_json`. A string value's span covers its raw token, delimiters included (`de/parser/value.rs` `on_scalar`).
- **Literal strings** (high): `'''…'''` has no escapes; one newline after the opening `'''` is trimmed; the newline before the closing `'''` is KEPT; CRLF is NOT normalised; `'''` inside prose impossible → fallback `"""…"""` (needs `\\`). Impact: post-parse normalise CRLF→LF and trim one trailing newline per multi-line string, mirroring `read_flag_file`.
- **Errors** (high): `Display` prints `TOML parse error at line N, column M` + source line + caret; forward verbatim. tomlctl's own row errors need the span→line helper.
- **JSON-in-file** (medium, single 2024 benchmark on code): JSON keeps `\"`, `\\`, `\n` escaping burden; aider measured lower model scores wrapping code in JSON. `--json @path` already exists, so JSON stays as the zero-cost secondary form.
- **clap** (high): positional `Vec<String>` id list with `value_delimiter=','` is the crate's `ItemIds` pattern; take `@path` values as `String` and strip via `at_file_path` (clap has no argfile support). A clap conflict does not lift `required_unless_present` or a required group (`validator.rs` `validate_required`); a global sharing a subcommand's long name or id fails (`command.rs` `_propagate_global_args`, `debug_asserts.rs`).
