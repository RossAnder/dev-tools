Verdict: Yes for six of the seven row keys `plan run gate` reads (run_gate.rs:26-34). `failed_ids` is lossy: verification.md:62 joins ids with spaces and caps them at 20. Quoted commands come back verbatim in the block. They are lost or mangled only in the hand-written TOML staging step (drain.md:347-360), and the `--rerun` merge matches on the exact command string (run_gate.rs:77).

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = []

[[source]]
path = "claude/agents/verification.md"
line = 151
claim = "Pass line shape: outcome | command | duration | summary | log, optional | retried: transient; no exit"

[[source]]
path = "claude/agents/verification.md"
line = 156
claim = "Non-pass block field order: outcome, exit, duration_s, log, summary, failed_ids, retried, rerun, tail"

[[source]]
path = "claude/agents/verification.md"
line = 175
claim = "The worked example opens each block with a `command:` line, then `outcome:`; TANGENTIAL lines sit inside a flaky block"

[[source]]
path = "claude/agents/verification.md"
line = 136
claim = "Refused-command block carries only command, outcome, exit none and tail"

[[source]]
path = "claude/agents/verification.md"
line = 62
claim = "FAILED_IDS is the first 20 lines of the .ids file joined by spaces"

[[source]]
path = "claude/agents/verification.md"
line = 59
claim = "bun `(fail) <name>` extraction yields test ids that contain spaces"

[[source]]
path = "claude/agents/verification.md"
line = 47
claim = "Summary takes vitest `Test Files` / `Tests` lines, which print ` | ` between counts"

[[source]]
path = "claude/agents/verification.md"
line = 40
claim = "<D> is printed through cygpath -m on Windows, so log paths are C:/... mixed paths"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 26
claim = "ROW_KEYS: command, outcome, exit, duration_s, summary, failed_ids, log; any other key is refused"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 214
claim = "parse_row: command non-empty string, outcome in vocabulary, exit integer or null/absent, duration_s non-negative number (rounded), summary string, failed_ids string array, log string"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 77
claim = "--rerun merge and in-batch dedupe match on exact command string equality"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 85
claim = "Only non-pass rows with a non-empty log get a kept tail; the stored log is the kept repo-relative path"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 493
claim = "GateCommand fields; exit is Option<i64>, None meaning the agent reported no exit code"

[[source]]
path = "tomlctl/src/items.rs"
line = 1490
claim = "row_string/row_strings accept only JSON strings / arrays of strings"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 347
claim = "The orchestrator hand-stages one [[record]] per block into a TOML file, with the field-mapping rules at line 360"

[[source]]
path = "claude/skills/tomlctl/references/plan-run.md"
line = 77
claim = "Documented --ndjson row keys and defaults for plan run gate"

