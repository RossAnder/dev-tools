# Lumina — Work-item domain

Lumina is a SQLite-canonical flow-tracking store. Its core domain is a strict five-level work-item hierarchy and the planning/execution lifecycle layered on it. This glossary fixes what each level *means* and the terms that distinguish them; the decisions behind these terms live in `../docs/design/`.

## Language

### Hierarchy levels

**Work item**:
The single row type for every node in the hierarchy; its `kind` places it at one of the five levels below.

**Project**:
The root of a hierarchy (NULL parent); owns the linked GitHub repos.

**Epic**:
A closeable deliverable carrying a mandatory **Outcome** and ≥1 **Close-criterion** — the only level with both an independent closure gate and a rollup gate.
_Avoid_: bucket, category (a permanent bucket is the anti-pattern an epic must not become).

**Focus**:
A fluid functional grouping of stories within one **Epic**, carrying a mandatory **Shape**; its "done" is a pure rollup of its stories, with no outcome of its own.
_Avoid_: feature (the agile "releasable increment" sense is explicitly NOT this concept — see Flagged ambiguities).

**Story**:
The altitude at which a full plan (problem → approach → tasks) is authored; the smallest item that carries value, closed by its acceptance criteria.
_Avoid_: ticket, issue.

**Task**:
A leaf unit of work under a story, carrying effort/complexity/tier and a phase disposition (`task_kind`).

### Distinguishing attributes

**Outcome**:
An epic's mandatory intent statement (may be fuzzy, e.g. "POC usable for internal release"); no outcome ⇒ it is a folder, not an epic.

**Close-criterion**:
An acceptance criterion attached at the epic level (reusing the story mechanism) that gates epic closure; an epic needs ≥1 before its first story can be created.

**Framing**:
A focus's optional in/out-of-scope note; ambient context an agent reads when authoring a story under the focus, never copied onto the story.

