Status: done — plan-flow.md now names the omitted grammar row `apply`.

Changed: claude/skills/review-v2-engine/references/plan-flow.md, grammar sentence: "without `export`" became "without `apply`". triage-and-close.md confirms the row is `apply` (`apply F1,F4` / `apply all`).
The only other `export` mention (line 40, "the plan family never exports to one", about v1 ledgers beside `review sync-v1`) is not the disposition, so it is left alone.

Deviations: none. Handover: none.

Checks: compile check not applicable (markdown). Forward accept check (no 'without `export`' match) passes. Guard check (cargo test cli::dispatch::tests) writes build, so it is left to the gate.

TANGENTIAL: none
