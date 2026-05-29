# Lumina — Epic / Focus concept resolution (grill checkpoint)

**Status:** resolved — all open questions closed; ready to consolidate into a final spec
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
6. **Milestone is NOT a separate entity** — the date lives on the epic as an informational `target_date` (never policed). Epics always close **independently**; there is no gating cross-epic shared-release coupling (shared-date edge resolved: NO). If a genuine gating case ever arises it would be an *orthogonal* milestone tag grouping epics (the same cross-cutting shape as the `relevance` axis), **never epic nesting** — deferred under YAGNI until a real instance appears.
7. **Focus boundary rule:** the agent **proposes** focus boundaries (and any later re-home / split) and the human **discusses/confirms** — never silently auto-carved, never auto-applied. Carving is driven by **intent only**: the focus's `shape` plus the stories' shape / functional grouping. **File-footprint is explicitly NOT a carve or re-home signal** — `files_touched` overlap is orthogonal to the hierarchy and belongs to a separate subsystem (parallel-task-execution collision avoidance + sprint composition); it never triggers a re-home. A re-home/split is suggested on **intent/shape mismatch** (a story that no longer fits its focus's shape), agent-proposed and human-confirmed. (Resolves Q-A.2.)
8. **Focus carries a mandatory `shape`** — the genuine behavioural payload the level lacked beyond rollup. `shape ∈ {vertical-slice, cross-cutting, foundational}`, three **positive** classifications (no catch-all / "unclassified" value):
   - **`vertical-slice`** — a coherent end-to-end thread of user-facing value through the layers (e.g. an onboarding flow).
   - **`cross-cutting`** — one concern threaded across many areas at a single aspect/layer (applying an idiom codebase-wide, codebase-wide docs, observability).
   - **`foundational`** — the base layer **other focuses' stories depend on** (structural test: cross-focus dependency — NOT "didn't fit the other two").
   - **Mandatory at carve-time**, forcing the carving reflection — the focus-level twin of decision 5's *"no outcome ⇒ folder, not epic"*: **no shape ⇒ junk-drawer, not focus.**
   - **Revisable** — provisional up front; amended as analysis/categorisation tooling and sharper intent-understanding refine it (NOT driven by file-footprint — see decision 7 / Q-A.2).
   - **Fractal axis** — the same vertical/cross-cutting distinction recurs at the intra-story task-subset scale (a thinner thread of tasks). Shared *vocabulary*, separate *storage*: a focus has exactly ONE shape; a story has 0+ task-groups each with their own shape (the deferred `task_groups` table, which gains its own `shape` column when it lands).
   - **Naming**: `shape`, NOT `focus_kind` — a third `*_kind` column would collide with `work_items.kind` (hierarchy) and `work_items.task_kind` (`foundation|main|polish` phase disposition).
9. **Data-shape (resolves Q-4).**
   - **Epic `outcome`** is a single **mandatory free-text prose** field set at epic creation — a deliberately high-level description of the functionality the app will provide at the epic's *end*, **without** implementation specifics (e.g. a "Lumina Foundation" epic describes the end-state capability set, not the schema). It typically needs **teasing out interactively at creation** (an epic-creation prompt that interrogates the user, mirroring the story `problem-statement` 3-axis prompt). Distinct from the close-criteria: `outcome` is the prose north-star; close-criteria (decision 5, reused `acceptance_criteria`) are the testable breakdown.
   - **Attribute split after the rename**: the old shared `{context, grouping_rationale}` on `epic`|`feature` splits per-kind. Epic = `outcome` (mandatory prose) + optional `context` (background); **drop `grouping_rationale`** (an epic's grouping rationale *is* its outcome). `shape` is a real column (`work_items.shape`, mandatory closed enum), NOT a JSON attribute — like `task_kind`/`tier`/`relevance`.
   - **Focus framing = Option 2**: a focus carries `shape` (mandatory, decision 8) PLUS an **optional** free-text `framing` note (in/out-of-scope), reached for only when stories risk straying. NOT mandatory — `shape` is the carving gate; framing is elaboration. **Stories do NOT inherit framing as copied data** — it is *ambient context* the agent reads when authoring a story under the focus; denormalising it onto stories would only create drift.

### Resulting per-level "done" semantics

| Level | "Done" means | Carries an outcome? |
|-------|--------------|---------------------|
| Task | its work is done (leaf) | — |
| Story | acceptance criteria pass (existing `closure_gate`) | behaviour-shaped |
| **Focus** | **pure rollup** of its stories | **no** |
| **Epic** | **own close-criteria pass AND all stories terminal** | **yes** (mandatory intent) |

This is the genuine behavioural difference the two levels lacked: **epic = outcome + closure gate; focus = scoping context + rollup + mandatory `shape` (decision 8).** Original "two renamed containers" problem resolved.

---

## Open questions (resume here)

1. **Q-A.1 — RESOLVED (→ Locked decision 8).** Vertical-slice/cross-cutting is **one fractal axis** spanning the focus scale and the intra-story task-subset scale — same vocabulary, separate storage, **not** distinct concerns. Modelled as a mandatory, revisable `shape ∈ {vertical-slice, cross-cutting, foundational}` on the focus. The orphaned intra-story grouping is left where it is (prose, future `task_groups`) and gains a `shape` column when that table lands.
2. **Q-A.2 — RESOLVED (→ Locked decision 7, revised).** File-footprint is **not** a carve or re-home signal at all. `files_touched` overlap is orthogonal to the hierarchy — it feeds parallel-task-execution collision avoidance + sprint composition. Focus carving (and any re-home/split) is **intent/shape only**, agent-proposed, human-confirmed, never auto-applied.
3. **Shared-date — RESOLVED (→ Locked decision 6, confirmed).** Epics always close independently; `target_date` is informational only. No milestone entity. A future gating case (if one ever appears) → orthogonal milestone tag, never epic nesting; deferred (YAGNI).
4. **Epic data-shape — RESOLVED (→ Locked decision 9).** Epic `outcome` = mandatory high-level prose (no specifics; teased out at creation), distinct from testable close-criteria. Focus framing = Option 2: optional prose alongside mandatory `shape`, ambient (not inherited).
5. Cosmetic: plural "focuses" in tree headers — accepted, noted.

---

## Implementation sketch (NOT yet approved — notes for later)

- **Rename `feature` → `focus`:** `Kind` enum (`domain.rs:509`), `KINDS` const (`repo.rs:312`), migration trigger (`migrations/0001_init.sql:59-97`), `validate_attributes_for_kind` branch (`repo.rs:111-201`), plus `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` and `CONVENTIONS.md`. Migration to relabel existing `feature` rows. No behavioural code breaks (level is semantically empty today).
- **Epic outcome/intent:** new mandatory attribute on epic creation.
- **Extend `acceptance_criteria` + `closure_gate` to epic level** for close-criteria.
- **Story-creation gate:** reject the first story under an epic that has 0 close-criteria.
- **Epic done transition:** enforce (criteria met) AND (all stories terminal).
- **Mandatory `shape`** on focus — `shape ∈ {vertical-slice, cross-cutting, foundational}`, NOT NULL, CHECK-constrained, revisable. (Resolves the former `focus_kind` candidate, Q-A.1.)
- **Focus `framing`** — optional free-text attribute on focus (in/out-of-scope); ambient, not inherited by stories (decision 9).
- **Attribute split (post-rename):** epic = `outcome` (mandatory prose) + optional `context`, drop `grouping_rationale`; focus = optional `framing` + the `shape` column.
- **Epic-creation prompt:** interactive "tease out the outcome" step at epic creation (mirrors the story `problem-statement` 3-axis prompt) — the outcome is rarely well-formed on first utterance.
- Define `focus`/`epic` semantics explicitly in `SKILL.md` + `CONVENTIONS.md` so agents don't import agile-"feature" assumptions.

## Next step when resuming

Answer **epic data-shape** (last open question) → then consolidate into a final spec and decide the `CONVENTIONS.md` / `SKILL.md` landing + migration plan. (Q-A.1 → decision 8; Q-A.2 → decision 7; shared-date → decision 6.)
