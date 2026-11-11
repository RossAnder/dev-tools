Status: done — both plan workers check their own after: premises when told to, return needs_files, and pick accept checks by the Run by line

## What changed

The edit is inside the `plan-worker-procedure` shared block, made identically in both files:

- `claude/agents/plan-worker-deep.md` and `claude/agents/plan-worker-lite.md`:
  - "What your prompt names" gains the `after-premises: check yours before editing` line, present only when the task was released by an `after:` premise inside the launch.
  - Procedure gains a new step 5, between the read-only file check and marking the row `in-progress`. It applies when the prompt carries that line. The worker makes a `mktemp` file, writes `tomlctl plan probes --slug <slug> --emit --premises --task <n>` stdout to it, reads the script and runs it with `timeout 300 bash <file>`, recording nothing. On a non-zero premise it edits nothing, writes the note naming the premise and its output, sets the row `failed` and returns with `needs_files` empty. A row already `in-progress` on entry is a restart and skips the step. The later steps are renumbered 6–11, and step 5's cross-reference names step 10, which holds the status command.
  - The work step (now 7) says that a task which cannot be completed inside its `files` is `failed`, that the other file is left unedited, and that `needs_files` lists each repo-relative path, with one line per path in the note saying why.
  - The `failed` bullet adds a false `after:` premise and the `needs_files` naming.
  - Step 2 says that both the `Writes build:` and `Run by:` lines are in the view.
  - Checks: the worker runs accept checks marked `Run by: worker` and never one marked `Run by: gate`, including one with a `(covered by …)` tail. If the view prints no `Run by:` line, the worker falls back to the old rule: `Writes build: no` is the worker's and `yes` is the gate's.
  - Return: the object is `{task, status, note, files_touched, deviations, handover, needs_files}`, with `needs_files` described as in `design/shapes.md#worker-return`.

## Deviations

None. The detail's wording was turned into procedure steps. The decision to place the premise check after step 4 (the read-only `git status`/`git diff`) and before the `in-progress` mark is within the detail, which requires only that it come before that mark.

## Handover

None.

## Checks

- `bash scripts/verify-shared-blocks.sh`: `shared-block parity: OK`.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 53 passed, 1 failed. The failure is `lint::workflow_scripts_name_real_agents`, and its diagnostic names only `claude/workflows/implement-v2-milestone.js` (`agentType value WORKERS[t.executor].agentType`), a siblings file that another worker is editing. It does not touch these files.
- Accept "both workers return needs_files": pass.
- Accept "both workers check their after: premises when told to": pass.
- Accept "both workers pick their checks by the Run by line": pass.
- No compile check: only Markdown was edited.

TANGENTIAL: question | claude/workflows/implement-v2-milestone.js | the workflow_scripts_name_real_agents lint rejects an indexed agentType expression (`WORKERS[t.executor].agentType`) that the in-flight Workflow edit introduces | if the edit lands as is, either the script or the lint has to change before the milestone gate passes
