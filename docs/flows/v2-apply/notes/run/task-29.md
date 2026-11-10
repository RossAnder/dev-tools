Status: done — the worker view's findings block no longer trips the plan finding-class gate's line scan

## What changed

- `tomlctl/src/plan/view.rs` (`findings_block`): the gate's `unresolved_sites` flags any line under `src/plan` containing `Finding {` that has no `class:` field within eight lines. The trip was not the `Some(finding)` arm the task suggested but the `None` arm's format string `"_Finding {} ({role}) is not in {}._\n\n"`. That arm now pushes `"_Finding "` and then formats `"{} ({role}) is not in {}._\n\n"`, so the output bytes are unchanged and no source line contains `Finding {`. A two-line comment says why the push is split, so nobody merges it back. After the edit, `grep -rn "Finding {" tomlctl/src/plan/` lists only real construction sites in `check.rs`, plus the helper in `stage.rs`.

## Deviations

- The constraint gave the `Some(finding)` binding as the example of what to change. Reading the parser (`unresolved_sites`, case-sensitive `contains("Finding {")`) showed that `Some(finding) => {` does not match. The match is the `None` arm's string literal, which is also near the findings block. I weighed three ways to fix it: a `\x20` escape, which is obscure; joining the noun in as an argument, which reads awkwardly; and splitting the push. I chose the split push as the plainest.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/plan/view.rs`: clean. view.rs declares no `mod` children.
- `cargo clippy --manifest-path tomlctl/Cargo.toml`: clean.
- I did not run either accept check. Both have `Writes build: yes` and belong to the gate. The output is byte-identical by construction, because the two pushes concatenate to the same string as before.

TANGENTIAL: none
