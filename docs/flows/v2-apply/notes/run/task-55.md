Status: done — drain.md "Stamping on re-entry" now routes apply milestones through the reproduce check and settle drain, and runs Reconcile on the stamp-from-HEAD branch.

Changed: claude/skills/implement-v2-engine/references/drain.md, "Stamping on re-entry". The stamp-from-HEAD branch runs findings.md Reconcile (for a milestone holding an apply row) between the stamp and the flow-store commit. The otherwise branch sends an apply milestone through Reproduce check (a row fixes a finding), then Settle drain (a row settles one), then the gate. A non-apply milestone still reruns the gate from step 7. The text notes the settle drain is safe to rerun.

Not done: the finding's fix also names the matching "Unstamped milestones" sentence in claude/skills/implement-v2-engine/SKILL.md (Re-entry). That file is outside this task's files, so it is unedited. That sentence says "run its gate again" and defers to drain.md's subsection, so it now reads slightly inconsistent. A follow-up task with SKILL.md in its files should align it.

Checks: no compile applies (markdown). The accept check (reproduce-status, Writes build: yes) is the gate's. I did not run the dispatch tests subset.

TANGENTIAL: debt | claude/skills/implement-v2-engine/SKILL.md | "Unstamped milestones" sentence still says to run the gate again | it should match drain.md's new apply-row routing
