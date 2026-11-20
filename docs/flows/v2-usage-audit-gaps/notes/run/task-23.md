Status: done — visual.md keeps every state's capture each round and routes a fail at an unchanged location on a state the previous round passed to the user as an inspector disagreement

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/visual.md"]

[[deviations]]
what = "What the user can pick for a disagreement: two options, 'the earlier pass stands' (nothing minted) and 'the new fail stands' (mints as today before the last round, or joins the last round's failing states)"
why = "The detail says to put the disagreement to the user in the accept question but not what the user can choose. A fail the user upholds has to go somewhere, and minting it as today keeps the constraint that a real fail is never dropped. The question is asked in the round the disagreement arises, so nothing has to be carried between rounds. The other option weighed was holding disagreements until the last round's acceptance question, which needs a cross-round record that tomlctl's visual_accept payload has no field for."
means = "detail: put it to the user in the accept question with both inspectors' notes"

[[deviations]]
what = "Unchanged at the defect means: the geometry.json entries whose locator names or contains the defect's location are identical in both copies (or absent from both), and for a text-grid state the rows the location spans are byte-identical. A byte-identical PNG also counts as unchanged. A location found in neither file, or a capture-class defect, counts as changed"
why = "geometry.json holds layout violations, not element boxes (ui-capture SKILL.md, the geometry.json section), so 'compare the location in geometry.json' can only mean comparing the violations at that locator. Every case that cannot be judged falls back to minting, which keeps the 'changed mints as today' constraint conservative."
means = "detail: compare the defect's location in geometry.json, and the text grid where there is one"

[[deviations]]
what = "The route is per defect and covers only an inspector's fail. A fail the orchestrator settles from unsure is minted, never treated as a disagreement. Whether the state passed in round r-1 is read from that round's inspector note"
why = "A disagreement is between two inspectors, so the orchestrator's own settling is not one. A state with defects at both changed and unchanged locations mints only the changed ones. The round r-1 index is not kept across sessions, but its notes are."
means = "detail"

[[checks]]
command = "grep -q 'disagreement' claude/skills/implement-v2-engine/references/visual.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/visual.md`:

- Step 7, **Keep the evidence**, now comes before the per-state verdict table. It copies every asked state's capture and `geometry.json` into `notes/run/visual-<id>-<r>/` every round, whatever the verdict, so the next round has something to compare against.
- The verdict table's `fail` row checks for a disagreement first. The `unsure` row says a fail the orchestrator settles is never a disagreement.
- A new **An inspector disagreement** paragraph covers rounds 2 and later. A defect on a state that round r-1's inspector note passed is a disagreement when the location is unchanged against round r-1's copy (geometry entries at the locator, text-grid rows, or a byte-identical PNG). A disagreement mints nothing and goes to the user with both notes and both captures. The user picks either "earlier pass stands" or "new fail stands". It is reported with report.md's existing line `milestone <id> visual round <r>: inspector disagreement on <states>, decided by the user`. A changed location, a state that never passed, or anything that cannot be compared mints as before.
- **Accepting a failed last round** now asks the disagreement questions first, in any round. After round 2, an upheld disagreement becomes an ordinary failing state for the acceptance question. **Two rounds at most** counts it the same way.

report.md already carried the disagreement line, so it needed no edit. Nothing is recorded in the store for a disagreement decision: `plan run visual --accept` has no field for it, and re-asking it on re-entry is harmless.
