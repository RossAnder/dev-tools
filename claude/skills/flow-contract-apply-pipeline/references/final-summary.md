# Final summary reference

Detail behind the final summary of the apply pipeline: the console report skeleton the orchestrator
emits once Step 5 completes.

```
## <carrier report title>

### Implemented
- [<ID>] [file:line] [category] Summary of what was changed — (severity)
  - Tag `(partial)` for partial applies (see `resolution` for the split).
  - Tag `(chronic)` for items whose pre-apply `rounds >= 3` reached `<APPLIED>` (per the
    ledger-schema escalation rule).

### Verified Clean          # only where <NO-CHANGE> is a disposition distinct from <REJECTED>
- [<ID>] [category] Audit note

### Skipped
- [<ID>] [category] Reason — the ledger's rationale field carries the same text

### Escalated
- [<ID>] [file:line] <reason word> — evidence and next step (for `spec-stale`, re-run <PRODUCER> on
  the file); the item stays `open`

### Unknown IDs
- <ID>: not present in ledger at <path> — check <PRODUCER>'s most recent output

### Downgraded
- [<ID>] [file:line] Claimed `applied` but no diff detected — transitioned to `<REJECTED>`. Investigate.

### Requires User Confirmation
- [<ID>] [file:line] [category] [severity] Agent rationale — awaiting explicit disposition before
  the ledger transition.

### Changed During the Run
- [<ID>] `<field>` expected `<expected>`, found `<found>` — `skipped_stale`; not retried

### Verification
- Build: pass/fail/timeout
- Tests: pass/flaky/fail/timeout/none — with the block's `summary:` count
- Category-specific: per the carrier's checks, as applicable

### Regressions Triggered
- [<ID>] [file:line] Regression of [<old ID>] — dedup-rule match details

### User inputs
- I<n> (<kind>) [<ID>, …] Outcome — the handle note's text
```
