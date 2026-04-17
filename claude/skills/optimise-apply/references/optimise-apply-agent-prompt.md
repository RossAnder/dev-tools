# Implementation Agent Contract

Each implementation sub-agent launched in Step 4 receives a prompt built
to this contract. The prompt is authored by the main conversation after
Step 2 pre-analysis, so agents execute rather than deliberate.

## Required prompt contents

Every agent prompt MUST include:

- **Exact files** the agent will read and modify — no wildcards, no
  "look around the codebase."
- **The findings to apply**, restricted to the agent's cluster. Include
  item number, severity, file:line, and the `Recommended` action from
  the findings report.
- **Pre-analysed reasoning** from Step 2 for any complex finding. If
  the main conversation already picked the implementation approach
  (e.g. which API variant, which data structure, which lock type),
  state it explicitly so the agent does not re-deliberate.
- **Instruction**: "Reason through each change step by step before
  editing."
- **Instruction**: "Use Context7 MCP tools (`resolve-library-id` then
  `query-docs`) to verify API signatures and correct usage for any new
  APIs before writing code."
- **Instruction**: "Use WebSearch if the recommended approach needs
  clarification or you are unsure about the correct implementation."

## Agent behaviour requirements

Every agent MUST:

- **Read the target file(s) in full** before making any changes.
- **Read surrounding code** to ensure changes are consistent with
  existing patterns, naming conventions, and formatting style.
- **Make the minimum change necessary** to address each finding — do
  not refactor surrounding code, do not rename unrelated symbols, do
  not tighten types that are not part of the finding.
- **Preserve existing code style**, naming conventions, and formatting.
- **Add a brief inline comment** only when the optimisation would be
  non-obvious to a reader — otherwise keep the code clean.

## Skip-and-report protocol

If a finding cannot be safely applied — it would break behaviour, has
unclear semantics, the research does not hold up on closer inspection,
or the code at the referenced line has drifted since `optimise` ran —
the agent **MUST skip it and report why**. Skipping is the correct
action when uncertain. Do not guess, do not paper over, do not apply a
"close enough" alternative. Return the skipped item number, file:line,
and a one-sentence explanation so the main conversation can include it
in the final `### Skipped` summary.
