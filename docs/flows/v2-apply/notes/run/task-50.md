Status: done — the triage apply step previews picks through `review schedule --dry-run` instead of a hand-copied rule, and `--all` is described as spanning both families

## What changed

- `claude/skills/review-v2-engine/references/triage-and-close.md`, step 3 (Apply picks): removed the hand-listed refusal reasons and the `apply all` hold-back sentence. The step now names `tomlctl review schedule` in the tomlctl skill's `references/review-apply.md` as the rule's home and runs `tomlctl review schedule <picked-ids> | --all --slug <slug> --dry-run` (with `--scope <scope>` for a flow-less store, no `--flow` or `--checkpoint`). Its `refused` and `held` rows are reported with their reasons, and its `settle` rows as settle tasks. Run the preview after decision answers are recorded. A new paragraph says `apply all` covers code and perf findings alike, earlier runs' included. It keeps the two `review list` commands, asks for the whole list to be shown, and asks for this run's ids to be offered explicitly when the user meant only those.
- `claude/commands/review-v2.md`, stage 8 Hand-off: deleted the sentence restating the schedule rule, which wrongly made every `evidence` pick a settle task.
- `claude/commands/optimise-v2.md`, stage 8: deleted the claim that `--all` takes only perf rows, along with the restated refusal and hold-back clause. It now says `--all` also schedules a shared store's open code findings, and asks for this run's perf ids to be offered explicitly when the user meant only those.

## Deviations

- The F104 edit deletes optimise-v2's text from "`--all` takes every open perf defect" to the end of the paragraph. F126 asks for a rewording at that spot. I combined the two: the rule is not restated there (that is F104), and the carrier keeps one sentence saying `--all` also takes code findings and asking for the perf ids to be offered (the F126 fix). The full list-and-show procedure now lives once, in triage-and-close step 3, which both carriers already point to.

## Handover

none

## Checks

- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed.
- I ran `tomlctl review schedule F104 --slug v2-apply --dry-run` to confirm the preview runs with no flow or checkpoint. It printed the documented object, with `refused` carrying reasons.
- `tomlctl review anchor F104 --slug v2-apply` now exits 1 because the `old` text is gone, which is what a forward fix should produce.
- Both accept checks (`review reproduce-status F104/F126 --expect gone`) are marked `Writes build: yes`, so they belong to the gate and I did not run them.

TANGENTIAL: none
