Verdict: Of 55 After Merge bullets in the 12 final plans, 35 (64%) are agent-performable, 12 (22%) are human-only and 8 (15%) bundle agent steps with a human residue. The v1 contract invites this mix by calling every bullet a step "the user must take by hand" (claude/commands/implement.md:330; claude/skills/flow-contract-plan-output-format/SKILL.md:206-211).

## Answer

The corpus is the 12 final plans under `docs/plans/` with an `## After Merge` section. Their 9 `.premerge.md` copies and the 2 archived copies under `.claude/flows/review-system-v2/archive/` restate the same bullets with small edits, so they are excluded from the counts to avoid double counting (drift noted below). A top-level bullet is one unit, and its sub-bullets belong to it.

| Class | Bullets | Share |
|---|---|---|
| Agent-performable | 35 | 64% |
| Human-only | 12 | 22% |
| Mixed (agent steps plus a human residue) | 8 | 15% |
| Total | 55 | |

20 of 55 bullets (36%) touch a human at all. Split into single steps, the human work is small and repetitive: restarting Claude Code sessions (8 plans), restarting glimpse panes (4 plans), attended interactive checks (5 bullets), one permissions edit, one elevation and one trial run over time. None of the bullets is human-only because of credentials or an external system.

**Human-only, by reason (12, plus the human step inside each of the 8 mixed bullets)**
- Session restart, Claude Code (3 pure plus 6 inside mixed bullets): eventual-churning-sunrise.md:563, proud-leaping-conway.md:321, twinkling-hopping-otter.md:732. Mixed: federated-beaming-hollerith.md:600, review-system-v2.md:1086, temporal-snuggling-pnueli.md:1195, toasty-wobbling-stonebraker.md:521, we-ve-just-implement-the-shimmying-kettle.md:341.
- Session restart, glimpse panes (3 pure plus 1 mixed): giggly-conjuring-brook.md:398, polymorphic-wondering-magpie.md:596, temporal-snuggling-pnueli.md:1196. Mixed: glimpse-ux-backlog.md:652. This is borderline: an agent could kill the process but cannot reopen the pane the user is watching.
- Elevation or permissions (1 pure plus 1 mixed): federated-beaming-hollerith.md:602 (ask rules in `.claude/settings.json`, "made by hand"; an agent must not widen its own permissions). Mixed: shimmying-kettle.md:341 (file symlinks "need an elevated window unless Developer Mode is on").
- Attended or interactive run (4 pure plus 1 mixed): glimpse-ux-backlog.md:653 (touch-hold-drag in three terminals), :655 (watching for lag), :656 (the manual pass), temporal-snuggling-pnueli.md:1197 (action-menu smoke). Mixed: shimmying-kettle.md:342 (`/plan-v2` is attended-only).
- Judgement over time (1 pure plus 1 mixed): review-system-v2.md:1089 (trial over five or more runs). Mixed: polymorphic-wondering-magpie.md:597 (leave glimpse idle and watch).

**Agent-performable, by kind (35)**
- Install, build or deploy (14): eventual-churning-sunrise.md:560, 561, 562; federated-beaming-hollerith.md:599, 601; giggly-conjuring-brook.md:396; polymorphic-wondering-magpie.md:594; proud-leaping-conway.md:320; review-system-v2.md:1085; temporal-snuggling-pnueli.md:1194; toasty-wobbling-stonebraker.md:520, 522; twinkling-hopping-otter.md:731; v2-planning.md:660.
- Memory, doc or config edit (8): federated-beaming-hollerith.md:603; polymorphic-wondering-magpie.md:595, 600; proud-leaping-conway.md:322; temporal-snuggling-pnueli.md:1198; twinkling-hopping-otter.md:733; v2-planning.md:662; shimmying-kettle.md:343.
- Run a check or success criterion (4): giggly-conjuring-brook.md:397; glimpse-ux-backlog.md:654; proud-leaping-conway.md:323; v2-planning.md:665.
- Follow-on plan or backlog item (4): review-system-v2.md:1090; v2-planning.md:661, 663; shimmying-kettle.md:344.
- Trial or measurement (3): review-system-v2.md:1087, 1088; twinkling-hopping-otter.md:741. The first two depend on a restart first, because the new commands load only in a fresh session.
- Git or worktree housekeeping (1): v2-planning.md:664.
- File deletion (1): shimmying-kettle.md:345.

