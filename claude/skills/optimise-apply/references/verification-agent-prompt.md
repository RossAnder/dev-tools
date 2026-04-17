# Verification Agent Contract

After all implementation agents complete, Step 5 launches one
verification sub-agent. Its job is to run the project's build and test
commands and return a pass/fail summary — keeping verbose build/test
output out of the main conversation context.

## Required behaviour

The verification agent MUST:

- **Determine the project's build and test commands** by checking, in
  order: (a) `CLAUDE.md` for documented commands, (b) project root
  files (e.g. `Cargo.toml`, `package.json`, `*.sln`, `Makefile`,
  `pyproject.toml`). If ambiguous, ask the user before running
  anything.
- **Run the appropriate build command(s)** for the changed files. If
  the project uses a workspace or multi-crate layout, build only the
  affected packages when possible.
- **Run relevant tests** — focused on the touched modules first,
  broadening if the changes cross-cut.
- **Report the specific errors with file paths and line numbers** if
  builds or tests fail. Do not dump full logs into the response.
- **Return a concise pass/fail summary**, not the full output. Aim for
  a few lines per command.

## Concurrency-specific extra checks

For findings that modified concurrency primitives, synchronisation,
task spawning, or async call paths, the agent MUST additionally verify:

- **Synchronisation primitives are appropriate** for the access pattern
  and runtime — async-aware vs blocking locks, read-write vs exclusive,
  and locks must not be held across `.await` points.
- **Spawned tasks are bounded or tracked** — either via a task tracker,
  a join set, or a structured cancellation token. Unbounded
  `tokio::spawn` loops are a regression.
- **Channel and queue capacity choices are intentional** and documented
  with rationale. Unbounded channels must be justified.
- **Cancellation safety** is preserved — futures that hold locks or
  partial state across `.await` must remain cancel-safe, and any new
  `select!` arms must not drop in-flight work silently.
- **Shared state access patterns** still hold their invariants after
  the edit — no new races introduced by changed lock scope.

If any concurrency check fails, report it with the same specificity as
a build/test failure: file, line, and the concrete problem.
