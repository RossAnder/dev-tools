---
name: set-task-spec
description: Walk a story's task children and capture per-task spec (execution_detail, files_touched, dual-track outcome, dispatch tier).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:set-task-spec`

Per-task spec writer. Invoked on a STORY id; walks each TASK child of the story and collects four spec attributes — `execution_detail` (free-text step-by-step plan), `files_touched` (concrete file paths, with R25 pattern-replacement drift check when applicable), `outcome` (dual-track per R23: `automated` + `manual`), and `dispatch` (tier: `lite` or `deep`) — writing them via `mcp__lumina__set_task_spec`. The skill assumes the story's tasks have already been created by `/lumina:decompose-tasks`; it does NOT create tasks. The downstream consumer is `/lumina:wire-task-deps` (which writes the task→task edges and fires the R27 complexity-high gate, not this skill).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied **per-task-child** per §b-per-element), §c (provenance recording — one entry per TASK touched, not per skill invocation), §e (Sentry pattern — skill = instructions, MCP = execution), §i (story-review pattern, informational), §j (batch-scheduled task execution — informational: this skill writes the spec rows `/lumina:wire-task-deps` consumes).

## MCP tools used

- `mcp__lumina__get_work_item` — story read (for kind precondition + task-children list); per-task read inside the loop to inspect existing `attributes.execution_detail` / `attributes.files_touched` / `attributes.outcome` / `attributes.dispatch` / `attributes.task_kind` / `attributes.files_touched_pattern`.
- `mcp__lumina__set_task_spec` — the ONE write tool. Set-or-leave per key: absent keys preserve existing attributes (`SetTaskSpecParams` in `lumina/src/mcp.rs` uses `#[serde(default)]` per field; the tool builds a sub-object of the present keys and makes ONE `set_work_item_attributes` call). Calling with only `outcome` set DOES NOT clobber `files_touched` or `execution_detail`.
- `mcp__lumina__record_task_activity` — provenance per §c, one entry per TASK touched.

This skill MUST NOT call any other lumina write tool (no `add_*`, no `update_work_item`, no `set_*` other than `set_task_spec`). For the drift check (step 5) it MAY use `Grep --files_with_matches` — a read tool, not a lumina write.

## Target

Invoked on a `kind = story` work-item. Step 1 verifies `detail.kind == "story"`; each `set_task_spec` call targets a TASK child's id (NOT the story's). The per-task iteration mirrors `acceptance-criteria/SKILL.md`.

## Body — per-task iteration loop

### 1. Read story + verify kind

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` (MUST equal `"story"`; abort otherwise with `"set-task-spec requires a story work item; got kind=<kind>."`) and `detail.children` filtered to `kind == "task"`. If that list is empty, abort with `"set-task-spec: story has no task children — run /lumina:decompose-tasks <story_id> first."`. Surface the count: `"Story has <N> task child/children: <comma-separated titles>."`

### 2. Per-task read

For EACH task child (in `detail.children` order), call `mcp__lumina__get_work_item({id: <task_id>})` to read THAT task's own attributes (the story-level `get_work_item` does not pre-fold per-task spec attributes). Bind:

- `attributes.execution_detail` — may be null/absent.
- `attributes.files_touched` — may be null/absent or a partial list.
- `attributes.outcome` — may be null/absent (and, when present, may be a plain string OR the JSON-in-string dual-track encoding — see step 3 below).
- `attributes.dispatch` — may be null/absent or `{"tier": "lite"|"deep"}`.
- `attributes.task_kind` — read-only here; relevant because `task_kind == "pattern-replacement"` enables the drift-check option in step 3.
- `attributes.files_touched_pattern` — optional informational key; if `decompose-tasks` recorded the Grep pattern, it lives here. If absent on a pattern-replacement task, the drift-check branch prompts the user for the pattern interactively.

### 3. Per-task triage

Surface a per-task `AskUserQuestion`:

> **Question header**: `Spec task '<task title>' (id=<task_id>)`
>
> **Question body**: `Current spec: execution_detail=<set|absent>, files_touched=<N items|absent>, outcome=<set|absent>, dispatch=<tier or absent>. Choose:`
>
> **Options**:
> - `Edit` — `Collect execution_detail / files_touched / outcome / dispatch and write via set_task_spec`
> - `Skip` — `Leave this task's spec unchanged; move to the next task`
> - `Pattern-replacement drift check` — *(presented ONLY if `attributes.task_kind == "pattern-replacement"`)* `Re-run Grep against the recorded pattern and reconcile any new files`

On `Skip`, log `"set-task-spec: task '<title>' skipped per user."` and move on (no write, no activity entry). On `Edit`, proceed to step 4. On `Pattern-replacement drift check`, proceed to step 5.

### 4. Edit branch — collect inputs sequentially

Each sub-prompt is its own `AskUserQuestion` so the user can stage one field at a time. Each field is independently optional — picking the `Skip this field` option leaves that key untouched on the task (set-or-leave semantics; absent keys in `set_task_spec` preserve the existing value).