**Representative quotes**
- Human, restart: "Restart open Claude Code sessions so the updated skills and commands load." (proud-leaping-conway.md:321)
- Human, permissions: "Add `ask` permission rules … to `.claude/settings.json` … This is a permissions change, made by hand." (federated-beaming-hollerith.md:602)
- Human, attended: "touch-hold-drag the divider in ghostty, ghostty+herdr and Windows Terminal, and inspect the log." (glimpse-ux-backlog.md:653)
- Agent, install: "`cargo install --path tomlctl` then `cargo install --path glimpse` (glimpse links tomlctl's library)." (giggly-conjuring-brook.md:396)
- Agent, check labelled "by hand": "Run the **predicted, unverified** datetime criterion by hand in a scratch repo: a fresh `tomlctl backlog add` row's `created` ends `Z`." (giggly-conjuring-brook.md:397)
- Agent, memory edit: "Update the memory note `v2_pipeline_direction` to record the design's changes, and point it at `docs/plans/v2-planning.md`." (v2-planning.md:662)
- Agent, deletion: "Delete `docs/plans/typed-dazzling-goblet.md`, the superseded untracked checkpoint." (shimmying-kettle.md:345)

**Bundled bullets (several steps in one bullet; 12)**
federated-beaming-hollerith.md:600 (check plus restart); glimpse-ux-backlog.md:652 (install plus restart), :653 (run, inspect, then capture a backlog item if needed); polymorphic-wondering-magpie.md:597 (two unlike criteria); review-system-v2.md:1086 (deploy, copy out of band, restart), :1087 (five or more commands across three repos), :1088 (a five-item scratch-worktree loop); temporal-snuggling-pnueli.md:1195 (junction, mirror symlink, `git update-index` staging, restart); toasty-wobbling-stonebraker.md:521 (deploy plus restart); v2-planning.md:660 (install plus other clones); shimmying-kettle.md:341 (link, elevation, restart), :342 (seven steps: worktree add, new session, attended `/plan-v2`, backlog, doc line, worktree remove, branch delete).

**No trigger, no done condition, or not an after-merge step (14)**
- Mid-execution steps placed in After Merge: temporal-snuggling-pnueli.md:1194 ("reinstall glimpse at checkpoint A too"); toasty-wobbling-stonebraker.md:520 ("Also run it at checkpoint A … Do not run `/review-v2` between task 21 and the deploy"). Both apply before the merge, and the list is read only after it.
- Standing instructions with no end: review-system-v2.md:1086 ("recopy after every catalogue or script change"); v2-planning.md:660 ("reinstall in any other clone before it touches one"), :664 ("In any other clone or worktree with `core.autocrlf` on").
- Conditional, with the condition left unstated or self-cancelling: polymorphic-wondering-magpie.md:600 ("only if the tail's parsing changed. It shouldn't have."); twinkling-hopping-otter.md:731 ("if anything in tomlctl changed after checkpoint C"); v2-planning.md:665 ("if `/implement` Phase 3 did not cover them", with no list).
- Pointers with no inline outcome: glimpse-ux-backlog.md:656, proud-leaping-conway.md:323.
- No done condition or owner: v2-planning.md:661 ("Decide what to do with …"); review-system-v2.md:1090 (four follow-ons in one line); twinkling-hopping-otter.md:741 ("the first sessions that use 0.15").
- Redundant: toasty-wobbling-stonebraker.md:522 resyncs a Workflow that its own parenthetical says deploy-claude already links (the deploy step is :521).

## Findings

