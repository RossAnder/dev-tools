Status: done — both plan workers are granted SendMessage and carry a shared "Asking the user" procedure, with their standing rules brought into line

```toml run-note
files_touched = ["claude/agents/plan-worker-deep.md", "claude/agents/plan-worker-lite.md"]

[[deviations]]
what = "The placeholder in the question id is written `<input-id>`, not `<input id>`, in the SendMessage line and the wait and read commands"
why = "Project CLAUDE.md requires single-token placeholders in tomlctl commands in agent files, and the command_lint gate rejects `<input id>` as an unknown argument (it does so today in the sibling's questions.md). `QUESTION <input-id> task <n>` still opens `QUESTION <`, so the C4 contract check and the accept check match"
means = "Detail's literal spelling `QUESTION <input id> task <n>`"

[[deviations]]
what = "plan-worker-deep.md's Judgement gains a line that a choice which is neither a compromise nor hard to unwind and changes nothing a user sees stays the worker's, recorded as a deviation so the milestone report lists it; both descriptions and the lite intro mention asking"
why = "D9 says such choices stay the deep worker's and are listed in the milestone report; the worker side of that listing is the note's deviations, so the Judgement section names it. The frontmatter descriptions otherwise contradicted the new procedure"
means = "Detail lists the rule edits; these two are consistency edits it does not name"

[[tangential]]
kind = "bug"
area = "claude/skills/implement-v2-engine/references/questions.md"
summary = "command_lint fails on two tomlctl lines there that use the multi-token placeholder `<input id>` (an `inputs list --where id=<input id>` and a `plan decide --ndjson ...-<input id>.toml`)"
why = "cli::dispatch::tests fails the milestone gate until the placeholder is spelled `<input-id>`; the worker side now uses `<input-id>`, so the two files should agree"

[[checks]]
command = "grep -q '^tools:.*SendMessage' claude/agents/plan-worker-deep.md && grep -q '^tools:.*SendMessage' claude/agents/plan-worker-lite.md && grep -q 'QUESTION <' claude/agents/plan-worker-deep.md && grep -q 'QUESTION <' claude/agents/plan-worker-lite.md"
result = "pass"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

Both `claude/agents/plan-worker-deep.md` and `claude/agents/plan-worker-lite.md`:

- `tools:` now includes `SendMessage`.
- The plan-worker-procedure block (byte-identical in both, parity verified) gains an **Asking the user** section. It covers when to ask (a compromise or a hard-to-unwind choice the spec leaves open), at most one question per run with related choices batched together, and parking on any such choice met after that one question. The four steps are: `tomlctl inputs add` a `question` (author implement-v2, ledger plan, flow slug, items the task id, a self-contained prompt, choice single, two to four costed options); load SendMessage through ToolSearch if needed and send one message to `main` whose first line is `QUESTION <input-id> task <n>`; one bounded Bash wait (timeout 600000, a 590 s `until` loop testing `--get id` for non-empty output); then one re-test that picks the branch: read `picked` from the answer and continue, or park with the question id and options in the note.
- Procedure step 7 points to that section. `parked` now also covers an unanswered question. "Tasks that answer review findings" says a compromise or hard-to-unwind choice on a finding-fix task is asked, and an unanswered one parks the row. Checks says a `Run by: look` entry is never the worker's to run.
- The plan-worker-forbidden-ops block (byte-identical) allows exactly one `tomlctl inputs add` question record per run besides the row update, and no other `inputs` write.

plan-worker-deep.md only: "Divergent behaviour is a person's call" now asks first and parks only when the question goes unanswered. A new Judgement line says choices that are neither compromises nor hard to unwind stay the worker's and are recorded as deviations. The description mentions asking.

plan-worker-lite.md only: the description and intro mention asking. The first item under "When to return failed" excludes a compromise or hard-to-unwind choice, which is asked instead and parks the task when it goes unanswered.

## Checks

The accept check and the shared-block verifier pass. `cli::dispatch::tests` ran because `claude/**/*.md` files were edited: 53 passed and 3 failed. All three failures name only sibling files. `command_lint` flags two lines in `claude/skills/implement-v2-engine/references/questions.md`. The two `finding_classes` failures name `tomlctl/src/plan/schema.rs`. Neither agent file is flagged. I did not check the deployed copies under the Claude config dir; the edits take effect only once they are linked there.
