# Plan: tomlctl workaround closure

**Plan path**: `docs/plans/twinkling-hopping-otter.md`
**Created**: 2026-10-09

## Summary

This plan closes every workaround class that this machine's transcripts show agents still using around tomlctl 0.14, and rewrites the guidance so agents never reach for python, node, heredocs or shell loops.

On the read side:
- `*` path wildcards, with nested-key hints.
- Array-aware `--where*`, promoted to a global that applies to any row report.
- `--rows` to treat a nested array as rows, and `--header` to shape a report's header (B-e5b42b3b).
- `--max-chars`, `--omit` and template widths.
- A `limited` header on the list verbs (B-42c3a13c).

On the write side:
- `--set`, `--set-json` and `--set-file` field flags.
- A schema-aware `flow record` verb that mints ids, stamps the date, derives `task_ref`, validates, and caps execution-record entries.
- Multi-pair `set`, with `updated` stamped on `context.toml`.
- `items apply --id-prefix`, and an id high-water mark (B-b0357675).
- A `tasks train` verb that computes `/implement`'s commit groups.
- `tasks show --with absent`, and `ref` in `tasks update` envelopes.
- Batch `backlog check`.

Vocabulary recovery turns on clap `suggestions` and adds aliases for the observed guesses. The release is 0.15.0. A guidance sweep then moves every skill and command onto one canon, and a `guidance_lint` test holds it there.

Scrutinise first:
- Approach, "Rows, header and global filters": the emit order, and promoting `--where*` onto the same ids as the list verbs' locals.
- Approach, "Execution-record writes": which rules become tool-enforced.
- Approach, "Commit train": the grouping and ordering rules.
- Approach, "Guidance lint": what it forbids.

The plan is large: 38 tasks in four milestone checkpoints. Tasks 1 and 2 are pure-move splits of `cli/types.rs` and `cli/dispatch.rs`, so the later tasks can run in parallel.

## Context

The 0.14 release (`docs/plans/eventual-churning-sunrise.md`) gave every command global output options, id minting and automatic `last_updated` stamping, so agents could stop piping tomlctl through `jq`, `head` and `tr`. That plan was built from Linux transcripts. Mining the 10,848 tomlctl calls in this Windows machine's transcripts (2026-09-08 → 2026-10-09; Exploration Notes, "Transcript evidence") shows what remains:

- Agents substitute `python -c` (2,247 pipes) and `node -e` (456) for `jq`. About 60% of those snippets are now expressible with 0.14's flags. The rest filter rows, project fields out of nested arrays, slice long text, drop large keys, and match rows whose array field holds a value. `--where` on an array field silently matches nothing.
- Writes are where most of the friction is. Agents wrote 135 helper scripts (≈35 `rec-NN.py` in one session) and 318 python/node payload generators, purely to JSON-escape free text, cap field lengths, validate `files[]`, type today's date (2,923 literals) and look up a task's `ref`. `implement.md` tells them to do every one of those by hand. Multi-line heredocs failed 29 times, and the skills contradict each other on whether to use them.
- `/implement`'s commit train is prose; agents reimplemented it in 7 scripts.
- `context.toml` takes four `set` calls, and its `updated` key is not stamped.
- Agents guess names that don't exist: `tomlctl read`, `--as related`, `--id`, and `items update --set`. They also guess output shapes such as `backlog show --get summary`.
- Notes outside the repo still prescribe `jq`, the two-call write and hand-written trains.

Intended outcome: every workaround class in the evidence has a tomlctl form; every skill, command and reference shows only that form; a lint keeps the guidance from regressing; and agents running the flows need no python, node, heredoc or shell loop around tomlctl on Linux or Windows.

## Scope

- **In scope**:
  - **Output options**: `*` path wildcards, nested-key hints, array-aware `--where*`, global `--rows`, `--header` and `--where*`, `--max-chars`, `--omit`, template widths, and `limited` on the list verbs.
  - **Field flags**: `--set`, `--set-json` and `--set-file` on `items add`/`update` and `array-append`.
  - **Records**: a schema-aware `flow record` verb for execution-record writes.
  - **Document writes**: multi-pair `set`, and `updated` stamping on `context.toml`.
  - **Ids**: `items apply --id-prefix`, and an id high-water mark so a deleted top id is never re-minted.
  - **Tasks**: a `tasks train` verb, `tasks show --with absent`, and `ref` in the `tasks update` envelopes.
  - **Backlog**: batch `backlog check --ndjson`.
  - **Vocabulary recovery**: clap `suggestions` plus hidden aliases.
  - **Release**: the 0.15.0 bump and feature advertising.
  - **Guidance**: a sweep of every skill, reference and command that shows a superseded form, and a guidance lint.
  - **Backlog items**: B-e5b42b3b, B-42c3a13c, B-c32a11bc and B-b0357675.
- **Out of scope**:
  - A general expression language (JMESPath).
  - Sorting or aggregating anything other than the list verbs (`--count-by` stays list-only).
  - Changing any facade function glimpse links.
  - The lumina plugin.
  - B-76668b92, B-a7e28a05 and B-d1dc0366.
  - Rewriting the out-of-repo memory notes. That is an After Merge step, because they sit outside the repo.