- **File**: claude/commands/implement.md:330 and claude/skills/flow-contract-plan-output-format/SKILL.md:206-211 (the `## After Merge` template)
- **Source**: file read; the template lists "install or reinstall a binary, resync a plugin cache, restart open sessions, update a memory note, run a **predicted, unverified** success criterion by hand", and implement.md:330 says "those are steps the user must take by hand".
- **Evidence-grade**: high
- **Finding**: The v1 contract defines After Merge as a human checklist but seeds it with agent-performable kinds, so 64% of the corpus is agent work handed to the human.
- **Details**: Four of the template's five example kinds (install, resync, memory edit, run a criterion) fall in the agent classes above, and only "restart open sessions" is human-only. `/implement` echoes the list verbatim into its final report and never executes any of it.
- **Counter**: If installs could not run inside the session (for example, a locked running glimpse exe on Windows blocking `cargo install --path glimpse`), the 14 install bullets would move toward mixed. This was not tested, because testing it means installing.
- **Impact on plan / report**: A v2 handover should hold only the human residue (restart, elevation or permissions, attended interactive check, judgement over time), each as one step. Installs, memory edits, criteria runs, deletions and follow-on items become tasks or agent steps.

- **File**: docs/plans/*.md After Merge sections (55 bullets, anchored above)
- **Source**: awk extraction of each section (command in coverage line)
- **Evidence-grade**: high (the counts); medium (the borderline classifications: glimpse-pane restarts counted as human, and review-system-v2.md:1087-1088 counted as agent although they need a fresh session)
- **Finding**: The human residue repeats across plans. Session restart appears in 8 of 12 plans, glimpse-pane restart in 4, attended interactive checks in 2 (glimpse-ux-backlog, temporal-snuggling-pnueli) plus the attended `/plan-v2` proof. Credentials and external systems never appear.
- **Details**: Of the 20 human-touching bullets, 8 are bundles where the human step sits beside agent steps. Unbundled, they reduce to roughly one restart line per plan plus a few attended checks.
- **Counter**: A plan corpus outside `docs/plans/`, such as lumina stories, with credential or external-system steps would add classes this corpus lacks. Only `docs/plans/` and the flow archives were searched.
- **Impact on plan / report**: A v2 handover vocabulary can be closed and small: restart, elevate, attended check, observe over time. Each needs a trigger and a done condition, which the 14 defective bullets above lack.

Adjacent lens (one line, not investigated): v2 already has a `[[handover]]` record with `id`, `command`, `source_task` and `done` (tomlctl/src/plan/schema.rs:299-304), surfaced as `stops` at approve (claude/skills/plan-v2-engine/references/approve.md:60). It has no field for why the step is human-only, or for its trigger or done condition.

Premerge and archive drift (excluded from counts): federated-beaming-hollerith.premerge.md:493 had an agent-runnable criteria bullet that the final plan dropped. we-ve-just-implement-the-shimmying-kettle.premerge.md:328 ran the proof with `git switch -c` in the primary checkout, which the final plan moved to a linked worktree. The archived review-system-v2 drafts (:795-799, :952-956) carried the same install, deploy, smoke and trial shape.

Coverage: Read in full every `## After Merge` section in the 12 final plans (eventual-churning-sunrise, federated-beaming-hollerith, giggly-conjuring-brook, glimpse-ux-backlog, polymorphic-wondering-magpie, proud-leaping-conway, review-system-v2, temporal-snuggling-pnueli, toasty-wobbling-stonebraker, twinkling-hopping-otter, v2-planning, we-ve-just-implement-the-shimmying-kettle). Also read the bullets of 9 `.premerge.md` copies and 2 archive drafts, plus the contract lines in flow-contract-plan-output-format/SKILL.md:206-211 and implement.md:320-330. Checks run: Grep `^#+ After Merge` over `docs/plans/*.md` and `.claude/flows/*/archive/**`, an awk section extract per file, Grep for the glimpse-ux-backlog and temporal-snuggling-pnueli manual passes (glimpse-ux-backlog.md:172, temporal-snuggling-pnueli.md:329), and Grep for after-merge or handover in the plan-v2 sources. Ruled out: other `.premerge.md` plans with no After Merge section (binary-jumping-yao and 13 others) have no bullets to classify; the lumina story plans are outside the named scope. No web search was needed.
