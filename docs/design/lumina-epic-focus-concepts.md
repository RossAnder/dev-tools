# Lumina — Epic / Focus concept resolution (grill checkpoint)

**Status:** in progress — mid-grill checkpoint
**Last updated:** 2026-05-29
**Purpose:** Bed down what lumina's `epic` and `feature` work-item levels actually *mean*, what *data* they carry, and how to make that meaning crisp for agents and users. This file checkpoints the resolution so we can resume; several questions are still open (see [Open questions](#open-questions-resume-here)).

---

## Why this exists

`epic` and `feature` were **byte-for-byte identical** in the implementation — same legal attributes (`context`, `grouping_rationale`), same `relevance="backlog"` default, no kind-specific setter, validation always grouping them `"epic" | "feature" =>`. The *only* difference was tree depth. Two renamed containers carrying identical data — the textbook over-nesting anti-pattern. This work defines genuine semantics so the two levels diverge meaningfully.

Approach: deep research on epic/feature concepts across agile frameworks + tools, a read of lumina's current model, then a Socratic grill to extract the intended semantics.

---

## Research summary (condensed)

- **"Feature" is not a universal level.** Have it: SAFe, Azure DevOps, Aha! (Epic→Feature→Story). Don't: Jira-default (Epic→Story), Mike Cohn/classic Scrum ("epic = big story; these words have no rigorous meaning"), Shortcut ("an epic *is* a feature"), Linear (deletes the vocabulary → Initiative→Project→Issue).
- **The real fault line is deliverable-vs-bucket**, not epic-vs-feature: is an epic a *closeable deliverable* (SAFe/Azure/Shortcut/Aha!) or a *permanent category* (the junk-drawer / "9-month epic is a lifestyle" anti-pattern)? The clean fix is to keep closeable deliverables in the tree and push permanent recurring groupings into a *separate orthogonal* dimension.
- **Data shape changes with altitude:** outcome/hypothesis-shaped at the top → benefit → behaviour (INVEST + acceptance) → effort at the leaf. A level that carries no distinct data shape from its neighbour is a renamed container.
- **Solo / AI-agent context:** most epic/feature machinery exists to coordinate *cross-team handoff* (SAFe feature = "deliverable by one ART in one PI"). With one dev + an agent, that justification evaporates; lightweight tools collapse levels (Linear 3, GitHub flat). The surviving justification for a mid-level is grouping/framing, not coordination.

Full research sources are in the conversation; the agile-"feature" (releasable increment) meaning is explicitly **not** the meaning we adopt — see the rename below.

---

## Current lumina state (the starting point)

- Strict 5-level tree `project → epic → feature → story → task` (`lumina/src/domain.rs:509`, `KINDS` const `lumina/src/repo.rs:312`, `validate_hierarchy_edge` `repo.rs:314`). Single `parent_id`; **no level-skipping** — a kind's parent must be exactly the preceding kind.
- `epic` & `feature` identical (see "Why this exists").
- Existing primitives worth reusing:
  - `work_item_context` (`migrations/0001_init.sql:165`) — an existing **many-to-many** junction ("the drift-killer": one shared row referenced by many work-items).
  - `acceptance_criteria` + `closure_gate` — currently **story-scoped** (`add_acceptance_criterion`, `check_…`, `set_closure_gate`).
  - `files_touched` — set at task-spec time (`set_task_spec`).
  - `task_kind` taxonomy — round-2 had `foundation|vertical-slice|pattern-replacement|polish`; **migration 0007 culled** it to `foundation|main|polish`. `vertical-slice`/`pattern-replacement` are now homeless: CLAUDE.md calls them informal *"intra-story task-subset groupings"* not modelled in schema "until a future migration adds `task_groups`".

---

## Locked decisions

1. **Hierarchy stays** `project → epic → focus → story → task`; strict single-parent tree retained.
2. **Rename `feature` → `focus`.** Rationale: (a) no deliverable connotation — matches "no intrinsic deliverable"; (b) it's the user's own first word ("*areas of focus*"); (c) avoids the agile-"feature" (releasable increment) collision that misled the research. Cheap now because the level is semantically empty (~6 mechanical touch-points, no behavioural break). Plural wrinkle ("focuses") accepted as cosmetic.
3. **Value lives at the story.** The story is the smallest thing that means something; epic/focus are framing above it.
4. **Focus** = a *fluid functional grouping* within an epic.
   - **No intrinsic deliverable.** Can be a functional area, a doc area, a cross-cutting concern — fluid.
   - **Per-epic instance**, NOT a shared/many-to-many entity. The same *name* ("Documentation") recurring across epics is a fresh instance each time. This gives a focus a **natural lifespan — it dies when its epic closes**, so it structurally cannot rot into the eternal-bucket anti-pattern.
   - Provides **scoping context / framing** for its stories (in/out-of-scope).
   - **"Done" = pure rollup** of its stories. No outcome, no independent criteria.
