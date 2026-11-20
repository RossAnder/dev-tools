Status: done — Open asks for a capture config, the fan-out window forbids repo writes, and ui-layout reads the Open answer

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/open-and-challenge.md", "claude/skills/plan-v2-engine/references/ui-layout.md"]

[[deviations]]
what = "The Open capture-config decision is recorded before asking, with class ux, door two-way, binds naming the brief, and decision text opening `UI to capture:` so later steps can find it in the human plan view"
why = "The detail says only 'record the answer as a decision'; recording before asking and applying the answer by Surface's table matches how Surface and ui-layout record every asked item, the brief is the one path that exists at Open, and no read verb selects a decision by meaning, so a fixed text prefix is the locator. Alternatives weighed: record after the answer (diverges from the Surface idiom), bind a task id (none exist at Open)"
means = "detail"

[[deviations]]
what = "The no-writes rule exempts the `plan run start` and `plan run finish` writes the step itself names"
why = "Those write plan.toml under .claude/flows/<slug>/ inside the window by design, and the check already excludes that directory; a rule saying 'nothing writes by any tool' without the exception would contradict the step's own procedure"
means = "detail"

[[deviations]]
what = "ui-layout section 1 covers a plan opened before Open asked: with no config and no Open decision, the step asks the question once and records it as Open does; a no answer judges every milestone not UI unless one adds a visible surface, which goes in the list as UI flagged against the Open answer"
why = "Elaboration of an already-approved plan reaches ui-layout with no Open decision to read; without a fallback the config question would never be asked there, which is the reportdesignkit case"
means = "detail"

[[checks]]
command = "grep -q 'ui-capture.toml' claude/skills/plan-v2-engine/references/open-and-challenge.md && grep -q 'No repo writes while a launch is open' claude/skills/plan-v2-engine/references/open-and-challenge.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `claude/skills/plan-v2-engine/references/open-and-challenge.md`: section 1, Open, gains a **Capture config** paragraph after the read-back. In a repo with no `.claude/ui-capture.toml` the orchestrator asks once through `AskUserQuestion` whether the project has a UI to capture, defaulting to what the repo's sources suggest, records the question as a surfaced `ux` decision opening `UI to capture:` through `plan decide`, and applies the answer by Surface's table. A yes leads ui-layout's config row; a repo with the config is not asked. Section 3, Running it, gains the sentence opening `No repo writes while a launch is open`: nothing writes in the repo between a snapshot and its check, by any tool, besides the step's own `plan run start` and `finish`; a correction waits in the scratchpad until the check passes.
- `claude/skills/plan-v2-engine/references/ui-layout.md`: section 1 reads the Open decision rather than asking again, and asks it once only for a plan opened before Open asked. Section 5's config row now fires on the Open decision's yes, in the first milestone judged UI, and asks nothing more about the config.

The dispatch tests (56) pass, skill markdown links included.
