# Plan: lumina — MCP + schema foundation (Plan 1 of the harness-reshape rollout)

**Plan path**: docs/plans/lumina-mcp-schema-foundation.md
**Created**: 2026-05-22
**Status**: draft

> Plan 1 of two. This plan builds the **lumina-side data + MCP foundation**. The
> webui interactivity (editable bodies, per-kind detail/edit forms) is **Plan 2**,
> built on the schema this plan lands. Neither plan rewires the harness commands —
> that is a later phase of the rollout.

---

## Context

The lumina vertical slice (`docs/plans/lumina-vertical-slice.md`) proved one thin
thread — SQLite → MCP → axum → Vue — with a deliberately minimal surface: four MCP
tools (`list`/`get`/`create`/`update_status`), a flat `work_items` row with only
`title`/`body`/`status`, and a one-flow importer that **dropped** most of tomlctl's
flow data (execution-record types, ledger vet/rollback logs, flow-envelope fields).

The user's goal is to **reshape the harness around lumina** in phases — not graft
lumina into tomlctl, and not a literal tomlctl drop-in. The eventual shape:
*definition* commands (plan-new/review/review-plan/optimise) traverse and enrich the
`epic → feature → story → task` hierarchy and attach research/strategy/tasks;
*execution* commands (implement + the `*-apply` family) merge into running **sprints**
over defined work, recording results **back onto the original task records**.

This plan delivers the foundation that *enables* that vision: a richer, well-related
schema; the repository write-paths the model needs; a coherent **domain-shaped MCP
tool surface** that a Claude Code agent can drive idiomatically; and a `lumina` skill
doc. It does **not** change any `claude/commands/*` or `claude/agents/*` flow logic.

