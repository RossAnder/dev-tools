Status: done — `/apply-v2` and the `apply-v2-engine` skill exist, the carrier gate requires the skill, and both link scripts link them.

## What changed

- `claude/commands/apply-v2.md` (new): thin carrier modelled on `implement-v2.md`. Bindings (`<slug>` or `--scope <scope>`, then `<finding ids>` or `--all`), a pre-flight (the `tomlctl` skill; the six milestone-A features plus the implement-v2 engine's features; `AskUserQuestion`; `Workflow`), and a Run section that invokes `apply-v2-engine`, then `commit-conventions` at each commit and `backlog-capture` at the engine's tangential-discoveries step.
- `claude/skills/apply-v2-engine/SKILL.md` (new): role, rules, arguments, a ten-step table and its procedure: resolve the home (an approved/running/done v2 flow; a non-approved plan stops; a v1 flow, detected as `plan stage` refusing for a missing `plan.toml` while `flow list` lists the slug, is refused with the reason named; a flow-less store gets `<scope>-apply[-<n>]`), preview, gate resolution from the latest gated milestone (`plan gate --rows stops --lines`) plus the `CLAUDE.md` commands, `plan open` with the four brief sections filled, `plan milestone add` (`apply-<n>`, `--reason` on an approved flow), `review schedule`, the new-flow commit with the active-flow pre-check, `plan check` then `plan approve --review skipped-flag`, the status-transition record and the approval commit, invoking `implement-v2-engine` with the flow slug, and `review stats --apply` as the close. Plus Re-entry and Out of scope. One fix milestone, no settle milestone. It dispatches no agent.
- `tomlctl/src/cli/dispatch/tests/skills.rs`: `carrier_invokes_required_skills` gains `("apply-v2.md", ["tomlctl", "apply-v2-engine", "commit-conventions", "backlog-capture"])`.
- `scripts/link-v2-review.sh`, `scripts/link-v2-review.ps1`: `commands/apply-v2.md` and `skills/apply-v2-engine` entries beside their implement-v2 counterparts.

## Deviations

- **Preview without `review schedule --dry-run`.** The design's step 3 previews with `--dry-run`, but `ReviewScheduleArgs` (`tomlctl/src/cli/types/review/export.rs`) has no such flag and the constraint says every invocation must parse. The preview reads the selection with `review show --for orchestrator|implementer` (or `review list --status open --kind ...` for `--all`) and applies schedule's documented rule. I considered and rejected two alternatives: scheduling first and adding the milestone after, which `--checkpoint` refuses because the group must already exist; and dropping the preview, which risks a milestone with no rows. A residual case remains: when schedule refuses everything the preview passed, the milestone stays empty, and Re-entry reuses it.
- **The dirty-file check moved earlier.** The design refuses to go on while a scheduled task's files are dirty. The skill checks the findings' edited paths during the preview, before any write, so no minted rows are left behind.
- **The carrier pre-flight is wider than milestone A's features.** It also carries `/implement-v2`'s engine features, `AskUserQuestion` and `Workflow`, because the skill hands over to `implement-v2-engine`, which relies on its carrier having checked them.
- **The approval is gated on `plan check`, not on `plan stage` reading `ready`.** Schedule-minted rows with build-free accept checks have no probes. That is a warning in `plan check` but puts `plan stage` at `prove`, and `plan approve` does not refuse it.
- **A status-transition record and the render.** The approve step records these as `plan-v2-engine`'s approve step 6 does.
- **The carrier gate's required list.** It omits `implement-v2-engine`, because the skill invokes it, not the carrier.

## Handover

After landing, run `cargo install --path tomlctl`, since the installed binary is 0.22.0 and lacks `review schedule` and `review stats --apply`. Then run `scripts/link-v2-review.sh` (or `.ps1`) so `/apply-v2` and `apply-v2-engine` are live. You can tell it is done when `tomlctl --version` reads 0.23.0 and the link script reports `skipped` or `made` for both new entries.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/cli/dispatch/tests/skills.rs`: clean (a leaf file).
- Compile check: not run. The Rust edit adds one tuple to a `&[(&str, &[&str])]` literal, which I reasoned compiles.
- I checked every `tomlctl` line in the new files' bash fences against the clap types in source (`cli/types/review/{export,read}.rs`, `cli/types/plan.rs`), or copied it from already-linted skills. The installed binary predates the new verbs, so the source is the authority, and `command_lint` at the gate parses against it.
- Accept checks, writes build `no`: the carrier invokes the skill — pass; the skill names a v1 flow — pass; the link scripts name both — pass (`link-v2-review.sh --dry-run` lists both as `would`); the gate names `"apply-v2.md"` — pass.
- Accept check `cargo test ... cli::dispatch::tests`: writes build `yes`, so it is the gate's to run and I did not run it.

TANGENTIAL: question | claude/skills/apply-v2-engine/SKILL.md | On a reviewed flow with an unfinished earlier milestone, invoking implement-v2-engine with the slug runs that plan work before the fix milestone, and `--milestone` is refused while an earlier one is unfinished | /apply-v2 can run plan work the user did not ask for; the skill only announces it, so a person may want it to stop instead
TANGENTIAL: debt | tomlctl/src/cli/types/review/export.rs | `review schedule` has no `--dry-run`, though the design calls for one to preview before a milestone is added | without it the preview approximates the selection rule in prose and can drift from the verb
