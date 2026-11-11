Status: done — Unstamped milestones paragraph now matches the re-entry routing in drain.md

Changed: claude/skills/implement-v2-engine/SKILL.md, Re-entry section, Unstamped milestones paragraph only. The old "stamp it from HEAD or run its gate again" wording is replaced by a summary that defers to drain.md "Stamping on re-entry". Stamping from HEAD also runs the findings reconcile for a milestone holding an apply row. Otherwise an apply milestone goes back through the reproduce check and the settle drain before its gate, and a milestone with no apply row reruns the gate. The sentence about counting fix rounds already spent is kept.

Deviations: none.

Checks:
- Accept check 1 (old wording gone): passes.
- Accept check 2 (paragraph names the reproduce check): passes.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed.

TANGENTIAL: none