The guiding data-model principle (user's words): *avoid disparate objects that are
intrinsically linked but only loosely referenced; schemas must be well-designed for
tight relationships and loose references. Execution history, vet notes, and user
comments fold onto the original task record. A story carries a problem statement,
research notes, a high-level execution strategy, and child tasks — together the
"plan" concept, extensible.*

## Scope

**In scope (Plan 1):**
- Additive migration `0002`: a per-kind `attributes` JSON column on `work_items`, a
  `deleted_at` soft-delete tombstone, and a new **`work_item_activity`** child table
  (tight FK + `ON DELETE CASCADE`) for append-only execution/vet/comment history.
- Domain structs + **typed enums** (`Kind`/`Status`/`Severity`/`ActivityType`/
  `Disposition`) so MCP params advertise legal values; `WorkItemDetail` gains
  `activity`; `WorkItem` gains `attributes`.
- Repository write-paths under the existing single-mutation-path + events-outbox
  discipline: `update_work_item` (partial), `append_activity`, attribute setters,
  context-block create/link/unlink, finding update/resolve, soft `delete_work_item`.
- An expanded **domain-shaped MCP tool set** (~17 tools) tagged definition/execution/
  read, with rmcp tool annotations (read-only/destructive/idempotent hints).
- HTTP **read-side** fold (detail/tree return `activity` + `attributes`) and a generic
  `PATCH` over `update_work_item` to keep HTTP↔MCP single-source parity.
- Git-export fold: per-item TOML snapshots include `attributes` + `activity` and honour
  `deleted_at`.
- A new `claude/skills/lumina/SKILL.md` and a `CLAUDE.md` note.

**Out of scope (Plan 2 / later phases):** any Vue/webui change (edit forms, per-kind
panels, optimistic updates); HTTP write endpoints beyond the generic work-item PATCH
(activity-POST, finding endpoints stay MCP-only this plan); rewriting the harness flow
commands to call lumina; the sprint-execution engine + concurrency/locking; Postgres
driver wiring; auth / multi-user; replacing tomlctl in the flow commands.
**Importer activity-mapping is explicitly OUT:** `lumina/src/import.rs` keeps DROPPING the
execution-record entry types (`deviation`/`verification`/`status-transition`/`reconcile`/
`deferral`/`checkpoint`) it drops today — it is unchanged this plan — even though the new
`work_item_activity` model could now hold them. Folding dropped items into `append_activity`
is deferred to the harness-reshape phase. (`import.rs` is therefore intentionally absent from
Affected areas.)

**No-web-change is safe (not just asserted):** adding `attributes` to `WorkItem` and
`activity` to `WorkItemDetail` is additive JSON — the SPA's TypeScript interfaces ignore
unknown keys at runtime, so the un-touched frontend keeps working. One latent type-soundness
note for Plan 2: `web/src/api.ts` types `position: number` (non-null) but it is `Option<i64>`
server-side, and the new move/reorder path can surface `null` — harmless at runtime (TS types
are erased), to be tightened in Plan 2.

**Affected areas:** `lumina/migrations/`, `lumina/src/` (`domain.rs`, `repo.rs`,
`mcp.rs`, `http.rs`, `export.rs`, `db.rs`, `lib.rs` only if a module is added),
`lumina/tests/`, `lumina/.sqlx/` (regenerated), `claude/skills/lumina/` (new), `CLAUDE.md`.
**Estimated ~12–13 files.** Under the single-plan guard; tasks are waved so no parallel
batch touches the same file.

## Exploration Notes

- **Schema** (`lumina/migrations/0001_init.sql`): `work_items(id,kind,parent_id,title,
  body,status,position,created_at,updated_at)` with a BEFORE INSERT/UPDATE **trigger
  pair** (`trg_work_items_hierarchy_{insert,update}`) enforcing legal (kind,parent-kind)
  edges via a correlated subquery. `findings` (22 cols, all nullable but `id`). `events`
  outbox `(id,aggregate_type,aggregate_id,event_type,payload,actor,created_at,
  exported_at)`. `context_blocks` + `work_item_context` link table. `PRAGMA foreign_keys=ON`.
- **Repo** (`lumina/src/repo.rs`): single-mutation-path — `pool.begin()` → one domain
  write → `record_event(&mut tx, agg_type, agg_id, event_type, payload)` → `commit`.
  `rows_affected()==0` ⇒ `NotFound` (no spurious event). Present: `list_work_items`,
  `get_work_item_detail`, `list_findings`, `create_work_item`, `update_work_item_status`,
  `create_finding(&NewFinding)`. **Missing:** body/title/field update, delete, reorder,
  context-block writes, finding update/resolve, any append-history mechanism.
- **MCP** (`lumina/src/mcp.rs`): rmcp 1.7, `StreamableHttpService<LuminaTools,
  LocalSessionManager>` nested at `/mcp`, per-request `service_factory` closure cloning
  `Arc<SqlitePool>`. 4 `#[tool]` methods, each `Parameters<T: Deserialize+JsonSchema>`.
  Reads → `Content::json`; writes → `CallToolResult::structured`. `AppError`→ErrorData:
  NotFound→`resource_not_found`, Validation→`invalid_params`, Db/Other→`internal_error`.
  Domain read structs derive `Serialize` only (no `JsonSchema`).
- **HTTP** (`lumina/src/http.rs`): `GET /api/work-items` (tree or flat filtered),
  `GET /api/work-items/{id}` (`WorkItemDetail{item,children,findings,context_blocks}`),
  `POST`, `PATCH` (status only), `GET /api/health`. `build_tree` is O(n).
- **Export** (`lumina/src/export.rs`): `export_pending` drains `events WHERE exported_at
  IS NULL`, renders each aggregate's current `get_work_item_detail` to
  `<root>/<kind>/<id>.toml` (atomic tempfile→rename), stamps `exported_at`. Default root
  `./.lumina/export`, overridable via `LUMINA_EXPORT_ROOT`. Background tick every 5s.
- **Cargo pins**: axum 0.8, tokio 1 full, sqlx **0.9** (`runtime-tokio,sqlite,macros,
  migrate`, committed `.sqlx/`), rmcp 1.7 (`server,transport-streamable-http-server,
  macros`), schemars 1, uuid 1 (v7), toml 1, rust-embed 8.
- **Frontend (Plan 2 territory, untouched here)**: Vue 3.5 / Vite 8 / Pinia 3 / vue-router
  5; single route; detail panel shows a read-only body + a status dropdown (the only
  write). Confirms no Plan-1 web changes ⇒ **no `npm run build` in this plan's gates**.

## Research Notes

> rmcp 1.7 tool-authoring features verified via Context7 by the design agents; no version
> changes from the slice. sqlx 0.9 + SQLite JSON1 behaviour confirmed from project usage.

- **rmcp 1.7 tool annotations** — `#[tool(annotations(read_only_hint=…, destructive_hint=…,
  idempotent_hint=…, open_world_hint=…))]` is supported via `ToolAnnotations`. Tagging
  tools lets Claude Code reason about retry/safety. Cheapest usability win. *Impact:* tag
  every read tool `read_only_hint`, `delete_work_item` `destructive_hint`, setters/status
  `idempotent_hint`, all `open_world_hint=false`. Grade: HIGH.
- **rmcp 1.7 typed output** — `Json<T>` (puts value in `structured_content` + advertises
  `outputSchema`) and `schema_for_output::<T>()` both require `T: Serialize + JsonSchema`;
  the domain read structs are `Serialize`-only and frozen. *Impact:* keep hand-built
  `Content::json` for read aggregates and `structured(json!{…})` for write returns;
  deriving `JsonSchema` on read structs is a deferred fast-follow, NOT this plan. Grade: HIGH.
- **schemars enums** — a Rust `enum` deriving `Deserialize + JsonSchema` with
  `#[serde(rename_all="snake_case")]` emits a JSON `enum` array, so Claude only sends legal
  `kind`/`status`/`severity` values. Field `///` docs surface as JSON-schema `description`
  (largest lever on correct calls). *Impact:* introduce the five enums in `domain.rs`. Grade: HIGH.
- **SQLite `ALTER TABLE`** — `ADD COLUMN` is O(1) and supported, but cannot add a column
  with a non-constant DEFAULT, PRIMARY KEY/UNIQUE, or a retro-CHECK on an *existing* column;
  CHECK/FK/cascade are only expressible at `CREATE TABLE` time. `json_valid()` is in the
  bundled SQLite. *Impact:* `attributes`/`deleted_at` are plain nullable `TEXT` adds;
  JSON-validity is enforced by a new BEFORE INSERT/UPDATE trigger (backstop) + repo
  validation (typed 422); `work_item_activity` gets its FK+CHECK+`UNIQUE(work_item_id,seq)`
  at create time. Grade: HIGH.
- **sqlx 0.9 `query!` + nullable JSON columns** — `attributes`/`payload` return as
  `Option<String>`; `serde_json::from_str` decodes them, and the committed `.sqlx/` cache
  MUST be regenerated (`cargo sqlx prepare`) or the offline build breaks. *Impact:* `.sqlx/`
  regen is part of the repo task; `--check` gate in the e2e task. Grade: HIGH.

## User Decisions

> Captured from the Phase-1 scoping questions. Treated as design data.

1. **Not a tomlctl drop-in** → reshape the harness around lumina in phases. Definition
   commands manage/enrich the hierarchy; execution commands (implement + `*-apply`) merge
   into sprints recording onto original task records. *This plan builds the enabling
   lumina foundation only — no harness command changes.*
2. **Two sequential plans** → this is Plan 1 (schema + repo + MCP + skill). Webui editing
   is Plan 2 on top of this foundation.
3. **Data model** → tight relationships, well-designed; execution history + vet notes +
   comments **fold onto the original task record**, not loosely-referenced separate
   objects. Stories carry problem statement + research notes + execution strategy + child
   tasks (= the extensible "plan"). *Resolved as the hybrid in ## Approach.*
4. **Domain-shaped MCP tools** → intent-named tools, not tomlctl-shaped generic verbs.

### Orchestrator decisions (not asked — stated for approval visibility)

- **Hybrid storage**: kind-specific narrative fields live in one repo-validated
  `work_items.attributes` JSON column (co-located with the row it belongs to — maximally
  tight, no per-kind sidecar tables, no migration per field). Cross-cutting/queryable
  fields stay real columns. History is a **real FK child table**, not a JSON array.
- **`work_item_activity` is distinct from `events`**: activity is durable, user-facing,
  tightly FK'd history folded onto the item; `events` remains the loosely-keyed export
  *outbox* drained then forgotten. They are not merged.
- **Soft-delete** for `work_items` (`deleted_at`): a work item owns export identity +
  cascaded activity, so hard-delete would orphan the export TOML and lose history.
  Findings + context links may hard-delete (no independent export identity).
- **HTTP write surface** stays minimal this plan (generic work-item PATCH only); richer
  write endpoints arrive with the Plan-2 UI. MCP is the full write path for Plan 1.

### Phase 5 outcome

_Skipped — every Phase-4 answer's key terms (domain MCP tools, attributes JSON, activity
table, rmcp annotations, sqlx JSON columns) are covered in Research Notes._

