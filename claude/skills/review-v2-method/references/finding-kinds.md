# Worked examples: finding rows

Illustrative rows, one per kind, per cleanup direction and per family. Paths and code are invented. Each row is one line of a `payloads/<cell>.ndjson` file, exactly as `review add` takes it: one JSON object per line, no line breaks inside an object, no fields tomlctl computes (`id`, `status`, `route`, `anchor`, `related`, `outcome`, `cell`, `run`, `first_seen`).

## defect — code family

A real failure with a nameable input. The `failure_scenario` is the claim the verifier will try to break; `counter` names the observation that would break it.

```json
{"kind": "defect", "lens": "correctness", "file": "src/store/merge.rs", "line": 142, "symbol": "merge::apply_rows", "summary": "apply_rows drops the last row when the batch length is a multiple of the chunk size", "severity": "major", "failure_scenario": "A 64-row batch with CHUNK = 32: the loop bound `i + CHUNK < len` stops at i = 32, so rows 32..63 are never applied and the call still returns Ok.", "fix": "Loop while `i < len` and take `min(i + CHUNK, len)` as the chunk end.", "fix_shape": "edits", "effort": "small", "counter": "A caller that pads every batch to a non-multiple of CHUNK would never hit this; grep of the three callers found none that pads.", "grade": "high", "evidence": ["src/store/merge.rs:142 `while i + CHUNK < len`", "src/store/merge.rs:150 returns Ok(()) without a remainder pass"]}
```

## defect — perf family

The scenario names the workload, the input size and the cost with its magnitude.

```json
{"kind": "defect", "lens": "algorithm", "file": "src/index/lookup.rs", "line": 58, "symbol": "lookup::find_all", "summary": "find_all rescans the whole id list per query, quadratic over a sweep", "severity": "major", "failure_scenario": "A sweep over 20k rows calls find_all once per row, and each call walks the 20k-entry Vec: about 4e8 comparisons, measured at 9.2 s against 0.04 s with a HashMap keyed by id.", "fix": "Build a HashMap<Id, usize> once in Index::new and look ids up through it.", "fix_shape": "edits", "effort": "medium", "counter": "If sweeps never exceed a few hundred rows the cost is negligible; the PROFILE BASELINE shows a 20k-row fixture in the bench set.", "grade": "high", "evidence": ["src/index/lookup.rs:58 linear `iter().position` per call", "bench sweep_20k: 9.2 s (see PROFILE BASELINE log)"]}
```

## defect — plan family

`file` is the plan, `line` the heading line of `plan_section`, `symbol` the task ref. `anchor_old` occurs exactly once under that heading; `anchor_new` replaces it.

```json
{"kind": "defect", "lens": "plan-executor", "file": "docs/plans/export.md", "line": 212, "symbol": "add-the-export-verb", "summary": "Task 4's acceptance runs a test binary no task creates", "severity": "major", "failure_scenario": "At acceptance the implementer runs `cargo test --test export_cli`; no task's Files line creates tests/export_cli.rs, so the command fails with `no test target named export_cli`.", "fix": "Add tests/export_cli.rs (new) to task 4's Files line, or point the acceptance at the existing tests/export.rs.", "fix_shape": "edits", "effort": "trivial", "counter": "A later task could create the file; the Files lines of tasks 5-9 do not name it.", "grade": "high", "evidence": ["docs/plans/export.md:219 acceptance names --test export_cli", "no Files line in the plan names tests/export_cli.rs"], "plan_section": "### 4. Add the export verb [M]", "anchor_old": "`cargo test --test export_cli`", "anchor_new": "`cargo test --test export`"}
```

## cleanup — `change`

`old` is copied byte for byte from the anchor line, after re-reading it.

```json
{"kind": "cleanup", "lens": "doc-drift", "file": "src/cli/args.rs", "line": 77, "symbol": "ExportArgs::to", "summary": "Help text names the removed --ledger flag", "edit": "Replace `see --ledger` with `see --to` in the help string.", "direction": "change", "effort": "trivial", "old": "help = \"Ledger path; see --ledger\""}
```

## cleanup — `add`