[[source]]
path = ".claude/flows/mid-run-requests/plan.toml"
line = 686
claim = "Real check commands carry both quote kinds and backslashes (test \"$(grep -c '#\\[test\\]' ...)\")"
```

## Answer

**What the agent returns** for each command it ran, in list order, comes in two shapes:

- A `pass` is one line (verification.md:151): `outcome: pass | command: <command> | duration: <DUR> | summary: <SUMMARY or none> | log: <D>/cN.log`, with ` | retried: transient` appended after a transient retry. It has no `exit` and no `failed_ids`.
- `fail`, `timeout` and `flaky` are blocks (verification.md:156-166, example at 175-195). Each opens with `command: <cmd>`, then has `outcome:`, `exit: <n|none>`, `duration_s:`, `log:` and `summary:`. These are optional: `failed_ids: <space-joined ids> [(+n more)]`, `retried: transient`, and `rerun: <cmd> → exit <n>; <summary>`. The last field is `tail:` (fail and timeout only) followed by up to 20 raw log lines. A flaky block also has `TANGENTIAL:` lines, and a stopped run ends with a `not_run:` line. A refused mutation carries only `command`, `outcome: fail`, `exit: none` and `tail:` (verification.md:136-142).

**What `plan run gate` reads** is one row per command with keys `command`, `outcome`, `exit`, `duration_s`, `summary`, `failed_ids` and `log` (run_gate.rs:26-34). Any other key refuses the whole batch. Each key has a fixed type, checked in parse_row (run_gate.rs:214-264):

| Key | Type the verb accepts |
|---|---|
| `command` | non-empty string |
| `outcome` | `pass`, `flaky`, `fail` or `timeout`, case-sensitive |
| `exit` | integer, or absent/null, stored as None |
| `duration_s` | non-negative number, fraction rounded, absent means 0 |
| `summary` | string |
| `failed_ids` | array of strings |
| `log` | string path (a source path, not the stored value) |

Nothing in tomlctl parses the agent's text today. The orchestrator turns blocks into a hand-written TOML `[[record]]` file under the mapping rules at drain.md:360.

**Can each field be parsed from the blocks?**

| Field | Lossless? | Why |
|---|---|---|
| `command` | Yes, given a grammar | A block's `command:` takes the rest of its line. Commands are single-line list items, copied byte for byte. Nested quotes are not escaped in the agent's text, so nothing is lost there. The pass line is delimited by ` | `, and a command may hold a shell pipe, so it must be read up to the ` | duration: ` that follows. A plain split on ` | ` breaks it. |
| `outcome` | Yes | Closed vocabulary. |
| `exit` | Yes for blocks (`none` means absent) | A pass line has no exit, so the stored value is None rather than 0. The record cannot tell "passed with exit 0" from "no exit reported" (schema.rs:496). |
| `duration_s` | Yes | It is `duration:` on a pass line and `duration_s:` in a block. Both are whole seconds. |
| `summary` | Yes, given a grammar | `none` is the absent sentinel. Vitest's `Tests  2 failed \| 10 passed` puts ` | ` inside the summary (verification.md:47), so on a pass line the summary has to be read up to the last ` | log: `. |
| `failed_ids` | **No** | `FAILED_IDS` is `head -n 20 … \| tr '\n' ' '` (verification.md:62). Ids containing spaces, such as bun's `(fail) suite > name` (verification.md:59), split into fragments. Every id past the 20th is gone, and only the `(+n more)` count survives. The full list exists at `<D>/cN.log.ids` (`.ids` for short), but the agent never returns that path. |
| `log` | Yes | It is the last `\|` field on a pass line (which the verb ignores for a pass) and its own line in a block. Paths are `C:/…` mixed paths through cygpath (verification.md:40). |

**Ambiguities a parser has to handle.** The lines after `tail:` are raw log text and can begin with `command:`, `outcome:` or `not_run:`. The block boundary is unambiguous only if `tail:` is treated as running until a line `command: X` that is directly followed by a line starting `outcome: `, a pass line `outcome: pass | command:`, or `not_run:`. Even that fails if a log prints such a pair. `TANGENTIAL:` lines sit inside a flaky block (verification.md:183) and have no row key.

**Nested quotes.** The agent returns quoted commands unharmed. The loss point is the hand-staged TOML: a basic string needs every `"` and `\` escaped, and real gate and check commands carry both (for example plan.toml:686, `test "$(grep -c '#\[test\]' …)"`). Two outcomes follow if that escaping goes wrong:

- A broken escape refuses the batch.
- A silently altered command records a different string. Under `--rerun` it then appends a second row instead of replacing the first, because the merge compares exact strings (run_gate.rs:77).

A verb that parses the agent's text directly removes that step.

## Findings

- **Library/API or file**: claude/agents/verification.md:62 (Step 0 extract.sh `FAILED_IDS`) with tomlctl/src/plan/run_gate.rs:259 (`failed_ids` via `row_strings`)
- **Source**: verification.md:62 `echo "FAILED_IDS: $(head -n 20 "$L.ids" | tr '\n' ' ')"`; verification.md:59 bun `(fail) (.+)` capture; drain.md:360 "its space-separated `failed_ids:` ids, one string each, without a `(+<n> more)` tail"
- **Evidence-grade**: high
- **Finding**: `failed_ids` cannot be rebuilt without loss from the block. Ids containing spaces split apart, and every id past the 20th is dropped.
- **Details**: The `.ids` file holds one id per line (verification.md:60). The returned line joins the first 20 with spaces, and drain.md:360 tells the orchestrator to split on spaces and drop `(+n more)`. To make it lossless, either have the block emit the `.ids` path or one id per line, or have the gate verb read `<cN>.log.ids` next to `log`. read_temp_log already allows reads under temp (run_gate.rs:295).
- **Counter**: If no gate runner in use ever emits an id with a space and no gate fails more than 20 tests, the loss is theoretical. bun's `(fail)` rule at verification.md:59 shows the first case is reachable.
- **Impact on plan / report**: A block parser has to either accept a capped, space-split `failed_ids` or change the agent's output (or the verb) to carry the full id list.