## Approach

**Storage — hybrid, tightness-first.** Add `work_items.attributes TEXT` (a JSON **object**,
repo-validated per kind) for the narrative "plan" fields. The per-kind contract is **PINNED**
— the repo validates known keys against their type, **rejects unknown keys** (tight schema),
treats all keys as optional, and accepts an empty object for kinds with no fields:

| kind | `attributes` keys (all optional) |
|------|----------------------------------|
| story | `problem_statement: string`, `research_notes: string`, `execution_strategy: string` |
| task | `execution_detail: string`, `files_touched: string[]`, `outcome: string`, `dispatch: { agent?: string, model?: string, tier?: string }` |
| epic / feature | `context: string`, `grouping_rationale: string` |
| project | (none — empty object) |

These are never filtered/joined across rows, so they do not earn columns; co-locating them on
the row is the tightest binding and keeps the schema stable. A field that later needs
cross-row querying is promoted to a real column (or a generated column over `json_extract`)
in a future additive migration. **TOML-export safety:** the attribute setter NORMALISES before
store — object root only (no scalar/array root), and **null-valued keys are dropped, not
stored** — so `toml::to_string_pretty(&WorkItemDetail)` (export.rs:188) cannot hit the `toml`
crate's null/scalar-root serialization failure. The same normalise + objects-only rule governs
activity `payload`.