An addition anchored to the line it goes after. When `target_file` names a file that does not exist yet, `old` is omitted.

```json
{"kind": "cleanup", "lens": "test-gaps", "file": "src/store/merge.rs", "line": 142, "symbol": "merge::apply_rows", "summary": "No test covers a batch whose length is a multiple of the chunk size", "edit": "Add a test applying a 64-row batch with CHUNK = 32 and asserting all 64 rows land.", "direction": "add", "effort": "small", "target_file": "tests/merge_chunks.rs"}
```

## cleanup — `remove`, one pattern at many sites

One row, every other site under `instances`.

```json
{"kind": "cleanup", "lens": "hygiene", "file": "src/report/mod.rs", "line": 3, "summary": "Unused `use std::fmt::Write` import in three report modules", "edit": "Delete the `use std::fmt::Write;` line in each listed file.", "direction": "remove", "effort": "trivial", "old": "use std::fmt::Write;", "instances": ["src/report/table.rs", "src/report/footer.rs"], "evidence": ["grep `fmt::Write` and `write!(` in each file: no use outside the import"]}
```

## advisory — `challenge`

A finding against a documented decision. `challenges` names the decision and where it is written.

```json
{"kind": "advisory", "lens": "architecture", "file": "src/store/io.rs", "line": 30, "symbol": "io::mutate_doc", "summary": "The single file lock serialises all agent writes; per-cell files would remove the contention", "angle": "challenge", "challenges": "docs/adr/0004-one-store-file.md: one store file per flow", "rationale": "Sixteen concurrent writers poll a 30 s try-lock; a wave's write time grows with the agent count. The ADR's reason (one sidecar to verify) still holds, so this is a trade, not a fault.", "adoption_cost": "A store-format change, a migration of existing stores and a new merge step in resolve.", "alternative": "One TOML file per cell, merged by resolve."}
```

## advisory — `probe`

A probe cell's answer that turns on a decision. `lens` is the cell's `probe:<slug>`.

```json
{"kind": "advisory", "lens": "probe:retry-idempotency", "file": "src/runner/retry.rs", "line": 88, "symbol": "retry::run_once", "summary": "Retried uploads are idempotent only while the server keeps request ids for 24 h", "angle": "probe", "rationale": "Question: is a retried upload safe to replay? The client reuses its request id and the server dedupes on it, so a replay within the server's retention is safe. Past 24 h a replay re-uploads; whether that matters is a product call.", "adoption_cost": "None to keep; a client-side ledger of sent ids if replays past 24 h must be safe."}
```

## A row naming a prior finding

`same_as` names a PRIOR row at the same file and symbol (or within 10 lines). The rest of the row is complete, because the verifier may strike the identity and the row then stands on its own.

```json
{"kind": "defect", "lens": "correctness", "file": "src/store/merge.rs", "line": 144, "symbol": "merge::apply_rows", "summary": "apply_rows still drops the trailing chunk after the bound was changed", "severity": "major", "failure_scenario": "The bound is now `i + CHUNK <= len`, which still skips the remainder for a 70-row batch with CHUNK = 32: rows 64..69 are dropped.", "fix": "Process the remainder after the loop, or loop while `i < len`.", "fix_shape": "edits", "effort": "small", "counter": "If F7's fix already handles the remainder in a second pass, this is the same row; it does not, see line 150.", "grade": "high", "evidence": ["src/store/merge.rs:144 `while i + CHUNK <= len`"], "same_as": "F7"}
```

## Coverage note

Not a row: the text staged as `payloads/<cell>.note.md` and passed with `review cover --note-file`, at most 600 characters.

```text
Read in full: src/store/merge.rs, src/store/io.rs. correctness: traced apply_rows for 0, 1, CHUNK and 2*CHUNK rows; empty batch returns early (merge.rs:131), fine. type-design: grep `String` ids in store/ — all wrapped in RowId. Ruled out: lock timeout on 0 s poll (io.rs:44 floors it at 1 s). Covered.
```

## Not a finding

This row would be refused (no `failure_scenario`, no `fix`) and belongs in the coverage note instead:

```text
"checked apply_rows for off-by-one at the chunk boundary — it is correct"
```
