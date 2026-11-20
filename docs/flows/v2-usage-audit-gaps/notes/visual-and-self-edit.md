Verdict: all three gaps stand in the current text: every visual round re-judges every state with no memory of an earlier pass (visual.md:53, :142), a project with no capture config never trips a plan check (check.rs:979) and its UI judgement hangs on one sentence (elaborate.md:53), and no engine has a self-edit rule; the docs show edited skills and agents take effect mid-session, which is the opposite of the "next session" premise, and nothing settles the AskUserQuestion claim (questions.md:48).

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = ["docs/flows/v2-usage-audit-gaps/brief.md"]

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 53
claim = "every round captures every state and control, not only the states that failed before"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 142
claim = "the orchestrator never re-judges a pass or talks a fail down; every inspector fail mints fix tasks"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 144
claim = "only fail or unsure captures are copied beside the notes; passed captures are not kept"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 213
claim = "two visual rounds at most; a fail after round 2 goes to the user's accept question"

[[source]]
path = "claude/skills/plan-v2-engine/references/ui-layout.md"
line = 11
claim = "UI candidacy is judged from the capture config's paths or web/QML files; a resized region counts as UI"

[[source]]
path = "claude/skills/plan-v2-engine/references/ui-layout.md"
line = 77
claim = "a project with no ui-capture.toml gets a row declaring one, but only inside the layout step for a milestone judged UI"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 53
claim = "elaboration runs the layout step only when the orchestrator itself judges the milestone UI; the list-and-ask of ui-layout.md section 1 is in a file it need not open"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 70
claim = "Design routes UI milestones through ui-layout.md"

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 18
claim = "a missing ui-capture.toml loads as an empty glob set, not an error"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 979
claim = "ui_scope_findings returns nothing when the glob set is empty, so a project with no config gets no plan/ui-milestone-no-states warning"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 866
claim = "ui_findings also drops the capture-path filter when no config exists"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 48
claim = "asserts the open picker blocks every later turn; unprobed"

[[source]]
path = "docs/flows/mid-run-requests/notes/worker-protocol.md"
line = 163
claim = "the queue-behind-picker claim was graded medium and marked not probed at the time"

[[source]]
path = ".claude/flows/mid-run-requests/review-v2.toml"
line = 3574
claim = "F36: one milestone would put drain/worker prose live mid-run while the installed tomlctl lacks the verb; cites glimpse-v2-visibility D12 and v2-harness-tuning D16"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/plan.toml"
line = 302
claim = "D12: engine references are read live while a flow runs, so contract edits go last"

[[source]]
path = ".claude/flows/v2-harness-tuning/plan.toml"
line = 360
claim = "D16: skills are linked, so an edited reference is live at once; this plan is run by the engine it edits"

[[source]]
path = ".claude/flows/implement-v2-run-hardening/plan.toml"
line = 334
claim = "D13: milestone A ends with cargo install as a plain gate, citing the link deploy"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 334
claim = "held commands such as cargo install run in a second verification call after the gate"

[[source]]
path = "docs/flows/v2-usage-audit-gaps/brief.md"
line = 116
claim = "the three items' summaries and the lens question"

[[source]]
url = "https://code.claude.com/docs/en/skills"
claim = "fetched 2026-11-20: Claude Code watches skill directories and picks up added, edited or removed SKILL.md within the current session; /reload-skills is the fallback; symlinked skill folders are read from the target; silent on whether edits under a symlink target are watched"

[[source]]
url = "https://code.claude.com/docs/en/sub-agents"
claim = "fetched 2026-11-20: Claude Code watches ~/.claude/agents and .claude/agents; an added or edited subagent file is detected within seconds and the next delegation uses it, with no restart"