**History — `work_item_activity`, a tight FK child.** Append-only, `REFERENCES
work_items(id) ON DELETE CASCADE`, ordered by a per-item `seq = MAX(seq)+1` allocated in
the write transaction, with `UNIQUE(work_item_id, seq)` so a future-Postgres race surfaces as
a constraint violation rather than silent duplication (Plan 1 is single-writer SQLite; no
retry path is built this plan). `entry_kind ∈ {execution, verification, deviation, deferral, reconcile,
status-transition, checkpoint, vet, comment}` maps the dropped tomlctl execution-record
types + ledger `vet_events`; type-specific data rides in a validated `payload` JSON. This
is the structural answer to "fold onto the original task record" — and is explicitly a
*different concept* from the export `events` outbox.

**Writes — same discipline, more verbs.** Every new repo fn opens one transaction, does
one domain write + one `record_event`, commits. `update_work_item` takes a partial-update
request struct; each field is **set-or-leave** (a `None` bind leaves the column) via
`COALESCE(?, col)` — so `body` is plain `Option<String>` (set or leave), **NOT**
clear-to-NULL (a dedicated body-clear is out of scope; with that constraint the single
`COALESCE` statement genuinely covers every field combination). `set_work_item_attributes`
is **read-modify-merge**: read the current `attributes` object, overwrite present keys, leave
absent keys, then write — so the partial setters `set_story_plan`/`set_task_spec` (which call
it with a constructed sub-object) compose without clobbering siblings. `get_work_item_detail`
gains one ordered `SELECT … ORDER BY seq` to fold activity. **Soft-delete reader policy
(pinned):** `list_work_items` and the tree reader filter `WHERE deleted_at IS NULL`;
`get_work_item_detail` does **NOT** filter — it returns the row with `deleted_at` populated, so
(a) the export path can render a tombstone and (b) a direct detail fetch shows the deleted
marker rather than 404.

**MCP — domain-shaped, ~17 tools, annotated.** Intent-named tools grouped definition /
execution / read (full table in Task 5), each mapping to exactly ONE repo write (preserving
the +1 work_items / +1 events invariant the slice test asserts). Five typed enums give
params legal-value schemas; tool annotations give Claude retry/safety hints. Read outputs
stay JSON content; write returns stay structured `{id}`.

**Skill — `claude/skills/lumina/SKILL.md`.** A model-discoverable skill cataloguing the
tools (grouped definition/execution/read), the Streamable-HTTP connection idiom
(`claude mcp add --transport http lumina http://127.0.0.1:<port>/mcp`; tools surface as
`mcp__lumina__<tool>`), and the explicit framing that lumina is the *data layer* of the
phased reshape and does **not** yet replace the tomlctl flow-state skill.