**4a. `execution_detail`** — single `AskUserQuestion` with options `Provide execution_detail` / `Skip this field`. On `Provide`, the user types free-text via the Other field; the text is written VERBATIM to lumina (no reformatting). Frame the prompt around a step-by-step plan: "What are the ordered steps for this task? Include the rough edit sequence, the files in scope (you'll list them precisely next), and any sequencing constraints (e.g. migration must run before the repo method that queries the new column)."

**4b. `files_touched`** — branch on `attributes.task_kind`:

- **Non-pattern-replacement task** (`task_kind ∈ {foundation, vertical-slice, polish, null}`): single `AskUserQuestion` with options `Provide files_touched` / `Skip this field`. On `Provide`, the user types newline-separated paths. Each path is sent as a bare string (the legacy `FileRef::Path` shape — resolves to the project's primary linked repo). If a path needs to reference a non-primary linked repo, format the line as `<owner>/<name>:<path>` and the skill converts that to the `{"repo": "owner/name", "path": "<path>"}` qualified shape per `lumina/src/mcp.rs::FileRef::Qualified`. Glob patterns are FORBIDDEN (R25 — every entry must be a concrete file path).

- **Pattern-replacement task** (`task_kind == "pattern-replacement"`): jump into the drift-check sub-flow at step 5, then return here with the reconciled list. The reconciled list is what gets written.

**4c. `outcome`** — dual-track per R23. Two sequential prompts:

- First `AskUserQuestion`: `automated` track. Options `Provide automated outcome` / `Skip automated track`. On `Provide`, free-text — typically executable commands / scripted verifiers / test assertions. Example: `cargo test --manifest-path lumina/Cargo.toml -p lumina --test e2e -- migration_0007`.

- Second `AskUserQuestion`: `manual` track. Options `Provide manual outcome` / `Skip manual track`. On `Provide`, free-text — typically human checks the automation can't cover. Example: `SPA detail panel renders the new column; eyeball ordering against design mock`.

**Outcome storage convention (JSON-in-string encoding)**: `SetTaskSpecParams.outcome` is currently `Option<String>` (see Plan-deviation note below). Until lumina adds a structured shape, the dual-track value is JSON-encoded into that string:

```
outcome = json_encode({ "automated": "<verbatim>", "manual": "<verbatim>" })
```

If only one track is provided, encode the other as `null`. If the user skips BOTH tracks, omit `outcome` from the `set_task_spec` call entirely (set-or-leave). Readers (`/lumina:story-review`, SPA detail panel) MUST `json_decode` the string. Literal key names are `automated` and `manual`; alternate casings/hyphenations produce drift and MUST NOT be introduced.

**4d. `dispatch`** — single `AskUserQuestion` with exactly two options:

> Header: `Dispatch tier for '<task title>'`
> Body: `Which implementer should handle this task?`
> Options:
> - `lite` — `Mechanical, fully-specified work; flow-implement-lite suffices`
> - `deep` — `Judgement-heavy, ambiguous, cross-cutting, or security-sensitive; flow-implement-deep required`

Written as `dispatch: {"tier": "lite"}` or `dispatch: {"tier": "deep"}` (an object, not a bare string — `SetTaskSpecParams.dispatch` is `Option<serde_json::Value>` so the JSON object goes through verbatim). If the user wants to defer the decision, expose a third option `Defer (leave dispatch unset)` which omits the `dispatch` key from the `set_task_spec` call.

**4e. Write**: invoke `set_task_spec` ONCE per task, passing ONLY the keys the user filled. Absent keys are omitted from the JSON body — `SetTaskSpecParams` uses `#[serde(default)]` per field and the tool builds a sub-object of the present keys, preserving any pre-existing values for omitted keys.

```
mcp__lumina__set_task_spec {
  id: "<task_id>",
  execution_detail: "<verbatim user text>",      # only if 4a was filled
  files_touched: [                                 # only if 4b was filled
    "src/foo.rs",
    {"repo": "owner/name", "path": "src/bar.rs"}
  ],
  outcome: "<JSON-encoded dual-track string>",   # only if at least one track in 4c was filled
  dispatch: {"tier": "lite"}                       # only if 4d was filled
}
```

### 5. Pattern-replacement drift check (R25)

Fires from either: the user picking `Pattern-replacement drift check` in step 3, OR step 4b on a `task_kind == "pattern-replacement"` task. The flow:

1. Bind the recorded pattern: prefer `attributes.files_touched_pattern` if `decompose-tasks` stored it. If absent, prompt the user: `"This task is a pattern-replacement but no Grep pattern is recorded. What pattern should drift-check against? (e.g. 'fn foo\\(' or 'set_story_plan')"` via `AskUserQuestion` with one `Provide pattern` option.
2. Re-run `Grep --files_with_matches` against the pattern, scoped to the project's affected-areas directories (the same scope `decompose-tasks` used). Capture the current matching file list as `current_files`.
3. Bind `prior_files = attributes.files_touched` (the list recorded at decompose time, normalised to bare-path strings for comparison; the qualified-form entries are compared by their `path` value within their declared repo).
4. Compute `added = current_files \ prior_files` and `removed = prior_files \ current_files`.
5. If both `added` and `removed` are empty, log `"set-task-spec: drift check on task '<title>' — no drift; no change."` and return to step 4 (or to step 3's triage if invoked directly).
6. Otherwise, present the delta via `AskUserQuestion`:

> Header: `Pattern-replacement drift on '<task title>'`
> Body: `Re-running Grep against pattern '<pattern>' found <A> new file(s) and <R> file(s) no longer matching:\n\n  added: <comma-separated added paths>\n  removed: <comma-separated removed paths>\n\nChoose:`
> Options (3):
> - `Accept new files` — `Overwrite files_touched with the current Grep result (added gained, removed dropped)`
> - `Decompose pattern again` — `Print pointer to re-run /lumina:decompose-tasks; do nothing here`
> - `Continue without` — `Leave files_touched unchanged; the drift remains unresolved`

- On `Accept new files`: this branch returns the reconciled list to step 4b (if entered from step 4) or writes via `set_task_spec({id, files_touched: <current_files>})` directly (if entered from step 3 as a standalone drift check). The §c activity entry summary then includes `… resolved drift on <task_id>`.
- On `Decompose pattern again`: emit a one-liner to the user: `"Re-run /lumina:decompose-tasks <story_id> to regenerate the pattern-replacement task; this skill made no changes."` Return to the next task in the loop without writing. No §c entry for this task (no write occurred).
- On `Continue without`: log `"set-task-spec: drift on task '<title>' acknowledged; files_touched left unchanged."` Return without writing. No §c entry.

### 6. §c provenance (one entry per TASK TOUCHED)

After each successful `set_task_spec` call, append exactly ONE activity entry via `record_task_activity`. **One entry per task touched, NOT one per skill invocation** — if N tasks are updated, N activity entries are appended (mirrors `acceptance-criteria` per-write semantics; differs from `decompose-tasks` per-invocation rollup). Apply the §c substitution guard (`${CLAUDE_SESSION_ID}` must substitute; fall back to `session=unknown` + one-line warning otherwise).

```
mcp__lumina__record_task_activity {
  work_item_id: "<task_id>",
  entry_type: "execution",
  origin: "plan",
  summary: "set-task-spec: updated <comma-separated-keys-touched> on <task_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

`<comma-separated-keys-touched>` lists exactly the keys passed in the `set_task_spec` call (e.g. `"execution_detail, outcome"` if only those two were filled; `"files_touched"` if a drift-check resolution touched only that key). The activity row's `work_item_id` is the TASK's id (where the spec was written), not the story's — activity entries fold onto the task record per §c.

### 7. Final summary

After the per-task loop completes, emit a one-line rollup:

```
set-task-spec: <N> tasks edited; <M> tasks skipped; <K> pattern-replacement drift checks resolved.
```

Where `<K>` counts only the drift checks that resulted in `Accept new files` (the writes that actually changed `files_touched`). `Decompose pattern again` and `Continue without` resolutions are NOT counted in `<K>` because they wrote nothing.

## 5-step idempotency mapping (per §b — applied per-task-child per §b-per-element)

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` on the task binds existing spec attributes (step 2). |
| 2. Inspect | Triage reflects which keys are set/absent (step 3). |
| 3. Absent → create | `Edit` fills empty keys; `set_task_spec` writes them (step 4). |
| 4. Present matches → no-op | `Skip` in triage = no write, no activity entry. |
| 5. Present differs → confirm | `Edit` collects replacement; `set_task_spec`'s per-key set-or-leave semantics overwrite in place. No separate §b-supersession prompt — the tool is partial-overwrite, not append+supersede like `research_notes`. |

## Sentry-pattern compliance (per §e)

The skill body decides WHICH keys to surface, WHEN to fire the drift-check (only on `task_kind == "pattern-replacement"`), and HOW to encode dual-track outcome (JSON-in-string until backend supports a structured shape). Lumina's `repo.rs` validates the work-item, validates `FileRef` qualified-form `repo` slugs against `repo_links`, runs the patch transaction, and emits exactly one event. The skill body MUST NOT compose the merged attributes blob client-side, MUST NOT validate the `repo` slug, and MUST NOT pre-compute activity body — lumina's job.

## Plan-deviation note

Dual-track `outcome` is JSON-in-string within `SetTaskSpecParams.outcome: Option<String>` today. If a later migration widens to structured `{automated, manual}`, step 4c MUST switch in lockstep; the literal key names align so the migration is a wire-shape rename only.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §i, §j.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — Planning & decision tools (`set_task_spec`, `record_task_activity`).
- Upstream: [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md) — creates the tasks; records `task_kind` (step 4b branch) and optional `files_touched_pattern` (step 5).
- Downstream: `/lumina:wire-task-deps` — consumes dispatch + complexity for the R27 high-complexity gate; composes Kahn-batches per §j.
- Round-2 plan: [`../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../docs/plans/lumina-story-planning-round-2.md) — R23 (dual-track outcome), R25 (pattern-replacement `files_touched`), R27 (complexity gate; fires in wire-task-deps).
