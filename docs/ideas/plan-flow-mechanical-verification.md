# Plan-flow hardening — measured diagnosis, applied changes, deferred work

> Response to `tradewinds-portal/docs/plans/reportdesignkit-theme-packages-consumption.plan-defects.md`
> Drafted 2026-08-11, **revised the same day after an independent adversarial review**
> disproved the first draft's central claim. Targets `/plan-new`, `/review-plan`, `/implement`.

## 0. Retraction: the first draft's root cause was wrong

The first draft argued that `/review-plan` under-enumerated because its finding budget was
binding — `research-deep` defaults to ≤8 findings (`claude/agents/research-deep.md:77`),
`/review` overrides to 20 (`review.md:43`) and `/review-plan` does not, so 4 lenses × 8 = 32
slots against a recorded 33 findings. "The harness told the reviewer to stop."

**That is arithmetic coincidence over a distribution that is nowhere near uniform.** Measured:

```
feasibility 11 | executability 9 | risk 8 | completeness 5      (all review_round = 1)
```

Two lenses **exceeded** the supposedly-binding cap. The lens owning §2's twelve unowned
files — completeness — returned **5 of 8** slots and still missed twelve. Per-lens word volume
ran 1393–2772 against a ≤700 default. **No cap constrained anything.** Raising the ceiling would
have bought nothing, and would have contradicted the agent's own anti-padding contract
(`research-deep.md:78-80`: "1 high-evidence finding … beats 8 marginal ones").

Two further factual corrections:

- **The reviewed document was not the 1073-line, 28-task plan.** `/review-plan` read
  `.premerge.md`: **727 lines, 27 tasks**. 1073/28 is the *post-merge* document — the review's
  own output.
- **The "File-claim check" self-audit block was written by `/review-plan`, not by the plan
  author.** It does not exist in `.premerge.md`. The first draft built a generalisable rule
  ("never trust a plan's self-audit") aimed at the wrong author.

The corrected diagnosis follows from that last point.

## 1. Actual root cause: the merge is an unreviewed write

`/review-plan` Step 4A rewrote the plan in place: **727 → 1073 lines (+48%)**, 27 → 28 tasks,
~550 changed lines. All 33 findings went to `merged`, `round = 1`. **No round-2 review ever
ran.** The merged text — the least-reviewed prose in the document — is what `/implement` then
executed.

Running the first draft's own flagship check against both versions:

