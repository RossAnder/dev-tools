Status: done — added `apply` to the v2 review verb list in the tomlctl SKILL quick-reference table

- `claude/skills/tomlctl/SKILL.md`: the review row now reads `triage\|settle\|apply\|stats`.

Deviations: none.

Checks:
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed.
- Accept check (`tomlctl review reproduce-status F88 ... --expect gone`) has `Writes build: yes`, so it is left to the gate.

TANGENTIAL: none
