# tomlctl — `.claude/generated.toml` reference

A project's map of generated files: which inputs, when edited, change which outputs, how to rebuild
them and how to tell they are stale. The file is optional. A project without it has no generators,
and nothing below reports or lists anything for it.

## The file

```toml
[[generators]]
id = "tokens"
inputs = ["src/tokens.css", "src/tokens/**"]
outputs = ["src/tokens.gen.ts"]
regenerate = "bun run tokens"
check = "bun run tokens --check"
```

| Key | Required | Holds |
|---|---|---|
| `id` | yes | The generator's name, unique in the file. It names the `generated:<id>` source of a gate-list command and appears in findings. |
| `inputs` | yes | A non-empty array of globs over repo-relative paths. `*` does not cross `/`, a leading `./` is ignored and case is folded. A path a row holds that matches any glob is an input edit. |
| `outputs` | yes | A non-empty array of literal repo-relative paths, never globs, since a row's `files` holds paths and a grant has to name the exact path to add. |
| `regenerate` | yes | The command, run from the repo root, that rebuilds the outputs. The worker view prints it for a row that holds an input. |
| `check` | yes | The command that exits non-zero when the outputs are stale. It enters the milestone gate list. |

## What the loader enforces

- A missing file means no generators.
- Outputs must be literal: an `outputs` entry holding `*`, `?`, `[` or `{` is refused.
- `id` must be unique, and a key outside the five above is refused.
- Every key is required and non-blank, and each array non-empty.
- A malformed file is refused with an error naming the generator and the key. `plan check` turns that
  into one warning naming no row; `plan emit gate-list` refuses with it. `plan request add`, for a
  payload with paths, falls back to no generators and reports the problem in a `warnings` array of
  its output, so no output is granted mechanically and every path stays open for the orchestrator;
  the worker view prints one line naming the problem.

## The readers

- **`plan check`** reports `plan/generated-output-unheld` for a pending row, in a milestone that is
  not deferred, that holds an input but not every output of that generator. See the class table in
  [plan.md](plan.md#check-classes).
- **`plan emit gate-list`** appends each touched generator's `check` after the accept checks with
  `source = "generated:<id>"`, a generator being touched when any row of the milestone holds one of its
  inputs. A command already listed is not listed twice. See
  [plan-engine.md](plan-engine.md#plan-emit-gate-list).
- **The worker view** prints one line per generator whose input the row holds: its outputs and its
  `regenerate` command.
- **`plan request add`** grants a path that is a declared output of an input the task holds, without
  the user. See [plan-request.md](plan-request.md).
