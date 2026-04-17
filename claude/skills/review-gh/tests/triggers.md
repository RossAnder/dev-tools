# review-gh Trigger Utterances

Positive utterances should fire the `review-gh` skill. Negative utterances reference reviews or GitHub Projects but do not ask for a multi-lens audit persisted to a GitHub Project, and must not fire the skill.

## Positive

- review src/api/ into the GH project
- audit the worker module and file findings as project items
- push review findings to the project board
- do a GH-project-backed review of the payment flows
- review auth into the project
- run a full audit of the ingestion pipeline and track findings on the GitHub project
- review recent changes and file them as draft issues in the project
- multi-lens review of src/services/pricing/ with findings persisted to the GitHub Project

## Negative

- review this function
- create a GitHub issue for this bug
- look at the GitHub project status
- review the PR comments
- close issue #42
- show me the open items on the project board
- what's on my GitHub Projects backlog
- open a draft issue for the regression I just hit
- refactor this module — don't worry about reviewing it
- explain how GitHub Projects custom fields work
