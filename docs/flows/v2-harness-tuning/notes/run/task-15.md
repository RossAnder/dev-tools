Status: done — review method skill and four review agents now cover fix_files, the add dry run, the cover-note cut and plan-review names from records

## What changed

- `claude/skills/review-v2-method/SKILL.md`: `fix_files` row added to the `defect` and `cleanup` field tables (optional, at most 8 repo-relative paths, every file the fix edits or creates beyond the anchor, tests included), with a sentence saying a scheduled fix task holds only the files the finding names. Store writes: a `review add ... --dry-run` step with its fenced command before the real write. Length: `review cover` cuts a note over 600 to the cap (`…(+K)`, `note_truncated` in the envelope); a verdict `rationale` over 600 is still refused.
- `claude/agents/review-finder.md` and `claude/agents/review-finder-cf.md` (shared block `review-finder-body`, edited identically): write step 1 tells the finder to list in `fix_files` every file its `fix`/`edit` edits or creates beyond the anchor, `target_file` and `instances`, tests included; step 2 dry-runs `review add` first; step 4's "600 is the limit `review cover` enforces" now says the cover cuts a longer note.
- `claude/agents/review-verifier.md`: Unblock paragraph says to name in the rationale any file a fix sketch names that the finding lacks.
- `claude/agents/review-sweeper.md`: step 6 takes `NAMES` from `tomlctl plan view --slug <slug> --for human` and the slice files read whole when a plan review's diff is empty because the flow's files are untracked; step 11 dry-runs `review add` first; step 12's "hard limit" now describes the cut.

The `review-v2-store-writer` and `research-delivering` blocks are untouched.

## Deviations

- The finder bodies got the dry-run in step 2 and the cut wording in step 4 as well as the one `fix_files` sentence the detail named; without them each body's explicit steps would contradict the skill's new write idiom and Length rule. Alternative was leaving the bodies to defer to the skill, which a step-by-step agent would not do.
- The sweeper's step 11 got the dry-run too, for the same reason; the detail named only steps 6 and 12.

## Handover

None.

## Checks

- `bash scripts/verify-shared-blocks.sh`: shared-block parity OK.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed (the `--dry-run` flag did not fail it).
- Accept 1 (`fix_files` in method skill): pass. Accept 2 (both finders): pass. Accept 3 (`plan view --slug` in sweeper): pass.

TANGENTIAL: none
