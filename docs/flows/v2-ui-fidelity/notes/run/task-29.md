Status: done — intent-fidelity lens, its two concern tags and the flow-bound compose rule are in place

```toml run-note
files_touched = [
  "claude/skills/review-v2-lenses/lenses/intent-fidelity.md",
  "claude/skills/review-v2-lenses/vocabulary.toml",
  "claude/skills/review-v2-engine/references/compose.md",
]

[[checks]]
command = "grep -q 'brief.md' claude/skills/review-v2-lenses/lenses/intent-fidelity.md && grep -q 'user-stated' claude/skills/review-v2-lenses/lenses/intent-fidelity.md && grep -q 'intent-fidelity' claude/skills/review-v2-engine/references/compose.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"

[[checks]]
command = "tomlctl review catalogue --check --builtin claude/skills/review-v2-lenses"
result = "fail"
```

## What changed

- `claude/skills/review-v2-lenses/vocabulary.toml`: a `# intent-fidelity` group after `# interaction`, with
  `criteria-fidelity` and `decision-fidelity`, both code family only. CRLF kept.
- `claude/skills/review-v2-lenses/lenses/intent-fidelity.md` (new): opt-in, `slice = "whole-scope"`,
  `v1_category = "completeness"`, code family, owns both tags, `methods = ["trace"]`. Its probes read
  `docs/flows/<slug>/brief.md` (Intent, Non-goals, Success criteria) and the user decisions through
  `tomlctl items list .claude/flows/<slug>/plan.toml --array decision --where-in provenance=user-stated,user-confirmed`
  (run against this flow to confirm it works). They follow each decision's `binds` to task files and commits through `tasks show --select id,title,files,commit`.
  A divergence in code is a defect anchored on the code. A sign that a decision is now wrong is an advisory
  (`challenge`, or `probe` for a decision open to two readings) and never an edit. Does not flag lists
  agent-assumed decisions (contrarian's decision-challenges), plan-time goal-coverage (plan-structure), and
  look and interaction defects (ui-visual, interaction).
- `claude/skills/review-v2-engine/references/compose.md` section 2: a new bullet that adds `--lens +intent-fidelity` on
  every code-family run bound to a flow with `plan.toml`. A user's `--lens -intent-fidelity` wins, and the rule never
  fires on a `--scope` run, a perf or plan-family run, or a flow without `plan.toml`.

## Checks

- Accept check: pass.
- `cli::dispatch::tests`: `catalogue_validates` passed with the new lens and tags, and so did `command_lint`, which covers the
  lens's fenced `tomlctl` lines. 54 passed. 2 failed in `finding_classes`, and both name only `tomlctl/src/plan/schema.rs`
  (`no severity resolves for class`). That file is outside this task and is another task's in-flight edit.
- The installed `tomlctl` and `target/debug/tomlctl.exe` both predate the `ui_capture_paths` detector key, which another
  task added to `vocabulary.toml`. Their `review catalogue --check` refuses that key and reports nothing about this
  task's entries. The source-built `catalogue_validates` above is the check that applies.