**Shape**:
The mandatory, revisable classification of a **Focus** — `vertical-slice`, `cross-cutting`, or `foundational` — answering "why is this a separate focus?".
_Avoid_: focus_kind, type (`kind` is reserved for the hierarchy level and `task_kind` for a task's phase disposition).

**Vertical-slice** (shape): a coherent end-to-end thread of user-facing value through the layers.

**Cross-cutting** (shape): one concern threaded across many areas at a single aspect/layer (codebase-wide idiom, docs, observability).

**Foundational** (shape): the base layer other focuses' stories depend on (a structural test, not a leftover bin).

**Task-group** (not yet modelled in schema):
An intra-story subset of tasks implemented, tested, and committed as a unit; carries its own **Shape** — the same axis as a **Focus**, one scale down. A story has zero or more; deferred to a future `task_groups` table.

### Status & lifecycle axes

**Relevance**:
A work item's orthogonal lifecycle axis — `active`, `backlog`, `deferred`, or `rejected` — independent of both its status and its place in the tree.
_Avoid_: status ("relevance" is *should we be doing this?*; status is *where is the work?*).

**Phase disposition** (`task_kind`):
A task's role within its story's execution order — `foundation` (prerequisite, sorts earliest), `main` (core work, the default / NULL), or `polish` (hardening, sorts latest).
_Avoid_: kind, shape (three distinct "kind-ish" concepts — see Flagged ambiguities).

**Closure gate**:
A story's `closure_gate` mode; in `hard` mode it blocks a task reaching `done` while any of the story's acceptance criteria are unchecked.

**Terminal**:
A final work-item status — completed (`done`) or abandoned (`cancelled`) — that no longer blocks its parent's closure rollup.

**Target date** (not yet modelled in schema):
An epic's optional, purely informational ship-date target; never policed or enforced. A domain concept only — no migration or `domain::WorkItem` field carries it yet.

### Execution

**Sprint**:
A separately-composed execution session over a chosen subset of ready **Tasks** — the unit one implement **Team** works end to end. Multiple sprints may run concurrently within the same project/epic/focus/story; a sprint is **composed before it is queued** for execution, and is the scope within which file-edit collisions are avoided (cross-sprint isolation is physical — see Worktree under Flagged ambiguities). A sprint carries a typed lifecycle `status` — `draft → ready → active → review → done` (or `cancelled`) — enforced at the repo layer (legal transitions: `draft→ready`; `ready→{active,cancelled}`; `active→{review,done,cancelled}`; `review→{done,cancelled}`; `done`/`cancelled` terminal). **Its tasks are claimable ⟺ `status='active'`** (the claim's runnable guard); a sprint that **owns** a **Worktree** stays in `review` until that worktree is **Merged** or rejected, and can only reach a terminal status through the merge/rejection record (never a bare `set_sprint_status` flip), so the merge audit is never skipped.
_Avoid_: iteration; milestone (a sprint is a sizing/execution unit, never a dated release).

**Composition** (sprint composition):
The up-front, human- or agent-driven act of choosing which ready tasks enter a **Sprint** and how big the sprint is. Computed task batches are *suggestions* offered to the composer (a quick-add aid), never an imposed schedule — the human controls sprint sizing (a slice of a plan, or the whole thing).

**Team**:
The pool of implement/review agents assigned to exactly one **Sprint**, its member mix chosen to suit that sprint's task types. Agents *draw* the next ready task from their sprint rather than being assigned individual tasks up front.

**File scope** (advisory):
The best-effort set of files a **Task** expects to touch — a *caution* signal for collision, never a hard constraint. It informs **Composition** and is surfaced as a prima-facie overlap warning while a sprint runs, but the implementation may legitimately touch files beyond it, so it encourages caution without ever restricting which task an agent may take.

**Run**:
A review or optimise pass over a **Sprint** or **Story** (status `open → triaged → closed`) that produces findings — the *coarse* review granularity, distinct from the per-task review the execution queue cascades. A review **Run** over a completed sprint can motivate a follow-up review/fix **Sprint** that **targets** the same **Worktree** (sharing its `worktree_id` without owning it) before that worktree's **Merge** — chained back to its predecessor via `predecessor_sprint_id`.

**Merge** (worktree merge):
The reconciliation of a **Worktree**'s accumulated work into the base branch, once its **chain** of sprints is done and any required review/fix round has passed (a judgement call, not always required). Performed by the **Merge supervisor** driving the **Companion** — the execution plane runs git; a worktree **merges once**, at the end of its sprint chain. The **Control plane**'s role is **pure audit**: the lumina store records the worktree/merge lifecycle (path, merge ref, `merged`/`rejected` outcome) as a durable audit/intent log, transitions the **owning Sprint** `review→done` on merge (or `review→cancelled` on rejection), and the **server/store never shells out to git** — git stays the source of truth for actual merge state.

**Checkpoint** (task flag):
A flag marking a **Task** as a sprint-wide **commit barrier**. The instant a checkpoint task is claimed (`in_progress`), the queue freezes new claims across the whole sprint; in-flight work drains to a coherent state; the holding agent stages and commits the entire shared worktree, then completes the checkpoint (releasing the freeze) and commits the staged snapshot. This is a **runtime freeze only** — the barrier is a live claim-time guard (`claim_next_task` yields `Ok(None)` while any checkpoint task is `in_progress`), **not** wired as a task→task dependency edge (smart DAG ordering is layer-3's concern). The commit↔task map is held in lumina as **explicit task-id-list** provenance (`record_task_commits` — the committing lead passes the covered task-ids; pure audit, never derived from completion timestamps). Yields whole, non-broken commits at chunk boundaries on the shared worktree, clean git with no harness trailers. See `../docs/adr/0003-commit-checkpoint-provenance.md` (the two open items it left) and `../docs/adr/0005-sprint-lifecycle-worktree-ownership.md` (their resolution).

### Observation & analysis plane

**Session**:
One `claude` process's conversation transcript — the atomic unit of capture; either *spawned* (lumina launched it under a PTY) or *ingested* (a terminal session lumina did not launch, captured once at its end via a `SessionEnd` hook).
_Avoid_: conversation; run (a **Run** is a review/optimise pass — a different concept).

**Transcript**:
The ordered record of a **Session**, stored losslessly (every JSONL record kept verbatim) with a curated render-view derived from it.

**Corpus**:
The single cross-project collection of all captured **Sessions** — the durable substrate harness analysis reads.

**Stitch**:
To compose a higher-level transcript by gathering the **Sessions** that share a correlation key (sprint, story, agent, project, or time window) and interleaving their records by time.

**Dreaming**:
A scheduled, after-the-fact analysis pass over the **Corpus** that surfaces recurring patterns for documentation and harness tuning — the *engine* layer (deferred); only its trigger seam is built now.
_Avoid_: reflection, mining.

**Clone directory**:
The per-machine absolute path of a linked repo's local working copy (absent when the repo is not cloned on this machine); the anchor that resolves a **Session**'s cwd to its **Project** and repo-relative paths to absolute.

**Clone root**:
The per-machine default parent directory under which an unbound linked repo is offered for cloning.

### Control & execution planes

**Control plane**:
The lumina **Server** and its SQLite store — the canonical record of work-item, sprint, **Worktree**, and **Merge** intent + outcome. It is **record-only**: it never shells out to git and carries no filesystem-git code. A remotely-hosted control plane is physically unable to reach a local working tree, which is *why* execution is a separable plane.
_Avoid_: "lumina" as a synonym for the whole system when stating the record-only invariant — record-only is a property of the **Control plane**, not of the **Companion**.

**Execution plane**:
The local, detachable side that performs the filesystem and git work a remote **Control plane** cannot — git execution and PTY-hosted agent **Sessions**. Realised by the **Companion**. Named to sit alongside the **Observation & analysis plane**.

**Companion**:
The local execution-plane binary. Performs mechanical git operations (e.g. worktree add, branch create, the checkpoint commit choreography) triggered by workflow steps, hosts PTY agent sessions, and reports outcomes back to the **Control plane** for recording. Defined against the **Server**: the server records, the companion executes.
_Avoid_: calling it the "consumer" or "overseer" — those conflated judgement with mechanism (see **Merge supervisor**).

**Merge supervisor**:
The agent that makes the judgement calls on a **Worktree**'s **Merge** — whether the review gate is passed, and which branches merge in what order — performs the merge, and reports what it did for the **Control plane** to record. The concrete realization of the merge-judgement slice of ADR-0002's deferred overseer engine; it supplants the older undifferentiated "consumer/overseer" for the merge case.

## Relationships

- A **Project** contains one or more **Epics**.
- An **Epic** contains one or more **Focuses**; it closes only when its **Close-criteria** pass AND every descendant **Story** is terminal.
- A **Focus** belongs to exactly one **Epic** (a recurring name like "Documentation" is a fresh per-epic instance, never shared) and carries exactly one **Shape**.
- A **Focus** contains one or more **Stories**; its "done" is their rollup.
- A **Story** contains one or more **Tasks**.
- **Shape** is a fractal axis: the same `vertical-slice`/`cross-cutting` distinction recurs at the intra-story task-subset scale (one focus has one Shape; one story has 0+ task-groups, each with its own shape).
- A **Session** binds to its **Project** (via the **Clone directory** its cwd falls under) and, when it ran lumina commands, to its **Sprint** / **Agent** / **Task** — all *recovered at ingest* by parsing the lumina MCP tool records embedded in the session's own **Transcript** (the tools return lumina-minted ids), never from injected environment. **Task** attribution follows the agent's claim/lease timeline within those records.
- A **Sprint**'s transcript is **Stitched** from the **Sessions** its **Team**'s agents produced (one `claude` process = one **Session**, so a team yields many).

## Example dialogue

> **Dev:** "This 'Documentation' work touches every module — is it one **Focus** shared across the two **Epics** that need docs?"
> **Domain expert:** "No. A **Focus** lives in exactly one **Epic**, so that's two separate 'Documentation' focuses — each dies when its epic closes. Its **Shape** is `cross-cutting`. Whether its tasks can run in parallel with the code focuses' is a *separate* question — that's file-overlap, not shape."
> **Dev:** "What stops a **Focus** rotting into a permanent bucket?"
> **Domain expert:** "Its mandatory **Shape** forces a reason to exist, and its lifespan is bounded by its epic. The eternal-bucket risk lives at the **Epic** level instead — which is why an epic needs a real **Outcome**."

## Flagged ambiguities

- **"Feature" → "Focus".** The level was renamed: the agile "feature = releasable increment" meaning is explicitly NOT adopted. A focus has no intrinsic deliverable; the closeable deliverable is the **Epic**.
- **Three "kind-ish" concepts kept distinct.** `kind` = hierarchy level (project…task); `task_kind` = a task's phase disposition (`foundation|main|polish`); **Shape** = a focus's vertical/cross-cutting/foundational axis. The focus axis is named `shape` precisely to avoid a third `*_kind`.
- **Vertical-slice/cross-cutting at two scales.** The same axis describes a coarse focus and a fine intra-story task-subset; same vocabulary, separate storage — not distinct concepts, not one shared table.
- **File overlap is not a carving signal — and is advisory, not a gate.** `files_touched` overlap is a best-effort *caution* signal that informs sprint composition and intra-sprint collision *awareness*, not focus boundaries; carving is intent/shape only. It is never a hard constraint: file scope is best-effort (implementation routinely touches files outside the declared set), so the queue surfaces overlap as a prima-facie caution and never blocks a claim on it.
- **Worktree = the inter-sprint isolation boundary AND the merge unit — owned by exactly one sprint, with status derived.** Each **Team** runs in a git worktree; a worktree hosts a *chain* of **Sprints** in sequence — an implementation sprint followed (by judgement, *not* always) by a review/fix sprint that cleans up the first sprint's findings in the *same* worktree — and **merges once**, at the end of that chain (`worktree : sprint = 1 : many`, now resolved to a designated **owner**). A worktree is **owned by exactly one Sprint** (`worktrees.owning_sprint_id`, a UNIQUE FK → `sprints`); **follow-up sprints *target* the same worktree** (they share `sprints.worktree_id`) **but do not own it**. There is **no free-floating worktree status**: a worktree has **no `status` column** — its **effective status is wholly derived** from its owning sprint (`get_worktree` returns it by JOIN). The owning sprint stays in `review` until the worktree is **Merged** or rejected; the worktree carries only audit-terminal fields (`merged_at`, `merge_ref`, `outcome ∈ merged|rejected`), stamped at merge/rejection. File-edit conflicts are reckoned only *within* a sprint's run; the worktree→base **Merge** is performed by the **Merge supervisor** driving the **Companion**, never by the lumina **server/store**. lumina's *queue* stays worktree-agnostic (being **Sprint**-scoped it is already correct for any number of concurrent worktree-isolated sprints), but lumina *does* **record** the sprint/worktree lifecycle — worktree path, run records, merge/rejection details — as a durable audit/intent log (relations to higher work items — story/focus/epic — are *inferred* via the task hierarchy, not stored as explicit sprint links). **git remains the source of truth for actual merge state; the lumina Control plane (store/server) records intent + outcome and never shells out to git.** (See ADR-0005.)
- **Milestone is not an entity.** A coordinated multi-epic dated release is not modelled — epics close independently and the date is an informational `target_date` on the epic (a domain concept not yet modelled in schema). A genuine gating case would become an orthogonal tag, never a tree level.
- **"Session" — runtime PTY state vs captured corpus.** lumina's `pty_sessions` began as *runtime* state for a spawned REPL (explicitly "not a domain entity"). A captured **Session** generalises that row family to cover *ingested* terminal transcripts too and elevates it to a durable, analysed **Corpus** record — but it stays **export-inert**: a **Session** is an *observation*, not work intent, so it never participates in the `+1 work_items / +1 events` invariant.
- **Clone directory is single-machine for now.** The per-machine clone path lives on the shared repo-link row, which is correct only while one lumina serves one machine. A shared-remote lumina + local stubs would make that path per-machine-ambiguous and force a per-machine path layer — deliberately deferred (see ADR-0004).
- **A "harness session" is defined by its content, not its launch.** A captured **Session** counts as harness-controlled — and gains **Sprint**/**Agent**/**Task** correlation — *iff its transcript contains lumina MCP tool calls*. A session with none binds to its **Project** by cwd only (ambient), and may be dropped from the **Corpus** entirely. The `SessionEnd` hook fires indiscriminately; the keep/correlate decision is made at ingest by harvesting lumina's own tool records (see ADR-0004).
