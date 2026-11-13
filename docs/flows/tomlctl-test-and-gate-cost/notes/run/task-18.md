Status: failed — only field_flags.rs could drop its working directory; the at-most-two accept check cannot be met

```toml run-note
files_touched = ["tomlctl/tests/field_flags.rs"]

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^(field_flags|backlog_check_batch)::/)'"
result = "backlog_check_batch removal failed one test; reverted; field_flags passes"

[[checks]]
command = "grep -l '.current_dir(' <five files> | wc -l"
result = "4 (need at most 2)"
```

## What changed

- field_flags.rs: removed `.current_dir(&root)` from the tomlctl command (path was absolute); its tests pass.
- backlog_check_batch.rs: removing it and joining the path made `an_at_file_summary_is_refused_with_the_stdin_form` fail (the test depends on the relative `@notes.txt` form). Restored unchanged.
- plan_lifecycle.rs, plan_stage.rs, plan_probes.rs: the only `.current_dir(..)` there is on a `bash` command, which the detail says to leave.

## Why failed

Four files must keep a working directory, so the accept check (at most two) cannot be met within the stated rules. The spec needs re-speccing: either relax the accept bound to at most four, or allow the bash probe runs to drop the directory (the emitted scripts may rely on cwd for git), or let the backlog test change its assertion (which the constraints forbid).
