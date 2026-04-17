# Trigger Tests — implement

Auto-invocation is intentionally **disabled** for this skill
(`disable-model-invocation: true`). These utterances are documentation for
the user and for future description tuning if auto-invocation is ever
re-enabled. Today they describe the shape of requests the skill is meant
to handle (positive) versus requests it must never be auto-triggered on
(negative).

## Positive

- "execute the plan at docs/plans/prod-prep/01-security.md"
- "implement items 3,4,5 from docs/plans/security-hardening.md"
- "start working through docs/plans/queue-refactor/"
- "run the implementation plan — all items"
- "ship the plan at docs/plans/x.md now"
- "/implement docs/plans/todo/prod_preparation/01-security-hardening.md"
- "work through the outline at docs/plans/refactor-ingestion/00-outline.md end to end"

## Negative

- "implement a helper function for date parsing"
- "add a retry loop here"
- "can you write this function for me?"
- "implement the strategy pattern for handlers"
- "implement the fix we just discussed"
- "implement this one-line change in src/foo.rs"
- "implement the TODO I left on line 42"
