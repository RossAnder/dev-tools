Verdict: Independent sibling milestones are recurrent (3 of 7 flows, 4 pairs, all file-disjoint) but the deferred-sibling diamond is a one-off (package-design-tools only); store order under run-milestone.md:58 cost ready rows 44 to 79 in-session minutes per affected flow, and most of that time was a one-worker serial tail, which the proposed pick does not recover. Only about 24 minutes of zero-worker elaboration in that one flow is.

Measured from each flow's `tasks.toml` (cross-milestone `needs`, files, `elaboration`), its `plan.toml` `[[run]]` rows, `agents.toml` worker segments where one exists, and the milestone commit times. All times are UTC.

| Flow | Store order | Milestone graph from `needs` | Independent pairs | Deferred |
|---|---|---|---|---|
| v2-harness-tuning | A B C D E | A -> B -> {C, D}; E needs A, B, C, not D | C‖D, D‖E | none |
| milestone-cadence | A | single | none | none |
| tomlctl-test-and-gate-cost | A B C | A -> B -> C, linear | none | C, elaborated 12:11Z |
| glimpse-v2-visibility | A B C | A -> {B, C} | B‖C | none |
| human-only-handovers | A B | A -> B | none | none |
| v2-apply | A B C apply-1 | A -> B -> C -> apply-1 (C also needs A) | none | none |
| package-design-tools | A B C D | A -> {B, C} -> D | B‖C | B (elaborated 13:11Z), D still deferred; C elaborated before approval |

In every independent pair the two milestones share no file and no `coupling` edge. The shape is therefore an A -> {B, C} fan-out in three flows, two of which were planned after the `How many` cut rule landed (write-and-prove.md:53). The full diamond, and a deferred milestone sitting ahead of an independent ready one, appear only in package-design-tools. tomlctl-test-and-gate-cost's deferred C left 21 minutes with no worker (B committed 11:53:27Z, C launched 12:14:35Z), but nothing else was ready, so no work waited on it.

The wait splits three ways, as follows.

- **On a deferred or elaborating milestone, zero workers:** package-design-tools only, 24.5 minutes (A commit 12:53:00Z to B's elaboration commit 13:17:32Z). This is the only wait a dependency-ordered pick recovers. In one session it is recovered only if elaboration overlaps C's run, through a second session or an elaboration task in the pool.
- **On a serial-chain tail of the running milestone (pool of six, `policy.max_parallel = 6`):** package-design-tools B ran 45.0 minutes with one worker for 35.5 of them while C's 12 rows were ready. glimpse-v2-visibility B ran 74.7 minutes with one worker for 49.5 of them while C's 4 rows were ready. v2-harness-tuning C (intra-milestone critical path 7 of 9 rows) ran 39.6 minutes while D was ready, though no `agents.toml` exists to measure its occupancy. Only running two milestones together or lookahead recovers this, and both are non-goals. It does, however, bear on whether the cut rule (B-1fa8d53f) holds.
- **On store order alone between upfront milestones:** glimpse C waited 79 minutes in session (ready 12:49:16Z, B committed 14:08:21Z, then the session ended and C ran the next day). v2-harness-tuning D waited 44 minutes (ready 10:45:34Z, launched 11:29:44Z) and E waited about 8 minutes behind D. In a one-milestone-at-a-time engine these swap places and total wall time does not change. The only real loss is the idle pool slots counted in the previous item.

- **Library/API or file**: `.claude/flows/{glimpse-v2-visibility,v2-harness-tuning}/tasks.toml:14` `[[checkpoints]]`; `../reportdesignkit/.claude/flows/package-design-tools/tasks.toml:3041` `[[milestones]]`
- **Source**: `tomlctl get <tasks.toml>`, cross-milestone `needs` and file sets computed over `items`
- **Evidence-grade**: high
- **Finding**: Independent sibling milestones recur in 3 of 7 flows (pdt B‖C, glimpse B‖C, harness C‖D and D‖E), all file-disjoint with no coupling. A deferred milestone ahead of an independent ready one occurs once (pdt).
- **Details**: pdt C rows need only A rows (19->3, 20->5, 24->38, 27->39, 28->1) and C held `elaboration = "elaborated"` from approval commit 02282cc9 onward, while B was `deferred` until 08f7671c. glimpse C rows 34 to 36 need only A rows 5, 6, 8 to 10 and 13. harness D needs only B (13, 15, 16), and E needs A, B and C (18, 20) but no D row.
- **Counter**: An implicit dependency not written as `needs` (for example glimpse C's agent edits wanting B's glimpse install, or harness E's engine drive wanting D's pool) would make a pair dependent in fact. Only the stored edges and files were checked.
- **Impact on plan / report**: The pick rule must handle a fan-out, not just skip a deferred milestone. The deferral case alone does not justify a large engine change on frequency.

- **Library/API or file**: `../reportdesignkit/.claude/flows/package-design-tools/plan.toml:868` `[[run]]`; `agents.toml` worker segments for pdt and glimpse-v2-visibility
- **Source**: worker segments sampled every 10 s over B's implement-run windows: pdt `{0: 4.0, 1: 35.5, 2: 4.0, 3: 0.7, 4: 0.8}` minutes; glimpse `{0: 5.7, 1: 49.5, 2: 13.5, 3: 6.0}`
- **Evidence-grade**: high
- **Finding**: Most of the time ready sibling rows waited, the running milestone used one worker of six. The serial tail costs more than the deferral wait.
- **Details**: pdt: B ran 13:18:06Z to 14:03:06Z, and its rounds dispatched 6, 3 and 2 tasks (29, 31 and 32 make a chain). glimpse: B ran 12:51:59Z to 14:06:39Z, with intra-milestone critical path 12 over 21 rows. Its rounds dispatched 19, 12, 9, 3 and 2 tasks.
- **Counter**: Segment times come from hook events. A missed start or stop event would understate occupancy, and the run rows' `tasks` lists show only dispatch, not concurrency.
- **Impact on plan / report**: A dependency-ordered pick plus a milestone-scoped pre-flight recovers about 24 minutes, in one flow. The 35 to 50 minute one-worker tails come back only through lookahead or concurrent milestones, which are non-goals. That is evidence the plan may cite on whether the B-1fa8d53f cut rule fails for fan-outs.

- **Library/API or file**: `claude/skills/implement-v2-engine/references/run-milestone.md:58` (pick in store order) and `:73` (deferred stops)
- **Source**: commit times from `git show -s --format=%cI` (dev-tools 0b87a8a3..affff3d4, 7bae12e1, e1379104, 7838eb2b; reportdesignkit dbbfb4bd, 08f7671c, f9662889, e63b548b)
- **Evidence-grade**: high
- **Finding**: Store order delayed ready milestones by 70 minutes (pdt C, in session), 79 minutes (glimpse C) and 44 minutes (harness D). Only pdt's portion includes a zero-worker wait (24.5 minutes on B's elaboration).
- **Details**: In pdt, B committed the next day (07:34:59Z) and C launched at 07:36:11Z, so the overnight gap is a session boundary, not engine wait. In glimpse, C launched the next day at 06:54:28Z. In tomlctl-test-and-gate-cost the deferred C left 21 minutes idle, but with a linear graph no ready work waited.
- **Counter**: If a session ended for a reason tied to the hold (for example the user stopping because C was blocked), the overnight gaps would also count as wait. The session-friction lens's transcripts settle that.
- **Impact on plan / report**: The plan's measurement citation should separate the three classes. Most of the observed hold is the serial tail, not the deferral.

