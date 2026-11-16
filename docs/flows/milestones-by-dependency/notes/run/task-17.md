Status: done — apply-v2 orders apply-<n> after settle-<n> with --after and records the unfinished-work choice the same way

```toml run-note
files_touched = ["claude/skills/apply-v2-engine/SKILL.md"]

[[checks]]
command = "grep -qF -- '--after settle-<n>' claude/skills/apply-v2-engine/SKILL.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/apply-v2-engine/SKILL.md`: the `apply-<n>` add in the settle section gains `--after settle-<n>`, with a sentence explaining the edge and that the settle drain names the fix milestone by its id pair. Step 2's unfinished-work question now offers: run plan work first (adds `--after <id>` per unfinished milestone to the fix milestone and to `settle-<n>`), let fixes run as soon as they can, or stop. Step 9's paragraph now says the engine runs milestones in its schedule's order.

The installed `tomlctl plan milestone add --help` did not show `--after` in the part I read, since the flag belongs to a sibling task.
