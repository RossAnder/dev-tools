Status: done — fenced review show/settle/schedule commands in findings.md now use `--slug <slug>`

- claude/skills/implement-v2-engine/references/findings.md: replaced `<store flag>` with `--slug <slug>` in the five fenced commands (review show, three review settle, review schedule). The schedule line was also fenced and carried the placeholder, so it was changed too. The note at line 27 already covers `--scope`; no note added. Prose mentions untouched.
- Checks: grep accept check passes; command_lint test passes (6 passed). Compile check not applicable (markdown only).

TANGENTIAL: none
