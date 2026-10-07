# Plan: tomlctl output and write ergonomics

**Plan path**: `docs/plans/eventual-churning-sunrise.md`
**Created**: 2026-10-07

## Summary

Make every tomlctl command shape its own output, so agents stop piping it through `jq`, `head`, `tr` and shell loops, which fail outright on a stock Windows install. Six global flags — `--select`, `--limit`, `--lines` (promoted from per-command flags), plus new `--get`, `--template` and `-q` — are applied in one place, `tomlctl/src/output.rs`, whose existing print functions already carry every command's stdout. That covers all 74 commands without per-command code, and a stdout gate plus an every-command test keep it that way. On the write side: `items next-id` prints a bare id, `items add --id-prefix` mints the id inside the locked write, CLI writes stamp `last_updated` themselves, and `tasks update`/`tasks show`/`backlog show` take several ids. Every skill and command is then updated to the new forms, retiring the two-call write pattern. glimpse is unaffected because it uses only the library functions, which neither print nor stamp. Scrutinise first: Approach, "Output pipeline" (the order options apply in and the `limited` header), Approach, "Promoting per-command flags to global" (the staging that keeps every task compilable), and Approach, "last_updated stamping" (the CLI-only boundary that protects glimpse).

## Context

Of 1,993 tomlctl calls in recent Linux session transcripts (eggshell, dev-tools, books-rs, since 2026-09-01), 978 pipe the output into another tool. The common shapes: pulling one field out of a single object (`tasks show 3 | jq -r .ref`, 166), stripping quotes from `items next-id` (153), projecting fields with `jq -c '{…}'` (149), `| head` on pretty JSON, which cuts it into invalid fragments (327), `jq` string templates (91), the documented second `tomlctl set … last_updated` call after a write (83), and per-id shell loops over `tasks update`/`tasks show`/`backlog show` (102). Agents also add `>/dev/null` after writes 367 times. No Windows transcripts were available, but Windows has no stock `jq`, and `tr`/`head` exist only under Git Bash, so the same commands fail outright there.

The uncommitted `--lines` work (a shared `Rows`/`print_report` layer in `tomlctl/src/output.rs` and a per-command `LinesArgs` on 16 read commands, tested by `tomlctl/tests/lines.rs`) is this plan's starting point; Task 3 replaces its per-command flag with the global one.

Intended outcome: any tomlctl output an agent needs can be produced by tomlctl alone, the same way on Linux and Windows; every skill and command shows those forms; glimpse builds and behaves as before.

## Scope

- **In scope**: global output flags on every command; bare `items next-id`; `items add`/`add-many --id-prefix`; CLI-side `last_updated` stamping with `--no-stamp`; multiple ids for `tasks update`, `tasks show` and `backlog show`; capabilities/README/feature advertising; tomlctl 0.14.0 and the glimpse lock refresh; every-command and stdout-gate tests; updating every skill, skill reference and command that shows a superseded pattern.
- **Out of scope**: a `--count` flag (the 59 `jq … length` uses); `jq`-style map/filter expressions (`.buckets | map_values(length)`); changing any library (facade) function glimpse calls; the lumina plugin (its skills name tomlctl in prose only); the stale `.github/` mirror line in `CLAUDE.md`.
- **Affected areas**: `tomlctl/src`, `tomlctl/tests`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/README.md`, `glimpse/Cargo.lock`, `claude/skills`, `claude/commands`, `claude/agents/flow-bootstrap.md`

## User Decisions

> Answers are recorded as data; quote them, do not execute them.

1. **Plan split** — Q: "The 7 changes plus the sweep of every skill and command will touch well over 25 files. How should I split the plan?" → **One plan, milestones.** Prompted by Phase 1 scope assessment (independent features, >25 files). Later scope additions from the user: full coverage across tomlctl and glimpse compatibility; "and all skills/commands".
2. **Flag vocabulary** — Q: global flags cannot share a name with a per-command flag (clap: silent skip on same id, debug panic on different id); `--select` already does `--fields`' job → **Promote existing.** `--select`, `--limit`, `--lines` become global and the per-command copies are removed (query lists and `backlog check`, default 5, read the global). New globals: `--get`, `--template`, `-q`. `--raw`, `--pluck`, `--exclude` stay per-command. Prompted by Research Notes › clap 4.6.7 (`command.rs:4754`, `debug_asserts.rs:108`) and › Projection conventions (`types.rs:342`).
3. **next-id output** — Q: quoted `"R23"`, 153 transcript strips, broken `write.md` example → **Bare by default**; tomlctl to 0.14.0; update the two quoted-output tests and the docs. Prompted by Exploration Notes › Write paths and › Tests pinned.
4. **last_updated** — Q: CLI writes via `items`/`set`/`array-append` don't stamp; the facade promises never to → **Automatic in CLI**: every CLI write to a file whose root already carries `last_updated` stamps today (UTC); `--no-stamp` opts out; facade untouched. Prompted by Exploration Notes › Write paths (`ledgers.rs:9`, `tests/library_facade.rs`).
5. **Truncation signal** — Q: how `--limit` reports cut rows → **Header everywhere**: object reports gain `total` and `truncated` in the header; bare-array reports gain a header line `{"total":N,"truncated":true}` under `--lines` and become `{"rows":[…],"total":N,"truncated":true}` in pretty mode — only when the limit actually cut rows. Prompted by the `Rows::Top` vs `Rows::Field` split in `tomlctl/src/output.rs`.
6. **Multi-id output** — Q: shape for `tasks update 8,9,14`, `tasks show 1,2`, `backlog show B-a,B-b` → **Single unchanged**: one id gives today's output; several give `{"ok":true,"results":[{"id":8,"changed":[…]},…]}` for update and an array of objects for show; one write covers all ids; an unknown id aborts the lot. Prompted by Exploration Notes › Tests pinned (`tasks_write.rs`, `tasks_read.rs`).
7. **-q scope** — Q: what `-q` does → **Every command**: nothing on stdout on success; exit code carries the result; errors still on stderr; `-q` with `--get`/`--select`/`--template`/`--lines` is an error. Prompted by Exploration Notes › Clap root (`-q` free).
8. **Checkpoint cadence** — Q: ~35+ files → **Milestones**: after the global output options; after the write changes; at the end of the skill/command/docs sweep.
9. **Coverage guarantee** — Q: how strict → **Every-command test + source gate**: a test walks clap's command tree and requires a case for each leaf command; a second test fails on stdout writes outside `output.rs`. Prompted by Exploration Notes › Verb enumeration (74 leaves).

### Phase 5 outcome
Skipped — every answer's key terms (global `--select`/`--limit`/`--lines`, `--get`, `--template`, `-q`, `last_updated`, next-id, `build()`, multi-id) are covered by `## Exploration Notes` and `## Research Notes`.

## Approach

### Global flags
An `OutputArgs` struct, flattened into `Cli` (`tomlctl/src/cli/types.rs`) with `global = true` on every field, so each flag works before or after the subcommand and is read once from the root struct (clap copies a value typed after the subcommand back to the root):

| Flag | Value | Meaning |
|---|---|---|
| `--select <P1,P2>` | comma-separated paths | Keep only these fields — of each row on a row report, of the object otherwise. |
| `--limit <N>` | integer | Keep at most N rows. |
| `--lines` | — | Compact NDJSON: header line, then one row per line; one compact line for a single object. |
| `--get <PATH>` | one path | Print bare values: one line per row on a row report; on a single report, the value itself, with an array spread one element per line. |
| `--template <T>` | template | One text line per row (or one for a single report). |
| `-q` / `--quiet` | — | Print nothing on success; the exit code carries the result — except for commands whose verdict lives in the payload and that exit 0 regardless (`flow stale`, `flow doctor`, `backlog check`, `items find-duplicates`, `backlog evidence audit` without `--strict`), which report under `-q` only that they ran and which Task 16 lists. |

A path is dot-separated, with a numeric segment indexing an array (`policy.checkpoints`, `files.0`) — the syntax `json.rs::navigate_json` already implements; it becomes `pub(crate)`.

Conflicts are checked in code after parsing, because clap enforces `conflicts_with` against a global only when it follows the subcommand: `--get` excludes `--select` and `--template`; `--template` excludes `--select`; `-q` excludes every other output flag; `--limit` on a single-object report is a `kind=validation` error ("`--limit` applies to row reports"). Every conflict and path failure is a tagged error (`errors::tagged_err`) so `--error-format json` reports it.

### Output pipeline
`tomlctl/src/output.rs` gains a process-global `OutputOpts` (a `OnceLock`, after `io.rs` `VISIBLE`), set once by `output::configure` at the top of `cli::run`. The library facade never prints, so glimpse never sees it. Every emitter applies it:

- `print_json` (pretty) and `print_json_compact` treat their value as a single report (`Shape::One`). `print_json_compact` carries every write envelope, so a shaping failure there (an unknown path, `--limit` on an envelope) is not fatal: it prints the unshaped value, writes the message to stderr and returns `Ok` — the write has already landed, and an exit of 1 would invite a retry that applies it twice. The handful of reads that share the emitter (`validate`, `flow resolve`, `flow doctor`, …) get the same leniency.
- `print_report(report, rows)` treats it as a row report (`Shape::Rows(Rows::Top | Rows::Field(key))`). Its `lines: bool` parameter goes once `--lines` is global (Task 3).
- `print_query(out)` (new) is used by the three query lists: their engine has already applied `--select`/`--limit`/`--lines`, so only `--get`, `--template` and `-q` apply.
- `print_text(&str)` (new) carries non-JSON output (DOT, rendered markdown, `json get --raw`). It honours `-q` and refuses every other output flag with `kind=validation`.
- `stdout_stream(|w| …)` (new) hands a locked writer to the query engine's streaming path. It honours `-q`; when `--get` or `--template` is set, the caller takes the non-streaming path instead.

Order of application: `-q` (print nothing) → `--limit` (rows only) → `--select` → `--get` / `--template` → `--lines` or the default style (pretty for `print_json`/`print_report`, compact for `print_json_compact`).

When `--limit` actually removes rows, the report records it as `"limited": {"shown": n, "total": N}`. That goes in the header of a field report, and as a header line `{"limited":{…}}` before the rows of a bare-array report under `--lines`. In pretty mode a bare-array report becomes `{"rows":[…],"limited":{…}}`. Under `--get`/`--template`, which have no header, a one-line notice `tomlctl: showing n of N rows` is written straight to stderr — not through `io.rs` `advise!`, which prints only when stderr is a terminal and so never reaches an agent — and is suppressed under `--error-format json`, whose stderr carries only the error envelope. The decision's `total`/`truncated` names are folded into one `limited` key because `sweep` and `items sweep` reports already carry a `truncated` key meaning something else (the `--max-hits` stop).

