Status: done — verifier prompt in agent-waves.md now carries the blocked-claim/unblock instruction and the panel-seat override.

- claude/skills/review-v2-engine/references/agent-waves.md: appended the blocked_on/unblock sentences to the verifier instruction line (before "Return the verdict-return object."); extended the Panel seat bullet to replace the unblock sentence with the panel-seat one.
- Deviations: none.
- Checks: `cargo test --lib -- cli::dispatch::tests` passed (54). The accept check (`reproduce-status ... --expect gone`) writes build: yes, so it is left to the gate.

TANGENTIAL: none
