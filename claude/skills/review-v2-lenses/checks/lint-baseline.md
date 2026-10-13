+++
id = "lint-baseline"
status = "stable"
families = ["code"]
executor = "verification"
fence = "LINT BASELINE"
activation = "floor"
discover = [
  "plan: the bound flow plan's `## Verification Commands` `lint:` key, run as written",
  "claude-md: the lint or type-check command the repo-root CLAUDE.md declares under `Build & test`, or a CLAUDE.md inside the scope's directories; a declared command beats every default below",
  "lang:rust: for each stack root meeting the scope, `cargo clippy --manifest-path <root>/Cargo.toml --all-targets`",
  "lang:ts: for each stack root meeting the scope, the package's `lint` script, else its `type-check` or `typecheck` script, run as `<pm> run <script>` with the package manager its lockfile names (bun.lock or bun.lockb → bun, pnpm-lock.yaml → pnpm, yarn.lock → yarn, otherwise npm); with no script, `npx tsc --noEmit --pretty false -p <root>/tsconfig.json`, or `vue-tsc` in place of `tsc` when the root also carries framework:vue",
  "lang:csharp: for each stack root meeting the scope, `dotnet build <solution or project> --no-incremental --tl:off`, naming the `.sln` or `.slnx` at or above the root when one exists, otherwise the root's `.csproj`",
  "none — no lint or type-check is declared and no stack signal with a default meets the scope",
]
+++

# Lint baseline

## Runs

The project's lint or type-check, once per run, against the tree as it stands, so no finder re-reports what the linter already says and no cell spends a build lock reproducing it.

The surveyor walks `discover` in order and stops at the first rung that yields a command. It runs nothing. The ecosystem rungs yield one command per stack root that meets the scope, so a repo with a Rust crate and a TypeScript SPA in scope resolves two; a root outside the scope resolves none. The digest lists each command on its own `verification:` line, keyed `lint-baseline` for the first and `lint-baseline-2`, `lint-baseline-3` and so on for the rest, in ladder order. The orchestrator dispatches the `verification` agent once per line, one at a time, and records each returned log path under that key in the run's `baselines`.

The resolved command is never altered to fail harder or to fix:

- no `--fix`, `cargo clippy --fix` or `dotnet format` without `--verify-no-changes`, because each writes the working tree;
- no `-D warnings`, `-warnaserror` or `--max-warnings 0`, because an error stops dependent crates and projects from compiling and hides their diagnostics;
- a declared command keeps the project's own strictness, so a project whose `TreatWarningsAsErrors` or `#![deny]` turns the baseline red stays red.

`dotnet build` runs with `--no-incremental` because an up-to-date project skips its compile step and its warnings are not emitted again. `cargo clippy` replays cached diagnostics, and `tsc` reports every file each run, so neither needs a forced rebuild.

## Interpreting the result

The log is the full output of one command. A finder reads only the diagnostics that name a file in its slice.

- **Matching a file.** Tools print paths relative to the directory they ran in, to the crate or workspace root, or as absolute paths. Match a diagnostic to a scope file by path suffix, not by exact string, and normalise `\` to `/` first.
- **Rust (clippy, rustc).** A diagnostic opens with `warning:` or `error` and a code, and its location is the next ` --> <path>:<line>:<col>` line. The block runs to the next blank line. The summary lines `warning: \`<crate>\` (lib) generated N warnings` name no file and are not diagnostics.
- **TypeScript (tsc, vue-tsc with `--pretty false`).** One line per diagnostic: `<path>(<line>,<col>): error TS<code>: <message>`.
- **ESLint, default formatter.** A line holding only a file path opens a block. Each indented `<line>:<col>  error|warning  <message>  <rule>` line under it belongs to that file, up to the next blank line.
- **.NET (dotnet build).** One line per diagnostic: `<path>(<line>,<col>): warning|error <code>: <message> [<project>]`. MSBuild prints each diagnostic again in its closing summary, so count a repeated line once.

A red baseline is information, not a halt. A diagnostic in the slice is already known. A run whose log shows the command never reached compilation (a missing toolchain, a failed restore, an unresolved manifest) has no diagnostics to subtract, so a finder treats the slice as unlinted rather than clean.

## Fence or finding mapping

`pack.md` carries a `LINT BASELINE` fence holding only `see the run's recorded baselines`. `review show <cell> --for finder` carries the recorded log paths, and a key recorded as `none — <reason>` means no lint ran, which a finder must not read as a clean lint.

This check writes no findings. A diagnostic in the log for a file in the slice is never a finding of any kind, and a verifier refutes a finding that only restates one. A finding may still cite a diagnostic as evidence for a defect the linter does not itself name, such as the unhandled state behind an `unused_must_use` warning, stating the failure the diagnostic omits.