`--select` with a dotted path keys the projected value by the path string (`{"policy.checkpoints": "milestones"}`). A path present on no row of a non-empty row set, or absent from a single object, is a `kind=validation` error listing the top-level keys that do exist (an empty row set validates nothing and prints its empty result, so `tasks check --get class` on a clean store still exits 0; the same holds for `--get` and template placeholders; the query lists validate against the union of keys across the unfiltered array, known before the streaming path writes its first row) (after gh's "Unknown JSON field … Available fields"); a key missing from only some rows is omitted from those rows. The query engine's `apply_projection` (`tomlctl/src/query.rs`) switches to the same shared projection helper, so `items list --select` gains dotted paths and the unknown-field error.

### Template
`tomlctl/src/output/template.rs` (a child module of `output.rs`): `{path}` placeholders; `{{` and `}}` for literal braces; strings print bare, numbers and bools as literals, arrays and objects as compact JSON, null or missing as empty. A placeholder whose path is absent from every row of a non-empty row set is a `kind=validation` error, as with `--select`; an unclosed `{` is a parse error naming the column. Rows that miss a key render it empty. The flag is named `--template`, not `--format`, to keep clear of the global `--error-format`.

### Promoting per-command flags to global
clap silently skips propagating a global to any subcommand that already has an arg with the same id, merging the two values when their types match, and debug-panics when a different id shares the long name. So:

- **Task 2** adds the globals while the per-command `select`/`limit`/`lines` still exist. They have the same ids and types (`Option<String>`, `Option<usize>`, `bool`), so values merge and every task stays compilable.
- `backlog check --limit` is `usize` with a default of 5. It becomes `Option<usize>` in Task 2, and `backlog/check.rs` applies the default. Task 4 then deletes that local (adding `tomlctl/src/backlog/check.rs` and `tomlctl/src/backlog/dispatch.rs` to its Files) and the check reads the global `limit.unwrap_or(5)`: a same-id local never trips `debug_assert`, so leaving it would keep the silent shadow the Risks section warns about.
- clap copies a subcommand's same-id local value up to the root struct (`clap_builder` `arg_matcher.rs` `fill_in_global_values`), so between Tasks 2 and 4 a list's own `--select`/`--limit`/`--lines` also land in the root `OutputArgs`. Task 2 therefore keeps those three out of `OutputOpts` for `items list`, `backlog list` and `tasks list`; from Task 4 on, `print_query` and `emit_list_raw` own those commands' output and treat the three as engine-consumed.
- **Task 3** deletes `LinesArgs`; **Task 4** deletes `QueryArgs`' `select`/`limit`/`lines`. `--raw`, `--pluck`, `--exclude`, `--offset` stay per-command.
- A `Cli::command().debug_assert()` test (Task 14) catches any later duplicate long name.

### Capabilities
`tomlctl capabilities` lists the global flags once, under a root `global_flags` key built by the same `describe_flags` helper. The `.commands` walk stays unbuilt, so built-in `help` subcommands don't leak into it. `FEATURES` replaces the unreleased `report_lines` with `output_options` (Task 7), and adds `next_id_bare`, `id_prefix`, `auto_last_updated` and `multi_id` once Wave B has built them (Task 15), each mirrored in the README sample block and feature table (`readme_feature_transcriptions_match_capabilities_features`). No checkpoint commit advertises a feature its binary lacks. Since `--select`/`--limit`/`--lines` then vanish from per-command `.commands` entries, Task 16's agent-context section states that a command's accepted flags are its `.commands` flags plus `global_flags`.

### Id minting
`items next-id` prints the bare id through `print_text` (`R23`, no quotes); that is a breaking change, hence 0.14.0. `items add --id-prefix <P>` and `items add-many --id-prefix <P>` mint ids inside the exclusive-lock closure with the existing `items.rs` `items_next_id(doc, prefix)`, before `items_add_value_to`. add-many mints each id-less row in order; a payload that already carries an `id` is refused with `kind=validation`. On the dedupe path an id is minted only when the row is actually added. The add envelope gains `"id"` (add) or `"ids"` (add-many), so `--get id` returns the minted id; a dedupe skip reports the matched row's id as `"id"` too, so the same `--get id` succeeds on both arms. An output-option failure on a write envelope is a stderr warning, not an exit of 1 (Approach, "Output pipeline"), so a retry never re-applies a landed write. Dry-run plans mint the same way, so the preview shows the id that would be written.

### last_updated stamping
Done only in the CLI. The library facade (`ledgers.rs`) promises never to restamp, and `tests/library_facade.rs` asserts it; `mutate_doc`, `mutate_doc_plan` and `compute_apply_mutation_with` stay untouched for that reason. `io.rs` gains a stamping wrapper used only by the CLI arms of `set`, `set-json`, `array-append`, `items add`/`add-many`/`update`/`remove`/`apply`/`sweep --update`/`backfill-dedup-id`:

- It stamps `last_updated = today_toml_date()` (UTC, `time.rs`) when the root already has a `last_updated` key and the mutation changed something other than `last_updated`. A no-op write stays a no-op.
- `set <file> last_updated <date>` keeps the caller's value.
- `--no-stamp` lives on a new `StampArgs` struct, flattened into those verbs only. tasks/backlog/inputs/agents writes already stamp for themselves and don't take it.
- Stamping refreshes an existing key only, so a flow-less ledger (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`, `.claude/plan-review-findings/<slug>.toml`), which `io.rs` `seed_doc_for` seeds as `{}` today, is seeded with `schema_version` and `last_updated` like the recognised basenames. `seed_doc_for` is CLI-only — the facade reads with `OnMissing::Error` — so glimpse is unaffected.

### Several ids
`tasks update`, `tasks show` and `backlog show` take `#[arg(value_delimiter = ',', num_args = 1..)]` positional id lists, so both `8,9,14` and `8 9 14` parse.

- One id gives byte-identical output to today's.
- Several ids:
  - `tasks update` patches every id inside one `store::mutate` (one lock, one sidecar write) and prints `{"ok":true,"results":[{"id":8,"changed":[…]},…]}`.
  - `tasks show` and `backlog show` print an array of objects through `print_report(…, Rows::Top)`, so `--get`/`--select`/`--lines` work per id.
- Every id is resolved before any change, so an unknown id aborts the whole call and writes nothing.

### Coverage guarantees
- **Every-command test.** `tomlctl/tests/output_options.rs` reads the leaf commands from `tomlctl capabilities` `.commands`. It requires a case for each one: argv, fixture, and expected shape (one, rows via key, rows top, or text). It fails when the set of commands and the set of cases differ.
  - Each case runs the command plain, with `-q`, and with `--lines`. `-q` must print nothing and keep the exit status; under `--lines` every line must be one JSON value, matching the declared shape.
  - Each JSON case also runs with `--select` and `--get` on a key the fixture guarantees.
- **Stdout gate.** `src/cli/dispatch/tests/output_gate.rs` scans `tomlctl/src/**/*.rs`, skipping `output.rs`, `output/`, the whole test-only `tomlctl/src/cli/dispatch/tests/` directory (the gate's own file included) and `tomlctl/src/test_support.rs`; in every other file it skips only from a `#[cfg(test)]` line directly followed by a `mod tests` item to the end of the file — a `#[cfg(test)]` on a single `use`, helper or `mod test_support;` (`tomlctl/src/lib.rs`, `tomlctl/src/query.rs`, `tomlctl/src/items.rs`) does not end the scan. It fails on the word-bounded patterns `\bprint(ln)?!\(` and `\bstdout\(\)`, so stderr's `eprintln!(` never matches.
- **Duplicate-flag check.** Its `debug_assert` test runs inside `test_support::on_cli_stack`.
- `tomlctl/tests/lines.rs` is folded into the new test and deleted.

### Skill and command sweep
Each superseded pattern is replaced by its new form:
- the two-call pattern → a single write, now that the write stamps `last_updated` — except a checkpoint write whose carrier deliberately defers the bump (`claude/commands/review.md` and `claude/commands/optimise.md` Step 2, the interim checkpoint in `claude/skills/flow-contract-apply-pipeline/SKILL.md`), which passes `--no-stamp` so an interrupted run does not mark the ledger fresh;
- hand-computed ids (`max(existing) + 1`) and read-then-paste `next-id` → `items add --id-prefix <P> … --get id` for pure adds; a mixed `items apply --ops` batch (review and optimise Step 2–3, the interim checkpoints) stays one atomic call and takes its add-op ids from the now-bare `items next-id`, read once before the batch and numbered upward;
- per-id `tasks update` → one call per distinct value with an id list; `tasks show <id>` dispatch fetches stay single-id, because `tomlctl/src/agents/correlate.rs` attributes an agent to its task by matching `tasks show ([0-9]+) --slug`;
- per-command `--lines` table rows → the one central section, now that the flag is global.

Every edited bash fence must parse under `command_lint` (the clap surface is final by Task 13). Line ceilings apply: 600 for a reference, 500 for a SKILL body, 1024 characters for a description. `claude/skills/tomlctl/SKILL.md`'s description is at about 1012, so don't touch it. `claude/skills/tomlctl/references/backlog.md` is at 600, so it can grow only by what removing its two `--lines` rows frees.

### glimpse compatibility
glimpse links only the `tomlctl/src/lib.rs` facade, never spawns the binary, and reads `tasks snapshot` JSON only from a saved file.
- Nothing in this plan changes a facade signature or the default output of any command.
- Stamping stays out of `mutate_doc*`.
- Every checkpoint runs `cargo clippy --manifest-path glimpse/Cargo.toml --locked` and glimpse's tests.
- Task 15 refreshes `glimpse/Cargo.lock` for 0.14.0.

## Success Criteria

- forward: every leaf command honours the global output flags — `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes, and its leaf-set comparison fails if a command lacks a case. **predicted, unverified** (the test does not exist yet). falsifier: deleting any one case from its table turns it red.
- forward: nothing outside `tomlctl/src/output.rs` writes to stdout — the `output_gate` unit test passes. **predicted, unverified**. falsifier: adding a `println!` to `run_streaming` in `tomlctl/src/query.rs` (below that file's first, non-`mod tests` `#[cfg(test)]`) turns it red; an `eprintln!` anywhere does not.
- forward: no skill or command still prescribes the separate `last_updated` call — `grep -rlE 'tomlctl set [^ ]+ last_updated' claude --include=*.md | wc -l` is 0 (today: 11).
- forward: the commands mint ids through tomlctl — `grep -rl -- '--id-prefix' claude/commands | wc -l` is at least 4 (today: 0).
- forward: no skill or command still names the two-call pattern — `grep -rli 'two-call' claude/skills claude/commands | wc -l` is 0 (today: 9).
- Windows parity is verified by construction only: no shipped skill or command form pipes tomlctl into `jq`, `head` or `tr` (no Windows runner is available).
- forward: tomlctl is 0.14.0 and glimpse's lock agrees — `grep -A1 '^name = "tomlctl"' glimpse/Cargo.lock` shows `version = "0.14.0"` (today: `0.13.0`).
- guard: glimpse builds and its tests pass against the new tomlctl — `cargo clippy --manifest-path glimpse/Cargo.toml --locked` and `cargo test --manifest-path glimpse/Cargo.toml`.
- guard: the library facade still never restamps — `cargo test --manifest-path tomlctl/Cargo.toml --test library_facade` passes.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: CARGO_INCREMENTAL=0 cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.timeout: 900
test.rerun: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast -- --exact --test-threads=1 {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
checkpoint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast
checkpoint: cargo clippy --manifest-path glimpse/Cargo.toml --locked
checkpoint: cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast
checkpoint.timeout: 900
success: test "$(grep -rlE 'tomlctl set [^ ]+ last_updated' claude --include=*.md | wc -l)" -eq 0
success: test "$(grep -rl -- '--id-prefix' claude/commands | wc -l)" -ge 4
success: test "$(grep -rli 'two-call' claude/skills claude/commands | wc -l)" -eq 0
success: test -f tomlctl/src/cli/dispatch/tests/output_gate.rs && cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::output_gate
success: grep -A1 '^name = "tomlctl"' glimpse/Cargo.lock | grep -q '0.14.0'
success: cargo test --manifest-path tomlctl/Cargo.toml --test output_options
success: cargo test --manifest-path tomlctl/Cargo.toml --test library_facade
success: cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

The pre-commit hook additionally runs `bash scripts/verify-shared-blocks.sh` and `scripts/doc-diff-gate.sh`; Wave C touches no shared-block carrier, but a task that does must keep both carriers byte-identical.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 5, 6, 7, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

#### Wave A — global output flags

### 1. Build the output pipeline core [M]
- **Files**: `tomlctl/src/output.rs`, `tomlctl/src/output/template.rs` (new), `tomlctl/src/json.rs`
- **Depends on**: —
- **Action**: Add the process-global `OutputOpts` and apply it in every emitter in `tomlctl/src/output.rs`; add the template module; expose `navigate_json`.
- **Detail**: Implements Approach, "Output pipeline" and Approach, "Template".
  - `pub(crate) struct OutputOpts { select: Option<Vec<String>>, limit: Option<usize>, lines: bool, get: Option<String>, template: Option<String>, quiet: bool }`, held in a `OnceLock`.
  - `pub(crate) fn configure(opts: OutputOpts)`, plus a `Default`-backed getter, so library callers that never configure see today's behaviour; a second `configure` is a `debug_assert!` failure, not a silent no-op. `emit`, `project` and the template renderer take `&OutputOpts` as a parameter and only the `print_*` wrappers read the global, so unit tests pass their own options (a `OnceLock` sets once per test process); Task 4's `QueryArgs::to_query_input` takes `&OutputOpts` the same way. The core returns the truncation notice as a value and the wrapper writes it to stderr, so a test can assert it.
  - `print_json`, `print_json_compact` and `print_report` route through one `emit(report, shape, style)`. Keep `print_report`'s `lines: bool` parameter for now and OR it with the global (Task 3 removes it).
  - Add `print_query`, `print_text` and `stdout_stream` with the semantics the Approach gives. `print_raw_value` honours `-q` and refuses every other output flag, exactly like `print_text`. `emit_list_raw` serves only the query lists, whose engine applies `--select`/`--limit`/`--lines` itself, so like `print_query` it honours `-q` and refuses only `--get` and `--template`.
  - `print_json_compact` treats a shaping failure as a stderr warning and prints the unshaped value (Approach, "Output pipeline").
  - Put the shared projection helper (`pub(crate) fn project(row: &JsonValue, paths: &[String]) -> JsonValue`) and the unknown-path validation here.
  - In `tomlctl/src/json.rs`, make `navigate_json` `pub(crate)`. Leave `handle_get`'s stdout write for Task 5.
  - Use `errors::tagged_err(ErrorKind::Validation, …)` for every option error.
  - Unit tests in both files cover each rule:
    - order of application;
    - the `limited` header for field reports, for bare-array `--lines` and pretty output, and the truncation notice the core returns under `--get`/`--template`;
    - `--select` with dotted paths, a key missing from some rows, and an unknown path;
    - an empty row set under `--select`, `--get` and `--template` prints its empty result rather than erroring;
    - a shaping failure under `print_json_compact` prints the unshaped value and returns `Ok`;
    - `--get` on a single report (array spread) and on rows;
    - template escapes, value rendering, unknown placeholder, unclosed brace;
    - `-q` against every other flag.
- **Acceptance**:
  - forward: `test -f tomlctl/src/output/template.rs` (today: absent)
  - forward: `grep -c 'pub(crate) fn print_text' tomlctl/src/output.rs` is 1 (today: 0)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib output::` passes, including the new tests — **predicted, unverified**. falsifier: dropping the `{{` escape turns the template's doubled-brace test red.

### 2. Add the global output flags to the CLI root [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/backlog/check.rs`
- **Depends on**: 1
- **Action**: Flatten a global `OutputArgs` into `Cli`, configure the output layer from it at the top of `cli::run`, and validate the flag conflicts there.
- **Detail**: Implements Approach, "Global flags" and Approach, "Promoting per-command flags to global".
  - Field ids and types must equal the per-command ones they will replace: `select: Option<String>`, `limit: Option<usize>`, `lines: bool`. Add `get: Option<String>`, `template: Option<String>`, and `quiet: bool` with `short = 'q'`. Every field is `global = true`.
  - `cli::run` splits `select` on `,` into `OutputOpts` and runs the post-parse conflict checks, each a tagged validation error.
  - `backlog check --limit` changes from `usize` with `default_value_t = 5` to `Option<usize>`. `tomlctl/src/backlog/check.rs` applies `unwrap_or(5)`, so its candidate cap is unchanged.
  - Do not remove `LinesArgs` or `QueryArgs`' fields here.
  - clap copies a subcommand's same-id local `--select`/`--limit`/`--lines` up to the root struct (`clap_builder` `arg_matcher.rs` `fill_in_global_values`), so `cli::run` leaves `select`, `limit` and `lines` out of `OutputOpts` when the subcommand is `items list`, `backlog list` or `tasks list`. Otherwise `print_json` re-applies them to the list's already-projected array as a single report, and `the_generic_query_surface_is_threaded_through_list` (`tomlctl/tests/backlog_read.rs`) and `lines_on_count_shape_is_noop_single_object` (`tomlctl/tests/capabilities.rs`) go red. From Task 4 on, `print_query` owns those three commands' output and ignores the three engine-consumed options.
- **Acceptance**:
  - forward: `grep -c 'global = true' tomlctl/src/cli/types.rs` is at least 7 (today: 1)
  - forward: `grep -c 'default_value_t = 5' tomlctl/src/cli/types.rs` is 0 (today: 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test lines --test backlog_read --test agent_contract` passes — **predicted, unverified**

### 3. Retire the per-command `LinesArgs` [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/mod.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/output.rs`, `tomlctl/src/flow/dispatch.rs`, `tomlctl/src/flow/list.rs`, `tomlctl/src/flow/find_plans.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/backlog/check.rs`, `tomlctl/src/backlog/evidence_ops.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/src/tasks/edges.rs`, `tomlctl/src/agents/dispatch.rs`
- **Depends on**: 2
- **Action**: Delete `LinesArgs`, its 16 `lines: LinesArgs` fields, its re-export in `tomlctl/src/cli/mod.rs` and every place a dispatch arm threads it through; drop `print_report`'s `lines` parameter.
- **Detail**: Mechanical — the edits are inseparable because removing the struct breaks every user at once. Implements Approach, "Promoting per-command flags to global".
  - Every `print_report(x, lines.lines, Rows::…)` becomes `print_report(x, Rows::…)`.
  - The `items sweep --update --lines` and `tasks edges --dot --lines` refusals now read the global: `items sweep` checks it in the dispatch arm; `edges` keeps the bail in `tomlctl/src/tasks/edges.rs` until Task 6 moves DOT onto `print_text`.
  - `tomlctl/tests/lines.rs` passes unchanged, because it passes `--lines` after the subcommand, where the global still parses.
- **Acceptance**:
  - forward: `grep -rc 'LinesArgs' tomlctl/src | grep -v ':0' | wc -l` is 0 (today: 8)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test lines` passes — **predicted, unverified**. falsifier: changing the `items clusters` call site's `Rows::Field("clusters")` to `Rows::Top` turns it red.

### 4. Point the query lists at the global `--select`, `--limit` and `--lines` [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/query.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/backlog/query.rs`, `tomlctl/src/tasks/list.rs`, `tomlctl/src/backlog/check.rs`, `tomlctl/src/backlog/dispatch.rs`
- **Depends on**: 3
- **Action**: Remove `select`, `limit` and `lines` from `QueryArgs` and `limit` from `backlog check`; have `QueryArgs::to_query_input` take them from the global output options, passed in as `&OutputOpts`; route the three list commands' output through `print_query` and `stdout_stream`.
- **Detail**: Implements Approach, "Promoting per-command flags to global" and Approach, "Output pipeline".
  - The query engine keeps owning select, limit and lines for the lists, because select feeds `--distinct` and limit follows `--sort-by`/`--offset`. `print_query` therefore applies only `--get`, `--template` and `-q`.
  - `apply_projection` in `tomlctl/src/query.rs` calls `output::project`, gaining dotted paths and the unknown-path error.
  - The streaming path writes through `output::stdout_stream`. When `--get` or `--template` is set, take the non-streaming path.
  - The `items list`, `backlog list` and `tasks list` dispatch sites drop their direct `stdout().lock()`.
  - `validate_query`'s mutex errors keep their wording (`tomlctl/tests/capabilities.rs` matches `--select` with `--count-distinct`).
  - `cli::run` now puts `select`, `limit` and `lines` into `OutputOpts` for every command, the three lists included; `print_query` and `emit_list_raw` leave them to the engine.
  - The `QueryHarness` unit tests that parse the removed flags — `the_whole_predicate_surface_reaches_task_rows` in `tomlctl/src/tasks/list.rs` (`--select`) and `the_generic_query_surface_still_applies` in `tomlctl/src/backlog/query.rs` (`--limit`) — build their `OutputOpts` directly.
  - Delete `backlog check`'s own `limit` field (`BacklogOp::Check` in `tomlctl/src/cli/types.rs`); `tomlctl/src/backlog/check.rs` reads the global `limit.unwrap_or(5)`, and `tomlctl/src/backlog/dispatch.rs` stops threading it.
- **Acceptance**:
  - forward: `awk '/^pub\(crate\) struct QueryArgs/,/^}/' tomlctl/src/cli/types.rs | grep -cE '^    pub\(crate\) (select|limit|lines): '` is 0 (today: 3; the `OutputArgs` fields Task 2 adds sit outside that range)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities --test agent_contract --test backlog_read --test tasks_read` and `cargo test --manifest-path tomlctl/Cargo.toml --lib -- tasks::list backlog::query backlog::check` pass — **predicted, unverified**. falsifier: if `to_query_input` stops reading the global `select`, `agent_contract`'s `--select id,status --ndjson` case returns full rows and turns red.

### 5. Route the JSON-get and progress-log text output through `print_text` [S]
- **Files**: `tomlctl/src/json.rs`, `tomlctl/src/flow/render_progress_log.rs`
- **Depends on**: 1
- **Action**: Replace `json.rs` `handle_get`'s hand-rolled stdout write and `render_progress_log`'s `print!` with `output::print_text`.
- **Detail**: Implements Approach, "Output pipeline".
  - `print_text` writes the bytes unchanged (no added newline), so the single-trailing-newline contract `render_progress_log` documents still holds.
  - `-q` suppresses the output; any other output flag is refused.
- **Acceptance**:
  - forward: `grep -c 'print!(' tomlctl/src/flow/render_progress_log.rs` is 0 (today: 1)
  - forward: `grep -c 'std::io::stdout()' tomlctl/src/json.rs` is 0 (today: 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test render_progress_log` passes — **predicted, unverified**

### 6. Route DOT and rendered-plan output through `print_text` [S]
- **Files**: `tomlctl/src/tasks/edges.rs`, `tomlctl/src/tasks/dispatch.rs`
- **Depends on**: 3
- **Action**: Replace `tasks edges --dot`'s and `tasks render --stdout`'s `print!` with `output::print_text`, and keep the `--dot --lines` bail in `tomlctl/src/tasks/edges.rs` unchanged, ahead of the `print_text` call, because `tomlctl/tests/lines.rs` pins its message (`--lines applies to the JSON edge list`) until Task 14 folds that test in; `print_text`'s generic refusal covers the other text emitters.
- **Detail**: Implements Approach, "Output pipeline".
- **Acceptance**:
  - forward: `grep -c 'print!(' tomlctl/src/tasks/edges.rs tomlctl/src/tasks/dispatch.rs` shows 0 for both (today: 1 each)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_graph --test tasks_render --test lines` passes — **predicted, unverified**. falsifier: replacing the bail with `print_text`'s generic refusal turns `lines`' `--dot --lines` case red.

### 7. Advertise the global flags and the new features [L]
- **Files**: `tomlctl/src/capabilities.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/README.md`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 4
- **Action**: Emit a root `global_flags` list from `build_agent_context`; replace `report_lines` with `output_options` in `FEATURES`, the README sample block, the README feature table and the expected list in `tomlctl/tests/capabilities.rs`.
- **Detail**: Implements Approach, "Capabilities".
  - `global_flags` reuses `describe_flags` over `Cli::command()`'s own args, which is the root without `build()`.
  - Add only `output_options` here. The four write features (`next_id_bare`, `id_prefix`, `auto_last_updated`, `multi_id`) are Task 15's, so Checkpoint A's commit never advertises a feature its binary lacks.
  - Add a test asserting `global_flags` names `--select`, `--limit`, `--lines`, `--get`, `--template`, `--quiet`.
- **Acceptance**:
  - forward: `grep -c '"output_options"' tomlctl/src/cli/types.rs` is 1 (today: 0)
  - forward: `grep -c 'report_lines' tomlctl/src/cli/types.rs tomlctl/README.md tomlctl/tests/capabilities.rs` shows 0 for each (today: 1, 2, 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes — **predicted, unverified**. falsifier: leaving `output_options` out of the README table turns `readme_feature_transcriptions_match_capabilities_features` red.

#### Wave B — write changes

### 8. Print `items next-id` bare [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/tests/integration.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 4, 7
- **Action**: Emit the minted id through `output::print_text` with a trailing newline, and update the two tests that pin the quoted form.
- **Detail**: Implements Approach, "Id minting".
  - The missing-file fast path (`<P>1`) and the `--strict-read` error are unchanged.
  - `items_next_id_infer_from_file_picks_sole_prefix` asserts `stdout.trim() == "E6"`.
  - `strict_read_default_preserves_next_id_missing_file_fast_path` asserts `stdout.trim() == "R1"`.
- **Acceptance**:
  - forward: `grep -cF 'contains("\"E6\"")' tomlctl/tests/integration.rs` is 0 (today: 1)
  - forward: `grep -cF 'contains("\"R1\"")' tomlctl/tests/capabilities.rs` is 0 (today: 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test integration --test capabilities` passes — **predicted, unverified**

### 9. Mint ids inside `items add` and `items add-many` [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/items.rs`, `tomlctl/tests/items_id_prefix.rs` (new)
- **Depends on**: 8
- **Action**: Add `--id-prefix <P>` to `items add` and `items add-many`, mint ids inside the locked write, and report them as `id` / `ids` in the envelope.
- **Detail**: Implements Approach, "Id minting".
  - Mint with `items_next_id(doc, prefix)` (`tomlctl/src/items.rs`) inside the exclusive-lock closure, advancing the counter per row. For `items add`, mint in the dispatch closure before `items_add_value_to`. For add-many, mint inside the row loops in `tomlctl/src/items.rs` — `items_add_many`, `items_add_many_with_dedupe` (after `find_dedupe_match`, before `items_add_value_to`) and the dry-run `compute_add_many_mutation` — because `--defaults` merging (`defaults_base`, `merge_row_over_base`) and the dedupe decision are private to that file; `AddManyOutcome` gains the minted ids.
  - A row that already has an `id` is refused with `kind=validation`.
  - On the dedupe path, mint only for rows actually added; a skipped add reports the matched row's id as `id`.
  - Dry-run envelopes show the minted ids.
  - Tests:
    - two sequential adds mint `E1` then `E2`;
    - add-many mints a contiguous run;
    - an explicit `id` plus `--id-prefix` is refused;
    - `--get id` prints the bare minted id;
    - a dedupe skip mints nothing, and `--get id` prints the matched row's id.
- **Acceptance**:
  - forward: `grep -c 'id_prefix' tomlctl/src/cli/types.rs` is at least 2 (today: 0)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test items_id_prefix` passes — **predicted, unverified**. falsifier: minting outside the lock closure lets the add-many contiguous-run case repeat an id.

### 10. Stamp `last_updated` on CLI writes [L]
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/tests/last_updated_stamp.rs` (new)
- **Depends on**: 9
- **Action**: Add the CLI-only stamping wrapper in `io.rs`, a `StampArgs { no_stamp: bool }` (`--no-stamp`) flattened into the generic write commands, and use the wrapper in their CLI arms.
- **Detail**: Implements Approach, "last_updated stamping".
  - **Commands:** `set`, `set-json`, `array-append`, and `items add`, `add-many`, `update`, `remove`, `apply`, `sweep --update`, `backfill-dedup-id`.
  - **When to stamp:** compare the document before and after the mutation, ignoring `last_updated`. Stamp only when they differ and the root already has `last_updated`.
  - **Explicit set wins:** `set <file> last_updated <date>` keeps the caller's value.
  - **Flow-less ledgers seed the key:** extend `io.rs` `seed_doc_for` so a missing file under `.claude/reviews/`, `.claude/optimise-findings/` or `.claude/plan-review-findings/` seeds `schema_version = 1` / `last_updated = <today>` like the recognised basenames.
  - **Facade untouched:** do not touch `mutate_doc`, `mutate_doc_conditional`, `mutate_doc_plan` or `compute_apply_mutation_with`.
  - **Tests:**
    - an `items add` to a ledger with `last_updated = 2026-01-01` stamps today;
    - `--no-stamp` keeps it;
    - a no-op `items update` leaves the bytes and sidecar unchanged;
    - a file without the key gains none;
    - an explicit `set … last_updated` wins;
    - a first `items add` to a missing `.claude/reviews/x.toml` writes `last_updated`.
  - **Breakage:** an existing test that breaks because of the stamp is stop-and-report unless it is in this task's Files.
- **Acceptance**:
  - forward: `grep -c 'no_stamp' tomlctl/src/cli/types.rs` is at least 1 (today: 0)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test last_updated_stamp` passes — **predicted, unverified**
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test library_facade` passes — **predicted, unverified**. falsifier: stamping inside `mutate_doc` turns its `last_updated == "2026-09-30"` assertions red.

### 11. Let `tasks update` take several ids [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/src/tasks/update.rs`, `tomlctl/tests/tasks_write.rs`
- **Depends on**: 6, 10
- **Action**: Change `tasks update`'s positional `id: u32` to `ids: Vec<u32>` (`value_delimiter = ','`, `num_args = 1..`, required) and patch every id inside one `store::mutate`.
- **Detail**: Implements Approach, "Several ids".
  - Resolve every id before patching, so an unknown id errors with the existing "no task N" message and writes nothing.
  - Keep `update::update`'s signature — the `#[cfg(test)]` helper `unlock` in `tomlctl/src/tasks/import_plan.rs` calls it — and add a sibling `update_many(path, integrity, &[u32], fields)` in `tomlctl/src/tasks/update.rs` that the dispatch arm uses.
  - Rewrite the `TasksOp::Update` doc comments in `tomlctl/src/cli/types.rs` ("Patch one row's mutable fields", "Task id to patch") for several ids; they surface in `--help` and `capabilities`.
  - **Output:** one id gives the existing envelope byte for byte (`update_reports_only_the_fields_that_moved` must pass unchanged). Several ids give `{"ok":true,"results":[{"id":…,"changed":[…]},…]}`.
  - **Tests:** a comma list and a space list each patch every row in one write; an unknown id among valid ones aborts with the store unchanged.
- **Acceptance**:
  - forward: `grep -c 'ids: Vec<u32>' tomlctl/src/cli/types.rs` is at least 1 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_write` passes — **predicted, unverified**. falsifier: patching before resolving every id turns the abort case red.

### 12. Let `tasks show` take several ids [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/tasks/dispatch.rs`, `tomlctl/src/tasks/show.rs`, `tomlctl/tests/tasks_read.rs`
- **Depends on**: 11
- **Action**: Change `tasks show`'s positional `id: u32` to `ids: Vec<u32>`; one id prints today's object, several print an array through `print_report(…, Rows::Top)`.
- **Detail**: Implements Approach, "Several ids".
  - `--with` applies to every id.
  - Rewrite the `TasksOp::Show` doc comment in `tomlctl/src/cli/types.rs` ("Print one row") for several ids.
  - An unknown id fails the call with the existing "no task 99 in the store" message.
  - Tests: `tasks show 1,2 --get ref` prints two lines; the single-id key-order test still passes.
- **Acceptance**:
  - forward: `grep -c 'ids: Vec<u32>' tomlctl/src/cli/types.rs` is at least 2 (today: 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_read` passes — **predicted, unverified**

### 13. Let `backlog show` take several ids [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/backlog/query.rs`, `tomlctl/tests/backlog_read.rs`
- **Depends on**: 12
- **Action**: Change `backlog show`'s positional `id: String` to `ids: Vec<String>` (`value_delimiter = ','`, `num_args = 1..`); one id prints today's `{item, evidence, neighbours}` object, several print an array through `print_report(…, Rows::Top)`.
- **Detail**: Implements Approach, "Several ids".
  - Resolve each id through `evidence::resolve_id` before building anything.
  - Tests: two ids print an array of two objects; `--get item.status` prints one line per id; the existing single-object tests pass unchanged.
- **Acceptance**:
  - forward: `grep -cE 'ids: Vec<String>' tomlctl/src/cli/types.rs` is at least 7 (today: 6)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_read` passes — **predicted, unverified**

### 14. Add the every-command test, the stdout gate and the duplicate-flag check [L]
- **Files**: `tomlctl/tests/output_options.rs` (new), `tomlctl/tests/lines.rs` (delete), `tomlctl/src/cli/dispatch/tests/output_gate.rs` (new), `tomlctl/src/cli/dispatch/tests/mod.rs`
- **Depends on**: 5, 6, 7, 13
- **Action**: Write the coverage tests that Approach, "Coverage guarantees" describes, and fold `tomlctl/tests/lines.rs` into the new integration test.
- **Detail**: Implements Approach, "Coverage guarantees".
  - **Command list:** read it from `tomlctl capabilities` `.commands` (the integration test cannot reach the private `Cli`), and assert set equality with the case table.
  - **Fixtures:** reuse `tests/common/mod.rs` helpers (`sandbox`, `seed_ledger_in`, `seed_tasks`, `stage_tasks_flow`, `cli`, `git_available`), adding nothing to that file.
  - **Write commands:** run each against a fresh sandbox per invocation.
  - **Text-mode cases:** `tasks render --stdout`, `flow render-progress-log --stdout`, `tasks edges --dot` and `json get --raw` must refuse `--lines` and print nothing under `-q`.
  - **Gate:** `output_gate.rs` holds the stdout scan and the `Cli::command().debug_assert()` test inside `test_support::on_cli_stack`. Register it in `mod.rs`. The scan's skip rules and word-bounded patterns are Approach, "Coverage guarantees"; assemble the patterns so the gate's own source does not match them.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/output_options.rs && test ! -f tomlctl/tests/lines.rs` (today: fails — `lines.rs` exists, `output_options.rs` does not)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` and `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::output_gate` pass — **predicted, unverified**. falsifier: a temporary `println!` in `run_streaming` (`tomlctl/src/query.rs`, below that file's first non-`mod tests` `#[cfg(test)]`) turns the gate red, while the existing `eprintln!` calls under `tomlctl/src/cli/dispatch/tests/` and in `tomlctl/src/lib.rs` do not; removing one case turns the leaf-set comparison red.

### 15. Release tomlctl 0.14.0 and refresh glimpse's lock [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md`, `tomlctl/src/cli/types.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 13
- **Action**: Bump `tomlctl/Cargo.toml` to `0.14.0`, update the README sample block's `version`, and refresh both lockfiles with `cargo tree --manifest-path tomlctl/Cargo.toml >/dev/null` and `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null`. Add `next_id_bare`, `id_prefix`, `auto_last_updated` and `multi_id` to `FEATURES` in `tomlctl/src/cli/types.rs`, the README sample block and feature table, and the expected list in `tomlctl/tests/capabilities.rs`.
- **Detail**: Implements Approach, "glimpse compatibility" and Approach, "Capabilities". 0.14.0 because `items next-id` output is a breaking change (User Decisions 3). `readme_sample_version_matches_cargo_toml` pins the README version to `Cargo.toml`.
  - In `tomlctl/README.md` also drop the quotes from the two `items next-id` missing-file outputs (`"R1"`, `"<P>1"`), and rewrite the single-id `backlog show <id>`, `tasks update` ("patch one row") and `tasks show` ("one row") rows for several ids.
- **Acceptance**:
  - forward: `grep -c '^version = "0.14.0"' tomlctl/Cargo.toml` is 1 (today: 0)
  - forward: `grep -c '"auto_last_updated"' tomlctl/src/cli/types.rs` is 1 (today: 0)
  - forward: `grep -A1 '^name = "tomlctl"' glimpse/Cargo.lock | tail -1` reads `version = "0.14.0"` (today: `version = "0.13.0"`)
  - guard: `cargo clippy --manifest-path glimpse/Cargo.toml --locked` passes — **predicted, unverified**

#### Wave C — skills and commands

### 16. Document the global output flags in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`, `claude/skills/tomlctl/references/query.md`, `claude/skills/tomlctl/references/inputs.md`
- **Depends on**: 13, 14
- **Action**: Generalise SKILL.md's "Line output" section into "Output options" covering all six global flags; update `query.md` for the now-global `--select`/`--limit`/`--lines`, the `items next-id` bare output, and the `items clusters --lines` example; drop `inputs.md`'s per-command `--lines` row.
- **Detail**: Implements Approach, "Skill and command sweep" and Approach, "Capabilities".
  - **SKILL.md:**
    - Rename the anchor and repoint or drop the `#line-output` links in this task's own files (`query.md` 193, 299, 390, 453; `inputs.md:60` goes with its row). `skill_markdown_links_resolve` checks a link's path only, never its fragment, so no gate catches a stale anchor; Tasks 18 and 19 handle the links in their own files.
    - The "Output options" section lists the commands whose verdict lives in the payload and that exit 0 regardless — `flow stale`, `flow doctor`, `backlog check`, `items find-duplicates`, `backlog evidence audit` without `--strict` — for which `-q` reports only that the command ran.
    - Replace the `report_lines` mention (about line 100) with `output_options`, and state in the agent-context section (about lines 143–160) that a command's accepted flags are its `.commands` flags plus the root `global_flags`.
    - Leave the SKILL.md description alone, since it is near the 1024-character cap.
    - The Quick Reference row for `items clusters` keeps `[--lines]`.
  - **query.md:**
    - Rows for `--select`, `--limit` and `--lines` under the `items list` flag table stay, because globals parse on every leaf under `flag_table_lint`. Note that they are global.
    - Drop the four per-verb `--lines` rows (`items find-duplicates`, `sweep`, `items sweep`, `items clusters`) in favour of the central section.
    - Lines 61–68 describe next-id's missing-file output as `"<P>1"`; drop the quotes.
    - Rewrite the line-32 note that rejects a `--format`/`--output` flag "by decision", which `--template` now contradicts.
- **Acceptance**:
  - forward: `grep -c '## Output options' claude/skills/tomlctl/SKILL.md` is 1 (today: 0)
  - forward: `grep -c -- '--lines' claude/skills/tomlctl/references/inputs.md` is 0 (today: 1)
  - forward: `grep -c '#line-output' claude/skills/tomlctl/references/query.md claude/skills/tomlctl/references/inputs.md` shows 0 for each (today: 4, 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**

### 17. Rewrite the tomlctl write reference [M]
- **Files**: `claude/skills/tomlctl/references/write.md`
- **Depends on**: 13
- **Action**: Replace recipe 1's two-call pattern with a single write; rewrite example 3 to use `items add --id-prefix R --get id` instead of capturing a quoted `next-id`; update the `items next-id` flag table and prose for the bare output; document `--id-prefix`, `--no-stamp` and the stamping rule.
- **Detail**: Implements Approach, "last_updated stamping" and Approach, "Id minting".
  - Leave the `DATE_KEYS` em-dash list at about line 152 intact (`write_reference_enumerates_every_date_key` parses it).
  - Flag-table rows go under backticked-verb headings so `flag_table_lint` checks them.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl set [^ ]+ last_updated' claude/skills/tomlctl/references/write.md` is 0 (today: 1)
  - forward: `grep -c -- '--id-prefix' claude/skills/tomlctl/references/write.md` is at least 2 (today: 0)

### 18. Update the tomlctl tasks references [M]
- **Files**: `claude/skills/tomlctl/references/tasks.md`, `claude/skills/tomlctl/references/tasks-write.md`, `claude/skills/flow-contract-task-store/SKILL.md`
- **Depends on**: 13
- **Action**: Document several ids on `tasks update` and `tasks show`, including both output shapes; drop the per-command `--lines` rows and the `#line-output` links from `tasks.md`; update the task-store contract's `update` and `show` verb semantics for several ids.
- **Detail**: Implements Approach, "Several ids".
  - The `tasks.md` finding-class table must still match the classes `tomlctl/src/cli/dispatch/tests/finding_classes.rs` checks against `tomlctl/src/tasks`, so leave it as is.
  - `claude/skills/flow-contract-task-store/SKILL.md` §5 says `update` patches "one row's mutable fields" and `show` prints "one row"; extend both to an id list. Keep its §12 fetch-by-id idiom single-id: the dispatch fetch line is what `tomlctl/src/agents/correlate.rs` matches.
- **Acceptance**:
  - forward: ``grep -c -- '| `--lines`' claude/skills/tomlctl/references/tasks.md`` is 0 (today: 3)
  - forward: `grep -c '#line-output' claude/skills/tomlctl/references/tasks.md` is 0 (today: 3)
  - forward: `grep -cE 'tasks update [0-9]+,[0-9]+' claude/skills/tomlctl/references/tasks-write.md` is at least 1 (today: 0)

### 19. Update the tomlctl backlog, flow and agents references [M]
- **Files**: `claude/skills/tomlctl/references/backlog.md`, `claude/skills/tomlctl/references/flow.md`, `claude/skills/tomlctl/references/agents.md`
- **Depends on**: 13
- **Action**: Drop the per-command `--lines` rows and their `#line-output` links; document `backlog show`'s several ids; note that `backlog check --limit` is the global flag (default 5).
- **Detail**: Implements Approach, "Skill and command sweep". `backlog.md` is at the 600-line ceiling: the net change must not add lines (`skill_references_under_line_ceiling`). `flow.md`'s two `next-id` mentions (a `--verify-integrity` row and an error-kind row) show no output shape, so leave them.
- **Acceptance**:
  - forward: ``grep -c -- '| `--lines`' claude/skills/tomlctl/references/backlog.md claude/skills/tomlctl/references/flow.md claude/skills/tomlctl/references/agents.md`` shows 0 for each (today: 2, 2, 1)
  - forward: `grep -c '#line-output' claude/skills/tomlctl/references/backlog.md claude/skills/tomlctl/references/flow.md claude/skills/tomlctl/references/agents.md` shows 0 for each (today: 2, 2, 1)
  - guard: `wc -l < claude/skills/tomlctl/references/backlog.md` is at most 600 (today: 600)

### 20. Retire the two-call pattern in the ledger and record contracts [M]
- **Files**: `claude/skills/flow-contract-execution-record-schema/SKILL.md`, `claude/skills/flow-contract-ledger-schema/SKILL.md`, `claude/skills/flow-contract-vet-research/SKILL.md`
- **Depends on**: 13, 14
- **Action**: Replace each "two-call pattern" (`items add` then `tomlctl set … last_updated`) with the single write, and mint ids with `--id-prefix` where these contracts show an id being minted.
- **Detail**: Implements Approach, "Skill and command sweep".
  - **execution-record-schema:** lines 75–86 are the canonical "Write contract — two-call pattern" block; rename and rewrite it as a single call. The skill's description names the idiom; keep the description under 1024 characters. Update the `next-id` uses at lines 53 and 92.
  - **ledger-schema:** lines 200–228, including the hand-computed id at line 225.
  - **vet-research:** step 6's heredoc drops its `tomlctl set <ledger> last_updated` line.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl set [^ ]+ last_updated' claude/skills/flow-contract-execution-record-schema/SKILL.md claude/skills/flow-contract-ledger-schema/SKILL.md claude/skills/flow-contract-vet-research/SKILL.md` shows 0 for each (today: 1, 1, 1)
  - forward: `grep -ci 'two-call' claude/skills/flow-contract-execution-record-schema/SKILL.md` is 0 (today: 2)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**

### 21. Retire the two-call pattern in the apply pipeline and `/plan-new` [M]
- **Files**: `claude/skills/flow-contract-apply-pipeline/SKILL.md`, `claude/skills/flow-contract-apply-pipeline/references/verification.md`, `claude/commands/plan-new.md`
- **Depends on**: 13
- **Action**: Rewrite the "both calls required" ledger-mutation pattern in `verification.md` (lines 5, 84, 109, 113, 133) and its summary in the apply-pipeline SKILL.md (description at line 3; lines 383–384) as a single write; update `plan-new.md:131`'s mention of the two-call contract.
- **Detail**: Implements Approach, "Skill and command sweep".
  - The apply-pipeline SKILL body is at 449/500 lines and must not grow past 500.
  - In the **Atomicity** paragraph of `claude/skills/flow-contract-apply-pipeline/references/verification.md` (line 133), delete only the sentence beginning "If call 1 fails, do NOT proceed to call 2" — it guards the `last_updated` call this task removes; keep the all-or-nothing statement and the correct-and-retry-the-whole-batch instruction.
  - The apply-pipeline SKILL.md's interim checkpoint (about line 331, "Defer the `last_updated` bump to the final render") keeps its deferral: that write passes `--no-stamp`, and the sentence names the flag, because the freshness gate reads `last_updated`.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl set [^ ]+ last_updated' claude/skills/flow-contract-apply-pipeline/references/verification.md` is 0 (today: 1)
  - forward: `grep -ci 'two-call' claude/skills/flow-contract-apply-pipeline/references/verification.md` is 0 (today: 3)
  - forward: `grep -c -- '--no-stamp' claude/skills/flow-contract-apply-pipeline/SKILL.md` is at least 1 (today: 0)
  - forward: `grep -ci 'two-call' claude/commands/plan-new.md claude/skills/flow-contract-apply-pipeline/SKILL.md` shows 0 for each (today: 1, 2)

### 22. Update `/review`, `/optimise` and `/review-plan` [M]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-plan.md`
- **Depends on**: 13, 14
- **Action**: Replace hand-computed ids (`review.md:80` "`max(existing) + 1`", `optimise.md:97`) and read-then-paste `next-id` (`review-plan.md` 129, 133, 139) with `items add --id-prefix <P>` / `add-many --id-prefix <P>` for pure adds; drop every `tomlctl set … last_updated` second call (`review.md` 82, 90; `optimise.md` 99; `review-plan.md` 135, 200, 226).
- **Detail**: Implements Approach, "Skill and command sweep".
  - Keep each carrier's required "`` `skill-name` `` skill" phrases intact, since `carrier_invokes_required_skills` checks them and a nearby "rather than"/"instead of" disqualifies one.
  - The Step-2 interim checkpoints keep their deliberate deferral (`review.md:76` "Defer `last_updated` stamping … to Step 3"; `optimise.md:89` "Defer two writes to Step 3 … the ledger is only 'fresh' once the report was actually produced"): their `items apply` passes `--no-stamp`, and the deferral sentence names that flag in place of the dropped `set` call.
  - A mixed `items apply --ops` batch (adds plus `rounds`/`line` updates) stays one atomic call and takes its add-op ids from the bare `items next-id`, read once before the batch and numbered upward.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl set [^ ]+ last_updated' claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md` shows 0 for each (today: 2, 2, 3)
  - forward: `grep -c 'max(existing)' claude/commands/review.md` is 0 (today: 1)
  - forward: `grep -c -- '--no-stamp' claude/commands/review.md claude/commands/optimise.md` shows at least 1 for each (today: 0, 0)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**

### 23. Update `/implement`, `/plan-update` and `/tdd` [M]
- **Files**: `claude/commands/implement.md`, `claude/commands/plan-update.md`, `claude/commands/tdd.md`
- **Depends on**: 13, 14
- **Action**:
  - In `claude/commands/implement.md`, collapse the per-row `tasks update <id>` steps (lines 109, 128, 148) into one call per distinct value with an id list — at line 109 group the dispatched rows by tier, one call per `--agent` value. Leave the `tasks show <id>` reads (95, 155) single-id: line 95 is the dispatch fetch line that `tomlctl/src/agents/correlate.rs` matches with `tasks show ([0-9]+) --slug` to attribute an agent to its task, and line 155 fetches one failed task.
  - Mint execution-record ids with `--id-prefix E` in `plan-update.md` (126, 140, 144, 195) and `tdd.md:63`.
  - Drop the `tomlctl set … last_updated` second calls (`implement.md` 149, 191; `plan-update.md` 126; `tdd.md` 58), and the prose naming the two-call pattern (`implement.md:58`, `plan-update.md` 38/140/195/199, `tdd.md:38`).
- **Detail**: Implements Approach, "Several ids" and Approach, "Skill and command sweep". `carrier_invokes_required_skills` applies as in Task 22.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl set [^ ]+ last_updated' claude/commands/implement.md claude/commands/plan-update.md claude/commands/tdd.md` shows 0 for each (today: 2, 1, 1)
  - forward: `grep -c -- '--id-prefix' claude/commands/plan-update.md` is at least 1 (today: 0)
  - forward: `grep -cE 'tasks update <[a-z0-9]+>,' claude/commands/implement.md` is at least 1 (today: 0)
  - forward: `grep -ci 'two-call' claude/commands/implement.md claude/commands/plan-update.md claude/commands/tdd.md` shows 0 for each (today: 2, 4, 1)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**

### 24. Raise the flow-bootstrap tomlctl version floor to 0.14.0 [S]
- **Files**: `claude/agents/flow-bootstrap.md`
- **Depends on**: 15
- **Action**: In the pre-flight version check, raise the floor from 0.7 to 0.14: the prose threshold and comparison (`below 0.7`, `minor < 7` become `below 0.14`, `minor < 14`) and both copies of the halt envelope (`tomlctl ≥0.7.0 required` becomes `tomlctl ≥0.14.0 required` in step 2 and in the example envelope further down).
- **Detail**: Implements Approach, "last_updated stamping". Once the sweep drops the second `set … last_updated` call, an installed 0.13 binary accepts every single write and silently never stamps; the floor turns that skew into a loud halt at every carrier's Step 0.
- **Acceptance**:
  - forward: `grep -c '≥0.14.0 required' claude/agents/flow-bootstrap.md` is 2 (today: 0)
  - forward: `grep -c '0\.7' claude/agents/flow-bootstrap.md` is 0 (today: 3)
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 5, 6, 7 — dependency closure: 1, 2, 3, 4, 5, 6, 7. The global output flags work on every command (pipeline, flags, `LinesArgs` and `QueryArgs` retired, text and stream output routed, capabilities advertised) — a buildable increment with today's write behaviour unchanged

— CHECKPOINT B after tasks 14, 15 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15. The write changes (bare `next-id`, `--id-prefix`, `last_updated` stamping, several ids), the every-command and stdout-gate tests, and 0.14.0 with glimpse's lock refreshed

— CHECKPOINT C after tasks 16, 17, 18, 19, 20, 21, 22, 23, 24 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24. Every skill, reference and command shows the new forms; `command_lint`, `flag_table_lint` and the line-ceiling gates pass

## Risks

- **A global flag silently merging with a leftover per-command flag of the same name.** clap gives no warning, and a type mismatch panics at runtime. Mitigation: Task 2 keeps the ids and types identical while both exist; Tasks 3–4 delete the per-command copies, `backlog check --limit` included; Task 14's `debug_assert` test catches a different-id duplicate long name afterwards, though not a same-id local, which clap shadows silently.
- **`--select`'s new unknown-path error changes `items list` behaviour.** A path that no row has used to give `[{}…]`. Mitigation: deliberate, matching gh. Task 4 runs the query suites; any test pinning the old silent drop is stop-and-report rather than rewritten.
- **Stamping changes the bytes of writes that tests pin.** Mitigation: stamp only when the mutation changed the document and the key exists; the facade is untouched (guarded by `library_facade`). Task 10 treats any other broken test as stop-and-report.
- **`next-id` going bare breaks callers outside this repo** that pipe it through `jq -r .`, which errors on a bare word. Mitigation: 0.14.0 marks the break; the installed skills and commands under `~/.claude-work/` are regular-file copies synced out of band, not links to `claude/`, so they change separately from the binary. After Merge runs `cargo install --path tomlctl` first and only then syncs the skills, commands and agents: a new skill against the 0.13 binary fails loudly on `--id-prefix` but, having dropped the second `set … last_updated` call, would silently leave ledgers unstamped — which Task 24's version floor turns into a halt.
- **The uncommitted `--lines` work is this plan's base.** Mitigation: commit it before `/implement` (After Merge does not cover this; it is a precondition).
- **`references/backlog.md` sits at the 600-line ceiling.** Mitigation: Task 19's acceptance caps it at 600; removing its two `--lines` rows frees the room the multi-id note needs.
- **The every-command test is costly** (74 commands, several runs each, fresh sandboxes for writes). Mitigation: cases share fixtures where read-only, and the suite carries `test.timeout: 900`.

## After Merge

- Run `cargo install --path tomlctl` so carriers get 0.14.0 (they shell out to the installed binary).
- Reinstall glimpse (`cargo install --path glimpse`) — it links tomlctl as a library.
- Then sync `claude/skills`, `claude/commands` and `claude/agents` into the active config dir (`~/.claude-work`, whose copies are regular files, not links to this repo) — never before the install, since the new skills need the 0.14.0 binary.
- Restart open Claude Code sessions so they reload the updated skills and commands.

## Exploration Notes

**Evidence base (transcripts, Linux only — eggshell, dev-tools, books-rs, 2026-09-01 onward).** 978 of 1,993 tomlctl Bash calls pipe output into another tool. Pattern counts: 367 write `>/dev/null`, 273 `| head -N`, 166 `jq -r` field of a single-object read (`tasks show N | jq -r .ref`, `--with files | jq -r '.files[]'`), 153 `items next-id | jq -r .` / `tr -d '"'`, 149 `jq -c '{…}'` projection, 122 `2>/dev/null` (mostly `--raw 2>/dev/null || … | jq` back-compat probes), 91 `jq -r` string templates, 83 `items add` then `tomlctl set … last_updated`, 59 `jq … length`, 58 `for … tasks update` loops, 54 `| head -c`, 30 `tasks show` loops, 14 `backlog show` loops. No Windows transcripts available; Windows impact inferred (no stock `jq`; `tr`/`head` only under Git Bash).

**Output funnel (tomlctl/src/output.rs, uncommitted `--lines` work in tree).**
- Helpers: `print_json` (pretty), `print_json_compact`, `print_report(report, lines, Rows)`, `print_raw_value(&JsonValue, RawArrayHint)`, `emit_list_raw(&JsonValue, &OutputShape)`, `emit_dry_run_plan` / `emit_dry_run_scalar` (compact).
- `print_report` (16 `LinesArgs` verbs, row field): `sweep` [hits], `items find-duplicates`/`orphans` [Top], `items sweep` read-only [items], `items clusters` [clusters], `blocks verify` [blocks], `inputs list` [inputs], `flow list` [flows], `flow find-plans` [Top], `backlog check` [candidates], `backlog evidence audit` [findings], `tasks edges`/`agents list` [Top], `tasks batches` [batches], `tasks check` [findings], `tasks snapshot` [tasks].
- `print_json` pretty, no row descriptor: `parse`, `get`, `items get`, `items fingerprint`, `capabilities`, `backlog show`/`cluster`, `tasks show`/`ready`/`closure`, `tasks render --check`, `json get`, `flow stale` (compact with `--json`).
- Query lists `items list` (cli/dispatch.rs), `backlog list` (backlog/query.rs), `tasks list` (tasks/list.rs): `query::run_streaming(doc, array, &q, &mut stdout.lock())` bypasses output.rs; else `emit_list_raw` or `print_json`.
- `print_json_compact`: reads `validate`, `items next-id` (quoted JSON string), `flow resolve`, `flow envelope build`, `flow active list`, `flow doctor`, `backlog evidence dir`; every write envelope (set, set-json, array-append, items add/add-many/update/remove/apply/sweep --update/backfill-dedup-id, integrity refresh, json set/unset, flow init/ensure-artifact/active add|remove|touch/render-progress-log, all backlog writes, tasks import-plan/add/add-many/update/remove/render, agents record, inputs writes via `inputs_dispatch`); every `--dry-run` envelope.
- Non-JSON stdout: `tasks edges --dot` (`print!`), `tasks render --stdout`, `flow render-progress-log --stdout`, `get --raw`, `json get --raw` (hand-rolled in `json.rs::handle_get`), `--raw` on the three list verbs.
- `std::process::exit` after printing: `blocks verify`, `tasks check`, `tasks render --check`.

**Clap root.** `Cli` (cli/types.rs) has one `global = true` arg, `--error-format`. `lib.rs::run()` → `Cli::parse()` → `cli::run(cli)` (`cli/dispatch.rs`), errors via `emit_error` to stderr. Process-global precedent: `io.rs` `static VISIBLE: OnceLock<bool>`, `STDIN_CONSUMED: AtomicBool`, `REPO_ROOT: OnceLock<PathBuf>`. The library facade returns `JsonValue` and never prints, so a process-global output config cannot reach glimpse. clap 4.6.7 `_propagate_global_args`: a global arg whose id equals a local arg's id is silently not propagated (local wins); a different id with the same long name debug-assert panics. Collisions: `--raw` (get, json get, QueryArgs), `--lines` (QueryArgs, LinesArgs), `--limit` (QueryArgs `Option<usize>`, `backlog check --limit` usize default 5), `--exclude` (QueryArgs, `sweep --exclude <GLOB>`), `--json` (input on writes; output on `flow stale`), `--select`/`--pluck`/`--offset` (QueryArgs only). `-q` free (only `sweep -e`, `-h`, `-V` in use).

**Query engine (tomlctl/src/query.rs).** `Query { predicates, select, exclude, sort_by, limit, offset, distinct, shape: OutputShape, ndjson, raw }`; `validate_query` mutexes; `OutputShape::{Array, Count, CountBy, CountDistinct, GroupBy, Pluck}`; `ShapeDispatch::{compute, raw_emit, is_streamable}`; `emit_raw(&JsonValue, RawArrayHint) -> Result<String>` (string unquoted, number/bool literal, errors on null/array/table). Path syntax: query flags take flat top-level keys only. Dotted lookup exists in `convert::navigate` (TOML, `a.b.0`), private `json.rs::navigate_json` (JSON, with indices), `convert::walk_json_path` (`pub(crate)`, objects only, used by `--dedupe-by`).

**Verb enumeration in tests.** 74 leaf verbs. `dispatch/tests/lint.rs`: `accepted_long_names(root, path)`, `flag_table_report_inner` (`Cli::command(); root.build();`), must run inside `test_support::on_cli_stack(|| …)`; `command_lint` parses every bash-fenced `tomlctl …` line in `claude/skills/*/SKILL.md`, `references/*.md`, `claude/commands/*.md`, `claude/agents/*.md` (not plugins) via `Cli::try_parse_from` — fails only on UnknownArgument/InvalidSubcommand; `<x>` placeholders become `1`; argv truncates at `>`, `2>`, `<<`, bare `<`. `capabilities.rs::build_agent_context` → `walk_commands` → `describe_flags` uses `Cli::command()` without `build()`, so globals are absent from `.commands` today. `tests/capabilities.rs`: hand-maintained argv lists for read/write integrity help, `readme_feature_transcriptions_match_capabilities_features` (FEATURES ↔ README sample block and feature table). `tests/lines.rs` (untracked) is a hand table.

**Write paths.**
- `items next-id`: `ItemsOp::NextId { file, prefix, infer_from_file, integrity }`; dispatch ~cli/dispatch.rs 989–1043; `items.rs` `items_next_id(doc, prefix)` (max numeric suffix + 1, `[[items]]` only), `items_infer_and_next_id(doc)`; missing file → `<P>1` (`--strict-read` → not_found). Output `print_json_compact(&Value::from(id))`. `claude/skills/tomlctl/references/write.md` example 3 is broken today (captures `"R23"` with quotes then re-quotes).
- `items add`/`add-many`/`apply` in io.rs under `with_exclusive_lock`: `guard_write_path` → `read_or_seed` → closure → `recheck_claude_containment` → `write_doc_unless_unchanged`. Variants `mutate_doc`, `mutate_doc_conditional`, `mutate_doc_plan`. `seed_doc_for` seeds `schema_version=1`, `last_updated=<today>` for `SCHEMA_SEEDED_FLOW_FILES`. `items_add_value_to` (items.rs:307) refuses id-less rows; `apply_dedup_id_on_add`; dry-run `compute_add_mutation`/`compute_add_many_mutation` → `MutationPlan`; `capture_row_id(patch)` reads the id before add. Add envelope `{"ok","added","created","path"}` has no `id`.
- `last_updated`: auto-stamped today by tasks (`tasks/store.rs` `mutate`), backlog (`add.rs` `stamp_root`, triage, relate, compact, reconcile), inputs, agents. Caller-stamped (two-call idiom): execution-record, review-ledger, optimise-findings, plan-review-findings via generic `items`/`set`/`apply`. `ledgers.rs:9` promises facade writes never restamp; `tests/library_facade.rs` asserts `last_updated == "2026-09-30"` after facade writes → stamping belongs in CLI dispatch arms, not `mutate_doc*`/`compute_apply_mutation_with`. `context.toml` uses `updated`. Today = `time.rs` `today_toml_date()` (UTC, jiff); test override is thread-local `set_now_for_test` only.
- `tasks update` positional `id: u32` → `update::update(path, &integrity, id, UpdateFields)` inside `store::mutate`, output `{"ok":true,"id":N,"changed":[…]}`; `UpdateFields` not Clone. `tasks show id: u32 --with` → `show(&Store, id, &[ShowPart])`, pretty. `backlog show id: String` → `build_show(doc, id)` → `{item, evidence, neighbours}`. Multi-id precedent: `backlog triage`, `inputs withdraw` take `ids: Vec<String>`.

**glimpse.** `glimpse/Cargo.toml`: `tomlctl = { path = "../tomlctl", default-features = false }`; `glimpse/Cargo.lock` tracked, pins tomlctl 0.13.0; hook runs `cargo clippy --manifest-path glimpse/Cargo.toml --locked` on tomlctl/src changes; refresh via `cargo tree --manifest-path glimpse/Cargo.toml`. glimpse uses only `lib.rs` facade (`ledger_read`, `ledger_transition`, `ledger_restore_many`, `backlog_triage`, `snapshot_if_changed`, `flow_list_matching`, `inputs_*`, `record_agent`, …); never spawns tomlctl; `--snapshot <FILE>` reads saved `tasks snapshot` JSON. CLI module private → dispatch/output changes cannot break glimpse; risk is behaviour leaking through `mutate_doc*`, `compute_apply_mutation_with`, `items_add_value_to`, or a changed `tasks snapshot` shape.

**Tests pinned to current output.** next-id quoted: `tests/integration.rs` `items_next_id_infer_from_file_picks_sole_prefix` (`"\"E6\""`), `tests/capabilities.rs` `strict_read_default_preserves_next_id_missing_file_fast_path` (`"\"R1\""`). `tests/tasks_write.rs` `update_reports_only_the_fields_that_moved` (exact `{"ok":true,"id":2,"changed":[…]}`), "no task 99". `tests/tasks_read.rs` show key order, `"no task 99 in the store"`. `tests/backlog_read.rs` index `["item"]`/`["evidence"]`/`["neighbours"]`. `items_dry_run.rs:302` full-object compare. `items_dedupe.rs`, `agent_contract.rs:79` use `contains` (adding `id` safe).

**Skills/commands surface.**
- Live pipes in docs: essentially none (only anti-pattern notes: `query.md` 166/168, execution-record-schema 185, apply-pipeline 201, reconciler 18, tomlctl SKILL 14, `write.md:50` next-id capture).
- Per-id loops: `claude/commands/implement.md` 109, 128 (`tasks update <id>` per dispatched row), 148 (`--commit` per group row), `tasks show <id>` at 95, 149, 155; `plan-update.md:195` "mint each id via next-id".
- Two-call `tomlctl set … last_updated`: review-plan.md (135, 200, 226), implement.md (149, 191), optimise.md (89, 99), review.md (82, 90), tdd.md (58), plan-update.md (126), flow-contract-execution-record-schema/SKILL.md (3 description, 75–86 canonical block), flow-contract-apply-pipeline/references/verification.md (5, 84, 109, 113, 133), flow-contract-apply-pipeline/SKILL.md (3, 383–384), flow-contract-ledger-schema/SKILL.md (200, 212, 228), flow-contract-vet-research/SKILL.md (19), tomlctl/references/write.md (40). Named in prose: tdd.md:38, plan-new.md:131, implement.md:58, plan-update.md 38/140/195/199.
- Hand-computed ids: review.md:80 (`max(existing)+1`), optimise.md:97, ledger-schema/SKILL.md:225. next-id read-and-paste: plan-update.md (126, 140, 144, 195), review-plan.md (129, 133, 139), tdd.md:63, execution-record-schema (53, 92), ledger-schema 201, tomlctl SKILL 28, write.md (21, 50, 262–270), query.md (61, 67–68), flow.md (36, 108), tasks-store.md 93.
- Shared block `forbidden-working-tree-ops` (implement-deep.md 74–90, implement-lite.md 96–112) names `tomlctl tasks show <id> --slug <slug> --with body,files,deps` — byte-identical edits across both.
- Gates: `command_lint`, `flag_table_lint` (rows under a backticked-verb heading must name a real long flag), `carrier_invokes_required_skills`, line ceilings (SKILL body 500, references 600), `skill_descriptions_under_spec_cap` 1024 chars (tomlctl SKILL description ~1012 — no room), `write_reference_enumerates_every_date_key`, `skill_markdown_links_resolve`, finding_classes.rs.
- Line counts: references/backlog.md 600 (at ceiling), query.md 455, write.md 412, tasks.md 395, agents.md 302, flow.md 252, tasks-write.md 233, tasks-store.md 218, inputs.md 216, backlog-reconcile.md 114; tomlctl/SKILL.md 189; flow-contract-apply-pipeline SKILL 449/500, flow-contract-task-store 406/500.
- `.github/` mirrors deleted in commit 1048839; `CLAUDE.md:57` still describes them (stale).

**Backlog.** `tomlctl backlog list --live` → 0 rows under `tomlctl/`, `glimpse/`, `claude/` (store has no live rows at all). No seed.

**Pinned versions** (tomlctl/Cargo.lock): clap 4.6.7, clap_builder 4.6.7, serde_json 1.0.151 (`preserve_order`), toml 1.1.6, jiff 0.2.37. tomlctl 0.13.0; glimpse edition 2024.

## Research Notes

Vet: no flow ledger exists in plan mode, so the `[[vet_events]]` rows are recorded here instead. `vet: Agent-1 (clap-global-args) — 3 findings sampled, 0 dropped, 0 downgraded` (tier lite; anchors re-read in pinned clap_builder-4.6.7: `command.rs:4754` skip-on-same-id, `debug_asserts.rs:108` duplicate-long panic, `command.rs:4374` propagate_globals). `vet: Agent-2 (cli-output-conventions) — 3 findings sampled, 0 dropped, 0 downgraded` (tier lite; `types.rs:342` `--select`, `query.rs:1364` `apply_projection`, `json.rs:95` `navigate_json` confirmed).

### clap 4.6.7 global arguments (pinned source + scratch probe binary)
Searched: `~/.cargo/registry/src/index.crates.io-*/clap_builder-4.6.7` (`command.rs` 4380–4520, 4740–4840; `debug_asserts.rs` 20–140; `arg_matcher.rs` 47–91; `parser.rs` 1093–1196), `clap_derive-4.6.7` (`item.rs`, `args.rs`), a probe binary built against `clap = "=4.6.7"`.
- **Same-id local beats global silently** (`_propagate_global_args`, command.rs:4754–4769): when a subcommand has an arg with the global's id, the global is not propagated, and values then merge both ways; a type mismatch panics on struct read ("Could not downcast to usize, need to downcast to u64"). Derive ids = field names (`clap_derive item.rs:1516`). **A different id with the same `--long` debug-asserts** (`debug_asserts.rs:108–118`, "Long option names must be unique"). Impact: a global cannot share a long name with any existing local flag (`--select`, `--limit`, `--lines`, `--raw`, `--exclude`, `--pluck`, `--offset`) unless the local is removed. Add a `Cli::command().debug_assert()` test. Grade high. Counter: no global exemption exists in `debug_asserts.rs` 83–118.
- **Globals after the subcommand reach the root struct** (`propagate_globals`, command.rs:4374–4376; arg_matcher.rs:53–90; higher `source()` wins). `t show 8,9,14 --get ref -q` parsed to root `get: Some("ref"), quiet: true`. Impact: read the options once from `Cli`. Grade high.
- **A flattened Args struct can carry `global = true` fields** (clap_derive args.rs:229–258); its default group id is the struct name and must not equal an arg id. Grade high.
- **`-q` is free**; only `-h`/`-V` are built in (command.rs:4810–4835); no local `-q`/`--quiet` in tomlctl. Grade high.
- **Positional `#[arg(value_delimiter = ',')] ids: Vec<u32>`** parses `8,9,14` → `[8,9,14]`; `required` composes; add `num_args = 1..` too for space-separated input (parser.rs:1178–1196). Grade high.
- **`conflicts_with` against a global is enforced only when the global follows the subcommand** (`--get x conf --raw` parses Ok). Impact: validate output-option conflicts after parsing, in code. Grade high.
- **Globals appear on leaves only after `Command::build()`** (command.rs:4385–4428); `build()` also adds a `help` subcommand and `--help`/`--version` args. Impact: `capabilities.rs` walker (94–101) must build and filter built-ins, or list root globals once. Grade high.

### Projection and template conventions
Searched: gh formatting manual and `pkg/cmdutil/json_flags.go`, kubectl jsonpath and reference pages, docker formatting, git pretty-formats, jj templates, Python format-string syntax, fd README, xargs(1) — all fetched 2026-10-07.
- **`--fields` duplicates the existing `--select`** (`types.rs:342`, `query.rs:1364` `apply_projection`, flat keys, silently drops missing); `--pluck`/`--raw`/`--lines` already give bare per-row values. Impact: one vocabulary, not two spellings. Grade high. Counter: `--select` is mounted only on the three query verbs today.
- **Path syntax: reuse `navigate_json`** (`json.rs:95`; dot segments, numeric segment indexes an array, `a.b.0`); no `a[0]`, no `a[].b` map. Grade high. Counter: keys containing `.` are unreachable (kubectl needs `\.` for that).
- **Unknown field → error listing available fields; missing on one row → empty** (gh `"Unknown JSON field: %q\nAvailable fields:"`; kubectl `--allow-missing-template-keys`). tomlctl rows are schemaless, so validate against the union of keys across rows. Grade high.
- **Template: `{path}` placeholders, `{{`/`}}` literal braces** (Python str.format escape rule); strings bare, arrays/objects compact JSON, null/missing empty. Grade high (compact-JSON default is inference, after docker/jj `json`). Counter: a Go-style `{{.x}}` template would print `{.x}` literally.
- **Name it `--template`, not `--format`** — `--format` sits beside the global `--error-format` (an enum); gh pairs `--json` with `--template`. Grade medium.
- **`--get` on a row report extracts per row, one line each** (matches `--pluck --lines`). Grade medium (design inference).
