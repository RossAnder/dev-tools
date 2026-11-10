Status: done — review-v2.js has a reproduce mode: one review-verifier per unit, no cells, its own arg checks and return

## What changed

- `claude/workflows/review-v2.js`
  - `meta`: `whenToUse` now names the reproduce launch; a third phase, `Reproduce`, is listed. The meta stays literal (no `${`, no escaped quote).
  - `SCRIPT_VERSION` raised from `'2'` to `'3'`.
  - New block right after `const a = args || {}`, entered only when `a.mode === 'reproduce'`. It returns before any cell-mode code runs, so a launch without that mode reaches the unchanged cell path. Any other `mode` value is still ignored, as before.
    - Arg checks run before any agent starts. On any problem it pushes each one plus `no agent launched` to `errors` and returns with `units: []`. The checks: `run_dir` is one line; `store` holds exactly one of `slug`/`scope`, and that value matches the slug pattern `tomlctl review` resolves with (`validate_slug` for both); `flow` matches the slug pattern; `milestone` is an id; `round` is a positive integer; `units` is a non-empty array; each unit's `task` matches the plan ref charset (`is_ref` in `tomlctl/src/plan/target.rs`) and is not repeated; `files` is an array of one-line paths; `findings` is a non-empty array of `F<n>`.
    - The script holds the unit return schema itself: `{task, reproductions: [{finding, verdict ∈ gone|reproduces|unsettled}]}`. It reads no `schemas` and no pack.
    - Each unit gets one `review-verifier` in a single parallel phase. The prompt gives store, flow, milestone, round, task, files, findings and run directory. It tells the verifier to read with `tomlctl review show <ids> <store flag> --for implementer`, judge against the working tree, write NDJSON to `<run_dir>/payloads/reproduce-<task>.ndjson` (the path the sibling verifier docs name), record with `tomlctl review reproduce <store flag> --ndjson <path>`, and change no other file.
    - Return: `{mode: 'reproduce', units: [{task, status, reproductions, error}], errors, script_version}`. `status` is `ok`, `no-return` (agent returned nothing) or `error`. `error` covers a thrown dispatch, a mismatched `task`, or a finding with no returned reproduction. Reproductions naming a finding outside the unit are dropped.

## Deviations / choices

- `ledger:` line: for a `store.slug` launch the prompt opens with `ledger: .claude/flows/<slug>/review-v2.toml` at column 0, which agent correlation reads. I derived it instead of taking a `ledger_line` arg, because the design fixes the args and has no such key. A `scope` store gets no ledger line, since no flow file of that name exists. Alternatives were to omit it, which loses correlation, or to add an arg, which departs from the fixed design.
- Invalid units refuse the whole launch rather than being skipped one by one, per the constraint "Bad reproduce args refuse the whole launch".

## Checks

- Compile: none applies to JS (`node --check` cannot parse a Workflow body). Instead I loaded the script body inside a stub harness (stubbed `agent`/`parallel`/`phase`/`log`) under node and ran three launches. A valid reproduce launch gave an `ok` unit and a `no-return` unit, plus the expected prompt. A bad reproduce launch gave every problem and `no agent launched`, with zero agents. A cell-mode launch gave the unchanged cell index with `script_version: "3"`.
- Accept `grep -q 'review reproduce' … && grep -q 'reproduce' …`: PASS.
- Accept `cargo test … cli::dispatch::tests` (Writes build: yes): left to the gate. By reading the gate code: the only agentType is the literal `'review-verifier'`, there are no override keys, and the tomlctl spans (`review show <ids> <flag> --for implementer`, `review reproduce <flag> --ndjson <path>`) use only flags the installed binary's `--help` lists.

TANGENTIAL: none
