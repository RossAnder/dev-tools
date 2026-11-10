Status: done — fenced reproduce commands now spell the store as `--slug <slug>`.

- claude/skills/implement-v2-engine/references/findings.md: the two fenced commands using `<store flag>` (`review reproduce-status`, `review reproduce`) now use `--slug <slug>`; one sentence after the first fence says a scope store takes `--scope <scope>` in its place. Inline prose mentions and the defining paragraph are unchanged.
- Compile check: not needed (markdown only).
- Accept 1 (grep for fenced `<store flag>`): passes.
- Accept 2 (command_lint test, run although Writes build is yes): passes, 6 tests.

TANGENTIAL: none