- **Library/API or file**: claude/agents/verification.md:151 (pass-line format) and :47 (summary grep)
- **Source**: verification.md:151 format string; verification.md:47 `grep -E '^ +(Test Files|Tests) +[0-9]'` (vitest prints `Tests  2 failed | 10 passed (12)`)
- **Evidence-grade**: medium. The vitest line format comes from knowledge of the runner and was not fetched. A shell pipe inside a command is certain.
- **Finding**: The pass line's ` | ` delimiter also occurs inside `command` (shell pipes) and inside `summary` (vitest). A parser must anchor on the known keys: command up to ` | duration: `, summary up to the last ` | log: `.
- **Details**: The field order is fixed: outcome, command, duration, summary, log, then optionally retried. Reading `command` from the left up to the first ` | duration: <int> | summary: ` and `log` from the right recovers every field. It fails only if a command literally contains ` | duration: `.
- **Counter**: If the agent were changed to emit pass results as blocks too, the delimiter problem would disappear.
- **Impact on plan / report**: The parser grammar for pass lines must be key-anchored, and needs test cases for a command with a pipe and a vitest summary.

- **Library/API or file**: claude/agents/verification.md:166 (`tail:` field) and :183 (`TANGENTIAL:` inside a block)
- **Source**: verification.md:166 "the lines after `--- tail ---`"; example at 175-195
- **Evidence-grade**: high
- **Finding**: Block boundaries are ambiguous after `tail:`, because raw log lines may start with `command:`, `outcome:` or `not_run:`.
- **Details**: A new block opens with `command: …` followed directly by a line starting `outcome: `. A safe rule ends a tail only on that pair, on a line starting `outcome: pass | command: `, or on a `not_run:` line. `TANGENTIAL:` lines must be skipped as non-row lines. `rerun:`, `retried:` and `tail:` have no row key and are dropped legitimately. The verb keeps the log tail from `log` itself (run_gate.rs:85-92).
- **Counter**: If the agent fenced or indented its tail lines, the ambiguity would go away. Today it does neither.
- **Impact on plan / report**: The parser needs a boundary rule plus a test where the tail contains a `command:` line, or the agent's output gains a tail terminator.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:347 (hand-staged `[[record]]` TOML) with tomlctl/src/plan/run_gate.rs:77 (exact-string merge key)
- **Source**: drain.md:349-358 TOML basic-string example; run_gate.rs:74-84 `position(|c| c.command == command.command)`; .claude/flows/mid-run-requests/plan.toml:686 command with `"`, `'` and `\[`
- **Evidence-grade**: high
- **Finding**: Nested quotes survive in the agent's text. The hand-transcription into TOML basic strings is where `"` and `\` must be escaped, and a slip either refuses the batch or changes the command string that `--rerun` matches on.
- **Details**: The command is returned flush, single-line and unescaped, so parsing it from the block is lossless. drain.md shows `command = "…"` basic strings, and real check commands mix both quote kinds and backslashes. A verb that parses the agent's text (or a JSON/NDJSON row built by code) removes the hand-escaping step and keeps the merge key byte-identical to `plan emit gate-list` output.
- **Counter**: If recorded gate rounds always used commands with no `"` or `\`, there would be no exposure. Accept checks routed to the gate carry them, as plan.toml:686 shows.
- **Impact on plan / report**: This supports parsing the blocks in code, and suggests asserting the parsed command against the `plan emit gate-list` entries.

- **Library/API or file**: claude/agents/verification.md:151 (pass line has no `exit`) with tomlctl/src/plan/schema.rs:496 (`exit: Option<i64>`)
- **Source**: verification.md:151; drain.md:360 "`exit` is its `exit:` value, left out when that is `none`"
- **Evidence-grade**: high
- **Finding**: A pass row's `exit` is always absent, so the store reads "the agent reported no exit code" for every pass. A parser may set `exit = 0` for `pass` (Step 3 rule 3 means pass implies `EXIT=0`, verification.md:108).
- **Details**: A plain pass is classified only on `EXIT=0`, so 0 is implied without loss. A pass after a transient retry also had `EXIT=0` on the `cNt` run.
- **Counter**: If readers of `[[gate_round]]` never consult `exit` for pass rows, this changes nothing.
- **Impact on plan / report**: A small decision for the parser: set 0 or keep None.

Coverage: read in full claude/agents/verification.md and tomlctl/src/plan/run_gate.rs. Read GateCommand in schema.rs, row_string and row_strings in items.rs, read_ndjson_source in io.rs, drain.md:320-389 and plan-run.md:55-97. Grepped for an existing block parser (none in tomlctl; glimpse only reads the store) and for quoted gate and check commands in flow stores. Ruled out:

- `duration` vs `duration_s` naming: mapped in drain.md:360, no loss.
- `summary: none`: a sentinel, no loss.
- Unix `/tmp` log paths on Windows: outside this question; cygpath -m handles them.
- Refused blocks without duration or log: verb defaults cover them.
