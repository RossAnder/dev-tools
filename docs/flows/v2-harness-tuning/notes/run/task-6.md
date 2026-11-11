Status: done — role rule now names the step 2 unfinished-plan-work question as its one exception

- claude/skills/apply-v2-engine/SKILL.md: the `**Triage decided the selection.**` bullet gains "The one exception is the question about unfinished plan work that step 2 asks." and "Every other condition ..." replaces "Every condition ...". Step 2 untouched.
- Deviations: none.
- Checks: accept grep passes; `cargo test --lib -- cli::dispatch::tests` 54 passed.

TANGENTIAL: none
