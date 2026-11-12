Status: done — README and CLAUDE.md describe the Plan surface, links, document view, needs-you rows, authored feed and document lane.

Changed:
- glimpse/README.md: seven surfaces and `1`-`7`; `--surface` lists `plan`; Inbox opens with needs-you rows; new Plan section (eleven sections, one line each) and a subsection on following links and reading documents; keys table gains Plan, `Tab`/`Shift+Tab` on Plan, `b` and `Backspace`; data flow gains the authored feed and the document lane.
- glimpse/CLAUDE.md: write model says Plan surface, links list and needs-you rows write nothing; Structure names model/plan.rs, authored.rs, source/doc.rs, plan_state.rs, nav.rs, rollup.rs and the new views; two gotchas (document lane on the poller thread with hex-checked shas and root-contained paths; docs/flows watch scope with its own miss streak, re-statted every scan).

Deviations: none.

Checks: both accept checks pass (grep commands run).

Unchecked: the one-line descriptions of the timeline, changes, departures and health sections are written from the section names and row builders skimmed, not each row's full content; a reviewer may tighten them. The header tab example line was left unchanged.

TANGENTIAL: none