**Reuse:** mirror `0001`'s trigger idiom for the JSON-validity backstop; reuse the
`record_event` helper, the `rows_affected()` NotFound pattern, `app_error_to_mcp`, and the
atomic-write path in `export.rs` verbatim.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo test --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
```

Additional gates (acceptance, not the standard triplet):
- `cd lumina && cargo sqlx prepare --check` — fails if the committed `.sqlx/` cache is stale
  after the new `query!` macros (standalone crate; do NOT use `--workspace`).
- `cargo audit --file lumina/Cargo.lock` — RUSTSEC check (mirrors the tomlctl cadence).
- **No `npm run build`** — Plan 1 makes no web changes; the committed `web/dist/` placeholder
  already satisfies release `rust-embed`.

## Tasks

> Greenfield-ish additive work; waved so no parallel batch shares a file. ≤4 agents/wave.

### Wave A — schema + domain (foundation)

#### 1. Additive migration 0002 (attributes, soft-delete, activity table) [M]
- **Files:** `lumina/migrations/0002_attributes_and_activity.sql`
- **Depends on:** —
- **Action:** Author the additive migration: `ALTER TABLE work_items ADD COLUMN attributes
  TEXT;` and `ADD COLUMN deleted_at TEXT;`; `CREATE TABLE work_item_activity(id TEXT PK,
  work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE, seq INTEGER NOT
  NULL, entry_kind TEXT NOT NULL, author TEXT, summary TEXT NOT NULL, payload TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, CHECK(payload IS NULL OR
  json_valid(payload)), UNIQUE(work_item_id, seq));` + `CREATE INDEX
  idx_activity_work_item ON work_item_activity(work_item_id, seq);`; and a BEFORE
  INSERT/UPDATE trigger pair rejecting non-NULL invalid `work_items.attributes`
  (mirroring the `0001` trigger idiom).
- **Detail:** `PRAGMA foreign_keys = ON;` at top. Keep ANSI-ish (plain TEXT; on Postgres
  `attributes`→`JSONB` and the validity trigger drops). `ADD COLUMN` leaves existing rows
  (slice + imported flow + MCP-test items in `lumina/lumina.db`) at `attributes=NULL`,
  read as "no kind-specific fields".
- **Acceptance:** `sqlx migrate run` applies cleanly to a fresh DB (**authoritative gate**;
  in-memory/temp). Applying to the existing `lumina/lumina.db` is a manual smoke-check that
  requires stopping the port-8080 server first (see Risks). A test asserts (a)
  `work_item_activity` rejects a row with malformed JSON `payload`, (b) the attributes trigger
  rejects malformed `work_items.attributes`, (c) deleting a work_item cascades its activity rows.

#### 2. Domain structs + typed enums [M]
- **Files:** `lumina/src/domain.rs`
- **Depends on:** 1
- **Action:** Add `WorkItemActivity` (Serialize), `UpdateWorkItemRequest` +
  `UpdateFindingRequest` (Deserialize+JsonSchema, all-optional, partial-update), and the
  five enums `Kind`/`Status`/`Severity`/`ActivityType`/`Disposition` (Deserialize+JsonSchema,
  `#[serde(rename_all="snake_case")]`). Extend `WorkItem` with `attributes:
  Option<serde_json::Value>` and `WorkItemDetail` with `activity: Vec<WorkItemActivity>`.
- **Detail:** `body: Option<String>` on the update struct — **set-or-leave**, NOT
  clear-to-NULL (matches the single-`COALESCE` write; see ## Approach). `///` doc every
  request field (becomes JSON-schema `description`). Do NOT add `JsonSchema` to the
  Serialize-only read structs (deferred per Research Notes).
- **Acceptance:** `cargo build` compiles; a unit test round-trips each enum through serde
  (snake_case) and asserts the schemars schema for `Kind` lists all five variants.

### Wave B — repository write-paths (after Wave A)

#### 3. Repository write-paths + read fold + .sqlx regen [L]
- **Files:** `lumina/src/repo.rs`, `lumina/src/db.rs` (only if migration wiring needs a
  note), `lumina/.sqlx/` (regenerated)