5. **Epic** = a **closeable deliverable**. The *only* level with **both** an independent outcome gate **and** a rollup gate.
   - **Mandatory at creation:** an outcome / intent statement (may be fuzzy, e.g. "POC usable for internal release"). *No outcome ⇒ it's a folder, not an epic.*
   - **Optional & emergent:** `target_date` (purely **informational, never policed/enforced**) and a set of **close-criteria**.
   - Close-criteria **reuse `acceptance_criteria` + `closure_gate`**, extended up from story to epic — no new mechanism.
   - **Creation gate:** an epic must carry **≥1 close-criterion before its first story can be created** beneath it (since `focus` sits between, the gate fires when the first *story* in the epic's subtree is created). Purpose is to **force reflection** — articulate *a* definition of done before committing work. Criteria are **revisable** thereafter (acceptance-criteria are mutable), so "emergent" = *provisional up front, refined as you go*, not *absent until late*.
   - **Done when:** close-criteria met **AND** all stories **terminal** (`done` *or* `cancelled`/`wontfix` — abandoned work never blocks closure). No early close; rollup is a **gate**, not just info.
6. **Milestone is NOT a separate entity** — the date lives on the epic. (Pending only the multi-epic-shared-release edge, currently assumed "no".)
7. **Focus boundary rule:** the agent **proposes** focus boundaries and the human **discusses/confirms** them (not silently auto-carved). Informing signals:
   - **Story shape** — vertical-slice vs cross-cutting.
   - **File-footprint overlap** with existing focuses' children — but this is known *late* (at task-spec time), so it's a **re-home / split** signal, not an initial-carve signal.

### Resulting per-level "done" semantics

| Level | "Done" means | Carries an outcome? |
|-------|--------------|---------------------|
| Task | its work is done (leaf) | — |
| Story | acceptance criteria pass (existing `closure_gate`) | behaviour-shaped |
| **Focus** | **pure rollup** of its stories | **no** |
| **Epic** | **own close-criteria pass AND all stories terminal** | **yes** (mandatory intent) |

This is the genuine behavioural difference the two levels lacked: **epic = outcome + closure gate; focus = scoping context + rollup only.** Original "two renamed containers" problem resolved.

---

## Open questions (resume here)

1. **Q-A.1 — Is "vertical-slice vs cross-cutting" on a *focus* the same axis as the culled intra-story `task_kind` groupings?**
   The vertical-slice/pattern-replacement axis was removed from `task_kind` in migration 0007 and left as informal intra-story groupings with no schema home. The focus may be that axis's *true level*. If so:
   - candidate typed attribute **`focus_kind ∈ {vertical-slice, cross-cutting}`** (more distinguishing data; gives the agent a crisp first-cut rule);
   - possibly retires/relocates the orphaned intra-story grouping concept.
   - OR they're **distinct concerns**: a focus groups *stories by shape*; an intra-story group bundles *tasks for co-implementation*. Need the user's read.
2. **Q-A.2 — Confirm file-footprint is a re-home/split signal** (known late), not an initial-carve signal.
3. **Shared-date edge** — confirm no scenario where several *different* epics must ship as one coordinated dated release (if there is one → a milestone entity is needed after all).
4. **Epic data-shape detail** — exact form of the outcome/intent field; does a `focus` carry an explicit scope / out-of-scope framing its stories inherit?
5. Cosmetic: plural "focuses" in tree headers — accepted, noted.

---

## Implementation sketch (NOT yet approved — notes for later)

- **Rename `feature` → `focus`:** `Kind` enum (`domain.rs:509`), `KINDS` const (`repo.rs:312`), migration trigger (`migrations/0001_init.sql:59-97`), `validate_attributes_for_kind` branch (`repo.rs:111-201`), plus `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` and `CONVENTIONS.md`. Migration to relabel existing `feature` rows. No behavioural code breaks (level is semantically empty today).
- **Epic outcome/intent:** new mandatory attribute on epic creation.
- **Extend `acceptance_criteria` + `closure_gate` to epic level** for close-criteria.
- **Story-creation gate:** reject the first story under an epic that has 0 close-criteria.
- **Epic done transition:** enforce (criteria met) AND (all stories terminal).
- **Candidate `focus_kind`** typed attribute — pending Q-A.1.
- Define `focus`/`epic` semantics explicitly in `SKILL.md` + `CONVENTIONS.md` so agents don't import agile-"feature" assumptions.

## Next step when resuming

Answer **Q-A.1, Q-A.2, shared-date** → then consolidate into a final spec and decide the `CONVENTIONS.md` / `SKILL.md` landing + migration plan.
