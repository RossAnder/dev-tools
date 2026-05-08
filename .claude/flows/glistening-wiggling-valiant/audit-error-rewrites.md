# Audit ledger: error-message rewrites for the agent-native CLI plan

Generated as Phase 0 of `glistening-wiggling-valiant`. Tasks 5 and 6c read this file as their input list. Each row is one error site that needs rewriting (or marked KEEP for tier-C).

**Class legend**:
- `enum-rejection` (zero-enum) — error names a value that's wrong but doesn't enumerate the valid set.
- `state-precondition` (zero-enum) — error reports a missing prerequisite without telling the caller how to discover/satisfy it.
- `path-shape` (zero-enum) — error rejects a path/shape without quoting the expected form.
- `type-coercion` (zero-enum) — error reports a type mismatch without enumerating acceptable types.
- `partial-enum` — existing message expects a shape but doesn't quote what was actually received.
- `tier-c-keep` — already at template quality; do not rewrite.

| file:line | current_message | class | proposed_rewrite |
|---|---|---|---|
| items.rs:198 | `no item with id = {}` | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:224 | `no item with id = {}` | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:234 | `parsing --json` (context on `serde_json::from_str`) | partial-enum | `parsing --json (expected JSON object, e.g. `{"id":"R1","status":"resolved"}`)` |
| items.rs:257 | `--json must be a JSON object` | partial-enum | `format!("--json must be a JSON object (e.g. {{\"id\":\"R1\",\"status\":\"open\"}}); got JSON {}", crate::convert::json_type_name(&v))` (where `v` is the parsed `JsonValue` rebound before the bail) |
| items.rs:432 | `parsing --json` (context on `serde_json::from_str`) | partial-enum | `parsing --json (expected JSON object, e.g. `{"status":"resolved"}`)` |
| items.rs:454 | `--json must be a JSON object` | partial-enum | `format!("--json must be a JSON object (e.g. {{\"status\":\"resolved\"}}); got JSON {}", crate::convert::json_type_name(&v))` |
| items.rs:483 | `no item with id = {}` | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:523 | `parsing --ops` (context on `serde_json::from_str`) | partial-enum | `parsing --ops (expected JSON array of op objects, e.g. `[{"op":"update","id":"R1","json":{"status":"resolved"}}]`)` |
| items.rs:525 | `--ops must be a JSON array` | partial-enum | `format!("--ops must be a JSON array (e.g. [{{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}]); got JSON {}", crate::convert::json_type_name(&ops))` |
| items.rs:539 | `op[{}] is a remove op, but --no-remove was set; this flag is used by review-apply/optimise-apply to prevent agent-generated payloads from erasing audit history` | tier-c-keep | KEEP |
| items.rs:559 | `op[{}] failed` (with_context) | partial-enum | `op[{}] failed (op shape: {{op:add\|update\|remove,...}})` — keep as-is if the inner op error already enumerates; otherwise extend |
| items.rs:563 | `op[{}] failed` (with_context) | partial-enum | (same as items.rs:559) |
| items.rs:595 | `op must be a JSON object` | partial-enum | `format!("op must be a JSON object (e.g. {{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}); got JSON {}", crate::convert::json_type_name(&op))` |
| items.rs:600 | `op missing `op` field` | path-shape | `op missing `op` field; required shape is {"op":"add\|update\|remove","id":"<id>","json":{...}}` |
| items.rs:606 | `add op missing `json` field` | path-shape | `add op missing `json` field; required shape is {"op":"add","json":{<row fields>}}` |
| items.rs:630 | `update op missing `id` field` | path-shape | `update op missing `id` field; required shape is {"op":"update","id":"<id>","json":{<patch>}}` |
| items.rs:634 | `update op missing `json` field` | path-shape | `update op missing `json` field; required shape is {"op":"update","id":"<id>","json":{<patch>}}` |
| items.rs:642 | `no item with id = {}` | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:652 | `remove op missing `id` field` | path-shape | `remove op missing `id` field; required shape is {"op":"remove","id":"<id>"}` |
| items.rs:688 | `unknown op `{}`` | enum-rejection | `unknown op `{}`; expected one of: add, update, remove` |
| items.rs:703 | `update op `unset` must be an array of strings, got {} at index {}` | type-coercion | `update op `unset` must be an array of strings (e.g. `["status","resolution"]`), got JSON {} at index {}` |
| items.rs:712 | `update op `unset` must be a JSON array of strings, got {}` | type-coercion | `update op `unset` must be a JSON array of strings (e.g. `["status","resolution"]`), got JSON {}` |
| items.rs:732 | `--json must be a JSON object` | partial-enum | `format!("--json must be a JSON object (e.g. {{\"status\":\"resolved\"}}); got JSON {}", crate::convert::json_type_name(&patch))` |
| items.rs:737 | `no item with id = {}` (`expected_id`) | state-precondition | `no item with id = {} (stale id-index; run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:740 | `no item with id = {}` (`expected_id`) | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:742 | `no item with id = {}` (`expected_id`) | state-precondition | `no item with id = {} (id-index drift; run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:775 | `op must be a JSON object` | partial-enum | `format!("op must be a JSON object (e.g. {{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}); got JSON {}", crate::convert::json_type_name(&op))` |
| items.rs:780 | `op missing `op` field` | path-shape | `op missing `op` field; required shape is {"op":"add\|update\|remove",...}` |
| items.rs:786 | `add op missing `json` field` | path-shape | `add op missing `json` field; required shape is {"op":"add","json":{<row fields>}}` |
| items.rs:793 | `update op missing `id` field` | path-shape | `update op missing `id` field; required shape is {"op":"update","id":"<id>","json":{<patch>}}` |
| items.rs:797 | `update op missing `json` field` | path-shape | `update op missing `json` field; required shape is {"op":"update","id":"<id>","json":{<patch>}}` |
| items.rs:811 | `remove op missing `id` field` | path-shape | `remove op missing `id` field; required shape is {"op":"remove","id":"<id>"}` |
| items.rs:815 | `unknown op `{}`` | enum-rejection | `unknown op `{}`; expected one of: add, update, remove` |
| items.rs:830 | `no item with id = {}` | state-precondition | `no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)` |
| items.rs:845 | `prefix must not be empty — use a letter like R, O, or A` | tier-c-keep | KEEP |
| items.rs:852 | `prefix must not be all-digit — would collide with numeric-suffix parsing` | tier-c-keep | KEEP |
| items.rs:898 | `--infer-from-file requires a non-empty ledger or explicit --prefix` | state-precondition | `--infer-from-file requires a non-empty ledger or explicit --prefix (the file has no items with a letter-prefixed `id` to infer from; pass --prefix R/O/A/E directly)` |
| items.rs:905 | `--infer-from-file found multiple prefixes ({}); pass --prefix explicitly` | tier-c-keep | KEEP |
| items.rs:936 | `line {}` (with_context on NDJSON parse) | partial-enum | `line {} (expected one JSON object per line; e.g. {"id":"R1","status":"open"})` |
| items.rs:951 | `--defaults-json must be a JSON object` | partial-enum | `format!("--defaults-json must be a JSON object (e.g. {{\"status\":\"open\",\"severity\":\"warning\"}}); got JSON {}", crate::convert::json_type_name(v))` |
| items.rs:1017 | `row {}: must be a JSON object` | partial-enum | `format!("row {}: must be a JSON object (e.g. {{\"id\":\"R1\",\"summary\":\"...\"}}); got JSON {}", i + 1, crate::convert::json_type_name(row))` |
| items.rs:1020 | `row {}` (with_context wrapping `items_add_value_to`) | partial-enum | `row {} (per-row add failed; row must be a JSON object with at minimum an `id` field)` |
| items.rs:1109 | `parsing --ops` (context on `serde_json::from_str`) | partial-enum | `parsing --ops (expected JSON array of op objects, e.g. `[{"op":"update","id":"R1","json":{"status":"resolved"}}]`)` |
| items.rs:1111 | `--ops must be a JSON array` | partial-enum | `format!("--ops must be a JSON array (e.g. [{{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}]); got JSON {}", crate::convert::json_type_name(&ops))` |
| items.rs:1317 | `row {}: must be a JSON object` | partial-enum | `format!("row {}: must be a JSON object (e.g. {{\"id\":\"R1\",\"summary\":\"...\"}}); got JSON {}", row_num, crate::convert::json_type_name(row))` |
| items.rs:1331 | `row {}` (with_context wrapping `items_add_value_to`) | partial-enum | `row {} (per-row dedupe-add failed; row must be a JSON object with at minimum an `id` field)` |
| dispatch.rs:115 | `--dedupe-by requires at least one field name` | path-shape | `--dedupe-by requires at least one field name (e.g. `--dedupe-by source,target` for a comma-separated list)` |
| dispatch.rs:138 | `stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call` | tier-c-keep | KEEP |
| dispatch.rs:142 | `stdin is a TTY — pipe JSON (e.g. `cat payload.json \| tomlctl … --json -`) or pass `--json '<literal>'`` | tier-c-keep | KEEP |
| dispatch.rs:153 | `stdin was empty — expected JSON payload` | path-shape | `stdin was empty — expected JSON payload (e.g. an object `{...}`, array `[...]`, or NDJSON depending on the flag)` |
| dispatch.rs:184 | `stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call` | tier-c-keep | KEEP |
| dispatch.rs:189 | `stdin is a TTY — pipe JSON (e.g. `cat payload.json \| tomlctl … --json -`) or pass `--json '<literal>'`` | tier-c-keep | KEEP |
| dispatch.rs:201 | `stdin was empty — expected JSON payload` | path-shape | `stdin was empty — expected JSON payload (e.g. an object `{...}`, array `[...]`, or NDJSON depending on the flag)` |
| dispatch.rs:358 | `key path `{}` not found` | state-precondition | `key path `{}` not found (run `tomlctl parse <file>` to inspect the document tree, or `tomlctl get <file>` with no --path to print the whole doc)` |
| dispatch.rs:421 | `array-append requires one of --json or --ndjson` | path-shape | `array-append requires one of --json or --ndjson (e.g. `--json '{"k":"v"}'` for a single row, `--ndjson rows.ndjson` for a batch)` |
| dispatch.rs:430 | `--json must be a JSON object` | partial-enum | `format!("--json must be a JSON object (e.g. {{\"k\":\"v\"}}); got JSON {}", crate::convert::json_type_name(&parsed))` |
| dispatch.rs:732 | `--ops contains {} operations, which exceeds the cap of {}; split the batch into smaller /review-apply or /optimise-apply invocations` | tier-c-keep | KEEP |
| dispatch.rs:784 | `--infer-from-file requires a non-empty ledger or explicit --prefix` | state-precondition | `--infer-from-file requires a non-empty ledger or explicit --prefix (the file does not exist yet; pass --prefix R/O/A/E directly to bootstrap)` |
| dispatch.rs:1011 | `--no-write-integrity is meaningless on `integrity refresh` — the subcommand's entire purpose is to write the sidecar` | tier-c-keep | KEEP |
| query.rs:95 | `invalid regex `{}`: {}` | partial-enum | `invalid regex `{}`: {} (regex must compile under size_limit={REGEX_COMPILE_SIZE_LIMIT}/dfa_size_limit={REGEX_DFA_SIZE_LIMIT})` |
| query.rs:279 | `internal: --count output missing `count` key` | tier-c-keep | KEEP |
| query.rs:287 | `internal: --count-distinct output missing `count_distinct` key` | tier-c-keep | KEEP |
| query.rs:299 | `internal: --pluck output was not a JSON array` | tier-c-keep | KEEP |
| query.rs:301 | `--raw requires single-value output (got 0 items)` | path-shape | `--raw requires single-value output (got 0 items); for a possibly-empty pluck use `--pluck <f> --lines` to stream zero-or-more JSON values one per line` |
| query.rs:303 | `--raw requires single-value output (got {} items); use --lines for newline-delimited` | tier-c-keep | KEEP |
| query.rs:315 | `--raw requires a scalar target; got array` | type-coercion | `--raw requires a scalar target (string\|number\|bool); got array — use `--lines` (or omit --raw) to emit JSON` |
| query.rs:323 | `--raw is not supported on --count-by / --group-by (output is a map, not a scalar)` | tier-c-keep | KEEP |
| query.rs:381 | `--select and --exclude are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:386 | `--select and --pluck are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:389 | `--exclude and --pluck are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:394 | `--count and --select are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:397 | `--count and --exclude are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:402 | `--count-by and --select are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:405 | `--count-by and --exclude are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:415 | `--select and --count-distinct are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:420 | `--exclude and --count-distinct are mutually exclusive` | tier-c-keep | KEEP |
| query.rs:443 | `--raw is not supported on --count-by / --group-by (output is a map, not a scalar)` | tier-c-keep | KEEP |
| query.rs:683 | `--raw cannot emit null value` | type-coercion | `--raw cannot emit null value; --raw expects a scalar (string\|number\|bool) — use plain JSON output (omit --raw) to round-trip null` |
| query.rs:686 | `--raw requires a scalar target; got array` | type-coercion | `--raw requires a scalar target (string\|number\|bool); got array — use `--lines` (or omit --raw) to emit JSON` |
| query.rs:689 | `--raw requires a scalar target; got table` | type-coercion | `--raw requires a scalar target (string\|number\|bool); got table — use plain JSON output (omit --raw) to emit the object` |
| query.rs:960 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (Int parse) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (expected an integer after `@int:`, e.g. `@int:42`)` |
| query.rs:971 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (Float parse) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (expected a finite float after `@float:`, e.g. `@float:1.5`)` |
| query.rs:982 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (Bool parse) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (expected `true` or `false` after `@bool:`)` |
| query.rs:993 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (Date/DateTime parse) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (expected ISO-8601 date or datetime after `@date:`/`@datetime:`, e.g. `@date:2026-01-15`)` |
| query.rs:1077 | `internal: missing compiled regex for --where-regex on key `{}`` | tier-c-keep | KEEP |
| query.rs:1159 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (eq fallback) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (recognised type prefixes: @int:, @float:, @bool:, @string:/@str:, @date:, @datetime:)` |
| query.rs:1233 | `invalid typed RHS `{}` for --where predicate on key `{}`: type hint `int` doesn't match field type` | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: @int: requires an integer field; the field's TOML type is not Integer` |
| query.rs:1241 | `invalid typed RHS `{}` for --where predicate on key `{}`: type hint `float` doesn't match field type` | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: @float: requires a float field; the field's TOML type is not Float` |
| query.rs:1249 | `invalid typed RHS `{}` for --where predicate on key `{}`: bool RHS not comparable against non-bool field` | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: @bool: requires a boolean field; the field's TOML type is not Boolean` |
| query.rs:1259 | `invalid typed RHS `{}` for --where predicate on key `{}`: datetime RHS not comparable against non-datetime field` | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: @date:/@datetime: requires a datetime field; the field's TOML type is not Datetime` |
| query.rs:1267 | `invalid typed RHS `{}` for --where predicate on key `{}`: string RHS not comparable against non-string field` | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: @string:/@str: requires a string field; the field's TOML type is not String` |
| query.rs:1282 | `invalid typed RHS `{}` for --where predicate on key `{}`: {}` (Untyped fallback) | type-coercion | `invalid typed RHS `{}` for --where predicate on key `{}`: {} (no @type: prefix — RHS was driven by the field's native type and parsing failed; consider an explicit prefix from: @int:, @float:, @bool:, @string:/@str:, @date:, @datetime:)` |
| query.rs:1682 | `expected KEY=VAL, got `{}`` | path-shape | `expected KEY=VAL (e.g. `status=open`), got `{}`` |
| query.rs:1685 | `KEY=VAL has empty key in `{}`` | path-shape | `KEY=VAL has empty key in `{}` (e.g. `status=open`); the LHS before `=` must be non-empty` |
| query.rs:1777 | `--where-has expects a KEY, got empty string` | tier-c-keep | KEEP |
| query.rs:1783 | `--where-missing expects a KEY, got empty string` | tier-c-keep | KEEP |
| dedup.rs:76 | `tier C is file-scoped; use --tier A or --tier B with --across` | tier-c-keep | KEEP |
| dedup.rs:499 | `tier C is file-scoped; use --tier A or --tier B with --across` | tier-c-keep | KEEP |
| convert.rs:109 | `empty key path` | path-shape | `empty key path; path must be a non-empty `.`-separated dotted path (e.g. `nested.inner.key`)` |
| convert.rs:117 | `path segment `{}` is not a valid array index` | type-coercion | `path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0.id`)` |
| convert.rs:123 | `array index `{}` out of bounds` | partial-enum | `array index `{}` out of bounds (use `tomlctl get <file> --path <parent>` to inspect array length first)` |
| convert.rs:128 | `path segment `{}` has a non-table parent` | type-coercion | `path segment `{}` has a non-table parent (intermediate path segments must resolve to TOML tables; only the array-index form `parent.<n>` may descend into an array)` |
| convert.rs:131 | (no error — auto-vivify branch) | n/a | (skip — not an error site) |
| convert.rs:136 | `final path segment `{}` is not a valid array index` | type-coercion | `final path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0`)` |
| convert.rs:141 | `array lost during traversal` | tier-c-keep | KEEP |
| convert.rs:143 | `array index `{}` out of bounds (len {})` | tier-c-keep | KEEP |
| convert.rs:150 | `target parent is not a table` | type-coercion | `target parent is not a table (cannot insert by key into a TOML scalar/array — final segment must address a table)` |
| convert.rs:163 | `` `{}` is not a valid int `` | type-coercion | `` `{}` is not a valid int (must parse as i64; range -9_223_372_036_854_775_808..=9_223_372_036_854_775_807) `` |
| convert.rs:167 | `` `{}` is not a valid float `` | type-coercion | `` `{}` is not a valid float (must parse as a finite f64; e.g. `1.5`, `-2.0e3`) `` |
| convert.rs:172 | `` `{}` is not a valid bool `` | type-coercion | `` `{}` is not a valid bool (expected `true` or `false`) `` |
| convert.rs:177 | `` `{}` is not a valid TOML datetime `` | type-coercion | `` `{}` is not a valid TOML datetime (expected ISO-8601 date `YYYY-MM-DD` or datetime `YYYY-MM-DDTHH:MM:SSZ`) `` |
| convert.rs:306 | `TOML has no null type` | type-coercion | `TOML has no null type; remove the field or replace with an explicit empty value (`""` for strings, `[]` for arrays)` |
| convert.rs:314 | `number `{}` is not representable in TOML` | type-coercion | `JSON number `{}` is not representable as TOML int or float (must fit i64 or be finite f64)` |
| convert.rs:491 | `` `{}` is not a valid ISO date `` | type-coercion | `` `{}` is not a valid ISO date (expected `YYYY-MM-DD` after `@date:`) `` |
| convert.rs:497 | `` `{}` is not a valid ISO datetime `` | type-coercion | `` `{}` is not a valid ISO datetime (expected `YYYY-MM-DDTHH:MM:SSZ` after `@datetime:`) `` |
| convert.rs:503 | `` `{}` is not a valid int `` | type-coercion | `` `{}` is not a valid int (expected an integer after `@int:`, e.g. `@int:42`) `` |
| convert.rs:509 | `` `{}` is not a valid float `` | type-coercion | `` `{}` is not a valid float (expected a finite float after `@float:`, e.g. `@float:1.5`) `` |
| convert.rs:515 | `` `{}` is not a valid bool `` | type-coercion | `` `{}` is not a valid bool (expected `true` or `false` after `@bool:`) `` |
| convert.rs:545 | `` `{}` is not comparable as int `` | type-coercion | `` `{}` is not comparable as int (RHS must parse as i64 to compare against an Integer field) `` |
| convert.rs:547 | `type hint `{:?}` doesn't match integer field` | type-coercion | `type hint `{:?}` rejected; expected one of: int, float (TOML's only numeric types). Field is Integer — use `@int:` or omit the prefix.` |
| convert.rs:553 | `` `{}` is not comparable as float `` | type-coercion | `` `{}` is not comparable as float (RHS must parse as a finite f64 to compare against a Float field) `` |
| convert.rs:556 | `type hint `{:?}` doesn't match float field` | type-coercion | `type hint `{:?}` rejected; expected one of: int, float (TOML's only numeric types). Field is Float — use `@float:` or omit the prefix.` |
| convert.rs:563 | `` `{}` is not comparable as bool `` | type-coercion | `` `{}` is not comparable as bool (expected `true` or `false`) `` |
| convert.rs:572 | `` `{}` is not a valid TOML datetime `` | type-coercion | `` `{}` is not a valid TOML datetime (expected ISO-8601 date `YYYY-MM-DD` or datetime `YYYY-MM-DDTHH:MM:SSZ`) `` |
| convert.rs:576 | `field is not a scalar; cannot compare` | type-coercion | `field is not a scalar; cannot compare with --where-gt/gte/lt/lte (only String, Integer, Float, Boolean, Datetime fields are orderable)` |
| blocks.rs:132 | `blocks verify: no files supplied` | path-shape | `blocks verify: no files supplied; pass one or more file paths (e.g. `tomlctl blocks verify a.md b.md`)` |
| io.rs:120 | `root is not a table` | tier-c-keep | KEEP |
| io.rs:126 | `` `{}` is not an array `` | type-coercion | `` `{}` is not an array (the named --array key exists but its value is not a TOML array; expected array-of-tables form `[[name]]`) `` |
| io.rs:471 | `pre-persist containment check failed: target parent {} is no longer under {} (possible TOCTOU symlink swap since guard_write_path — aborting)` | tier-c-keep | KEEP |
| io.rs:593 | `lock held on {} for {} seconds — another tomlctl process may be hanging. If no tomlctl process is running, check for stale lock and delete {} manually.` | tier-c-keep | KEEP |
| io.rs:688 | `shared lock blocked on {} for {} seconds — a writer may be hanging. If no tomlctl process is running, check for stale lock and delete {} manually.` | tier-c-keep | KEEP |
| io.rs:770 | `refusing to write outside .claude/ (path resolves to {}); pass --allow-outside to override` | tier-c-keep | KEEP |
| io.rs:878 | `write target `{}` has no file name` | path-shape | `write target `{}` has no file name (path must end in a filename component, e.g. `.claude/flows/<slug>/context.toml`)` |
| io.rs:887 | `write target `{}` contains a disallowed `..` or absolute root component after canonicalisation` | tier-c-keep | KEEP |
| io.rs:943 | `refusing to write through symlink at {} pointing outside .claude/ (resolves to {})` | tier-c-keep | KEEP |
| io.rs:1048 | `target `{}` has no file name` | path-shape | `target `{}` has no file name (path must end in a filename component for sidecar derivation)` |
| io.rs:1089 | `serialising TOML` (context) | tier-c-keep | KEEP |
| io.rs:1109 | `refreshed integrity sidecar but failed to persist {} (--strict-integrity was set, so this is a hard error)` | tier-c-keep | KEEP |
| io.rs:1193 | `atomic rename to {} failed: {}` | tier-c-keep | KEEP |