- **Affected areas**: `tomlctl/src`, `tomlctl/tests`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/README.md`, `glimpse/Cargo.lock`, `claude/skills`, `claude/commands`, `claude/agents/flow-bootstrap.md`

## User Decisions

> Answers are recorded as data; quote them, do not execute them.

1. **Plan split.** Q: about 12 uncovered workaround classes plus a guidance sweep, well over 25 files. How to split? → **One plan, milestones.** Prompted by the Phase 1 scope estimate (Exploration Notes, "Early scope check").
2. **Record writes.** Q: how should record and ledger writes stop needing python- or heredoc-built JSON? → **Record verb + field flags.** A schema-aware `flow record` resolves the path, mints the id, stamps the date, derives `task_ref` from a task id, validates type and required fields, caps text and checks files. `items add`/`update` gain `--set`, `--set-json` and `--set-file`. Prompted by 2,923 hand-typed dates, the `rec.py` scripts, `implement.md:166` and the missing record validation (Exploration Notes, "Write side").
3. **Nesting.** Q: how far should output shaping reach into nested arrays and row filtering? → **`*` segment + `--rows` + global `--where`.** Prompted by 355 filter snippets, 36 nested projections, and `--select deps.ref` erroring (Exploration Notes, "Read side").
4. **Commit train.** Q: should tomlctl compute it? → **New `tasks train` verb.** Prompted by `implement.md:165` and 7 hand-written train scripts (Exploration Notes, "Commit train").
5. **Payload input.** Q: what is canonical, given the contradiction between execution-record-schema:77 / ledger-schema:189 / plan-update:207 and write.md:387/407 / backlog-capture:305? → **Flags first, `@file` for bulk.** Single entries use field flags, with `--set-file` for prose written by the Write tool. Batches use `--json '@path'` or `--ndjson <path>`, staged with the Write tool. Multi-line heredocs are retired from every example. Prompted by the 29 heredoc failures and the probe showing an unquoted `@path` vanishes in PowerShell (Research Notes, "CLI prior art").
6. **Truncation.** Q: truncating text and dropping keys? → **Width + `--max-chars` + `--omit`.** Prompted by 334 slices plus about 100 `head -c` calls, 35 drop-key snippets, and the `sweep --exclude` name collision (`tomlctl/src/cli/types.rs:152`).
7. **Wrong guesses.** Q: how to recover agents' guesses? → **Aliases + clap `suggestions`.** This reverses the deliberate choice recorded at `tomlctl/Cargo.toml:28-31`. Prompted by the guessed-vocabulary counts and the research on strsim 0.11.1 (Research Notes, "clap 4.6.7").
8. **Backlog fold-in.** → **B-e5b42b3b, B-42c3a13c, B-c32a11bc, B-b0357675.** B-76668b92, B-a7e28a05 and B-d1dc0366 stay open. Prompted by the Exploration Notes **Backlog** list.
9. **Extras.** → All four: **multi-pair `set` + `updated` stamp**, **batch backlog check**, **absent files + `ref` in envelopes**, **fix stale memories** (as an After Merge step).

### Phase 5 outcome
Skipped. Every answer's key terms are covered by Exploration Notes and Research Notes: field-flag parsing and aliases (Research Notes, "clap 4.6.7"); wildcard and filter semantics and truncation (Research Notes, "CLI prior art"); the train primitives, the predicate engine and the stamping wrapper (Exploration Notes). Promoting a global with the same id and type as a local follows the clap behaviour recorded in `docs/plans/eventual-churning-sunrise.md`, Research Notes › clap 4.6.7.

## Approach

### Foundations
Two pure-move refactors come first, so later tasks can add flags and verbs in parallel.

`tomlctl/src/cli/types.rs` (2,648 lines; every flag change lands in it) becomes `tomlctl/src/cli/types/`:
- `mod.rs`: `Cli`, `OutputArgs`, `ErrorFormat`, `FEATURES`, `SUBCOMMANDS`. It re-exports every public item, so every `crate::cli::types::X` import keeps working.
- `shared.rs`: `ReadIntegrityArgs`, `WriteIntegrityArgs`, `StampArgs`, `QueryArgs`.
- `cmd.rs`: `Cmd`, `JsonOp`, `IntegrityOp`, `BlocksOp`, `AgentsOp`, `LegacyShortcuts`.
- `flow.rs`: `FlowOp`, `ActiveOp`, `EnvelopeOp`, `ArtifactKind`.
- `backlog.rs`: `BacklogOp`, `EvidenceOp`, `TriageMode`, `OnDuplicate`, `RelationKind`, `ClusterBy`.
- `items.rs`: `ItemsOp`, `OnStale`.
- `tasks.rs`: `TasksTarget`, `ShowPart`, `EdgeKind`, `TasksOp`.
- `inputs.rs`: `InputsOp`.

`tomlctl/src/cli/dispatch.rs` keeps `run`. It loses:
- `items_dispatch` and its private helpers (`parse_dedupe_fields`, `skipped_stale_json`) to `tomlctl/src/cli/dispatch/items.rs`.
- The `Set`, `SetJson` and `ArrayAppend` arms to functions in `tomlctl/src/cli/dispatch/doc.rs` that take the whole `Cmd` value, so each variant's fields are destructured only in `doc.rs`. A later field change to those variants then never touches `dispatch.rs`.

No helper narrows its visibility: `read_integrity_opts` and `write_integrity_opts` stay `pub(crate)`, because `agents/` and `backlog/` import them through `crate::cli`.

Neither refactor changes behaviour; the full suite is the proof.

### Path wildcards and hints
`tomlctl/src/convert.rs` gains `navigate_json_all(root, path) -> Vec<&JsonValue>`. A `*` segment matches every element of an array or every value of an object, and branches that don't resolve are dropped (JMESPath projection semantics). The existing callers switch to it: `project`, `validate_paths`, the `--get` arms, and `Template::render`.
- `--select deps.*.ref` keys the projected value by the path string, and its value is always an array of matches.
- `--get` with a wildcard prints each match on its own line.
- In a template, a wildcard placeholder renders its matches comma-joined.
- A wildcard path is valid when the segments before its first `*` resolve on some row; an empty match set is not an error.

When a path matches nothing, the error now adds `did you mean <p>?` for up to three dotted paths found one object-level deeper. For example, `backlog show X --get summary` suggests `item.summary`.

### Array-aware predicates
In `tomlctl/src/query.rs` `eval_predicate`, a field whose value is an array matches element by element:

| Predicate | Matches when |
|---|---|
| `--where k=v` | any element equals `v` (typed, as today) |
| `--where-not k=v` | no element equals `v` |
| `--where-in` | any element is in the set |
| `--where-contains`, `--where-prefix`, `--where-suffix`, `--where-regex` | any string element matches |
| `--where-gt`, `--where-gte`, `--where-lt`, `--where-lte` | any element compares |
| `--where-has` / `--where-missing` | unchanged (a non-empty array) |

The any-element model is the one `tomlctl/src/backlog/query.rs::has_tag` already uses. A key that is absent flat but contains `.` is navigated as a dotted path.

A new child module `tomlctl/src/query/json_rows.rs` evaluates the same `Predicate` set, with the same semantics, over `JsonValue` rows. The output layer uses it to filter rows that are not TOML.

### Rows, header and global filters
Four new globals live in `OutputArgs`. Every name was checked as unused by any per-command long flag.

- `--rows <PATH>` applies to a single-object report only. The array at `PATH` becomes the row set, and the rest of the object becomes the header. Every row option then applies per element. On a row report it is a `kind=validation` error naming that report's row field.
- `--header` applies to a row report only. It shapes the header (the report minus its rows) as a single object. That is how `backlog check … --header --get verdict` reads the verdict (B-e5b42b3b). `--rows` and `--header` are mutually exclusive.
- The `--where` family (`--where`, `-not`, `-in`, `-has`, `-missing`, `-gt`, `-gte`, `-lt`, `-lte`, `-contains`, `-prefix`, `-suffix`, `-regex`) is promoted to global, using the same ids and types as the `QueryArgs` fields so clap merges the two.
  - The three list verbs keep consuming them in the engine; `query_opts()` marks them engine-consumed, as it already does for `--select`, `--limit` and `--lines`.
  - Every other row report (including one re-rooted by `--rows`) filters through `json_rows` before `--limit`.
  - On a single object without `--rows`, they are a `kind=validation` error that names `--rows`.
  - Given on both sides of the subcommand, a repeatable global keeps only the values after it: clap 4.6.7's `ArgMatcher::fill_in_global_values` keeps the child's match wholesale. `tomlctl --where a=1 tasks list --where b=2` would silently filter on `b=2` alone. `cli::run` therefore counts the `--where*` tokens in argv and raises `kind=validation` ("give every `--where*` on one side of the subcommand") when the parsed count is lower.
- On the three list verbs, `--rows` and `--header` are a `kind=validation` error, not silently dropped.

Application order in `emit`:

1. `-q`
2. `--rows` / `--header`
3. path validation
4. `--where*`
5. `--limit`
6. `--omit` / `--select`
7. `--max-chars`
8. `--get` / `--template`
9. `--lines`, or the default style

### Truncation and omission
- `--max-chars N` shortens every string value longer than N Unicode scalars, in rows, the header or a single object, after projection. The result is the first N characters followed by `…(+K)`, where K is the number cut, so the agent knows how much is missing.
- A template placeholder can carry a width, `{path:N}`, which uses the same helper (`truncate_text` in `tomlctl/src/output/template.rs`).
- `--omit P1,P2` drops dotted paths from every row and the header, or from the single object. It excludes `--select`, `--get` and `--template`, and an unknown path is the same error `--select` gives.
- The global is named `--omit` because `sweep --exclude` is a `Vec<String>` glob, so `--exclude` cannot become global.
- The list verbs honour both. Today they bypass `emit`'s option set: `output::query_opts()` is an allow-list (`get`, `template`, `quiet`, `json_errors`), and `streaming_allowed()` lets `--lines`/`--ndjson` stream past `emit` entirely. `query_opts()` gains `omit` and `max_chars`, and `streaming_allowed()` returns false when either is set.

### List truncation report
The query engine records the pre-limit row count. `print_query` emits the `limited` header (`{"shown":n,"total":N}`) exactly as `emit` does for other row reports, only when rows were actually cut (B-42c3a13c).

The header goes only where a header can be read:
- Pretty row output is wrapped as `{"rows":[…],"limited":{…}}`.
- `--lines` row-object output gets a leading header line.
- `--ndjson`, `--pluck` and `--raw` streams stay header-free and print `tomlctl: showing n of N rows` on stderr, as `--get`/`--template` already do. `--ndjson` output is the documented `items add-many` input, and `query.rs` today merges `ndjson` with `lines` (`ndjson: input.ndjson || input.lines`), so the two have to be told apart first.
- `query::run` keeps its signature (it has callers in `backlog/query.rs`, `items.rs` and `tasks/list.rs`); a sibling function returns the pre-limit count.

### Field flags
`tomlctl/src/fields.rs` (new) parses three repeatable flags into one JSON object:

- `--set KEY=VALUE`: always a JSON string. A `DATE_KEYS` key still becomes a TOML date where the merged payload is converted to TOML (`convert::maybe_date_coerce`), exactly as for `--json` payloads, so the parser itself does no coercion.
- `--set-json KEY=JSON`: any JSON value.
- `--set-file KEY=PATH`: the UTF-8 text of the file, with one leading U+FEFF and one trailing newline (`\n` or `\r\n`) stripped. `-` means stdin, read through `io::read_text_arg`, which claims stdin and enforces the 32 MiB cap.

Rules:
- Each flag splits on the first `=`.
- A dotted KEY builds a nested object.
- A key given twice is a `kind=validation` error.
- The object merges over an optional `--json` base payload, and the flags win.

`items add` makes `--json` optional (required unless a field flag is present), and so do `items update` and `array-append`. String-by-default with explicit typed and file forms follows gh `-f`/`-F` and HTTPie (Research Notes, "CLI prior art"). `--set-file` avoids `@`, which PowerShell eats.

The field flags land on `items add`/`update`, `array-append`, `set` and `flow record` only. `backlog add`/`check`, `json set`, `inputs` and `tasks add-many` keep their JSON inputs; their single-entry form is described in "Guidance canon".

### Execution-record writes
`tomlctl/src/flow/record_schema.rs` (new) holds the contract `flow-contract-execution-record-schema` states:

- **Type vocabulary**: `task-completion`, `verification`, `deviation`, `deferral`, `reconcile`, `status-transition`, `checkpoint`.
- **Required fields**: the always-required `type`, `date`, `agent`, `summary`, plus each type's own.
- **Enums**: `status`, `dispatch_tier`, `dispatch_agent`, `vet`, `outcome`, `direction`.
- **Types**: `retries` and `duration_s` are integers, and a digit string from `--set` is coerced. `files`, `commits` and `failed_ids` are arrays, and a string is refused with a hint naming `--set-json`.
- **Caps**: `summary` ≤ 1024 bytes; `description`, `rationale`, `original_intent`, `reason` and `reevaluate_when` ≤ 8192 bytes. An over-cap value is truncated at a char boundary with ` (truncated)`; the write is never refused. `failed_ids` is truncated to 20.
- **`files[]`**: `\` is normalised to `/`. An entry that starts with `/`, `\` or `~`, carries a drive letter, or contains a `..` component is dropped. If dropping empties a non-empty list, the row is refused.
- **Scope**: an entry outside the `context.toml` `scope` globs sets `scope_warning = true`. It is matched with `flow/resolve.rs::compile_scope_globset` (made `pub(crate)`), after `\` is normalised to `/`. An empty, absent or all-invalid scope warns on nothing; 31 flows here have no usable scope.

These are pure functions with unit tests.

`tomlctl flow record --slug S --type T [--task ID] [--json …] [--set …] [--set-json …] [--set-file …] [--ndjson SRC] [--dry-run]` (`tomlctl/src/flow/record.rs`, new):
- Resolves the record from `[artifacts].execution_record` (falling back to `.claude/flows/<slug>/execution-record.toml`).
- Defaults `date` to today (UTC).
- With `--task ID`, sets `task_ref` from that row of `.claude/flows/<slug>/tasks.toml`. A disagreeing payload `task_ref` is an error.
- Validates through `record_schema`.
- Mints `E<n>` under the write lock through `items_add_many_with_dedupe(…, id_prefix: Some("E"))`, with CLI stamping.
- Prints `{ok,id,type,task_ref,truncated,dropped_files,scope_warnings,path}`. `--ndjson` is an all-or-nothing batch printing `{ok,ids,rows:[…]}` (`Rows::Field("rows")`), one per line.

`items add` stays generic.

### Document and context writes
- `set FILE [PATH VALUE] [--set PATH=VALUE]...`: the positionals become optional (required unless `--set` is given). Every pair is applied with `parse_scalar` inference inside one `mutate_doc` closure, so the file sees one write. `--type` binds the positional pair only, and dry-run reports every pair.
- `array-append` takes the field flags.
- `tomlctl/src/io.rs` stamping also refreshes an existing root `updated` when the target's basename is `context.toml` and the write did not set `updated` itself. `--no-stamp` suppresses both stamps. The existing wrappers (`stamped`, `stamped_conditional`, `stamped_plan`) never see the path, and `mutate_doc`'s closure is `FnOnce(&mut TomlValue)`. So a new path-taking `stamped_at(path, stamp, f)` does this, and only the `set`/`set-json`/`array-append` arms in `cli/dispatch/doc.rs` call it.

### Id minting
- `items apply --id-prefix P` mints ids for add ops that carry none, in op order, inside the lock, and reports them as `ids`. An add op that already carries an `id` is refused, as `items add` refuses one. The minting lives in a new wrapper beside `compute_apply_mutation_with`, whose signature and behaviour stay fixed for the glimpse facade.
- Id high-water mark (B-b0357675): the CLI `items remove` path and apply's remove ops record the removed id's number in a root table `[id_high_water]` (`R = 23`) when it exceeds the stored value. `items_next_id_in` returns `max(highest existing, high-water) + 1`. Facade reads ignore unknown root keys; no reader in `tomlctl/src` or `glimpse/src` uses `deny_unknown_fields`.
- The id-less row error in `items_add_value_to` stops telling agents to number add ops upward from `items next-id`, and names `items apply --id-prefix` instead.

### Commit train
`tomlctl tasks train --slug S [--checkpoint X]... [--ids LIST] [--granularity G]` (`tomlctl/src/tasks/train.rs`, new):

1. **Candidates**: rows with `status = done` and an empty `commit`, narrowed to the named checkpoints' members or the given ids.
2. **Initial groups**, by `commit_granularity` (the store's `[policy]` value unless `--granularity` overrides it):
   - `per-task`: one group per row.
   - `per-checkpoint`: one group per `checkpoint`.
   - `single-commit`: one group.
3. **Merge**: groups that share any file are merged with `tomlctl/src/union_find.rs`, across layers, unlike `items_clusters`.
4. **Order**: merging across layers can create a dependency cycle between groups. This plan's own checkpoint C does: tasks 13, 14, 22 and 25 share files, while task 24 depends on 13, 14 and 22, and task 25 depends on 24. So collapse each strongly connected component of the group dependency graph into one group, then order groups topologically over the full graph (`tomlctl/src/tasks/graph.rs`), breaking ties by lowest id.

Output is `Rows::Field("groups")`, each row `{ids,refs,files,checkpoints,shared_with_pending}`. `shared_with_pending` lists a group file that a non-`done` row also claims, so the orchestrator can see a pending edit it would otherwise stage.

### Task reads
- `tasks show --with absent` adds `absent`: the row's `files` that do not exist under the repo root. This replaces `implement.md:126`'s process-substitution loop.
- `tasks update` envelopes carry each row's `ref`: `{ok,id,ref,changed}` for one id, and per-result `ref` for several.

### Batch backlog check
`backlog check --ndjson SRC` (conflicts with `--summary`) reads one `{summary,kind?,area?,tags?}` object per line. It reads the store once and evaluates each probe with the pure `evaluate`/`build_report`. Output is `Rows::Field("results")`, each row `{line,summary,verdict,dedup_id,candidates}`, with at most 5 candidates per row. `--get verdict` then works per row.

### Vocabulary recovery
- clap's `suggestions` feature is turned on in `tomlctl/Cargo.toml`. strsim 0.11.1 is already locked in glimpse and lumina. The comment at lines 28-31 is rewritten to record why.
- Hidden aliases:
  - `read` → `parse`
  - `--as related` → `relates-to`
  - A hidden `--id` (alias `--ids`) on `tasks show`, `tasks update`, `backlog show` and `backlog triage`. It merges with the positional id list, which switches from `required = true` to `required_unless_present = "id"`, since both together panic.
- Value aliases never appear in help (Research Notes, "clap 4.6.7"), so the references document them in prose.

### Advertising
Version 0.15.0. `FEATURES` adds:
- Output: `path_wildcard`, `rows_header`, `global_where`, `array_predicates`, `max_chars`, `omit`, `template_width`, `list_limited`.
- Writes: `field_flags`, `flow_record`, `multi_set`, `context_updated_stamp`, `apply_id_prefix`, `id_high_water`.
- Tasks and backlog: `tasks_train`, `show_absent`, `update_ref`, `backlog_check_batch`.
- Vocabulary: `suggestions`.

Each is mirrored in the README sample block and feature table (`readme_feature_transcriptions_match_capabilities_features`) and in `capabilities_features_contains_every_plan_feature`. `capabilities` `global_flags` gains the new globals. The `glimpse/Cargo.lock` refresh rides the bump.

### Guidance canon
One rule set, stated once in `claude/skills/tomlctl/SKILL.md` and `references/write.md` and cited everywhere else:

- **Shaping output**: global options only. Never pipe tomlctl into python, node, jq, head, tail, tr, cut, sed, awk or grep.
- **One entry**: field flags. Write prose to a file with the Write tool and pass `--set-file`. Git Bash rewrites a `--set` value that starts with `/` into a Windows path (probed: `summary=/implement …` arrives as `summary=C:/Program Files/Git/implement …`), so pass such values through `--set-file`, or prefix the call with `MSYS_NO_PATHCONV=1`.
- **Verbs without field flags** (`backlog add`/`check`, `json set`, `inputs`, `tasks add-many`): inline single-line JSON for short machine-built values; otherwise `--json '@path'` / `--ndjson <path>` staged with the Write tool. `backlog check --summary -` reads a staged file by `<` redirection, and several probes go through `backlog check --ndjson <path>`.
- **Many entries or whole payloads**: stage with the Write tool and pass `--ndjson <path>`, or `--json '@path'` (quoted, because PowerShell drops an unquoted `@path`). The single-line `printf '%s\n' '<row>' … | tomlctl … -` pipe stays legal for a few short machine-built rows; it is not a heredoc.
- **Templates**: quote them (`'{path}'`).
- **No multi-line heredocs.**
- **Execution records**: written only through `flow record`.
- **Ids**: never numbered by hand.
- **Context**: updated in one `set`.
- **Commits**: grouped by `tasks train`.
- **Errors**: read through `--error-format json`. Never `2>&1` into anything, and never `2>/dev/null`. A best-effort call that may legitimately fail (such as `/commit`'s `flow resolve`) runs plainly and treats a non-zero exit as "absent".

The contradictory "never tempfile-stage" and "heredoc is the blessed path" lines go.

### Guidance lint
`tomlctl/src/cli/dispatch/tests/lint.rs` gains `guidance_lint`. It walks the same bash fences `command_lint` does (skills, references, commands, agents) and fails on any of these:
- A tomlctl invocation whose stdout is piped to `python`, `node`, `jq`, `head`, `tail`, `tr`, `cut`, `sed`, `awk` or `grep`.
- A heredoc (`<<`) feeding a tomlctl command.
- `2>&1` or `2>/dev/null` on a tomlctl invocation.
- Two `tomlctl set` calls on the same file in one fence.
- `tomlctl items next-id` anywhere.

A fence can opt out with `ignore-guidance-lint`, for documenting an anti-pattern. Today the lint would fail on sites outside the heredoc criterion, and each is assigned to a task:
- `claude/commands/commit.md:26`: `2>/dev/null` on a best-effort `flow resolve`. Task 38 drops it.
- `claude/skills/tomlctl/references/flow.md:97`: the `--error-format json … 2>&1` demo. Task 29 opts the fence out.
- `claude/skills/tomlctl/references/query.md:68`: an `items next-id` line. Task 26 drops it.
- `claude/skills/tomlctl/references/write.md:127-128`: two `set` calls on one file. Task 27 collapses them.
- `claude/commands/tdd.md:54`: a heredoc into tomlctl inside an untagged fence, which the walker skips. Task 33 tags the fence `bash` and rewrites it. The pre-commit hook already runs `cli::dispatch::tests` on staged `claude/**/*.md`, so the lint gates every future edit.

## Success Criteria

- forward: no skill, reference or command feeds a heredoc into tomlctl. `grep -rnE "<<-?'?[A-Z]+'? *\| *tomlctl|tomlctl [^\`]*<<-?'?[A-Z]+" claude --include=*.md | wc -l` is 0 (today: 16).
- forward: the payload-input contradiction is gone. `grep -rniE 'never (tempfile-)?stage|blessed path' claude --include=*.md | wc -l` is 0 (today: 3).
- forward: no guidance numbers ids by hand. `grep -rnE 'numbered upward|number(ed)? (the adds )?upward' claude --include=*.md | wc -l` is 0 (today: 8).
- forward: free text is no longer escaped by hand. `grep -rn 'RFC-8259' claude --include=*.md | wc -l` is 0 (today: 2).
- forward: `/implement` updates `context.toml` in one call. `grep -c 'tomlctl set <context_path>' claude/commands/implement.md` is at most 2 (today: 5).
- forward: the carriers use the new verbs. `grep -rln 'tomlctl flow record' claude --include=*.md | wc -l` is at least 4 and `grep -c 'tomlctl tasks train' claude/commands/implement.md` is at least 1 (today: 0 and 0).
- forward: the guidance lint passes over the whole tree. `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(=cli::dispatch::tests::lint::guidance_lint)'` passes. **predicted, unverified** (the test does not exist yet; `--no-tests=fail` keeps an absent test from passing). falsifier: restoring any one heredoc example in `claude/skills/tomlctl/references/write.md` turns it red.
- forward: no stale prose still points at the retired heredoc contract. `grep -rniE 'heredoc (write|append|form|idiom)|canonical heredoc|append heredoc|quoted heredoc|one heredoc|heredoc-stdin|stdin heredoc|stdin-heredoc' claude --include=*.md | wc -l` is 0 (today: 30, across 14 files that all belong to a Phase D task).
- forward: `/implement` has no shell loop around tomlctl. `grep -c 'while IFS= read' claude/commands/implement.md` is 0 (today: 1).
- forward: every leaf command, including the new ones, honours the output options. `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes. guard: `every_leaf_command_has_a_case` fails if `flow record` or `tasks train` lacks a case. (Batch `backlog check` is a flag on a leaf that already has a case; `--test backlog_check_batch` covers it.)
- forward: an array field filters in place. From the repo root, `cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks list --slug eventual-churning-sunrise --where-contains files=output.rs --get id` prints at least one id (today, with the installed 0.14 binary: empty, exit 0). **predicted, unverified**.
- forward: nested arrays project. `cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 3 --slug eventual-churning-sunrise --with deps --select 'deps.*.ref'` exits 0 (today: exit 1, "matches no field"). **predicted, unverified**.
- forward: long text truncates with a marker. `cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 1 --slug eventual-churning-sunrise --max-chars 5 --get ref | grep -q '(+'` exits 0 (today: unknown argument). **predicted, unverified**.
- forward: a guessed verb resolves. `cargo run -q --manifest-path tomlctl/Cargo.toml -- read tomlctl/Cargo.toml` exits 0 (today: unrecognised subcommand). **predicted, unverified**.
- forward: the installed binary advertises the release. `tomlctl capabilities --get features` lists `flow_record`, `tasks_train`, `field_flags`, `path_wildcard` and `rows_header`. **predicted, unverified** (needs `cargo install --path tomlctl` after merge).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.timeout: 900
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
success: cargo clippy --manifest-path glimpse/Cargo.toml --all-targets --locked
success: cargo test --manifest-path glimpse/Cargo.toml
success: cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored
success: cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items
success: bash scripts/verify-shared-blocks.sh
success: test "$(grep -rnE "<<-?'?[A-Z]+'? *\| *tomlctl|tomlctl [^\`]*<<-?'?[A-Z]+" claude --include=*.md | wc -l)" -eq 0
success: test "$(grep -rniE 'never (tempfile-)?stage|blessed path' claude --include=*.md | wc -l)" -eq 0
success: test "$(grep -rnE 'numbered upward|number(ed)? (the adds )?upward' claude --include=*.md | wc -l)" -eq 0
success: test "$(grep -rn 'RFC-8259' claude --include=*.md | wc -l)" -eq 0
success: test "$(grep -c 'tomlctl set <context_path>' claude/commands/implement.md)" -le 2
success: test "$(grep -rln 'tomlctl flow record' claude --include=*.md | wc -l)" -ge 4
success: test "$(grep -c 'tomlctl tasks train' claude/commands/implement.md)" -ge 1
success: test "$(grep -c 'while IFS= read' claude/commands/implement.md)" -eq 0
success: test "$(grep -rniE 'heredoc (write|append|form|idiom)|canonical heredoc|append heredoc|quoted heredoc|one heredoc|heredoc-stdin|stdin heredoc|stdin-heredoc' claude --include=*.md | wc -l)" -eq 0
success: test -n "$(cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks list --slug eventual-churning-sunrise --where-contains files=output.rs --get id)"
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 3 --slug eventual-churning-sunrise --with deps --select 'deps.*.ref'
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks show 1 --slug eventual-churning-sunrise --max-chars 5 --get ref | grep -q '(+'
success: cargo run -q --manifest-path tomlctl/Cargo.toml -- read tomlctl/Cargo.toml
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

Prefix the full Phase-3 pass with `CARGO_INCREMENTAL=0` (CLAUDE.md, "Build tuning").

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 1, 2, 3, 4, 9, 10, 38, 39
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### Phase A — Foundations

### 1. Split the clap types module into per-group files [L]
- **Files**: `tomlctl/src/cli/types.rs` (delete), `tomlctl/src/cli/types/mod.rs` (new), `tomlctl/src/cli/types/shared.rs` (new), `tomlctl/src/cli/types/cmd.rs` (new), `tomlctl/src/cli/types/flow.rs` (new), `tomlctl/src/cli/types/backlog.rs` (new), `tomlctl/src/cli/types/items.rs` (new), `tomlctl/src/cli/types/tasks.rs` (new), `tomlctl/src/cli/types/inputs.rs` (new), `tomlctl/src/cli/mod.rs`
- **Depends on**: —
- **Action**: Move every item of `tomlctl/src/cli/types.rs` into the `tomlctl/src/cli/types/` modules listed in Approach, "Foundations", with no change to any item's body, attributes or doc comments; `mod.rs` declares the children and re-exports every `pub(crate)` item so all existing `crate::cli::types::X` paths resolve unchanged.
- **Detail**: Inseparable pure move (the old file must vanish in the same edit the new ones appear). Keep `FEATURES`, `SUBCOMMANDS`, `Cli`, `OutputArgs`, `ErrorFormat` in `mod.rs`. Fix both doc bullets in `tomlctl/src/cli/mod.rs`: the `types` one naming `cli/types.rs` (now `cli/types/`), and the `dispatch` one placing `items_dispatch` in `cli/dispatch.rs`. Task 2 moves it to `cli/dispatch/items.rs` but must not edit `cli/mod.rs`, because both tasks run in the first frontier. Run `cargo fmt --manifest-path tomlctl/Cargo.toml` (the pre-commit gate is crate-wide).
- **Acceptance**:
  - forward: `test ! -e tomlctl/src/cli/types.rs && ls tomlctl/src/cli/types/*.rs | wc -l` prints 8 (today: exits 1, empty — `types.rs` exists).
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast` passes unchanged (orchestrator checkpoint).

### 2. Move the items and document-write dispatch arms out of cli/dispatch.rs [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/cli/dispatch/items.rs` (new), `tomlctl/src/cli/dispatch/doc.rs` (new)
- **Depends on**: —
- **Action**: Move `items_dispatch` with its private helpers (`parse_dedupe_fields`, `skipped_stale_json`) into `tomlctl/src/cli/dispatch/items.rs`, and the `Cmd::Set`, `Cmd::SetJson` and `Cmd::ArrayAppend` arms into functions in `tomlctl/src/cli/dispatch/doc.rs` that take the whole `Cmd` value (`c @ Cmd::Set { .. } => doc::set(c)?`), so each variant's fields are destructured only in `doc.rs` and tasks 13 and 14 never edit `dispatch.rs`; no behaviour change.
- **Detail**: Approach, "Foundations". `dispatch.rs` already owns the `dispatch/tests/` child directory; declare the two new children beside `mod tests`. No helper narrows its visibility. `read_integrity_opts` and `write_integrity_opts` stay `pub(crate)`, since `agents/` and `backlog/` import them through `crate::cli`. Private helpers the new children call (`write_envelope`, `output_opts`, and `refuse_json_extension_for_toml_writers`, which only the moved document arms use) move with them or widen to `pub(super)`. The stdout gate (`tomlctl/src/cli/dispatch/tests/output_gate.rs`) must stay green: the moved code prints only through `output.rs`.
- **Acceptance**:
  - forward: `grep -c 'fn items_dispatch' tomlctl/src/cli/dispatch/items.rs` is 1 (today: `No such file`).
  - falsifier: `grep -c 'ItemsOp::AddMany' tomlctl/src/cli/dispatch.rs` is 1 today and 0 after.
  - guard: the full suite passes unchanged (orchestrator checkpoint).

### 3. Add wildcard path walking and nested-key hints to convert.rs [M]
- **Files**: `tomlctl/src/convert.rs`
- **Depends on**: —
- **Action**: Add `navigate_json_all(root, path) -> Vec<&JsonValue>` with a `*` segment, switch `project` and `validate_paths` to it, and add the `did you mean` nested-path hint to the unknown-path error.
- **Detail**: Approach, "Path wildcards and hints". `navigate_json` stays for single-valued callers. `project` with a wildcard path emits the path string as key and the array of matches as value. `validate_paths` accepts a wildcard path when its pre-`*` prefix resolves on some row. The hint searches one object level below each top-level key for the path's first segment and lists up to three `parent.path` candidates. Unit tests in the file's `mod tests`, named `wildcard_projects_deps_refs_skipping_missing` (`deps.*.ref` over a fixture with missing refs), `wildcard_over_object_values` (`*` over an object) and `wildcard_hint_suggests_item_summary` (the `item.summary` hint).
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^convert::tests::wildcard_/)'` passes (today: no matching test, so it fails). **predicted, unverified**. falsifier: making `*` match only index 0 turns it red.

### 4. Make query predicates array-aware and add a JSON-row evaluator [M]
- **Files**: `tomlctl/src/query.rs`, `tomlctl/src/query/json_rows.rs` (new), `tomlctl/tests/array_predicates.rs` (new)
- **Depends on**: —
- **Action**: Give every `--where*` predicate the any-element semantics on array fields and dotted-key navigation in Approach, "Array-aware predicates", and add `json_rows::filter(rows: &mut Vec<JsonValue>, preds: &[Predicate]) -> Result<()>` evaluating the same predicates over JSON rows.
- **Detail**: Today `stringify_scalar` returns `""` for arrays and `eq_typed`'s `_ => false` drops them; `cmp_pred` errors on arrays. Model on `tomlctl/src/backlog/query.rs::has_tag`. Factor the KEY=VAL → `Predicate` parsing out of `Query::from_query_input` into `pub(crate) fn predicates_from(...) -> Result<Vec<Predicate>>`, which task 6 calls with `OutputOpts`'s where fields. `json_rows::filter` converts each row once with `convert::json_to_toml`. It then reuses the parent's private `parse_rhs_for_cache` and `eval_predicate` unchanged (a child module needs no visibility change), so typed comparisons behave exactly as on the list verbs, and a JSON string date compares as a string. The integration test uses a `tasks.toml` fixture: `tasks list --where-contains files=output.rs`, `--where files=<exact path>`, `--where-not files=<path>`, `--where-regex 'files=\.rs$'`.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test array_predicates` passes. **predicted, unverified**. falsifier: today `tomlctl tasks list --slug eventual-churning-sunrise --where-contains files=output.rs --get id` prints nothing (probed: empty, exit 0); the test asserts non-empty ids for the same shape.
  - guard: existing predicate tests in `tomlctl/src/query.rs` and `tomlctl/tests/integration.rs` pass.

### Phase B — Output options

### 5. Declare the new global output flags [M]
- **Files**: `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/output.rs`
- **Depends on**: 1, 2
- **Action**: Add `--rows`, `--header`, `--max-chars`, `--omit` and the promoted `--where*` family to `OutputArgs`, map them in `output_opts()`, and carry them in `OutputOpts` with the conflict rules from Approach, "Rows, header and global filters" and "Truncation and omission".
- **Detail**: The `--where*` globals use the exact ids and types of the `QueryArgs` fields (`where_eq: Vec<String>`, …) so clap merges them; `query_opts()` marks them engine-consumed for the three list verbs. Extend `UNCONFIGURED`, `shaping_flags()`/`ALL_SHAPING`, and `validate()` (`-q` excludes all; `--rows`×`--header`; `--omit`×`--select`/`--get`/`--template`). Semantics land in tasks 6 and 8; until then the flags parse and validate only. In `run` (`tomlctl/src/cli/dispatch.rs`), add the split-placement guard from Approach, "Rows, header and global filters": count the `--where*` tokens in argv, and raise `kind=validation` when the parsed count is lower. Unit tests in `output.rs`'s `mod tests`:
  - one `global_flag_conflict_*` case per conflict;
  - `global_where_parses_identically_before_and_after_subcommand`: `tasks list --where status=done` parses the same in two separate invocations, before and after the subcommand (via `Cli::try_parse_from`);
  - `global_where_split_across_subcommand_is_refused`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^output::tests::global_/)'` passes (today: no matching test). **predicted, unverified**. falsifier: dropping the `--rows`×`--header` rule turns its case red.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::output_gate` passes (no duplicate long names).

### 6. Apply wildcards, --rows, --header and global --where in the emitter [M]
- **Files**: `tomlctl/src/output.rs`, `tomlctl/tests/output_nesting.rs` (new)
- **Depends on**: 3, 4, 5
- **Backlog**: B-e5b42b3b
- **Action**: Implement `--rows`, `--header`, wildcard `--get`, and global `--where*` row filtering in `emit`/`emit_one` in the order Approach, "Rows, header and global filters" gives.
- **Detail**: `--get` arms switch to `convert::navigate_json_all`. `--rows` on a single report splits it into header + rows; on a row report it is a `kind=validation` error naming the report's row field. `--header` on a row report shapes the report minus its rows field as one object; on `Rows::Top` or a single report it errors. Filtering uses `query::json_rows::filter` (with `query::predicates_from`) before `--limit`. Write envelopes keep `compact_lenient`'s warn-not-fail behaviour. On the three list verbs, `--rows` and `--header` are refused with `kind=validation` rather than dropped by `query_opts()`. Wildcards need an explicit `*`: a star-less `deps.ref` over an array still errors. Integration cases: `tasks show <id> --with deps --rows deps --get ref`, `tasks show <id> --with deps --get 'deps.*.ref'`, `tasks check --where-not severity=info --get class`, `backlog check --summary … --header --get verdict`, `--where` on a single report without `--rows` erroring.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_nesting` passes. **predicted, unverified**. falsifier: today `tomlctl tasks show 3 --slug eventual-churning-sunrise --with deps --select 'deps.*.ref'` exits 1 with "matches no field" (probed 2026-10-09); the test asserts it prints `{"deps.*.ref":[…]}`.

### 7. Add template widths and wildcard placeholders [M]
- **Files**: `tomlctl/src/output/template.rs`
- **Depends on**: 3
- **Action**: Parse `{path:N}` into a width-carrying placeholder, render wildcard placeholders comma-joined via `navigate_json_all`, and add `pub(crate) fn truncate_text(s: &str, n: usize) -> Cow<str>` producing the `…(+K)` marker of Approach, "Truncation and omission".
- **Detail**: `Segment::Placeholder(String)` becomes `Placeholder { path, width: Option<usize> }`; `paths()` still yields the bare path for validation. A non-numeric width is a parse error naming the column, like an unclosed brace. Width counts Unicode scalars. Unit tests in the file are named `width_*` and `wildcard_*`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^output::template::tests::(width|wildcard)_/)'` passes (today: no matching test). **predicted, unverified**. falsifier: rendering `{detail:5}` over `"abcdefgh"` must give `abcde…(+3)`; returning the input unchanged turns it red.

### 8. Implement --max-chars and --omit [M]
- **Files**: `tomlctl/src/output.rs`, `tomlctl/tests/output_trim.rs` (new)
- **Depends on**: 6, 7
- **Action**: Apply `--omit` (with `--select`'s unknown-path error) and `--max-chars` (every string value, via `template::truncate_text`) at their positions in the emit order.
- **Detail**: Approach, "Truncation and omission". `--max-chars` walks rows, the header and single objects after projection, so it also caps `--get` output. The list verbs bypass `emit`'s option set, so extend `query_opts()` (an allow-list today) to pass `omit` and `max_chars`, and make `streaming_allowed()` false when either is set. Integration cases: `items list --lines --max-chars 40`, `tasks list --omit body`, `tasks render --check --omit diff`, `tasks show <id> --with body --max-chars 40`, `--omit` with an unknown path erroring.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_trim` passes. **predicted, unverified**. falsifier: removing the `--max-chars` walk leaves a 40+ char `detail` and turns the case red.

### 9. Report limited on the list verbs [L]
- **Files**: `tomlctl/src/query.rs`, `tomlctl/src/output.rs`, `tomlctl/tests/list_limited.rs` (new), `tomlctl/tests/integration.rs` — `items_list_sort_by_asc_then_limit` / `items_list_sort_by_desc_reverses` read `rows`
- **Depends on**: 4, 8
- **Backlog**: B-42c3a13c
- **Action**: Record the pre-limit row count in the query engine and have `print_query` emit the same `limited` header `emit` uses, only when rows were cut.
- **Detail**: Approach, "List truncation report". Covers `run` and `run_streaming`; the streaming path knows the filtered total before the first write only if it counts first — count, then stream. Keep `query::run`'s signature and add a sibling that returns the pre-limit count. Two outputs change:
  - Pretty bare-array output becomes `{"rows":[…],"limited":{…}}`.
  - `--lines` row-object output gets a leading header line (eventual-churning-sunrise's User Decision 5).

  `--ndjson`, `--pluck` and `--raw` streams stay header-free and report the cut on stderr, because `--ndjson` output is the documented `add-many` input. That means splitting the `ndjson: input.ndjson || input.lines` merge in `query.rs`. Keeping the pluck stream header-free leaves `tomlctl/tests/capabilities.rs::lines_with_pluck_and_limit` green. In `tomlctl/tests/integration.rs`, `items_list_sort_by_asc_then_limit` and `items_list_sort_by_desc_reverses` call `as_array()` on a cut pretty list; make them read `rows`. Integration cases for `items list`, `tasks list` and `backlog list`: `--limit` below and above the row count, plus `--ndjson`, `--pluck … --raw --lines` and `--count` under `--limit`, each header-free.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test list_limited` passes. **predicted, unverified**. falsifier: today `tomlctl tasks list --slug eventual-churning-sunrise --limit 1 --lines` starts with a row (`{"id":1,"ref":…}`, probed), not a `limited` header; the test asserts the header.

### 10. Route sweep --update and edges --dot through shared output-option behaviour [M]
- **Files**: `tomlctl/src/cli/dispatch/items.rs`, `tomlctl/src/tasks/edges.rs`, `tomlctl/tests/output_options.rs`
- **Depends on**: 2
- **Backlog**: B-c32a11bc
- **Action**: Make `items sweep --update` accept `--lines` leniently like every other write and make `tasks edges --dot` refuse output flags through `print_text`'s shared refusal, deleting both bespoke messages, and update the two cases in `tomlctl/tests/output_options.rs` that pin them.
- **Detail**: The bespoke refusals predate the global flag (B-c32a11bc context).
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes with the updated cases. **predicted, unverified**.
  - forward: `grep -c 'tasks edges --lines applies' tomlctl/src/tasks/edges.rs` is 0 (today: 1 — the bespoke `bail!` at `edges.rs:35`).

### Phase C — Write side, vocabulary, release

### 11. Add the shared field-flag parser [S]
- **Files**: `tomlctl/src/fields.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create `fields.rs` exposing `pub(crate) struct FieldArgs { set: Vec<String>, set_json: Vec<String>, set_file: Vec<String> }` (a clap `Args` with repeatable `--set`, `--set-json`, `--set-file`) and `pub(crate) fn build(args: &FieldArgs, base: Option<JsonValue>) -> Result<Option<JsonValue>>` implementing Approach, "Field flags"; declare `mod fields;` in `lib.rs`.
- **Detail**: Split on the first `=` (`split_once`); no `value_delimiter`. `--set` values stay JSON strings. `DATE_KEYS` coercion happens where the merged payload is converted to TOML (`convert::maybe_date_coerce`, exactly as for `--json` payloads), so `build` does not call it. `--set-json` parses with `serde_json`. `--set-file` reads UTF-8 and strips one leading U+FEFF and one trailing `\n` (or `\r\n`). `-` reads through `io::read_text_arg`, which claims stdin and enforces the 32 MiB cap; `io::claim_stdin` is private and only marks the claim. Dotted keys nest. Duplicate key → `errors::tagged_err` `kind=validation`. Returns `None` when no flag is given. Unit tests in the file's `mod tests` (string default, typed JSON, file with BOM and CRLF, dotted nesting, duplicate key).
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^fields::tests::/)'` passes (today: no `fields` module, so no matching test). **predicted, unverified**. falsifier: coercing `--set retries=3` to an integer turns the string-by-default case red.

### 12. Accept field flags on items add and items update [M]
- **Files**: `tomlctl/src/cli/types/items.rs`, `tomlctl/src/cli/dispatch/items.rs`, `tomlctl/tests/field_flags.rs` (new)
- **Depends on**: 1, 2, 10, 11
- **Action**: Flatten `FieldArgs` into `ItemsOp::Add` and `ItemsOp::Update`, make `--json` optional (required unless a field flag is present), and merge the flag object over the base payload before the existing add/update path.
- **Detail**: Approach, "Field flags". `items_update_to` takes `&str`; serialise the merged value and pass it through unchanged (no edit to `tomlctl/src/items.rs`, and none to the facade-reached `compute_apply_mutation_with`). `--id-prefix` keeps working with flags. `ItemsOp::Add`'s `json: String` becomes `Option<String>`: keeping `required` alongside `required_unless_present_any` panics clap's debug asserts. Integration cases: add with only flags + `--id-prefix R --get id`; prose via `--set-file` holding quotes, backslashes and newlines round-trips byte-identical; update via `--set status=fixed`; duplicate key error.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test field_flags` passes. **predicted, unverified**. falsifier: today `tomlctl items update <file> R1 --set status=fixed` fails with "unexpected argument '--set'" (probed 2026-10-09); the test asserts success.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::output_gate::the_command_tree_has_no_duplicate_flags` passes (clap debug asserts over the whole tree).

### 13. Accept field flags on array-append and several pairs on set [M]
- **Files**: `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch/doc.rs`, `tomlctl/src/io.rs`, `tomlctl/tests/doc_writes.rs` (new)
- **Depends on**: 1, 2, 11
- **Action**: Flatten `FieldArgs` into `Cmd::ArrayAppend` (making `--json` optional), and give `Cmd::Set` a repeatable `--set PATH=VALUE` with optional positionals, applying every pair in one `mutate_doc` closure per Approach, "Document and context writes".
- **Detail**: Positional `PATH`/`VALUE` become `required_unless_present = "set"` (never alongside `required = true`, which panics). Each `--set` value goes through `parse_scalar(value, None)`; `--type` binds the positional pair only. `ScalarMutationPlan` is shared with `set-json` and `tomlctl/src/output.rs` (`build_dry_run_scalar_envelope`, `emit_dry_run_scalar`), so leave it and the single-pair dry-run output unchanged. Only when there are two or more pairs does `doc.rs` emit `{"dry_run":true,"pairs":[…]}`, built from one `build_dry_run_scalar_envelope` per pair. Task 2 hands `doc.rs` the whole `Cmd` value, so no edit to `cli/dispatch.rs` is needed. Fix the "Mirrors the live arm in `cli/dispatch.rs`" comments in `io.rs` (near `compute_set_mutation`) to name `cli/dispatch/doc.rs`. Integration cases: `set ctx.toml status in-progress --set tasks.total=17 --set tasks.in_progress=5` yields one sidecar write and integer counters; `array-append ledger vet_events --set lens=x --set-json sampled_count=3`.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test doc_writes` passes. **predicted, unverified**. falsifier: applying only the positional pair leaves `tasks.total` unchanged and turns the case red.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::output_gate::the_command_tree_has_no_duplicate_flags` passes (`required_unless_present` never alongside `required = true`).

### 14. Stamp updated on context.toml writes [M]
- **Files**: `tomlctl/src/io.rs` — add `stamped_at(path, stamp, f)`, `tomlctl/src/cli/dispatch/doc.rs` — the `set` / `set-json` / `array-append` arms call `stamped_at`, `tomlctl/tests/updated_stamp.rs` (new)
- **Depends on**: 13
- **Action**: Extend `stamp_if_changed` so a changing CLI write to a file whose basename is `context.toml` also refreshes an existing root `updated` to today (UTC), unless the write set `updated` itself; `--no-stamp` suppresses it.
- **Detail**: Approach, "Document and context writes". `stamp_if_changed` and the `stamped*` wrappers take no path, and `mutate_doc`'s closure is `FnOnce(&mut TomlValue)`. So add a path-taking `stamped_at(path, stamp, f)` that applies the existing stamp, plus the `updated` refresh when `path`'s basename is `context.toml`. Only the document-write arms in `cli/dispatch/doc.rs` call it; the other callers keep the existing wrappers. Same conditions as the `last_updated` stamp (key must already exist; no-op writes stay no-ops). The facade's `mutate_doc*` stays unstamped. Also rewrite `read_text_arg`'s TTY error hint in `io.rs`, which suggests `- <<'EOF'`, to name a staged file or `--set-file`.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test updated_stamp` passes. **predicted, unverified**. falsifier: a fixture `context.toml` with `updated = 2020-01-01` after `set … status review` must carry today; leaving `stamp_if_changed` unchanged keeps 2020-01-01.
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test last_updated_stamp` passes.

### 15. Add the execution-record schema module [M]
- **Files**: `tomlctl/src/flow/record_schema.rs` (new), `tomlctl/src/flow/mod.rs`, `tomlctl/src/flow/resolve.rs` — `compile_scope_globset` becomes `pub(crate)`
- **Depends on**: —
- **Action**: Implement the vocabulary, required-field, enum, cap, `files[]` and scope rules of Approach, "Execution-record writes" as pure functions — `pub(crate) fn normalise(entry: &mut JsonMap, scope: &[String]) -> Result<RecordReport>` where `RecordReport { truncated: Vec<String>, dropped_files: Vec<String>, scope_warnings: Vec<String> }`.
- **Detail**: The table is `claude/skills/flow-contract-execution-record-schema/SKILL.md` "Type vocabulary + type-specific required fields" and "Field length caps"; copy the rules, not the prose. Truncate at a char boundary with ` (truncated)` inside the byte cap. `failed_ids` keeps its first 20. A missing required field or an out-of-vocabulary enum is `kind=validation` naming the field and the allowed values. `retries`/`duration_s` digit strings are coerced to integers; a string in `files`/`commits`/`failed_ids` is refused with a hint naming `--set-json`. Scope matching reuses `flow/resolve.rs::compile_scope_globset`, and `None` (empty, absent or all-invalid scope) means no warning. Unknown extra keys pass through. Unit tests per type, plus the type-coercion and empty-scope cases.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^flow::record_schema::tests::/)'` passes (today: no module, so no matching test). **predicted, unverified**. falsifier: dropping the `..` rule lets `../x.rs` through and turns its case red.

### 16. Add the flow record verb [L]
- **Files**: `tomlctl/src/flow/record.rs` (new), `tomlctl/src/flow/mod.rs`, `tomlctl/src/flow/dispatch.rs`, `tomlctl/src/cli/types/flow.rs`, `tomlctl/tests/flow_record.rs` (new)
- **Depends on**: 1, 11, 15
- **Action**: Add `tomlctl flow record` per Approach, "Execution-record writes": resolve the record path from the slug, default `date`, derive `task_ref` from `--task`, build the payload from `--json` + `FieldArgs`, validate with `record_schema::normalise`, mint `E<n>` under the lock via `items_add_many_with_dedupe(…, Some("E"))` with CLI stamping, and print the envelope; `--ndjson` writes an all-or-nothing batch.
- **Detail**: Read the task row through the tasks store reader used by `tasks show` (`tomlctl/src/tasks/show.rs`), not by re-parsing. A missing `agent` is a validation error naming `--set agent=<name>`. `--type` is a clap `ValueEnum` so `capabilities` advertises the vocabulary. Inseparable: a verb needs its clap variant, dispatch arm and module together. Integration cases: task-completion with `--task`, `--set-file summary=…` over a 2 KiB file (truncated, listed), `files` with an absolute path dropped, all-files-invalid refusal, deviation via `--ndjson` batch, unknown `--type` error, `--dry-run` writes nothing.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_record` passes. **predicted, unverified**. falsifier: today `tomlctl flow record --help` fails with an unrecognised subcommand.

### 17. Add the tasks train verb [L]
- **Files**: `tomlctl/src/tasks/train.rs` (new), `tomlctl/src/tasks/mod.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/src/cli/types/tasks.rs`, `tomlctl/tests/tasks_train.rs` (new)
- **Depends on**: 1
- **Action**: Add `tomlctl tasks train` per Approach, "Commit train", emitting `Rows::Field("groups")`.
- **Detail**: Reuse `tomlctl/src/union_find.rs` (`union`, `components`) and `tomlctl/src/tasks/graph.rs` (`nodes_of`, `Graph::build`, `layered_kahn`); `tomlctl/src/clusters.rs::items_clusters` is the closest template but unions only within a layer — the train merges across layers. Granularity from `Policy.commit_granularity` unless `--granularity` (a clap `ValueEnum`). Ordering collapses strongly connected components of the group dependency graph, then sorts topologically (Approach, "Commit train"). Integration cases: per-task with two tasks sharing a file merged; per-checkpoint grouping; single-commit; topological ordering; a group cycle (A and C share a file, C depends on B, B depends on A) collapsed into one group; a file shared with a pending row reported in `shared_with_pending`; committed rows excluded.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_train` passes. **predicted, unverified**. falsifier: today `tomlctl tasks train --help` fails with an unrecognised subcommand.

### 18. Add the absent part to tasks show [S]
- **Files**: `tomlctl/src/tasks/show.rs`, `tomlctl/src/cli/types/tasks.rs`
- **Depends on**: 17
- **Action**: Add `ShowPart::Absent` (`--with absent`) emitting `absent`: the row's `files` that do not exist under the repo root, per Approach, "Task reads".
- **Detail**: Resolve paths against the repo root (`tomlctl/src/repo_root.rs`), not the cwd. Unit test `absent_lists_missing_files_under_repo_root` in `show.rs`'s `mod tests` with a temp repo.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^tasks::show::tests::absent_/)'` passes (today: no matching test). **predicted, unverified**. falsifier: today `tomlctl tasks show 1 --slug eventual-churning-sunrise --with absent` fails with an invalid value for `--with`.

### 19. Carry ref in tasks update envelopes [M]
- **Files**: `tomlctl/src/tasks/update.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/tests/tasks_write.rs` — exact-equality update envelopes gain `ref`
- **Depends on**: 17
- **Action**: Return each updated row's `ref` from `update_many` and include it in both envelope shapes (`{ok,id,ref,changed}`, and per-result `ref`).
- **Detail**: Approach, "Task reads". The `store::mutate` closure already holds the row. The JSON envelope is built in `tasks/dispatch.rs`. `tomlctl/tests/tasks_write.rs` pins it with whole-object `assert_eq!` in three places: the single-id `{"ok":true,"id":2,"changed":[…]}` asserted twice, and the per-result object of a multi-id update. Add `ref` to all three.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_write` passes with `ref` asserted in the update envelopes. **predicted, unverified**. falsifier: omitting `ref` from the single-id envelope turns the case red.

### 20. Add the batch form of backlog check [L]
- **Files**: `tomlctl/src/backlog/check.rs`, `tomlctl/src/backlog/dispatch.rs` — the `BacklogOp::Check` arm, `tomlctl/src/cli/types/backlog.rs`, `tomlctl/tests/backlog_check_batch.rs` (new)
- **Depends on**: 1
- **Action**: Add `--ndjson SRC` to `backlog check` (conflicting with `--summary`), evaluating every line against one store read and emitting `Rows::Field("results")` per Approach, "Batch backlog check".
- **Detail**: Parse lines like `tomlctl/src/backlog/add_many.rs::parse_rows` (errors name the line). Reuse `evaluate` and `build_report`; cap candidates at 5 per line. Inseparable: `backlog/dispatch.rs` destructures every `BacklogOp::Check` field with no `..`, so the new field and the now-optional `summary` must land in the same edit. Integration cases: a duplicate and a novel line in one batch; `--get verdict`; malformed line error.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_check_batch` passes. **predicted, unverified**. falsifier: today `tomlctl backlog check --ndjson x` fails with an unexpected argument.

### 21. Mint ids in items apply and keep a high-water mark [M]
- **Files**: `tomlctl/src/items.rs`, `tomlctl/src/cli/types/items.rs`, `tomlctl/src/cli/dispatch/items.rs`, `tomlctl/tests/id_minting.rs` (new)
- **Depends on**: 12
- **Backlog**: B-b0357675
- **Action**: Add `items apply --id-prefix P` minting ids for id-less add ops via a new wrapper beside `compute_apply_mutation_with`, reporting `ids`; record removed ids in a root `[id_high_water]` table on the CLI remove paths and make `items_next_id_in` honour it, per Approach, "Id minting".
- **Detail**: Keep `compute_apply_mutation_with`'s signature and behaviour (glimpse facade, `tomlctl/src/ledgers.rs`). It is also the CLI's live apply path, so the new wrapper replaces that call site in `cli/dispatch/items.rs`. Rewrite the id-less row error in `items_add_value_to` to name `items apply --id-prefix` instead of numbering upward from `items next-id`. Its only pinned fragment, "non-empty string `id`", stays. Fix the `items.rs` comment that names `cli/dispatch.rs` as the apply call site. No reader rejects unknown root keys (no `deny_unknown_fields` in `tomlctl/src` or `glimpse/src`), so `[id_high_water]` is a root table. If glimpse's tests fail on it, stop and surface the failure rather than switching storage. Mint after id-less detection, before mutation, recording minted ids in the plan. Integration cases: mixed apply with two id-less adds and an update mints a contiguous run; an add op carrying an `id` under `--id-prefix` is refused; remove the top id then `items next-id` and `items add --id-prefix` skip it.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test id_minting` passes. **predicted, unverified**. falsifier: without the high-water read, re-adding after removing the top id re-mints it and turns the case red.
  - guard: `cargo test --manifest-path glimpse/Cargo.toml` passes (orchestrator checkpoint).

### 22. Turn on clap suggestions and alias read to parse [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/src/cli/types/cmd.rs`
- **Depends on**: 13
- **Action**: Add `suggestions` to the clap feature list (rewriting the comment that records it was left off), refresh both lockfiles, and add `#[command(alias = "read")]` to `Cmd::Parse`.
- **Detail**: Approach, "Vocabulary recovery". Neither lockfile lists `strsim` under `clap_builder` today (both show only `anstyle`, `clap_lex`), and glimpse links tomlctl, so feature unification adds it to both: refresh with `cargo tree --manifest-path tomlctl/Cargo.toml >/dev/null` and `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null`, and expect `strsim 0.11.1` (already locked elsewhere in the repo). The pre-commit hook's `--locked` glimpse clippy blocks a stale `glimpse/Cargo.lock`. Inseparable: the manifest change and both locks land together.
- **Acceptance**:
  - forward: `grep -c '"suggestions"' tomlctl/Cargo.toml` is 1 (today: 0; positive control on `    "suggestions",` prints 1).
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(=cli::types::cmd::tests::read_alias_parses_as_parse)'` passes. The new unit test asserts `Cli::try_parse_from(["tomlctl","read","f.toml"])` yields `Cmd::Parse` (today: no matching test). **predicted, unverified**. falsifier: removing the alias makes that parse fail.
  - guard: the full suite passes (orchestrator checkpoint) — suggestions add `tip:` lines only.

### 23. Add the value and id aliases [M]
- **Files**: `tomlctl/src/cli/types/backlog.rs`, `tomlctl/src/cli/types/tasks.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/tests/id_aliases.rs` (new)
- **Depends on**: 18, 19, 20
- **Action**: Add `#[value(alias = "related")]` to `RelationKind::RelatesTo`, make `backlog triage` split its positional ids on commas like `tasks update` and `backlog show` already do, and add a hidden `--id` (alias `ids`) arg on `tasks show`, `tasks update`, `backlog show` and `backlog triage`, merged with the positional list in dispatch, per Approach, "Vocabulary recovery".
- **Detail**: Positional lists switch from `required = true` to `required_unless_present = "id"`. The `--id` arg is a `Vec` with `value_delimiter = ','` and no `num_args`. `backlog triage <ids>` today treats `B-a,B-b` as one id and reports it under `skipped_stale` with `found: null` (observed 2026-10-09 during this flow's Phase 9 promote) — give its positional `value_delimiter = ','` so both spellings work, and keep a space-separated list working. Inseparable: each clap change needs its dispatch merge. Integration cases: `backlog relate … --as related`, `tasks show --id 3`, `backlog show --ids A,B`, `backlog triage A,B --promote …` equal their canonical forms; no ids at all still errors.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test id_aliases` passes. **predicted, unverified**. falsifier: today `tomlctl backlog relate B-x --to B-y --as related` fails with "invalid value 'related'" (transcript evidence).

### 24. Cover the new verbs and flags in the output-options test [M]
- **Files**: `tomlctl/tests/output_options.rs`
- **Depends on**: 6, 7, 8, 9, 10, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23
- **Action**: Add cases for `flow record`, `tasks train` and batch `backlog check`, and extend `check()` with probes for `--rows`, `--header`, `--omit`, `--max-chars`, a wildcard `--get`, and a global `--where` on a row report.
- **Detail**: `every_leaf_command_has_a_case` reads the leaf set from `capabilities`, so the new verbs fail it until cased. Extend the file's single-segment `nav()` to dotted paths. Drop the `!engine_listed` exclusion from the `--limit 1 --lines` probe, now that task 9 gives the list verbs a `limited` header. Add list-verb probes for `--max-chars` and `--omit`, and a probe that `--rows` is refused on a list verb. Probes run only where the case's shape allows (e.g. `--rows` on `Shape::One` cases with an array field).
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes. **predicted, unverified**. guard: deleting the `tasks train` case turns `every_leaf_command_has_a_case` red.

### 25. Advertise the features and release 0.15.0 [L]
- **Files**: `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`
- **Depends on**: 5, 22, 24
- **Action**: Add the `FEATURES` entries listed in Approach, "Advertising", mirror them in `capabilities_features_contains_every_plan_feature` and the README sample block and feature table, add the new globals to the `global_flags` assertions and README block, document the new verbs in README's verb listing, and bump the version to 0.15.0. The bump includes the `0.14.0` literal in `capabilities_version_matches_cargo_toml` and the README sample. Add `tasks train` / `flow record` to the hand-kept read/write help lists (`read_subs`, `write_subs`) in `tomlctl/tests/capabilities.rs`, document `ref` in README's `tasks_update` envelope row, and refresh both lockfiles.
- **Detail**: Inseparable: `readme_feature_transcriptions_match_capabilities_features` couples FEATURES to README in one assertion. Refresh `glimpse/Cargo.lock` with `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` (the pre-commit hook's `--locked` glimpse clippy blocks a stale lock).
- **Acceptance**:
  - forward: `grep -c '^version = "0.15.0"' tomlctl/Cargo.toml` is 1 (today: 0).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes. **predicted, unverified**. falsifier: omitting `flow_record` from the README table turns `readme_feature_transcriptions_match_capabilities_features` red.

### Phase D — Guidance sweep

### 26. Document the output options and the guidance canon in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`, `claude/skills/tomlctl/references/query.md`
- **Depends on**: 25
- **Action**: Extend SKILL.md's "Output options" section with wildcards, `--rows`, `--header`, global `--where*` (array semantics), `--max-chars`, `--omit` and template widths, and add the guidance canon of Approach, "Guidance canon" as a short section. In `query.md`, document array-aware predicates, dotted keys and the list `limited` header, and drop the `items next-id … --strict-read` line from its `--strict-read` fence. Correct the SKILL.md statements tasks 9 and 10 make false: that the list verbs report no `limited` key; that `items sweep --update` and `tasks edges --dot` refuse `--lines` with their own messages; the quick-reference row recommending `items next-id`; and the `"version":"0.14.0"` capabilities sample. In `query.md`, correct the `--limit` row saying a list "adds no `limited` key".
- **Detail**: Do not edit SKILL.md's frontmatter `description` (12 characters of headroom). Every bash fence must parse under `command_lint`; quote templates and `@path`. Replace the per-command `--lines` table remnants the canon supersedes. Name the stderr rule: read failures with `--error-format json`; never `2>&1` or `2>/dev/null`.
- **Acceptance**:
  - forward: `grep -c -- '--rows' claude/skills/tomlctl/SKILL.md` is at least 2 (today: 0).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes (command and flag-table lints).

### 27. Rewrite the write reference around field flags and staged files [M]
- **Files**: `claude/skills/tomlctl/references/write.md`
- **Depends on**: 25
- **Action**: Replace every heredoc example with field flags or a Write-tool staged file. Document `--set`/`--set-json`/`--set-file`, multi-pair `set` (collapsing the `set` section's two `set` calls on one `context.toml` into one), the `updated` stamp, `items apply --id-prefix` and the high-water mark. Rewrite the body of "Stdin input for large JSON payloads" to the canon, and the prose naming the heredoc contract.
- **Detail**: Approach, "Guidance canon" and "Field flags". Keep `write_reference_enumerates_every_date_key`'s date-key list intact. Keep the `Stdin input for large JSON payloads` heading text byte-identical: `claude/skills/backlog-capture/SKILL.md` (task 35, parallel), `tomlctl/README.md` and write.md's own TOC link to its anchor, and `skill_markdown_links_resolve` gates them. `items next-id` stays documented (it still exists) but stops being recommended for apply batches; its section's fences carry `ignore-guidance-lint` only if they still show the verb in a bash fence.
- **Acceptance**:
  - forward: `grep -cE '<<-?'"'"'?[A-Z]+' claude/skills/tomlctl/references/write.md` is 0 (today: 6).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::skills` passes (ceilings, date keys, links).

### 28. Document the new tasks verbs and parts [M]
- **Files**: `claude/skills/tomlctl/references/tasks.md`, `claude/skills/tomlctl/references/tasks-write.md`, `claude/skills/flow-contract-task-store/SKILL.md` — §5 train semantics, update envelope ref, absent part, §11 read-verb list
- **Depends on**: 25
- **Action**: Document `tasks train` (read verb) in `tasks.md`, `--with absent`, the `--id` alias, and the `ref` in `tasks update` envelopes in `tasks-write.md`. Then bring `flow-contract-task-store/SKILL.md`, the canonical contract for every `tasks` verb, up to date: add `train` to §5 "Verb semantics" and to §11's sub-agent read-verb list, add `ref` to the `update` envelope, and add `absent` to the `show` parts and the §12 fetch-by-id idiom.
- **Detail**: Flag tables must satisfy `flag_table_lint`. The task-store skill has headroom under its 500-line ceiling.
- **Acceptance**:
  - forward: `grep -c 'tasks train' claude/skills/tomlctl/references/tasks.md` is at least 1 (today: 0).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 29. Document batch check and flow record [M]
- **Files**: `claude/skills/tomlctl/references/backlog.md`, `claude/skills/tomlctl/references/flow.md`
- **Depends on**: 25
- **Action**: Document `backlog check --ndjson` and the `related` alias in `backlog.md`, and `flow record` with its validation, envelope and batch form in `flow.md`. Add `ignore-guidance-lint` to the info string of `flow.md`'s `--error-format json … 2>&1 >/dev/null` demo fence; it deliberately documents the stderr envelope.
- **Detail**: `backlog.md` is at its 600-line ceiling: offset additions by consolidating per-command output-shape notes that the global options now cover. Replace heredoc idioms there (including the `backlog check --summary - <<'SUMMARY'` example) with staged files or `--summary - < <staged-file>`.
- **Acceptance**:
  - forward: `grep -c 'flow record' claude/skills/tomlctl/references/flow.md` is at least 3 (today: 0).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::skills::skill_references_under_line_ceiling` passes.

### 30. Route execution-record writes through flow record in the schema skill [M]
- **Files**: `claude/skills/flow-contract-execution-record-schema/SKILL.md`
- **Depends on**: 25
- **Action**: Replace the "Heredoc write contract (one call)" section with a `flow record` write contract, state that type, required-field, enum, cap and `files[]` validation and the date default are tool-enforced, drop the "never tempfile-stage" and `items next-id` guidance, and update the frontmatter description's "heredoc write contract" wording within 1,024 characters.
- **Detail**: Approach, "Execution-record writes". Keep the schema tables (readers still need them). Keep the restricted-subcommand note for `items orphans|find-duplicates|sweep|clusters`.
- **Acceptance**:
  - forward: `grep -ciE 'heredoc|blessed path' claude/skills/flow-contract-execution-record-schema/SKILL.md` is 0 (today: 4).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::skills` passes (description cap).

### 31. Update the ledger schema skill and the apply verification reference [S]
- **Files**: `claude/skills/flow-contract-ledger-schema/SKILL.md`, `claude/skills/flow-contract-apply-pipeline/references/verification.md`
- **Depends on**: 25
- **Action**: Replace the ledger write idioms (heredoc, "never stage", `next-id` numbering) with field flags, staged `--ndjson`/`--ops '@path'` and `items apply --id-prefix`; replace the RFC-8259 hand-escaping instruction in `verification.md` with field flags or `--set-file`.
- **Detail**: Approach, "Guidance canon" and "Id minting".
- **Acceptance**:
  - forward: `grep -c 'RFC-8259' claude/skills/flow-contract-apply-pipeline/references/verification.md` is 0 (today: 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 32. Move /implement onto the new verbs [M]
- **Files**: `claude/commands/implement.md`, `claude/agents/flow-bootstrap.md`
- **Depends on**: 25
- **Action**: Collapse the Phase 1 `context.toml` update into one `set` with `--set` pairs (no `updated`, now stamped), replace the dispatch-time `test -e` loop with `tasks show <ids> --with absent`, route step 5b and every record append through `flow record --task <id>` (dropping the hand `task_ref` lookup, path validation and RFC-8259 escaping), replace step 5(iii)'s hand partition with `tasks train`, and raise `flow-bootstrap.md`'s tomlctl floor to 0.15.
- **Detail**: Approach, "Commit train", "Task reads", "Execution-record writes", "Document and context writes". Rewrite the surrounding prose that still names "the heredoc write contract" or "the skill's heredoc form" (find the sites with `grep -niE 'heredoc' claude/commands/implement.md`). Remove the `while IFS= read` loop. Keep the staging rule (`git add -- <paths>`, never `-A`) and the halt-on-failed-commit rule; `shared_with_pending` replaces the "files owned by still-pending tasks" prose check.
- **Acceptance**:
  - forward: `test "$(grep -c 'tomlctl set <context_path>' claude/commands/implement.md)" -le 2` (today: 5).
  - forward: `grep -c 'tomlctl tasks train' claude/commands/implement.md` is at least 1 (today: 0).
  - forward: `grep -c 'while IFS= read' claude/commands/implement.md` is 0 (today: 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes.

### 33. Move /plan-update and /tdd onto flow record [M]
- **Files**: `claude/commands/plan-update.md`, `claude/commands/tdd.md`
- **Depends on**: 25
- **Action**: Route every execution-record append through `flow record`, collapse `context.toml` updates into one multi-pair `set`, and delete the "never stage" and hand date-validation instructions the tool now covers.
- **Detail**: Keep `/plan-update`'s date-validation intent where it guards a regressing date, now as a single `get` + comparison rather than a hand-typed date. Rewrite the prose naming "the canonical heredoc write" / "the heredoc append idiom" in both files (find with `grep -niE 'heredoc' claude/commands/plan-update.md claude/commands/tdd.md`). `tdd.md`'s heredoc into tomlctl sits in an untagged fence that the guidance lint's walker skips; tag it `bash` while rewriting it to `flow record`.
- **Acceptance**:
  - forward: `grep -ciE 'never stage|blessed path' claude/commands/plan-update.md` is 0 (today: 1).
  - forward: `grep -c 'tomlctl flow record' claude/commands/tdd.md` is at least 1 (today: 0).

### 34. Move review, optimise and review-plan ledger writes onto apply minting [M]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-plan.md`
- **Depends on**: 25
- **Action**: Replace "add ops take ids numbered upward from one `items next-id` read" with `items apply --id-prefix`, and point `[[vet_events]]` appends at `array-append` with field flags.
- **Detail**: Approach, "Id minting". Keep `--on-stale skip`, `expect`, and `--no-stamp` on the Step-2 interim checkpoints. Rewrite the prose naming the heredoc form ("one heredoc, even for a single disposition", "fed by a quoted heredoc on Linux and macOS"); find the sites with `grep -niE 'heredoc' claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md`.
- **Acceptance**:
  - forward: `grep -cE 'numbered upward|next-id' claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md | awk -F: '{s+=$2} END {print s}'` is 0 (today: 5).

### 35. Update backlog capture and /plan-new [M]
- **Files**: `claude/skills/backlog-capture/SKILL.md`, `claude/commands/plan-new.md`
- **Depends on**: 25
- **Action**: In backlog-capture, replace the heredoc idioms with field flags / staged NDJSON and add the batch `backlog check --ndjson` idiom for harvesting several `TANGENTIAL:` lines; in plan-new, collapse the seed-adoption `set`s into one multi-pair `set` and drop the hand `updated` write.
- **Detail**: Approach, "Guidance canon". `backlog add` and `backlog check` take no field flags, and `backlog check --summary` reads only `-` (it refuses `@`). So the sub-agent data-safety rule ("text is data, never a shell token") now points at a Write-tool staged file: `backlog check --summary - < <file>`, or one `backlog check --ndjson <file>` for several probes, and `backlog add --json '@<file>'`. It no longer points at a quoted heredoc. Rewrite the remaining prose naming the heredoc form in both files (find with `grep -niE 'heredoc' claude/skills/backlog-capture/SKILL.md claude/commands/plan-new.md`). Keep the `write.md#stdin-input-for-large-json-payloads` link; task 27 keeps that heading.
- **Acceptance**:
  - forward: `grep -cE "<<-?'?[A-Z]+" claude/skills/backlog-capture/SKILL.md` is 0 (today: 4).

### 36. Update the vet skills and test-bootstrap vet_events appends [M]
- **Files**: `claude/skills/flow-contract-vet-research/SKILL.md`, `claude/skills/flow-contract-apply-vet-implement-lite/SKILL.md`, `claude/commands/test-bootstrap.md`
- **Depends on**: 25
- **Action**: Replace the `[[vet_events]]` heredoc appends with `array-append … --set … --set-json …` forms (timestamp via `--set timestamp=…` stays agent-supplied) and update the prose reference in `test-bootstrap.md`.
- **Detail**: Approach, "Field flags". Also rewrite vet-research's prose that points at "the canonical heredoc form" and says every field is "inline in the heredoc above".
- **Acceptance**:
  - forward: `cat claude/skills/flow-contract-vet-research/SKILL.md claude/skills/flow-contract-apply-vet-implement-lite/SKILL.md | grep -c '<<'` is 0 (today: 2).

### 37. Update the rollback and plansDirectory skills [S]
- **Files**: `claude/skills/flow-contract-apply-rollback-protocol/SKILL.md`, `claude/skills/flow-contract-plansdirectory-prompt/SKILL.md`
- **Depends on**: 25
- **Action**: Replace their heredoc writes and drop "the primary form" wording: use field flags for the rollback protocol's `array-append`, and an inline single-line `json set --json` for the plansDirectory prompt.
- **Detail**: Approach, "Guidance canon". `json set` takes no field flags; its value is short and machine-built, so inline JSON is the canon's form for it.
- **Acceptance**:
  - forward: `cat claude/skills/flow-contract-apply-rollback-protocol/SKILL.md claude/skills/flow-contract-plansdirectory-prompt/SKILL.md | grep -c '<<'` is 0 (today: 2).

### 38. Add the guidance lint [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/lint.rs`, `claude/commands/commit.md` — drop the `2>/dev/null` on the best-effort `tomlctl flow resolve` fence
- **Depends on**: 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37
- **Action**: Add `guidance_lint` per Approach, "Guidance lint", walking the bash fences `command_lint` walks and failing on the five anti-patterns, with an `ignore-guidance-lint` fence opt-out. In `claude/commands/commit.md`, drop the `2>/dev/null` from the best-effort `tomlctl flow resolve` call; a non-zero exit already means "no flow".
- **Detail**: Reuse `command_lint`'s fence walker, including its `\` continuation stitching, so a heredoc reaching tomlctl through a continuation is caught. Report every violation with file, line and the rule name, not just the first. The other pre-existing sites are cleared by tasks 26, 27, 29 and 33 (Approach, "Guidance lint").
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(=cli::dispatch::tests::lint::guidance_lint)'` passes (today: no matching test). **predicted, unverified**. falsifier: inserting `tomlctl tasks show 1 --slug x | python -c 'print(1)'` into a bash fence in `claude/skills/tomlctl/SKILL.md` turns it red.

### 39. Keep hidden arguments out of the capabilities flag listing [S]
- **Files**: `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 25
- **Action**: Make capabilities describe_flags skip arguments clap marks hidden, so the recovery aliases (--id/--ids) never appear as documented flags; add a capabilities test asserting tasks show lists no --id flag.
- **Acceptance**: cargo nextest run --manifest-path tomlctl/Cargo.toml --test capabilities passes, including a new case that fails when describe_flags lists hidden arguments.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 1, 2, 3, 4 — dependency closure: 1, 2, 3, 4. Module splits, wildcard path walker, array-aware predicates (refactor + engine increment; behaviour of every existing command unchanged)

— CHECKPOINT B after tasks 9, 10 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10. The read-side output options and the list `limited` header (complete output surface)

— CHECKPOINT C after tasks 39 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 39. The write-side verbs, vocabulary recovery, tests and the 0.15.0 release (final CLI surface)

— CHECKPOINT D after tasks 38 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38. The guidance sweep and the lint that holds it

## Risks

- **Global/local `--where` merge misbehaves** for a value given before the subcommand. clap 4.6.7 does merge a global with a same-id local, but it keeps only the child's values for a repeatable global (`ArgMatcher::fill_in_global_values`). So `--where` split across both sides would silently drop the earlier predicates. Mitigation: task 5's test runs a list verb with `--where` before the subcommand and, in a separate invocation, after it, and requires identical output. A split across both sides raises `kind=validation` through an argv token-count guard in `cli::run`. On failure, the list verbs read the global the way `--select` does today (`query_opts`).
- **clap `suggestions` changes stderr text that a test pins exactly.** Mitigation: task 22 runs the suite. Suggestions only add `tip:` lines, and the existing tests use `contains` (Research Notes, "clap 4.6.7").
- **An `[id_high_water]` root table surprises a reader** (glimpse's facade or `flow doctor`). Mitigation: task 21 runs glimpse's tests, and `ledger_read` reads `[[items]]` only. No reader rejects unknown root keys today (no `deny_unknown_fields` in `tomlctl/src` or `glimpse/src`); if glimpse's tests fail on the new table, stop and surface it rather than switching storage.
- **Truncation hides content an agent needed.** Mitigation: the `…(+K)` marker says how much was cut, and the guidance tells agents to re-run without `--max-chars` for full text.
- **Older installed binaries.** The carriers start using 0.15 verbs. Mitigation: `claude/agents/flow-bootstrap.md` gets the 0.15 floor in task 32, so skew halts at Step 0, as 0.14 did. Because `claude/{agents,commands,skills}` are symlinked into `~/.claude/`, each Phase D edit is live machine-wide when written. So the orchestrator runs `cargo install --path tomlctl` and `cargo install --path glimpse` right after checkpoint C commits, before dispatching any Phase D task.
- **Git Bash path conversion rewrites `--set` values that start with `/`.** Mitigation: the guidance canon routes such values through `--set-file` (or `MSYS_NO_PATHCONV=1`).
- **`backlog.md` is at its 600-line ceiling.** Mitigation: task 29 offsets its additions by consolidating the per-command shape notes the global options superseded; `skill_references_under_line_ceiling` gates it.
- **The pure-move splits collide with in-flight work.** Mitigation: both split tasks are the first frontier and nothing else touches those files until checkpoint A.

## After Merge

- Re-run `cargo install --path tomlctl` and `cargo install --path glimpse` (first run at checkpoint C, per Risks) if anything in tomlctl changed after checkpoint C, then confirm the release with `tomlctl capabilities --get features` (the **predicted, unverified** success criterion).
- Restart open Claude Code sessions so they reload the updated skills, which are symlinked.
- Rewrite these out-of-repo memory notes to the new forms:
  - `~/.claude/projects/C--Users-rossa-dev-reportdesignkit/memory/task-ref-is-the-store-rows-ref.md`: `--get ref`, then `flow record --task`.
  - `~/.claude/projects/C--Users-rossa-dev-tradewinds-portal/memory/flow-ledger-tomlctl-only.md`: drop the two-call write.
  - `~/.claude/projects/C--Users-rossa-dev-dev-tools/memory/tomlctl_raw_vs_lines_stderr.md`: the `completed` derivation is `--count-distinct`.
  - `~/.claude/projects/C--Users-rossa-dev-reportdesignkit/memory/promoted-backlog-items-need-reconciling.md`: `backlog reconcile --apply` exists.
  - `~/.claude/projects/C--Users-rossa-dev-dev-tools/memory/tomlctl_sidecar_lock_retry_windows.md`: retry once, with no shell loop.
  - `~/.claude/projects/C--Users-rossa-dev-reportdesignkit/memory/long-git-trains-go-in-a-script-file.md` and `architecture-programme-paused-mid-group-b.md`: use `tasks train`.
  - `~/.claude/projects/C--Users-rossa-dev-dev-tools/memory/feedback_heredoc_windows.md`: point at the guidance canon.
- Re-run the transcript extraction (Exploration Notes, "Transcript evidence") against the first sessions that use 0.15, and capture any new workaround class as a backlog item.

## Exploration Notes

### Transcript evidence (local Windows sessions, 2026-09-08 → 2026-10-09)
10,848 Bash/PowerShell calls naming tomlctl across 2,357 sessions (reportdesignkit 5,135, dev-tools 5,607); none used the 0.14 output/write features. Extracted by a scratchpad script over `~/.claude/projects/**/*.jsonl`.
- Post-processing is `python -c` (2,247 pipes) and `node -e` (456); `jq` once. ~1,260 of ~2,050 parsed snippets map onto `--get/--select/--template/--lines`; the remainder: row filter (355), field slice `[:N]` (334), string ops (177), `len` (103), sort/Counter (56), drop-one-key (35), nested-array projection (36), os.path.exists on `files` (35).
- `head` 1,642 / `tr` 694 / `cut` 398 / `grep` 988 tails: mostly `next-id` quote stripping, `grep '"ref"' | sed`, `cut -c1-200` on write envelopes, `head -c` on long objects — covered by 0.14 except `head -c`/field truncation.
- Payload construction for writes: 921 heredoc-script pipes, 318 python/node generators, 326 printf pipes, 151 `cat > file <<EOF` staging. Heredoc failures: 16 invalid escape, 7 unexpected EOF, 6 JSON parse. 344 heredoc bodies longer than 5 lines.
- 135 agent-written helper scripts call tomlctl (≈35 `rec-NN.py` in one session; `settle*.sh`, `complete.mjs`, `mint-NNN.py`, `train*.py/.mjs` ×7). `rec.py` docstring: "Every free-text field is RFC-8259 encoded by json.dumps; summary capped at 1 KiB, prose at 8 KiB."
- 2,923 hand-typed `"date":"YYYY-MM-DD"` literals in `items add` commands.
- 82 commands run 2–5 `tomlctl set` on one context.toml (keys: tasks.total 71, updated 64, tasks.in_progress 56, status 45, tasks.completed 30).
- 43 prefix strips (`tr -d '"E'`, `${ID#E}`) to number ids upward.
- 31 commands run 2–5 `backlog check`; harvest scripts loop check→add.
- Guessed vocabulary errors: `tomlctl read` 12, `--as related` 10, `--id` on show 14, `items update --set` 4, `backlog show --ndjson` 1, `--pluck` on `tasks show`/`inputs list` 2. 171 `--help` lookups (backlog triage 18, backlog list 17, items update 14, backlog 13, relate 9).
- Shape guessing: `d.get('item', d)` 54, list-or-wrapped 79. `backlog show X --get summary` fails (needs `item.summary`) — verified.
- Suppression: `2>/dev/null` on tomlctl 562, `2>&1` 2,500, `2>&1 | python/node` 176, `|| true` 22.
- No direct Edit/Write of flow TOML (only `documentation-conventions.toml` comments) — agents do route through tomlctl.

### Read side (`tomlctl/src/output.rs`, `output/template.rs`, `convert.rs`, `query.rs`)
- `OutputOpts{select,limit,lines,get,template,quiet,json_errors}` built in `cli/dispatch.rs::output_opts`, installed by `output::configure` → `validate()`. `emit()` order: `-q` → `validate_shaping_paths` → `--limit` → `--select` (`project`) → `--get`/`--template` → `--lines`/style. `emit_one()` for single objects; `--get` spreads a top-level array one element per line.
- `convert.rs`: `navigate_json(root, path) -> Option<&JsonValue>` (dot path, object key or usize index, no wildcard); `project(row, paths)` keys by path string; `validate_paths(rows, paths, flag)` → "{flag} path `p` matches no field; available fields: …". Wildcard needs a multi-valued walker used by `project`, `validate_paths`, `--get` arms, `Template::render`; query `validate_select` uses TOML-side `navigate`.
- `template.rs`: `Segment::Placeholder(String)`; `paths()`, `render()` → `value_text()` shared with `--get`. Width would be `Placeholder{path,width}` parsed on `:`.
- `query.rs`: `run`, `run_streaming`, `apply_filters`, `eval_predicate`. Flat `tbl.get(key)`. contains/prefix/suffix/regex via `value_as_string`→`stringify_scalar` returns "" for arrays; `--where/--where-not/--where-in` via `eq_typed` whose `_ => false` drops arrays; typed `@…:` `cmp_pred` errors on arrays. Precedent for any-element: `backlog/query.rs::has_tag`. `--exclude` = `QueryArgs.exclude: Option<String>` applied in `apply_projection`; `sweep --exclude` is `Vec<String>` glob → a global named `--exclude` collides (`cli/types.rs:152` comment on OutputArgs); `--count-by`/`--pluck`/`--sort-by` flat.
- Shapes: `tasks show N` One (deps/dependents arrays, `tasks/show.rs`); `tasks show 1,2` Rows::Top; `backlog show` One `{item,evidence,neighbours}` (`backlog/query.rs::build_show_resolved`); `tasks check` Field("findings") `{class,severity,ids,detail}` (`tasks/dispatch.rs:251`); `tasks import-plan` One compact, `findings` nested (`tasks/dispatch.rs:61`); `tasks render --check` One `{ok,path,findings}` (:298); `tasks snapshot` Field("tasks").
- Tests: `tomlctl/tests/output_options.rs` (`cases()` with `read/write/text`; `check()` probes plain, `-q`, `--lines`, `--select`, `--get`, `--template`, `--limit 1 --lines`; `every_leaf_command_has_a_case`); unit tests in `output.rs`; `cli/dispatch/tests/output_gate.rs` (stdout only from output.rs; clap debug_assert); query predicate tests in `query.rs` (~l.1988), `tests/integration.rs` (~1221), `tests/tasks_read.rs:616`.
- New global flag touches: `OutputArgs`, `output_opts()`, `OutputOpts`+`UNCONFIGURED`, `shaping_flags()`/`ALL_SHAPING`, `query_opts()`, `capabilities.rs::build_global_flags_names_every_output_flag`, `tests/capabilities.rs:1562`, README `global_flags` block, maybe `FEATURES`.

### Write side
- `io.rs`: `read_json_arg` (l.316), `read_json_value_from_arg` (l.402) handle `-`/`@path`/inline; `read_ndjson_source` (l.342); `read_text_arg` (l.358) `-` only; `claim_stdin()` one `-` per call; 32 MiB cap.
- `tasks/update.rs::patch()` splits `KEY=VALUE`, gates on `SETTABLE`, typed `assign` — only the split carries to items. `convert.rs`: `ScalarType`, `parse_scalar`, `infer_type` (bool→date→i64→string), `split_type_hint`/`parse_typed_value` (`@int:` prefix grammar → clashes with `@path`), `maybe_date_coerce` + `DATE_KEYS` (includes `date`, `updated`; pinned by skill test `write_reference_enumerates_every_date_key`).
- `items.rs`: `items_add_value_to` (l.307, requires non-empty `id` for `items`), `items_update_value_to` (l.526, skips null/""/[]); `items_update_to`/`compute_update_mutation` take `&str`. Dispatch: add l.551, add-many l.757, update l.894, apply l.1020. `ItemsOp::Add --json` required (types.rs:1669). Minting `items_next_id_in`/`mint_row_id`/`refuse_supplied_id` (l.1147–1219), used only by `items_add_many_with_dedupe` (l.1739); apply add arms (`apply_single_op` l.1042, `apply_op_indexed` l.809) never mint; `compute_apply_mutation_with` (l.1507) records add ids pre-mutation and is used by the glimpse facade — keep signature, add a variant.
- Stamping: `io.rs` `stamped*` (l.818–867) wrap `stamp_if_changed` (l.781), refreshes existing root `last_updated` only; `StampArgs` types.rs:312.
- No execution-record validation anywhere (type vocab, required fields, caps, files). Readers: `flow/render_progress_log.rs`, `tasks/import_plan.rs::record_completions` (l.1284), `flow/doctor.rs::record_completion_refs` (l.574), `tasks/snapshot_record.rs::record_view`. `items::Item::validate` called only by facade (`ledgers.rs:495`). Helpers: `time::today_toml_date()`, `io::relativise`, `path_under_root`.
- `set`/`set-json` (types.rs:580/600; dispatch l.247/295) one path/value via `parse_scalar` + `set_at_path` (convert.rs:188) in one `mutate_doc` closure; dry-run `compute_set_mutation` (io.rs:1157) single plan. context.toml has `updated` not `last_updated`; `flow/init.rs::build_seed_doc` (l.122); `doctor::check_tasks_counters` (l.612) warns only.
- `tasks/update.rs::update_many` (l.86) output `{ok,id,changed}`/`{ok,results}` has no `ref`.
- backlog: `check.rs::dispatch` (l.420) reads store once; `evaluate(doc,&Probe,&Thresholds)` + `build_report` pure → batch is a loop. `add_many.rs::parse_rows` (l.96) NDJSON line errors. `RelationKind{RelatesTo,Duplicates,Supersedes}` (types.rs:1522).
- No clap aliases in the crate. `#[command(alias="read")]`, `#[value(alias="related")]`; `--id` needs a hidden long arg merged with the positional. `FEATURES` at `cli/types.rs:21`; four places per feature (FEATURES, `tests/capabilities.rs::capabilities_features_contains_every_plan_feature` l.1596, README sample + table via `readme_feature_transcriptions_match_capabilities_features` l.2339). `capabilities.rs::describe_values` omits value aliases.
- glimpse facade (`tomlctl/src/lib.rs`, `ledgers.rs`): keep `compute_apply_mutation_with`, `mutate_doc*` unstamped, record rows readable by `record_view`.

### Commit train
- `claude/commands/implement.md:165` step 5(iii) prose: partition window's done tasks per `commit_granularity`, merge groups with overlapping `files[]`, commit per group in dependency order, then `tasks update <ids> --commit <sha>`; reused at 149, 208.
- Reusable: `tomlctl/src/union_find.rs` (`root`, `union`, `components`), `tomlctl/src/clusters.rs::items_clusters` (l.28; unions within a layer only), `tasks/graph.rs` (`layered_kahn` l.480, `Graph::build`, `groups` l.362, `overlap_pairs` l.338, `nodes_of`, `build_or_refuse`), `tasks/batches.rs`, `tasks/closure.rs`. `TaskRow.commit` (schema.rs:210), `Policy.commit_granularity` (l.78). `TasksOp` cli/types.rs:2151, dispatch `tasks/dispatch.rs` ~221-260.

### Guidance surface
- Heredoc openers feeding tomlctl: 17 in 11 files (16 single-line, plus `flow-contract-ledger-schema/SKILL.md` 191-194 via a `\` continuation, plus `claude/skills/tomlctl/references/backlog.md` 126; write.md 407 is prose) — `claude/skills/tomlctl/references/write.md` (35, 62, 190, 261, 381, 407), `backlog-capture/SKILL.md` (273, 288, 299), `flow-contract-ledger-schema/SKILL.md` 194, `flow-contract-execution-record-schema/SKILL.md` 80, `commands/plan-update.md` 143, `commands/tdd.md` 55, `flow-contract-vet-research` 16, `flow-contract-apply-vet-implement-lite` 28, `flow-contract-apply-rollback-protocol` 34, `flow-contract-plansdirectory-prompt` 18. Conflict: write.md 387/407 + backlog-capture 305 (stage a file on Windows) vs execution-record 77, ledger-schema 189, plan-update 207 ("never tempfile-stage").
- Multi-`set`: `implement.md` 59-63, `plan-new.md` 178-180, `plan-update.md` 210, `write.md` 128-129.
- `next-id` numbering upward: ledger-schema 202/226, execution-record 87, write.md 168/301-310, tomlctl SKILL.md 29, review.md 76/82, optimise.md 89/99.
- Hand dates: execution-record 81, plan-update 144, tdd 56, write.md 37, ledger-schema 193, implement 61, plan-new 180; `timestamp` in rollback 35, vet-lite 29, vet-research 17.
- Agent-enforced caps: execution-record 198-205 (+60 `failed_ids` ≤20), ledger-schema 165, vet-research 17; implement 166 RFC-8259 by hand.
- `tasks show` for ref/files: implement 105, 126 (per-id loop with process substitution), 166, 172; agents implement-lite 18/111, implement-deep 89; task-store 202, 381.
- Ceilings: tomlctl `SKILL.md` 238/500 lines, description 1,012/1,024 chars (don't touch); references: backlog.md 600/600 (full), query 464, write 454, tasks 399, agents 301, flow 250, tasks-write 242, tasks-store 218, inputs 217. task-store SKILL 407 lines.
- Gates: `tomlctl/src/cli/dispatch/tests/lint.rs` `command_lint` (l.265; parses bash-fence tomlctl lines, stops at `<<`, opt-out `ignore-command-lint`), `flag_table_lint` (l.676); `tomlctl/src/cli/dispatch/tests/skills.rs` (lib tests, run as `--lib -- cli::dispatch::tests::skills`) ceilings (654/704/799), `write_reference_enumerates_every_date_key` (563), `skill_markdown_links_resolve` (1024), `carrier_invokes_required_skills` (1193); `scripts/shared-blocks.toml` (implement agents share `file-edits` with the heredoc/CRLF warning, deep 69-71 / lite 91-93); `tomlctl/tests/tasks_corpus.rs`.
- Stale memories (outside repo): reportdesignkit `task-ref-is-the-store-rows-ref.md` l.19 (`| jq -r .ref`), tradewinds-portal `flow-ledger-tomlctl-only.md` l.14 (two-call write), dev-tools `tomlctl_raw_vs_lines_stderr.md` (completed derivation now `--count-distinct`), reportdesignkit `promoted-backlog-items-need-reconciling.md` (reconcile exists), dev-tools `tomlctl_sidecar_lock_retry_windows.md` l.12 (shell retry loop), reportdesignkit `long-git-trains-go-in-a-script-file.md` l.18 + `architecture-programme-paused-mid-group-b.md` l.25 (hand trains).
- Verification: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`, `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`, `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored`, `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items`, `bash scripts/verify-shared-blocks.sh`, glimpse `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets --locked` + `cargo test --manifest-path glimpse/Cargo.toml`.

**Early scope check**: well over 25 unique files (tomlctl src ~15, tests ~6, README/Cargo, skills/references ~12, commands ~6, agents 2). User chose one plan with milestones (Phase 1).

### Backlog
Live rows in `tomlctl/`, `claude/`: B-42c3a13c (lists never report `--limit` cut), B-76668b92 (last_updated unstamped after `--no-stamp` checkpoint + empty Step 3), B-a7e28a05 (tdd copy-up carries cycle dedup_id), B-b0357675 (next-id re-mints a deleted top id), B-c32a11bc (sweep --update / edges --dot refuse --lines with own messages), B-d1dc0366 (`get` 20% slower than parse), B-e5b42b3b (backlog check verdict unreachable by --get/--select). All `open`.

## Research Notes

Vet: Agent-1 (clap-internals, lite) — 3 sampled, 0 dropped, 0 downgraded. Agent-2 (cli-prior-art, lite) — 3 sampled, 0 dropped, 0 downgraded; its `ESCALATE-TO-DEEP` (which path syntax is least ambiguous) is a design judgement, routed to Phase 4 + Phase 6 rather than a research-deep re-dispatch. No `[[vet_events]]` append: no flow ledger exists in plan mode.

### clap 4.6.7 (installed source `~/.cargo/registry/src/*/clap_builder-4.6.7`)
- Long-flag aliases share the `long_flags` list with globals after `_propagate_global_args` (`builder/command.rs:4428`), so an alias equal to a global long name (`--get`, `--select`, `--limit`, `--lines`, `--template`, `--quiet`, `--error-format`) panics `detect_duplicate_flags` (`builder/debug_asserts.rs:93-94`); the existing `the_command_tree_has_no_duplicate_flags` test catches it. Subcommand aliases only dedupe against subcommand names (`debug_asserts.rs:359-366`). (high)
- `#[command(alias)]` parses but is hidden from help (`visible_alias` shows `[aliases: …]`); `#[value(alias)]` on a ValueEnum variant parses but never appears in help or the "possible values" error (`possible_value.rs`, help uses `get_name()`), so value aliases must be documented in prose. (high)
- Positional + `--id`: one arg cannot be both (`arg.rs:4431`); use a second `--id` Vec arg (`value_delimiter=','`, no `num_args`), and swap the positional's `required = true` for `required_unless_present = "id"` — `required` + `required_unless*` panics (`debug_asserts.rs:201-204`, verified). `items sweep`/`items clusters` already spell it `--ids` (`tomlctl/src/cli/types.rs:1985, 2021`). (high)
- `--set KEY=VALUE`: `Vec<(String,String)>` with a `fn(&str)->Result<(String,String),_>` splitting on the first `=` (`split_once`); no `value_delimiter`, so commas survive; Vec defaults to Append (`examples/typed-derive/fn_parser.rs`). Non-UTF-8 rejected before the parser. Newline survival through Windows argv: medium, unprobed. (high)
- `suggestions` feature = `dep:strsim` + `error-context` (clap_builder `Cargo.toml:76`); jaro ≥0.7 "did you mean" for subcommands, flags, enum values, added as `tip:` lines. strsim 0.11.1 latest (2024-04-02), MIT, zero deps, no OSV advisories, already locked in `glimpse/Cargo.lock` and `lumina/Cargo.lock`. `tomlctl/Cargo.toml:28-31` records leaving it off deliberately — a reversal needs the user. (high)
- Without the feature: `Cli::try_parse()`, on `ErrorKind::InvalidSubcommand` read `e.get(ContextKind::InvalidSubcommand)` and `e.insert(ContextKind::SuggestedSubcommand, …)` (`error/mod.rs:193,202`, verified) → renders "tip: a similar subcommand exists: 'parse'". Top level only (nested positional levels yield `UnknownArgument`). (high)
- Searched: installed clap_builder/clap_derive/clap_lex 4.6.7 sources; deps.dev cargo/strsim; OSV strsim 0.11.1; Scorecard (404). Fetched 2026-10-09.

### CLI prior art
- Field flags: `gh api -f k=v` always string, `-F` typed (`true/false/null/int`, `@file`, `@-`), `k[]=` arrays — free text under `-F` gets coerced (https://cli.github.com/manual/gh_api). HTTPie 3.2.4: separator decides type — `=` string, `:=` raw JSON, `=@` text file, `:=@` JSON file; values never coerced (https://httpie.io/docs/cli/request-items). jo guesses types → escaping traps. → string-by-default, explicit typed/file forms. (high)
- PowerShell 7.6: an unquoted token starting `@` vanishes (splat of undefined var) — re-probed here: `cmd /c echo A @foo B` prints `A  B`; `--json=@foo` survives. Unquoted `{path}` becomes a scriptblock and is mangled. → docs must show `--json '@path'`/`--json=@path` and `--template '{path}'` quoted (about_Parsing 7.6). (high, probed)
- Git Bash 5.3 non-interactive: `!`, `[*]`, `?`, `*` pass literally unless a cwd file matches the glob; PowerShell passes `deps[*].ref`, `a,b` literally. Show patterns quoted. (high, probed by agent)
- JMESPath: `[*]` projects later steps, dropping nulls; `[?a!='x']` filters; `contains(array, v)` array membership (https://jmespath.org/specification.html). nushell projects implicitly on list-of-records, numeric segments work (https://www.nushell.sh/book/navigating_structured_data.html). kubectl `[*]`, `?(@.x=="y")`. (high)
- Truncation: gh `truncate N` — ellipsis counted inside the limit, only when width ≥5 (go-gh `pkg/text/text.go`). (high)
- Agent-facing CLIs: actionable errors naming the correct form, truncation that tells the agent how to get more, a queryable schema/describe command, `--fields` masks, raw `--json` input (Anthropic "Writing tools for agents" 2025-09-11, high; Poehnelt 2026-03-04 and Algolia 2026-05-07, medium; clig.dev "if you can guess what they meant, suggest it").
- Searched: gh manual (gh_api, gh_help_formatting), go-gh text.go, httpie request-items, jo.md, jmespath spec, kubectl jsonpath, nushell book, about_Parsing 7.6, clig.dev, Anthropic, Poehnelt, Algolia; fetched 2026-10-09.
