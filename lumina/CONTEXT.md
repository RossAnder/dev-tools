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
A separately-composed execution session over a chosen subset of ready **Tasks** — the unit one implement **Team** works end to end. Multiple sprints may run concurrently within the same project/epic/focus/story; a sprint is **composed before it is queued** for execution, and is the scope within which file-edit collisions are avoided (cross-sprint isolation is physical — see Worktree under Flagged ambiguities).
_Avoid_: iteration; milestone (a sprint is a sizing/execution unit, never a dated release).

**Composition** (sprint composition):
The up-front, human- or agent-driven act of choosing which ready tasks enter a **Sprint** and how big the sprint is. Computed task batches are *suggestions* offered to the composer (a quick-add aid), never an imposed schedule — the human controls sprint sizing (a slice of a plan, or the whole thing).

**Team**:
The pool of implement/review agents assigned to exactly one **Sprint**, its member mix chosen to suit that sprint's task types. Agents *draw* the next ready task from their sprint rather than being assigned individual tasks up front.

**File scope** (advisory):
The best-effort set of files a **Task** expects to touch — a *caution* signal for collision, never a hard constraint. It informs **Composition** and is surfaced as a prima-facie overlap warning while a sprint runs, but the implementation may legitimately touch files beyond it, so it encourages caution without ever restricting which task an agent may take.

## Relationships

- A **Project** contains one or more **Epics**.
- An **Epic** contains one or more **Focuses**; it closes only when its **Close-criteria** pass AND every descendant **Story** is terminal.
- A **Focus** belongs to exactly one **Epic** (a recurring name like "Documentation" is a fresh per-epic instance, never shared) and carries exactly one **Shape**.
- A **Focus** contains one or more **Stories**; its "done" is their rollup.
- A **Story** contains one or more **Tasks**.
- **Shape** is a fractal axis: the same `vertical-slice`/`cross-cutting` distinction recurs at the intra-story task-subset scale (one focus has one Shape; one story has 0+ task-groups, each with its own shape).

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
- **Worktree = the inter-sprint isolation boundary (a consumer concern).** Each **Sprint**/**Team** runs in its own git worktree, so file-edit conflicts are reckoned only *within* a sprint; reconciling across concurrently-running sprints is a worktree *merge*, performed by a (deferred) overseer agent — never by lumina. lumina's queue stays worktree-agnostic: being **Sprint**-scoped, it is already correct for any number of concurrent worktree-isolated sprints. Whether lumina should *record* the sprint→worktree binding (for the overseer to locate) is an open follow-up.
- **Milestone is not an entity.** A coordinated multi-epic dated release is not modelled — epics close independently and the date is an informational `target_date` on the epic (a domain concept not yet modelled in schema). A genuine gating case would become an orthogonal tag, never a tree level.
