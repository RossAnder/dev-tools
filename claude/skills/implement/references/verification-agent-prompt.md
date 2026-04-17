# Verification Agent Prompt Contract

After all implementation batches complete, the `implement` skill launches a
single verification sub-agent. Its purpose is to run build and tests in a
child context so verbose compiler and test-runner output never pollutes the
orchestrator's main context.

## Inputs the orchestrator must pass

- **Pre-extracted commands from the plan.** If Phase 1 found a
  `## Verification Commands` section in the plan file, the orchestrator
  extracts the build, test, and lint commands there and passes them
  directly to the verification agent. The agent MUST use these commands
  and MUST NOT re-discover them.
- **Project context** — the plan path (if any) and the list of files
  changed in this run, so the agent can scope test selection if relevant.

## Discovery fallback

If no commands were provided from the plan, the verification agent
determines the project's build and test commands by checking, in order:

1. **CLAUDE.md** in the project root — most repos document the canonical
   build/test/lint commands here.
2. **Project root manifests** — `Cargo.toml`, `package.json`, `*.sln`,
   `Makefile`, `pyproject.toml`, `go.mod`, etc.

If the correct commands remain ambiguous after both checks, the agent must
ask the orchestrator rather than guess.

## Execution contract

The verification agent MUST:

1. Run the appropriate **build** command(s) for the project.
2. Run the appropriate **test** command(s) for the project.
3. If anything fails, report the **specific errors** with file paths and
   line numbers.
4. Return a **concise summary** — pass/fail counts, the first few errors
   with locations — NOT the full build or test output. The orchestrator
   does not want raw compiler spew in its context.

## Retry budget (enforced by the orchestrator, not the agent)

If verification fails, the orchestrator diagnoses using extended thinking
and either fixes directly or launches a targeted fix agent, then re-runs
verification. **Maximum 2 fix-and-reverify cycles for the entire
verification phase.** After 2 failed cycles, the orchestrator reports the
remaining failures and suggests the user investigate manually or update
the plan. The verification agent itself does not retry — it reports,
returns, and is re-invoked by the orchestrator if a fix was applied.