Aside, for other lenses: pdt C's change from `deferred` to `elaborated` happened before approval and left no `[[changes]]` row, so its elaboration time is not measurable from the store. The brief's "pool of four" does not match pdt's `max_parallel = 6`.

Coverage: read in full the seven flows' `tasks.toml` and `plan.toml` (via `tomlctl get`), pdt's and glimpse's `agents.toml`, pdt's `tasks.toml` at five commits (milestone elaboration states), run-milestone.md:58-73, and write-and-prove.md:53 and 206. Computed cross-milestone edges, intra-milestone critical paths, file overlap, coupling and pool occupancy, using scratch scripts. The milestones-by-dependency flow itself was excluded, since it holds no rows. Ruled out: v2-harness-tuning occupancy, because it has no `agents.toml`; the v2-apply and human-only-handovers waits, because their graphs are linear.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 58
claim = "the engine picks the first checkpoint in store order with a row not done"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 73
claim = "a deferred milestone stops the engine"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 53
claim = "the How many cut rule: default to one milestone, cut for gate, replan or deferral"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/tasks.toml"
line = 14
claim = "glimpse milestones A, B, C; C rows need only A rows; B and C file-disjoint"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/plan.toml"
line = 524
claim = "glimpse implement run times: B 12:51:59Z-14:06:39Z, C next day 06:54:28Z"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/agents.toml"
claim = "worker segments: one worker for 49.5 of B's 74.7 minutes"

[[source]]
path = ".claude/flows/v2-harness-tuning/tasks.toml"
line = 14
claim = "harness graph A -> B -> {C, D}, E needs A, B, C but not D"

[[source]]
path = ".claude/flows/v2-harness-tuning/plan.toml"
line = 390
claim = "harness implement run times; D launched 11:29:44Z after C"

[[source]]
path = ".claude/flows/tomlctl-test-and-gate-cost/tasks.toml"
line = 14
claim = "linear A -> B -> C with C deferred and elaborated at 12:11:51Z"

[[source]]
path = ".claude/flows/tomlctl-test-and-gate-cost/plan.toml"
line = 318
claim = "C launched 12:14:35Z after B's gate; report at 11:54:12Z names C deferred"

[[source]]
path = ".claude/flows/human-only-handovers/tasks.toml"
claim = "linear A -> B"

[[source]]
path = ".claude/flows/v2-apply/tasks.toml"
claim = "linear A -> B -> C -> apply-1"

[[source]]
path = ".claude/flows/milestone-cadence/tasks.toml"
claim = "single milestone A"

[[source]]
path = "../reportdesignkit/.claude/flows/package-design-tools/tasks.toml"
line = 3041
claim = "pdt A -> {B, C} -> D; B deferred until 13:11:01Z; C elaborated from approval; B and C file-disjoint"

[[source]]
path = "../reportdesignkit/.claude/flows/package-design-tools/plan.toml"
line = 868
claim = "pdt implement runs: B 13:18:06Z-14:03:06Z dispatching 6, 3, 2 tasks; C 07:36:11Z next day"

[[source]]
path = "../reportdesignkit/.claude/flows/package-design-tools/agents.toml"
claim = "worker segments: one worker for 35.5 of B's 45.0 minutes"
```
