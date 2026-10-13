+++
id = "format-contract"
status = "stable"
families = ["plan"]
executor = "surveyor"
fence = "FORMAT CONTRACT"
activation = "floor"
+++

# format-contract

## Runs

Decides whether the plan under review is in the house format, and if it is, points the pack at the plan-format contract. Take the plan path from your assignment. Run nothing but Grep, Glob and Read.

1. **House format.** Grep the plan for a line that is exactly `## Tasks` and a line that is exactly `## Dependency Graph`, allowing trailing spaces or a carriage return: the patterns `^## Tasks[ \t\r]*$` and `^## Dependency Graph[ \t\r]*$`. Note each line number. The plan is house-format only when both match. When either is missing, stop here: the plan is foreign-format.
2. **Locate the contract.** The contract is the `flow-contract-plan-output-format` skill's `SKILL.md`, which sits beside the catalogue. Take this check's `path` from the catalogue listing and remove its last three parts, `review-v2-lenses/checks/format-contract.md`; what remains is the skills directory. Add `flow-contract-plan-output-format/SKILL.md` to it. For example, the path `/home/u/.claude/skills/review-v2-lenses/checks/format-contract.md` gives `/home/u/.claude/skills/flow-contract-plan-output-format/SKILL.md`.
3. **Confirm it.** Grep that file for the line `**Format rules:**` and for the line `## Execution Policy`, and note both line numbers. If the file does not exist, or either line is missing, the contract is unavailable: keep the path you tried and what was missing.

Never copy the contract's text into the pack, and never edit the plan or the skill.

## Interpreting the result

- Both headings found and the contract confirmed: house-format, contract available.
- Both headings found but the contract file is missing or lacks either line: house-format, contract unavailable. Cell agents can still load the contract by invoking the `flow-contract-plan-output-format` skill.
- Either heading missing: foreign-format. The house rules do not bind this plan, and judging it by them is the error this check exists to prevent.

## Fence or finding mapping

Writes the `FORMAT CONTRACT` fence in `pack.md` and no findings. The fence holds one of three bodies.

House-format, contract available, four lines:

- `house-format: ## Tasks at line <n>, ## Dependency Graph at line <m>`
- `contract: <the SKILL.md path from step 2, exactly as resolved>`
- `read: the "**Format rules:**" block from line <r> to the end of the file, and the "## Execution Policy" template from line <p> to the next "## " heading`
- `these are the only rules for section order, task sizing, the per-task file cap, parallelism and the Execution Policy fields`

House-format, contract unavailable, two lines:

- `house-format: ## Tasks at line <n>, ## Dependency Graph at line <m>`
- `contract unavailable — <path tried>: <what was missing>; invoke the flow-contract-plan-output-format skill and read its Format rules`

Foreign-format, one line:

- `foreign-format — no "<the missing heading or headings>" heading; no house-format contract applies to this plan`

A cell agent that reads `foreign-format` treats the fence as absent: it judges only whether an executor could act on the plan, and never reports a missing house section.

In your digest, the `checks:` line for `format-contract` is `ran` in all three cases; it is `not run — <reason>` only when the plan path is missing from your assignment or the plan cannot be read.
