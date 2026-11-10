Status: done — schedule-rule pointer re-pointed to review-apply.md and the listing fence closed on its own line.

Changed claude/skills/apply-v2-engine/SKILL.md:
- The schedule-rule step now names `references/review-apply.md`. Other pointers (`review stats`, `review show`, `review list`) are not file references and stay as they were.
- The closing fence before "The engine's report is the run's outcome; ..." is now its own line, then a blank line, then the sentence.

Deviations: none.

Checks:
- grep for references/review-apply.md: pass.
- no fence sharing a line with prose: pass.
- command_lint cargo test (Writes build: yes): left to the gate.

TANGENTIAL: none
