Status: done — `run finish` section of review-runs.md now states the overdue-proposal refusal.

Changed claude/skills/tomlctl/references/review-runs.md: added a sentence to the `run finish` refusal paragraph (refuses while a deferred proposal of the run's family is overdue; `--incomplete` / `--incomplete-file` does not waive it; links to the Deferred proposals paragraph in review.md) and a clause in the `--incomplete` flag row.

Deviations: none. Handover: none.

Checks: the overdue grep passes. The cli::dispatch::tests accept check writes build, so it is left to the gate. File is under 600 lines.

TANGENTIAL: none
