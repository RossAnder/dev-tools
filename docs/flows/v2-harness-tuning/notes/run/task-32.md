Status: done — a node:test harness now runs the three Workflow scripts under stubbed globals, and a dispatch-gate Rust test runs it under node.

## What changed

- `claude/workflows/tests/harness.test.mjs` (new). It loads `review-v2.js`, `plan-v2-review.js` and `implement-v2-milestone.js` by rewriting `export const meta` to `const meta` and building an AsyncFunction over the globals `args, agent, parallel, pipeline, phase, log, workflow`. The loader asserts that no other `export` remains. The header comment states the stub semantics. `agent` returns a canned object picked by `agentType` or `label`, or null; a responder that throws makes the call reject. Its deferred mode returns a promise the test settles by hand and records each call when it is made. `parallel` and `pipeline` turn a throw into null and never reject. `workflow` runs the loaded child with the same stubs and throws on an unknown name. `phase` and `log` record. There are 17 cases:
  - review-v2: the agent count of a wave with a cell, a panel, a minor batch and a checker pool with one reroute (7); the 300-character escalation cap; `batch_defects` sending two singles to one batch while the `same_as` row stays single, compared with the unbatched run; reproduce mode sending checker units to one checker batch and the unit with an unsettled finding to a verifier (two cases); a null cell agent giving a `no-return` row, a thrown one giving the `status: 'error'` row, and a null reproduce verifier.
  - plan-v2-review: baseline key matching, where an unknown key is an error, a missing key falls back to `none — the verification agent returned no log`, and a `none` line passes through; the agent total adding the child pipeline's `agents` (5 + 2); reverify skipping an id already unverified; a `reverify_single` id dispatched on its own while the rest share one batch; an id the batch omits counted as unverified.
  - implement-v2-milestone (deferred agent): a short task starting while a long file-disjoint task is in flight; an `after` task starting once its target is done; the same task blocked when its target fails; two tasks sharing a file, spelled with a different case and separator, never in flight together; `needs_files` reaching the index; and the `workflow` stub throwing on an unknown name.
- `tomlctl/src/cli/dispatch/tests/lint.rs`: added `workflow_scripts_logic_passes_under_node` beside `command_lint_workflow_scripts`. It runs `node --test claude/workflows/tests/` from the repo root. It panics, naming node, when node cannot start, and asserts on a non-zero exit, printing stdout and stderr. As in the sibling tests, it skips only when the `claude/` directory itself is absent. A missing node never skips.

## Deviations

None from a `suggested` means; both `required` means are met. The detail's single case "reproduce mode sending a checker unit to the checker and its unsettled finding to a verifier" is covered by two tests: a three-unit mix and a minimal one-unit case.

## Handover

None. Decision D3 already records the open question: the Windows machine needs node on its Git Bash PATH, or this test fails there.

## Checks

- `node --test claude/workflows/tests/`: 17 pass, 0 fail (node v26.13.0).
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check tomlctl/src/cli/dispatch/tests/lint.rs`: clean.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- workflow_scripts_logic_passes_under_node`: 1 passed. This is the task's own narrow test.
- Accept check "the harness loads all three scripts": pass.
- Accept check "a Rust test in the dispatch gate runs the harness under node": pass.
- Accept check "the harness carries the pool, after-release, file-exclusion, reproduce-checker and batch_defects cases": pass.

TANGENTIAL: none
