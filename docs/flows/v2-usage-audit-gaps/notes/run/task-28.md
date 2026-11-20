Status: done — approve.md re-checks the review delta or records the user's waiver; plan-flow.md takes the delta as re-check targets and requires sourced hand fixes

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/approve.md", "claude/skills/review-v2-engine/references/plan-flow.md"]

[[deviations]]
what = "The delta re-check's run start also passes --set recheck_scope=delta, beside rechecks=<run>"
why = "tomlctl/src/review/runs.rs start_digest digests the review delta into the new run's plan_digest only when recheck_scope is delta; without it the re-check would finish and review_delta would stay non-empty, so the loop would never end. The approve refusal text in tomlctl/src/plan/approve.rs names the same pair."
means = "detail: start a re-check review run with rechecks=<the latest finished review run>"

[[deviations]]
what = "The re-check loop runs automatically before the gate; the user can stop it at any round, and the waiver is offered at the gate only when a review-delta risk row is present"
why = "The detail says both 'repeat until review_delta is empty' and 'at the gate the user may instead waive it'; this reading keeps the default as review and makes the waiver the user's explicit choice. Easy to unwind (prose only)."
means = "detail: approve.md flow"

[[checks]]
command = "grep -q -- '--waive-review-delta' claude/skills/plan-v2-engine/references/approve.md && grep -q 'review_delta' claude/skills/review-v2-engine/references/plan-flow.md"
result = "pass"

[[checks]]
command = "wc -l on both edited references (ceiling 600)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- `claude/skills/plan-v2-engine/references/approve.md`: a new **Review delta** subsection under Review: a non-empty `review_delta` from `plan stage` opens a new `review` step row (`plan run start --step review`), a re-check by plan-flow.md section 10 "From approval" (rechecks= the newest finished plan-family run, `recheck_scope=delta`, copied pack.md and profile.md, targets = the printed `review_delta`), triage, close, finish the row, and step 1 again until empty. The user may stop and waive; a skipped review has no delta; `--amend` never refuses and routes the optional re-check through section 12. Step 1 now reads `review_delta`; step 3 lists the `review-delta` risk row and offers "approve, waiving the review delta" and "re-check the delta"; step 5 shows `plan approve --slug <slug> --review ran --waive-review-delta '<why>'` (stored as `delta_waiver`); the refusal table gains the delta refusal row, worded after the message in tomlctl/src/plan/approve.rs.
- `claude/skills/review-v2-engine/references/plan-flow.md`: section 10 gains **From approval**: the newest finished plan-family run from `review run list --finished --where family=plan`, targets taken from `review_delta`, run start adding `--set recheck_scope=delta`; the run start says what `rechecks=` names; the targets and step-row lines cover the approval case; section 11 returns to Approve step 1 afterwards. The **Fixed by hand** rule now requires a fetched source for any outside value a hand fix introduces (never the local toolchain's output), an existence check for a version or tag pin, and cites write-and-prove.md's **Outside facts** as the owning rule.

The dispatch-tests check is marked `Run by: gate`, so it was not run here; every fenced command added uses existing verbs and flags (`plan stage`, `plan run start`, `review run list --finished --where`, `review run start --set recheck_scope=delta`, `plan approve --waive-review-delta`) checked against the CLI help and source.
