# plan-update trigger tests

## Positive

- "mark tasks 3-5 as done in docs/plans/prod-prep/00-outline.md"
- "record a deviation in docs/plans/foo.md — we switched from Redis to Postgres LISTEN/NOTIFY"
- "defer the S3 migration item in the plan with trigger 'when we hit 10 GB of attachments'"
- "reconcile docs/plans/prod-prep with current code, it's been two weeks"
- "catch the plan up, we've drifted after the big Axum 0.8 merge"
- "generate a snapshot of docs/plans/transaction-layer for the standup"
- "reformat the prod-prep plan — it's grown into one giant file and needs splitting"

## Negative

- "update this function to return Option<T>"
- "update the README with new CLI flags"
- "update your plan to include testing" (refers to the assistant's internal todo list, not a plan file)
- "plans for the weekend?"
- "update the config file at .env.example"
- "write a new plan for migrating to axum 0.8" (this is `plan-new`, not `plan-update`)
- "review the plan at docs/plans/foo.md" (this is `review-plan`, not `plan-update`)