[[source]]
url = "https://code.claude.com/docs/en/agent-teams"
claim = "fetched 2026-11-20: messages are delivered automatically to recipients; nothing on delivery while the lead waits on a question"
```

## Answer

**B-0e2db574 (visual rounds).** partnerhub_pages, flow site-worker-new-strapi, milestone C, 2026-11-20 (session b2a1b292). In round 1 an inspector's banner-01 note judged the 180 px brand selector as the native select's width at the design's 180 px cap, with the design "matching". Round 1 failed only the editor address descender clip, and fix task 48 changed only the `.phb-editor` rule. Round 2 (the last) failed four states. One fail was a real regression from task 48: the long address lost its ellipsis. The rest were the same unchanged selector, now judged "differs from the design" against the mock's ~116 px box. The user picked accept-and-commit at about 10:27Z, and the commit was 37bc17d. This came before run-hardening, so there is no `[[visual_accept]]` row. Today, visual.md:53 recaptures and re-judges every state, :142 mints every fail, and :144 keeps only failing captures. Nothing tells a new round what an earlier round passed. Byte-equal capture carry would **not** have saved this case. banner-01's capture did change, at the editor's descender pixels, so a whole-state hash would differ. Any carry rule has to work at the level of the defect's location.

**B-80fd8af7 (no capture config).** reportdesignkit has no `.claude/ui-capture.toml`, and no milestone of package-design-tools declares `ui_states`. K was elaborated on 2026-11-18, about 12:00-14:45Z (session fad99714, commit 066e90c6). That was after ui-layout.md landed (882d343f, 09:35 local that day). The user had reproduced an empty band on the Library page by eye under handover H2. The elaboration recorded D104/D107 ("size the Library page to its content", a region resize, which ui-layout.md:9 counts as UI). It wrote row 148 without running the layout step. The transcript shows elaborate.md:53 being read, but ui-layout.md was never opened. So section 1's "list every milestone and ask" (ui-layout.md:13) and the config row (ui-layout.md:77) never ran. Nothing in the store caught it: `load_paths` returns an empty set for a missing config (ui_capture.rs:18), and `ui_scope_findings` then returns nothing (check.rs:979).

**B-d05d1f05 (self-edit).** There are four such decisions: glimpse-v2-visibility D12, v2-harness-tuning D16, mid-run-requests F36 (which cites both) and run-hardening D13. Each one re-derives the same ordering rule from memory: put the engine-reference edits last, after an install gate. On 2026-11-16 at 09:13Z (session 208b1a5f, milestones-by-dependency milestone A), the orchestrator improvised the other remedy. It copied drain.md to its scratchpad and drained "against the snapshot". No engine text names either remedy: a grep for live, symlink, junction and snapshot finds nothing relevant. The official docs settle the premise, and it is the reverse of the backlog's wording. Edited `SKILL.md` text and edited subagent files take effect **within the running session**: the next delegation uses the new definition. Engine references (drain.md, visual.md and the rest) are read with Read when a step needs them, so they are live whatever the watcher does. That makes D12 and D16 right and the "next session" phrasing wrong.

Three questions are still open:
- Do edits made through this repo's links reach the watcher? Skills are junctions or symlinks and agents are file symlinks; the docs say symlinked skills are read from the target but are silent on watching it.
- Is a saved Workflow script re-read at each launch?
- Does a Workflow-dispatched agent resolve its definition per dispatch?

On AskUserQuestion: the transcripts never caught a SendMessage arriving while a picker was open, in any session with both (seven sessions across partnerhub_pages, reportdesignkit and dev-tools). Every QUESTION or REQUEST arrived before the picker opened. The claim is structurally plausible, because the orchestrator cannot take a turn while its tool call waits on the user, but it stays unprobed. What matters for the design is whether the message is queued or lost, and how long the worker's 600 s wait runs against an open picker.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/visual.md:53 and :142 (step 3 capture scope, step 7 verdict table)
- **Source**: visual.md:53, :142, :144, :213; partnerhub_pages notes run/visual-C-1-1.md vs visual-C-2-1.md (banner-01 design line "matches" vs "differs", the selector at x 155-334 in both); session b2a1b292 at 10:27Z
- **Evidence-grade**: high
- **Finding**: A later round can fail a state on a part that an earlier round passed and that no fix touched, so inspector variance can spend the last round. Give a new round the memory of earlier passes. needs-plan
- **Details**: Option A: re-judge only the states named by the fix rows' look entries. This is cheap, but it drops the sibling-regression guard that :53 states on purpose, and round 2 here caught a real sibling regression (the ellipsis). Option B: carry a pass whose capture bytes are unchanged. This is exact, but it needs the passed captures kept (:144 keeps only fails), and it would not have fired here because the descender pixels changed. Option C, recommended: a state that passed in round r-1 and fails in round r on a location whose pixels are unchanged between `<out>/<id>-<r-1>/` and `<dir>` is a disagreement, not a defect. It mints no fix round, and goes to the user with both notes in the existing accept question (:225). Disagreements on changed pixels mint as today. This needs the previous round's captures kept (copy every state's capture, not only fails) and a pixel diff over the defect's location, which the orchestrator or a tomlctl verb runs, not an inspector.
- **Counter**: If the round-2 inspector was right, because the design file does say natural width capped at 180 px and the mock shows ~116 px, then the round-1 pass was the error, and Option C would only route a real defect to the user instead of fixing it. That is a cost in UX, not a loss of the defect.
- **Impact on plan / report**: one milestone touching visual.md steps 3, 4 and 7 plus a diff helper. The accept record shape from run-hardening can carry a `disagreement` kind.

- **Library/API or file**: tomlctl/src/plan/check.rs:979 (`ui_scope_findings`) with tomlctl/src/ui_capture.rs:18 (`load_paths`)
- **Source**: check.rs:866 and :967-:1016; ui_capture.rs:15-25; reportdesignkit has no capture config and no `ui_states` on any milestone A-K
- **Evidence-grade**: high
- **Finding**: A project with no capture config gets no store signal that a milestone touches UI. The only safeguard is the orchestrator's judgement at elaborate.md:53 and write-and-prove.md:70.
- **Details**: Without paths there is nothing for a glob to match, so tomlctl alone cannot detect UI files. Option A: an `info` or `warning` class, `plan/ui-capture-missing`, raised when the config is absent and any decision has `class = "look"` or a milestone has `ui_states`. This is cheap, but misses K, which had neither. Option B: move the capture-config question out of ui-layout.md:77 into Open, once per plan. If the repo has no config, ask the user through `AskUserQuestion` whether the project has a UI to capture, and record the answer as a decision (a `has-ui` stance), so every later Design and elaboration reads it. Option C: make elaborate.md:53 and write-and-prove.md:70 always open ui-layout.md and record section 1's UI or not-UI list as a decision on every milestone, so a skip is visible in the store and `plan check` can require it once a has-ui decision exists. B plus C closes K's path. A alone does not.
- **Counter**: If K's resize was a fix inside an arrangement that stays as it is (ui-layout.md:9 exempts those), skipping the layout step was within the rules and only the missing config question stands. K's D104 changes the panel's height model, which reads as a reshaped region.
- **Impact on plan / report**: touches ui-layout.md sections 1 and 5, elaborate.md:53, write-and-prove.md:70, open-and-challenge.md, and optionally check.rs with a test.

- **Library/API or file**: claude/skills/implement-v2-engine (no self-edit rule anywhere); precedents at .claude/flows/v2-harness-tuning/plan.toml:360 (D16), .claude/flows/glimpse-v2-visibility/plan.toml:302 (D12), .claude/flows/implement-v2-run-hardening/plan.toml:334 (D13), .claude/flows/mid-run-requests/review-v2.toml:3574 (F36)
- **Source**: the four records; session 208b1a5f at 09:13Z (drain.md copied to scratch); code.claude.com/docs/en/skills and /sub-agents fetched 2026-11-20
- **Evidence-grade**: high for the gap and the documented reload behaviour; low for behaviour through symlinks and junctions and for Workflow script and agent resolution
- **Finding**: State one self-edit rule in both engines. Do not rest it on "loads next session", which the docs contradict for skills and agents alike. needs-plan
- **Details**: The facts are these. References are read on demand, so they are always live. The SKILL.md body already in context is stale until the skill is invoked again. The docs say agent files are re-read at the next delegation. Option A is the planning rule the precedents already follow: Design orders engine-reference and worker-agent edits into the last milestone, after any `cargo install` gate (drain.md:334 holds installs to a second call). Detection would be a row whose files resolve under a config-dir link target (`readlink` of `~/.claude/{skills,agents,workflows}` entries). Option B is the run rule the 11-16 orchestrator improvised: at launch, copy the engine's references into `<scratch>` and drain from the copy whenever any row's files fall under the engine's own base directory. A covers binary and verb skew. B covers prose drift during a drain, but not worker agents read by later workers in the same run. A plus B is the full answer. A tomlctl check would need dev-tools-only paths, so leave detection to the engine text rather than the binary.
- **Counter**: If the watcher does not follow junction or symlink targets on Windows, edits to agent files reach no running session through the links, and the agent half of the rule is moot here, though not in a repo that edits `.claude/agents/` directly. Settle it with one attended probe: put a marker line in a linked agent body, dispatch it and check the reply, then repeat for a linked SKILL.md before and after `/reload-skills`.
- **Impact on plan / report**: touches implement-v2-engine run-milestone.md (launch snapshot), plan-v2-engine write-and-prove.md section 1 (ordering), and CLAUDE.md's Deploy line ("not live until it reaches that directory" should say links make it live at once).

- **Library/API or file**: claude/skills/implement-v2-engine/references/questions.md:48
- **Source**: questions.md:48; docs/flows/mid-run-requests/notes/worker-protocol.md:163 (graded medium, not probed); picker and message timings from seven transcripts, extracted read-only (no message ever landed inside an open picker)
- **Evidence-grade**: medium (structural reasoning; no observation either way; the agent-teams docs say only that messages are "delivered automatically")
- **Finding**: "The open picker blocks every later turn" is unverified. The safe rules built on it (rule REQUESTs before opening the picker, and treat a timeout as denial) hold either way, so a probe would refine the claim rather than change the design.
- **Details**: Probe it attended in one session. Launch a background agent that sleeps 30 s and then sends one SendMessage to main. Open an `AskUserQuestion` at once, answer it after 90 s, and record whether the message arrives after the tool result (queued), interrupts the picker, or is lost. Then reword :48 to the observed fact.
- **Counter**: If the harness delivers a queued message into the picker's tool result or drops it, the worker's 600 s wait needs a longer horizon or a re-send, and the rule changes.
- **Impact on plan / report**: one handover-style probe in the plan's first milestone; a one-line edit to questions.md afterwards.

Coverage: read in full visual.md and ui-layout.md. Read the relevant spans of elaborate.md, write-and-prove.md, questions.md, drain.md, check.rs (ui_findings, ui_scope_findings), ui_capture.rs, the four self-edit records and worker-protocol.md. Read partnerhub_pages notes visual-C-1-1, visual-C-1-2, visual-C-2-1 and visual-C-2-2 and task 48. Searched transcripts b2a1b292 (partnerhub), fad99714, 54a05a11 and 79dae18c (reportdesignkit) and 208b1a5f (dev-tools), and ran a read-only timing extraction over seven sessions. Ruled out byte-hash state carry as the fix on its own (it would not have fired in the cited case) and a tomlctl-only engine-path check (it would hardcode dev-tools paths). No probe was run, because both probes need an attended session or a write to a linked file.

Searched: code.claude.com/docs/en/skills, /sub-agents and /agent-teams (fetched 2026-11-20); web search "Claude Code changelog skills hot reload symlink agents directory watch" (2026-11-20). That search found skill hot reload since v2.1.0 and /reload-skills since v2.1.152, but no source on watching symlink or junction targets or on Workflow-dispatched agent resolution. Third-party pages from that search (claudelog, dev.to, venturebeat) were not relied on. The newest source date seen is the current docs, which cite v2.1.317.
