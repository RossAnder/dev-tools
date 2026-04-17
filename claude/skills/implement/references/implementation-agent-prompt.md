# Implementation Agent Prompt Contract

Every implementation sub-agent dispatched by the `implement` skill MUST
receive a prompt containing the following elements. The orchestrator (main
conversation) assembles these from Phase 1 analysis.

## Required prompt contents

- **Exact files to read and modify** — absolute paths, no ambiguity. No two
  parallel agents may touch the same file.
- **File read instructions**: "Read every file listed in your Files section
  in full before making changes. Also read any file you import from or export
  to, so you understand the integration surface."
- **Why the code is changing** — describe what the code should do after the
  change and the motivating reason. Do not just restate the edit.
- **Complex-task research findings**: for any task classified as *complex* in
  Phase 1, paste the relevant research findings, API decisions, and reasoning
  from the main conversation directly into the prompt. Sub-agents cannot
  extend-think; this is how they compensate.
- **Specific API signatures or patterns** to use, drawn from Context7
  research done in Phase 1.
- **Clear success criteria** — a concrete definition of "done".

## Tool guidance (include in every agent prompt, tailored to the task)

- **Context7**: "Use `mcp__context7__resolve-library-id` then
  `mcp__context7__query-docs` to verify API signatures, method parameters,
  and correct usage patterns before writing any code that uses framework or
  library APIs."
- **WebSearch**: "Use WebSearch if you encounter an unfamiliar pattern, need
  to check for deprecations, or are unsure about the correct approach for
  the framework version in use."
- **Codebase exploration**: "Read related files to understand existing
  patterns before writing new code. Match the style, naming, and structure
  of surrounding code."
- **Diagnostics**: "LSP diagnostics are reliable when you first open a file.
  However, after making edits, new diagnostics may be stale — do not
  automatically act on post-edit diagnostics. Re-read the flagged lines to
  verify the issue is real before fixing. For definitive verification, run
  a targeted build command (e.g. `cargo check -p crate_name`, `dotnet build
  path/to/Project.csproj`, `tsc --noEmit`) rather than relying on LSP. Leave
  full build and test runs to the verification agent."
- **Step-by-step reasoning**: "Reason through each change step by step
  before editing." (Compensates for lack of extended thinking.)

## Plan deviation protocol (paste verbatim into every agent prompt)

"If you discover that the plan's assumptions are wrong — a file doesn't
exist, an API has changed, an interface differs from what the plan
describes — do NOT silently improvise. Complete whatever changes you can
that are unaffected, then report the deviation clearly in your output:
what the plan assumed, what you found, and what was left undone. The
orchestrator will decide whether to adapt or abort."