| Check | premerge (what the review read) | post-merge (what `/implement` ran) |
|---|---|---|
| file claimed by ≥2 tasks with no directed path between them | **0** | **1** — `demo/pipelineApi.ts`, tasks 16 ↔ 27 |
| `## Dependency Graph` diverges from per-task `Depends on` | 3 (exactly P31's, correctly graded `suggestion`) | 4 |

Premerge task 16's **Files** line was `demo/themeProvider.ts`, `demo/routes.tsx`. The merge
**added `demo/pipelineApi.ts`** to it while absorbing P3/P4, then wrote a self-audit asserting
"No edge-less collision remains" — and never re-derived that assertion against the Files line it
had just changed.

**The merge created the only defect of this class in the entire plan.** The fix belongs at merge
exit, not in the authoring or review contract.

Two structural consequences, both now fixed:

- `review-plan.md` Step 4A/4B had **no exit check** — A2 licenses rewriting Files lines, A3 wrote
  the file, and nothing re-derived cross-task consistency over the result.
- Re-run dedup said "`merged` / `discarded` findings are ignored by lens-agents", so a round 2
  would skip **precisely the highest-churn, least-reviewed regions** of the document. Backwards.

## 2. Second root cause: a lens that cannot run the check it is asked to make

`research-deep.md:4` grants Glob, Grep, Read, Skill, ToolSearch, WebSearch, WebFetch and
Context7/Playwright. **No Bash.**

Triage §4's largest class — "a claim about what the compiler or linter will do, asserted without
running it", four instances, three of them guards that silently do not fire — is therefore
*unsettleable* by any `/review-plan` lens. A lens can suspect it; only something with Bash can
verify it. Same for the cross-task acceptance contradiction (§3), which is a whole-plan scan the
orchestrator is better placed to run anyway.

**Resolved 2026-08-18.** Both research agents now hold `Bash`, scoped to non-mutating
verification (no Edit/Write, no working-tree or environment mutation, no whole-crate builds or
full suites — those stay with the orchestrator's `verification` agent). A lens can now settle a
tooling-behaviour claim itself and grade it `high`. The §3 cross-task scan stays orchestrator-owned
for the reason given above, and so does check 1's *baseline* command — one whole-tree run shared by
every lens, not four parallel ones against the same build directory.

## 3. Third root cause: judgement framing where a procedure was needed

`review-plan.md:51` asked the completeness lens for "affected-but-unmentioned
files/components/consumers" — an adjective, with no enumeration procedure. It returned five
findings, under budget, and missed twelve files. Seven of those twelve were **prose** — comments,
docblocks, UI copy — that the code change falsified. That is a different search from the one
"affected files" evokes, and no amount of extra budget produces it.

---

# Applied changes

All in `dev-tools`. Nothing was changed in `tradewinds-portal`.

### `claude/skills/flow-contract-plan-output-format/SKILL.md`

The skill is the highest-leverage surface: `/plan-new` Phase 7 loads it, `/plan-update reformat`
loads it, and `review-plan.md:47` already embeds its **Format rules** verbatim into Agent 3's
prompt because `research-deep` cannot invoke skills. **A rule landed here reaches all three
commands for free.** Added or extended:

- **Acceptance-reachability** — `Depends on` covers what the *Acceptance* transitively loads, not
  just what the Action needs; test files couple at collection time. (4 of the 6 missing edges.)
- **Falsifiable acceptance** — state what makes it fail; the vacuous forms enumerated.
- **Registration seams need a call site** — the one load-bearing miss in the whole flow.
- **Derive, don't transcribe** — narrowed on review to filenames, counts and allowlist
  memberships. Line numbers are explicitly **exempt**: they are the anchor agents use to *find* a
  site, drift costs a re-locate, and triage §10 records that the plan's citations were mostly
  right and load-bearing.
- **Repo-relative paths everywhere**, including prose and acceptance commands, with the working
  directory stated on any command that needs one. Dissolves the citation-resolution problem by
  convention rather than by code, and answers P25 (20 package-relative commands, no stated cwd).
- **Checkpoint coverage** — every task should fall inside some marker's closure.
- **No literal control bytes** in a plan document, with a detection command.

### `claude/commands/review-plan.md`

- **New Step 4A A3.5 / extended B4 — merge-exit consistency re-derivation.** File-claim
  *reachability* (a common ancestor is not a path, computed pairwise over the ancestor closure —
  not eyeballed), graph mirroring, checkpoint coverage, Files-line closure. Runs **after** the
  write so there is a file on disk to read, with `.premerge.md` as the rollback. Explicitly
  forbids emitting a self-audit block. The same re-derivation is now referenced from
  `flow-contract-plan-restructure`, since `/plan-update reformat` renumbers checkpoint markers
  and is the other op that rewrites a plan in place.
- **A6** — reports what the re-derivation changed and closes with
  `merged text is unreviewed — re-run /review-plan for round 2` whenever Files/Depends-on/task
  count moved.
- **Re-run dedup inverted** — `merged` findings are now passed as *merge-provenance context* and
  their sections are read **first**.
- **New Step 2.6 — orchestrator-only checks** (run the predicted break sets; cross-task
  acceptance contradiction; falsifiability), placed where Bash exists.
- **Finding budget** — stated as `≥ 3 / target 15 / ceiling 20`, mirroring `/review` exactly so
  the two carriers stop diverging, plus an **enumerate-don't-sample** rule: a finding covering N
  instances must name all N; merging is fine, sampling is not.
- **Agent 1/2 rebalance at constant lens count** — Agent 1 sheds codebase-alignment and its
  "if >10 findings … merge related ones" cap, keeping dependencies + execution policy + a new
  checkpoint-coverage check. Agent 2 gains alignment plus a **four-step enumeration procedure**
  (prose surface, seam call sites, inclusion criteria, transcribed enumerations), each reporting
  a line even when empty so a skipped sweep is visible rather than inferred.
  **No fifth lens — but this is not net-neutral on text, contrary to an earlier claim here.**
  Measured across the four lens bullets: **463 → 687 words (+48%)**, and Agent 1 grew
  (190 → 208) rather than shrank, because checkpoint-coverage outweighed what it shed. The
  growth is deliberate — 39 words for the lens that under-produced *was* the diagnosis — but the
  live risk is that Agent 2 now carries three whole-tree greps and does the first thoroughly
  while skimming the rest. The per-sweep reporting line is the cheap guard; if it proves
  insufficient, move sweep 2 (seam call sites) to Agent 1, whose scope already covers "who calls
  this".

### `claude/commands/plan-new.md`

Three extensions to existing Phase-6 steps (no new blocks, per the skim-readable constraint):
acceptance-reachability in step 4, prose-surface counting in step 6, and "run the predicted break
sets now" in step 8.

### `claude/commands/implement.md`

- **Falsifier check** in the dispatch rules — **in-editor only, never `git`**, since delegates are
  barred from working-tree ops and up to `max_parallel` agents share the tree.
- **Zero-tests-executed ⇒ fail** at the step-5(ii) corroboration, orchestrator-side, leaving the
  `verification` agent's no-interpretation contract intact.
- **New step 3c — task-mint protocol.** Confirmed gap: nothing in the file described minting, so
  this run minted tasks 29/30, set `[tasks].total = 30`, and left the document at 28.

---

# Deferred: `tomlctl plan lint`

The first draft proposed a nine-check linter. Most of it does not survive measurement.

**Dropped:**

- `files/closure` — I implemented the check and ran it: **80 of 117 path-shaped tokens flagged
  across 28 tasks, a 68% flag rate**, against ~4 real omissions (P7, P8). ~5% signal. The format
  contract *requires* most of those absences — `SKILL.md:115`: read-only references "MUST NOT be
  listed". Task 1's Acceptance alone names ten files it does not edit, because it is a predicted
  break set. No path-token heuristic separates "edit target" from "read-only reference" without
  reading the prose; that *is* the judgement task.
- `dag/graph-divergence` — `implement.md:42` is explicit that the graph prose is **advisory** and
  per-task edges authoritative. Hard-erroring on it would have blocked this plan on three
  harmless entries that P31 graded `suggestion`, correctly.
- `cite/*` — dissolved by the repo-relative format rule above; `research-deep` already does this
  well (P13, P14, P32).
- `effort/files-mismatch` — 4 hits on this plan, all deliberate.
- `checkpoint/invalid-cut` — **scores zero here.** The markers were valid.

**Kept, if built:** `dag/unreachable-claim` (reachability, not common ancestor),
`dag/dangling-ref`, `dag/cycle`, `dag/duplicate-number`, and — replacing invalid-cut —
**`checkpoint/orphan-task`**, which is the high-yield one: measured against the executed plan,
**10 of 28 tasks (36%) sit inside no marker's closure** (3, 11, 14, 16, 19, 20, 21, 22, 23, 25)
and are committed only by the Phase-3 final train, forfeiting the bisectability that chose
`milestones` over `single`.

**Cheaper interim, now in place:** Step 4A A3.5 makes the reachability re-derivation an explicit
orchestrator step. The orchestrator has Bash, Read, and the whole plan in context, and the check
is a closure over ≤30 tasks. Build the verb only if that proves unreliable in practice.

# Not harness

Triage §8's environment traps and §10a's tsconfig gap belong in `tradewinds-portal`'s own
`CLAUDE.md` — **with one exception now promoted**: the literal-`U+0000` hazard is a harness
problem, since `.premerge.md` carries 6 U+0000 and 2 U+0001 bytes and any tool shelling out to
`grep`/`diff` degrades silently on it. It is now a format rule.

# Open

- **A finding count is not a quality measure.** The first draft proposed re-running `/review-plan`
  and counting findings; since every defect class *was* reached, the measurable question is
  **per-class recall** against the triage's enumerated instances. A proper fixture needs the
  premerge document and a per-class expected set.
- **The observed high-recall detector in this flow was `/implement`'s delegates, not
  `/review-plan`.** All twelve unowned files were found by implementers noticing adjacent
  falsehood; four of five vacuous assertions were found by agents *volunteering* a falsifier
  check; the 16↔27 collision was caught by the orchestrator at DAG construction (`E5`), one phase
  earlier than the dispatch gate. That argues for continuing to strengthen the dispatch contract,
  not just the review contract.
- **The triage document's own figures drifted** — it reports 69 deviations while the record holds
  75, and declares the counts settled.
