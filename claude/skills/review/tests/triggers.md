# Trigger tests for `review` skill

These utterances are used to validate that skill auto-invocation matches the intended framing. Positive cases should trigger the skill; negative cases should NOT trigger it (they are ordinary conversational asks for a light once-over, not a structured multi-lens audit with a persistent ledger).

## Positive

- do a full review of src/worker/
- audit the auth module and give me a findings ledger
- run a security review over payment flows and persist findings
- review recent changes on this branch and persist findings
- fix R12
- defer R12 — low impact right now — re-evaluate after pipeline refactor
- wontfix R7 — timeout is intentionally hardcoded, overridden by env in prod
- run /review over src/api/endpoints and write the ledger
- review docs/plans/compliance work and produce an R-ID ledger
- audit recent commits on this branch for security and architecture issues

## Negative

- review this function
- does this look right?
- quick look at my change?
- is this idiomatic?
- review my last commit real quick
- can you glance at this diff?
- what do you think of this refactor?
- is there a cleaner way to write this?
- spot anything wrong with this snippet?
- does this match our conventions?
