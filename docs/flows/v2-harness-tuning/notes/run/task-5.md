Status: done — a `--template` path that resolves on no row now warns on stderr, exit status unchanged, unless it names a listed optional field

## What changed

- `tomlctl/src/convert.rs`: new `unmatched_row_paths(rows, paths)`, returning each path that resolves on no row (once each), by the same wildcard-prefix test `validate_row_paths` uses. `validate_row_paths` is unchanged, so its refusals hold as before.
- `tomlctl/src/output/template.rs`: new `OPTIONAL_FIELDS` table (`advisories`, `area`, `context`, `promoted_to`, `resolution`) and `Template::unmatched_paths(rows)`, which returns nothing for an empty row set and drops any path whose first segment is in the table. Added a unit test, `template_unmatched_paths_skip_optional_fields_and_repeats`.
- `tomlctl/src/output.rs`: `Emitted` gains `warnings: Vec<String>`. `emit_one` (single object) and `emit_rows` (row sets) fill it right after path validation, so before `--where` and `--limit` cut the rows. `write_emitted` prints the warnings, then the notice, and prints neither under `--error-format json`. Under `-q` the emitters return before validation, so they print nothing. The line reads `tomlctl: --template path <path> matched nothing`, as design/shapes.md#template-paths gives it. The other `Emitted` literals now use `..Emitted::default()`.
- `tomlctl/tests/output_trim.rs`:
  - `template_unknown_path_warns` covers a list verb (with a repeated path warned once), a single object, and the `--error-format json` suppression.
  - `template_optional_path_is_quiet` covers `items list --template '{id} {context}'` on a ledger with no `context`.
- `claude/skills/tomlctl/SKILL.md`: one sentence in the Unknown paths paragraph covering the warning, the optional-field list and the JSON suppression.

## How the optional-field table was seeded

I ran a survey of every `--template` in a `tomlctl` command under `claude/skills/`, `claude/commands/`, `claude/agents/` and `claude/workflows/`. The placeholders were line, verdict, summary, counts.*, action, id, advisories, status, kind, severity, class, ids, detail, title, area, context, resolution, files and file.

- `advisories` stays in the table because the plan names it, and the `add-many` envelope inserts it only when non-empty (`insert_advisories`).
- `area`, `context` and `resolution` are backlog item fields that `backlog::schema::validate` does not require (it requires only id, summary and status).
- `promoted_to` is named by the task.
- The tasks/plan check finding fields (`tasks::finding::Finding`) and the add outcome's `action` and `id` are always emitted, so they are not in the table.

## Deviations

- None from a `suggested` means; the task named none. Where the warning goes: I chose a `warnings` field on `Emitted`, printed by `write_emitted`, over an `eprintln!` inside the emitters. This follows how the `--limit` notice already travels, keeps the emitters pure for unit tests, and gets the `json_errors` suppression for free.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on template.rs, output.rs and convert.rs: clean. I applied rustfmt to output_trim.rs.
- `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(template_) | test(template::)'`: 33 passed.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, run because SKILL.md changed.
- Both accept checks (`grep -q 'fn template_unknown_path_warns'` and `grep -q 'fn template_optional_path_is_quiet'`): pass.

## Handover

None.

TANGENTIAL: none