- **Depends on:** 2
- **Action:** Add, each under the single-mutation-path + `record_event` discipline:
  `update_work_item(id, &UpdateWorkItemRequest)` (COALESCE partial; per-kind `attributes`
  validation → `Validation`), `append_activity(work_item_id, entry_kind, author?, summary,
  payload?) -> Uuid` (allocates `seq=MAX+1`, validates payload per `entry_kind`),
  `set_work_item_attributes(id, &Value)` (**read-modify-merge**: overwrite present keys, leave
  absent; normalise object-root-only + drop null-valued keys per ## Approach),
  `reorder_work_item(id, position)`, `create_context_block(title?, body?) -> Uuid`,
  `link_context_block` / `unlink_context_block`, `update_finding(id, &UpdateFindingRequest)`,
  `resolve_finding(id, disposition, resolution?, rationale?)`, `delete_work_item(id)` (soft —
  stamps `deleted_at`, emits `work_item.deleted`). Fold `activity` into `get_work_item_detail`
  (ordered by `seq`); add `WHERE deleted_at IS NULL` to `list_work_items` + the tree reader
  **ONLY** — `get_work_item_detail` returns the row with `deleted_at` populated (the export
  tombstone path and the deleted-marker detail fetch both need it; pinned reader policy in
  ## Approach). Regenerate and commit `lumina/.sqlx/` with **`cargo sqlx prepare -- --all-targets`**
  (per the CLAUDE.md lumina cadence — a bare prepare drops test-only queries and breaks the
  offline test build).
- **Detail:** Validate `entry_kind`/`disposition`/`kind`/`status` against the Task-2 enums
  (typed `Validation`, not panic). Per-kind `attributes` validation rejects unknown keys per
  the pinned table in ## Approach. `NotFound` via `rows_affected()==0` before any event.
  `seq` allocation + `UNIQUE(work_item_id,seq)` per ## Approach.
- **Acceptance:** repo tests prove: `update_work_item` writes exactly +1 work_items-update
  / +1 events and rolls both back on a forced mid-tx error; `append_activity` writes one
  activity row with `seq` monotonic per item + one event; an `attributes` object with an
  unknown key for a `kind` returns `Validation` (not 500); a `set_story_plan`-style partial
  merge leaves a previously-set sibling key intact; soft-`delete_work_item` hides the item from
  `list_work_items` but `get_work_item_detail` still returns it with `deleted_at` set;
  `cargo sqlx prepare --check` is clean.

### Wave C — entry points (parallel after Wave B)

#### 4. HTTP read fold + generic PATCH [M]
- **Files:** `lumina/src/http.rs`
- **Depends on:** 2, 3
- **Action:** Surface the new read data (detail + tree responses include `attributes` +
  `activity`) and replace the status-only `PATCH /api/work-items/{id}` with a generic
  handler over `update_work_item` accepting `UpdateWorkItemRequest` (title/body/status/
  position/attributes).
- **Detail:** The handler returns **`200` + `Json(WorkItem)`** (the updated row, re-fetched
  after the write) — **NOT `204`**. This also fixes a latent frontend bug: `web/src/api.ts`
  `handle<T>` (api.ts:93) calls `res.json()` unconditionally, so today's `204` status-PATCH
  would throw on an empty body; returning the item keeps `api.ts updateStatus`'s
  `Promise<WorkItem>` contract intact (no web change needed). No new write endpoints beyond the
  generic PATCH this plan (activity-POST, finding endpoints are Plan-2/MCP-only). Both entry
  points call the SAME `repo` fns (single-source parity).
- **Acceptance:** `GET /api/work-items/{id}` returns `activity` + `attributes`; `PATCH`
  with `{"body":"…"}` updates the body and is visible on the next GET; `PATCH` with
  `{"status":"done"}` returns `200` + the updated item JSON; an unknown id PATCH returns 404.

#### 5. MCP domain-tool surface (~17 tools) + annotations [L]
- **Files:** `lumina/src/mcp.rs`
- **Depends on:** 2, 3
- **Action:** Expand `LuminaTools` to the domain-shaped set, each `#[tool]` mapping to ONE
  repo fn, params using the Task-2 enums, with rmcp tool annotations. Each `#[tool]` declares
  its OWN `Parameters<T>` wrapper struct carrying the target `id` + fields (precedent:
  `UpdateStatusParams`, mcp.rs:96-125) — the `domain::*Request` structs lack `id` and are not
  reused directly; budget ~13 new param structs. `set_story_plan`/`set_task_spec` build a
  sub-object and call `set_work_item_attributes` (the read-modify-merge fn).
  - **Definition:** `create_work_item`, `update_work_item`, `move_work_item`,
    `delete_work_item`(destructive), `set_story_plan` (problem/research/strategy in one
    call), `set_task_spec` (files/outcome), `create_context_block` (optional `link_to`),
    `link_context_block`.
  - **Execution:** `record_task_activity` (entry_type=execution/vet/comment + body +
    outcome?), `transition_status` (rename of `update_work_item_status`, idempotent),
    `add_finding`, `update_finding`, `resolve_finding` (disposition enum).
  - **Read** (read_only_hint): `list_work_items` (parent/kind/status), `get_work_item`
    (item+children+findings+activity+context), `get_tree` (root?, max_depth?),
    `get_sprint_view` (story + task subtree + per-task activity).
- **Detail:** Reuse `app_error_to_mcp`; reads → `Content::json`, writes →
  `structured(json!{…})`. Keep `allowed_hosts` at the loopback default. Each write tool is
  a single repo call, not tool-layer orchestration (single-mutation-path). **Issue NO new
  `query!` macros touching the new columns** — route every read through the Task-3 repo fns,
  so the `.sqlx/` regen stays in Task 3 alone and Wave-C tasks 4/5/6 never contend on `.sqlx/`
  (parallel-safety guarantee).
- **Acceptance:** `#[tokio::test]` asserts the advertised tool list contains every tool
  name and each carries its annotation; a `record_task_activity` tool call writes +1
  activity / +1 events; a `set_story_plan` call writes the three story `attributes` keys in
  one transaction; an invalid `kind` enum value is rejected as `invalid_params`.

#### 6. Git-export fold (attributes + activity + soft-delete) [S]
- **Files:** `lumina/src/export.rs`
- **Depends on:** 3
- **Action:** Extend `render_work_item` so the per-item TOML snapshot includes
  `attributes` + the ordered `activity`, and so a soft-deleted item's snapshot is rewritten
  in-place with a top-level `deleted_at` marker (**tombstone; never file-deleted**, preserving
  the audit trail per the soft-delete decision).
- **Detail:** Reuse the atomic tempfile→rename path; idempotent drain unchanged. Read activity
  through the Task-3 `get_work_item_detail` (which now returns soft-deleted rows with
  `deleted_at` set) — **no new `query!` macro / no second `.sqlx/` regen**. The Task-3 setters
  already normalise `attributes`/`payload` to null-free objects, so the whole-`WorkItemDetail`
  `toml::to_string_pretty` cannot fail on a `serde_json::Value` null/scalar root.
- **Acceptance:** after a `set_story_plan` + `record_task_activity`, `export_pending`
  writes a snapshot whose `attributes` round-trips a **nested object** (asserting the
  TOML-serialization fix) and that contains the activity entry; after a soft-delete + drain,
  the snapshot still exists and carries a top-level `deleted_at`; a second drain is a no-op.

### Wave D — skill + docs + end-to-end

#### 7. `lumina` skill doc [M]
- **Files:** `claude/skills/lumina/SKILL.md`
- **Depends on:** 5
- **Action:** Author the model-discoverable skill: frontmatter trigger description; the
  tool catalogue grouped definition/execution/read with one-line when-to-use; the
  Streamable-HTTP connection idiom (`claude mcp add --transport http lumina
  http://127.0.0.1:<port>/mcp`; tools as `mcp__lumina__<tool>`; server must be running);
  top-down build → `set_story_plan` → `record_task_activity`/`transition_status` →
  `get_sprint_view` call patterns; and the explicit framing that lumina is the data layer
  of the phased reshape and does NOT yet replace the tomlctl flow-state skill.
- **Acceptance:** SKILL.md exists with valid frontmatter (name + description); every tool
  from Task 5 appears in the catalogue; the connection command and the tomlctl-relationship
  note are present.

#### 8. End-to-end test + CLAUDE.md note + offline-cache gate [M]
- **Files:** `lumina/tests/e2e.rs`, `CLAUDE.md`
- **Depends on:** 4, 5, 6
- **Action:** Extend the in-process e2e thread to exercise the new path: drive MCP
  `create_work_item` (story) → `set_story_plan` → create a task → `record_task_activity` →
  assert work_item/activity/event rows → `export_pending(&pool)` directly (no sleep) →
  assert the snapshot contains `attributes` + `activity` → `GET /api/work-items/{id}`
  returns them. **Each new tool needs its own in-process `Parameters<T>` drive helper**
  mirroring the existing `mcp_create` (which is bound to `CreateWorkItemRequest`) — budget
  for them. Add a `## lumina` note to the existing CLAUDE.md lumina section (the new
  skill + the MCP tool surface + `cargo sqlx prepare --check`); do NOT touch the tomlctl
  build section.
- **Acceptance:** `cargo test --manifest-path lumina/Cargo.toml` (incl. the deterministic,
  sleep-free e2e) passes; `cd lumina && cargo sqlx prepare --check` is clean; CLAUDE.md
  documents the new surface.

## Dependency Graph

```
Wave A:  1 ── 2
Wave B:       2 ── 3
Wave C:            3 ──┬── 4   (also needs 2 for UpdateWorkItemRequest)
                       ├── 5   (also needs 2 for the enums + param structs)
                       └── 6        (4,5,6 parallel — disjoint files)
Wave D:           5 ── 7
                  (4,5,6) ── 8
```
Critical path: 1 → 2 → 3 → 5 → 8. Tasks 4/5/6 parallelise after 3 (own `http.rs`/`mcp.rs`/
`export.rs`); 4 and 5 also consume Task-2 domain types (`2→4`, `2→5`), but ordering is already
safe because 3 depends on 2. **Parallel-safety holds only while 4/5/6 issue no new `query!`
macros** (Tasks 5 & 6 details pin this) — otherwise they would contend on the `.sqlx/` cache
that Task 3 regenerates. 7 needs 5's tool names; 8 needs the three entry points.

## Verification

1. **Build/lint/test** — the three Verification Commands pass on `lumina/`.
2. **Migration** — `0002` applies cleanly to a fresh DB (authoritative gate; live-DB is a
   manual server-stopped smoke-check); the activity-cascade, JSON-validity, and `UNIQUE(seq)`
   guards hold (Task 1).
3. **Single-source mutation** — new repo writes each emit exactly one domain row + one
   event and roll back atomically; soft-delete hides from lists (Task 3).
4. **Tight history** — `record_task_activity` folds onto the task via the FK child table;
   `get_work_item`/export surface it in `seq` order (Tasks 5, 6).
5. **MCP usability** — domain tools advertise enums + annotations; an illegal enum value is
   rejected as `invalid_params` (Task 5).
6. **HTTP↔MCP parity** — a body/attributes set via either entry point is readable via the
   other (Tasks 4, 8 e2e).
7. **Export** — snapshots include attributes (nested object round-trips) + activity, and a
   soft-deleted item is tombstoned in-place with a `deleted_at` marker (Task 6 + e2e).
8. **Skill** — `claude/skills/lumina/SKILL.md` catalogues every tool + the connection idiom
   (Task 7).
9. **Offline cache** — `cargo sqlx prepare --check` clean (Task 8).

## Risks

- **`.sqlx/` offline-cache drift** — new `query!` macros need `cargo sqlx prepare --
  --all-targets` (NOT a bare prepare — that drops test-only queries per CLAUDE.md) + committed
  `.sqlx/`, or the offline build breaks. *Mitigation:* regen with `--all-targets` in Task 3,
  `--check` gate in Task 8; Tasks 5/6 issue no new macros so the regen is single-point.
- **Soft-delete reader policy (cross-cutting)** — the readers do NOT all filter uniformly:
  `list_work_items` + the tree reader filter `deleted_at IS NULL`, while `get_work_item_detail`
  deliberately returns deleted rows (with `deleted_at` set) so export can tombstone in-place
  and a detail fetch can show the marker. Getting this split wrong either resurfaces deleted
  items in lists or breaks the export tombstone. *Mitigation:* policy pinned in ## Approach +
  Task 3 action/acceptance; e2e covers both sides.
- **`serde_json::Value` → TOML serialization** — a `Value` `attributes`/`payload` with null
  values or a non-object root fails `toml::to_string_pretty` and would wedge the export drain.
  *Mitigation:* the Task-3 setters normalise to null-free object roots; Task 6 e2e asserts a
  nested-object snapshot round-trips.
- **`seq` allocation portability** — `MAX(seq)+1` is safe under SQLite's serialized writes;
  `UNIQUE(work_item_id, seq)` makes a future-Postgres race surface as a constraint violation
  rather than silent duplication (no retry path built this plan). *Mitigation:* constraint
  added now (Task 1).
- **Migrating the live dev DB** — `lumina/lumina.db` holds slice + imported-flow +
  MCP-test rows; `ADD COLUMN` is non-destructive but the running release server (port 8080)
  holds the file open. *Mitigation:* stop the server before `migrate run`, or migrate a
  copy; acceptance tests run against fresh/in-memory DBs regardless.
- **Tool-count ergonomics** — ~17 tools is a larger surface for an agent to navigate.
  *Mitigation:* clear `///` descriptions + annotations + the grouped SKILL.md catalogue;
  the set is deliberately minimal-but-complete (flexible tools like `set_story_plan`/
  `record_task_activity` fold what could be many).
