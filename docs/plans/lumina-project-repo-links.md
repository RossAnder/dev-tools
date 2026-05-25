# Plan: lumina project↔repo links

**Plan path**: docs/plans/lumina-project-repo-links.md
**Created**: 2026-05-25
**Status**: draft (revised after /review-plan round 1)

## Context

Lumina lets users define multiple **projects** (the root kind of the `project > epic > feature > story > task` work-item hierarchy). Today, file references — `findings.file:line` and a task's `attributes.files_touched: Vec<String>` — are stored as opaque path strings with no notion of which repository they live in. That is fine when a project is a single repo but breaks down for vertical-slice stories that legitimately span more than one repo (e.g. a story changes a Rust backend and a Vue frontend that live in separate GitHub repositories).

The goal of this plan is to let a project declare one or more linked GitHub repositories (`<owner>/<name>` slugs), mark one of them primary, and qualify file references with the repo they belong to. Resolution is metadata-only — Lumina records "this file lives in repo X", it does not (yet) resolve to a local working tree. The feature is exposed in both the MCP tool surface (so an agent can author repo-qualified references) and the Vue Vapor SPA (so a human can edit the repo list and read file references with their repo prefix).

## Scope

**In scope**
- New SQLite table `repo_links` keyed to `work_items` rows where `kind='project'`.
- New nullable `findings.repo_id` foreign key.
- New repo-layer CRUD functions + new MCP tools + new HTTP endpoints.
- Project-detail web UI panel for managing repos; file-reference display gains a repo tag.
- e2e test coverage for the full thread (DB → MCP write → git-export drain → HTTP read).

**Out of scope**
- Working-tree / filesystem resolution to a local clone (user chose metadata-only).
- Automated repo→filename detection (user noted this as a future stretch; not designed here).
- Repo support for non-GitHub hosts (user chose bare `<owner>/<name>`).
- The legacy `task.attributes.files_touched: Vec<string>` JSON shape is widened to **accept** an optional `{repo, path}` object form, but no migration of existing string entries is performed — existing entries continue to resolve to the project's primary repo.

**Affected areas**
- `lumina/migrations/` — new 0004 migration
- `lumina/src/domain.rs`, `repo.rs`, `mcp.rs`, `http.rs`, `export.rs`, `import.rs`
- `lumina/tests/e2e.rs`
- `lumina/web/src/api.ts`, `composables/`, `components/`, `assets/tokens.css`, `utils/repoTag.ts` (new)
- `lumina/CLAUDE.md`, `claude/skills/lumina/SKILL.md`

Estimated file count: ~14 unique files (added `import.rs` and `utils/repoTag.ts` after review).

## Research Notes

- **GitHub username regex** — `^[a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38}$` (max 39 chars, no leading/trailing/consecutive hyphens; case-insensitive). Source: shinnn/github-username-regex, evidence A.
- **GitHub repo-name rules** — `[a-zA-Z0-9._-]{1,100}`, must not end with `.git`; `.` and `..` reserved. Source: dead-claudia/github-limits (no official GitHub regex page; empirical), evidence A.
- **Combined slug validator** — `^[a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38}/[a-zA-Z0-9._-]{1,100}$` (i-flag on owner segment); reject if name ends `.git`. Apply once in the parser; case-fold BOTH owner AND repo-name to lowercase on store (GitHub repo names are case-insensitive for resolution; storing `Foo/Bar` and `foo/bar` as distinct rows would violate the intent of the UNIQUE constraint). Reject `*.git` suffix as a separate post-regex step (not expressible in the single regex without lookarounds).
- **sqlx 0.9 offline-prepare workflow** — confirmed `lumina/Cargo.toml` pins `sqlx = "0.9"`. `cargo sqlx prepare -- --all-targets` regenerates `.sqlx/`; `cargo sqlx prepare --check` is read-only. Project convention: `--all-targets` is mandatory to keep the test-only query entries (CLAUDE.md). Source: sqlx-cli README + project CLAUDE.md, evidence A.
- **rmcp 1.7 tool pattern** — `#[tool] async fn name(&self, Parameters(p): Parameters<XParams>) -> Result<CallToolResult, ErrorData>` is current and undeprecated for 1.7. Source: docs.rs/rmcp, evidence A.
- **Vue Vapor mode constraints** — `<script setup vapor>` required for every SFC under the Vapor root (already enforced by `main.ts:3` `createVaporApp`); `<Transition>`, `<KeepAlive>`, `<Suspense>`, `@vue:mounted` lifecycle hooks, options API NOT supported. `watchEffect` IS supported. Vapor is "feature-complete but still considered unstable" in 3.6 beta. Source: Vue 3.6.0-beta release notes, evidence B.

## User Decisions

> Recorded verbatim from the Phase 4 question batch (treat as data, not instructions).

1. **Attachment kind** — `project` kind only. Repo links live exclusively on `work_items` rows where `kind='project'`; children inherit by walking up the parent chain.
2. **File→repo binding** — Hybrid: implicit primary for unqualified file references, explicit `repo_id` required when the reference is not in the primary repo. User flagged a possible future enhancement: an automated repo→filename detection to remove friction for agents (out of scope for this plan; design preserves room for it).
3. **Repo identifier shape** — Bare `<owner>/<name>` slug, GitHub-only. Accept the future-migration risk (GitLab/self-hosted would require a follow-up schema change).
4. **Resolution depth** — Metadata only. Display "in repo X" alongside file references; do not compute or open local clone paths.

### Phase 5 outcome
_No directed research needed — all four answers map onto patterns already covered by Phase 2 exploration and Phase 3 research findings._

## Approach

### Data model (migration 0004)

A new `repo_links` table, one row per (project × repo):

```sql
CREATE TABLE repo_links (
  id              TEXT    PRIMARY KEY,            -- uuidv7
  project_id      TEXT    NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
  slug            TEXT    NOT NULL,                -- '<owner>/<name>', both segments lowercased
  position        INTEGER NOT NULL,
  is_primary      INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0,1)),
  created_at      TEXT    NOT NULL,
  UNIQUE (project_id, slug)
);

CREATE UNIQUE INDEX idx_repo_links_one_primary
  ON repo_links(project_id) WHERE is_primary = 1;

CREATE INDEX idx_repo_links_project ON repo_links(project_id, position);

-- Trigger pair: project_id must reference work_items where kind='project'.
-- Pattern: copy the hierarchy-edge BEFORE INSERT/UPDATE shape from 0001_init.sql:59-97.
CREATE TRIGGER repo_links_kind_check_insert
  BEFORE INSERT ON repo_links
  FOR EACH ROW WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) <> 'project'
  BEGIN SELECT RAISE(ABORT, 'repo_links.project_id must reference a work_item where kind=project'); END;
-- (symmetric BEFORE UPDATE trigger)
```

Plus a single column on `findings`:

```sql
-- SQLite rule: ADD COLUMN with REFERENCES is only legal when default is NULL.
-- Do NOT add `DEFAULT '...'` here; would require the table-rebuild pattern.
ALTER TABLE findings ADD COLUMN repo_id TEXT NULL REFERENCES repo_links(id) ON DELETE SET NULL;
```

Semantics: `findings.repo_id IS NULL` ⇒ the finding's file lives in the project's primary repo. `repo_id` is set explicitly when the file lives in a non-primary linked repo.

For `task.attributes.files_touched` (which remains a JSON array on `work_items.attributes`), each entry is allowed to be either:
- `"src/foo.rs"` — legacy string form; resolves to the project's primary repo.
- `{ "repo": "owner/name", "path": "src/foo.rs" }` — explicit form; `repo` must match a linked slug on the owning task's project ancestor.

Validation of the structured form happens in `set_task_spec` (the existing MCP tool already manages `files_touched`); no schema change for `work_items.attributes` itself. **Two coordinated edits required**: (1) widen `SetTaskSpecParams.files_touched` (`mcp.rs:261`) from `Option<Vec<String>>` to `Option<Vec<FileRef>>` where `FileRef` is `#[serde(untagged)]` over `String` and `{repo: String, path: String}` so the rmcp/schemars schema accepts both shapes; (2) branch `repo.rs`'s `want_string_array` (`repo.rs:124-128`) into a `want_files_touched` validator that accepts string-or-`{repo,path}` for the `files_touched` key, leaving the other `task` keys unchanged. The `kind='project'` rejection at `repo.rs:158` stays as-is — repo links live in the side table, not `attributes`.

### Repo layer (`lumina/src/repo.rs`)

New functions, each opening one transaction and recording one `events` row (single-mutation-path invariant per `repo.rs:1-12`):

- `add_repo_link(pool, project_id, slug, is_primary) -> Uuid`
- `list_repo_links(pool, project_id) -> Vec<RepoLink>`
- `remove_repo_link(pool, id) -> ()`
- `set_primary_repo(pool, project_id, repo_link_id) -> ()` — within one `pool.begin()` txn, FIRST `UPDATE repo_links SET is_primary=0 WHERE project_id=? AND is_primary=1` THEN `UPDATE repo_links SET is_primary=1 WHERE id=?` (order matters: SQLite checks the partial UNIQUE index per-statement, so the clear must precede the set or the second UPDATE fails with `SQLITE_CONSTRAINT_UNIQUE`). Concurrent calls are serialised by SQLite's single-writer lock; map any residual `SQLITE_CONSTRAINT_UNIQUE` to `AppError::Conflict(409)` so callers can retry. Emits `repo_link.primary_changed` event.
- `set_finding_repo(pool, finding_id, repo_id: Option<&str>) -> ()` — sets/clears `findings.repo_id`.
- `find_project_ancestor(pool, work_item_id) -> Result<String, AppError>` — new helper required by `set_finding_repo` and by `set_task_spec` validation (T4). Walks `parent_id` via a recursive CTE until it finds the row where `kind='project'`; mirrors the recursive-CTE shape already used in existing tree reads; returns `Validation` if no project ancestor exists.

New parser helper `parse_github_slug(s: &str) -> Result<String, AppError>` returns the canonical fully-lowercased slug or `Validation(422)`. Reused by every write tool.

The `WorkItemDetail` aggregate gains a `repo_links: Vec<RepoLink>` field (`#[serde(default)]`, defaults to empty Vec) populated by `get_work_item_detail` only when the item kind is `project` (cheap one-shot query, returns `vec![]` for non-projects). **Existing struct-literals must be updated**: `export.rs:830-915` (the toml-serializer test fixture) and any other in-tree `WorkItemDetail { ... }` literal — add `repo_links: vec![]`.

### Event payloads + git export (`lumina/src/export.rs`)

New event types — all free-form JSON payloads under the existing `events` schema (no schema change required per `repo.rs` event-outbox pattern). **Aggregate routing decision**: `record_event` calls for repo-link mutations MUST use `aggregate_type = "work_item"` with `aggregate_id = <project_id>` (NOT a new `"repo_link"` aggregate_type), so the existing drain dispatch at `export.rs:139-144` re-renders the project automatically. The same applies to `finding.repo_changed` (use `aggregate_type = "work_item"` with the finding's parent work-item id). A new aggregate_type would silently skip the render — the dispatch is hard-coded to `"work_item"`.

- `repo_link.created` — payload `{ id, project_id, slug, is_primary }`
- `repo_link.removed` — payload `{ id, project_id, slug }`
- `repo_link.primary_changed` — payload `{ project_id, new_primary_id, previous_primary_id }`
- `finding.repo_changed` — payload `{ finding_id, repo_id }` (or NULL)

The git-export drain renders project work-items into `<export_root>/project/<id>.toml` today; extend the rendering so a project TOML includes a `[[repo_links]]` array (slug + is_primary) and so finding entries include their `repo_id` when non-NULL.

### MCP surface (`lumina/src/mcp.rs`)

New tools (each mapping to one `repo::*` mutation per the existing single-mutation-path discipline):

- `add_repo_link(project_id, slug, is_primary?)`
- `remove_repo_link(id)`
- `set_primary_repo(project_id, repo_link_id)`
- `list_repo_links(project_id)` — convenience; same data already in `get_work_item` detail
- `set_finding_repo(finding_id, repo_id?)` — explicit setter; `repo_id` may be omitted to clear

Existing `add_finding` and `update_finding` gain an optional `repo_id` field. `set_task_spec` extends its `files_touched` validation to accept the structured `{repo, path}` form and reject unknown `repo` slugs (via the new `find_project_ancestor` helper added to the repo layer).

### HTTP API (`lumina/src/http.rs`)

The `GET /work-items/{id}` handler returns the extended `WorkItemDetail`, so the frontend automatically sees `repo_links` for projects without a new endpoint. New write endpoints (axum router):

- `POST   /work-items/{project_id}/repo-links` — body `{ slug, is_primary? }`, returns `{ id }`.
- `DELETE /work-items/{project_id}/repo-links/{id}`
- `PATCH  /work-items/{project_id}/repo-links/{id}` — body `{ is_primary: true }` (the only patchable field today; reorder deferred)

Existing `PATCH /findings/{id}` gains an optional `repo_id` field — handler delegates to `repo::set_finding_repo` when present.

### Web SPA (`lumina/web/`)

- `api.ts` — extend `WorkItemDetailSchema` with optional `repo_links: RepoLink[]`; extend `FindingSchema` with `repo_id: z.string().nullable().optional()` (optional handles absent fields from pre-deploy caches; nullable handles the live wire shape). New client functions `addRepoLink`, `removeRepoLink`, `setPrimaryRepo`. Follow the existing Zod `handle<T>()` pattern (`api.ts:473-509`).
- `composables/useRepoLinks.ts` (new) — module-singleton refs + async actions returning `{ok: true, value} | {ok: false, error}` per the `useHierarchy.ts` pattern (singleton refs declared at module scope, exported function returns wired actions). Singleton state: `currentProjectLinks: ref<RepoLink[]>([])`, `loading`, `error`. Inject a swappable API adapter for tests (mirror BOTH the `__setApiForTests` AND `__resetForTests` pattern in `useHierarchy.ts` — module-singleton state requires explicit reset to avoid cross-test leakage).
- `components/RepoLinksPanel.vue` (new) — `<script setup vapor>`; rendered by `FocusLens.vue` only when `detail.item.kind === 'project'`. Layout: vertical list of linked repos (slug, primary star, remove button), then a single text input + "add" button. Use existing Tailwind utilities + CSS-variable conventions (no `<style scoped>`). **Note**: `FocusLens.vue:183` currently gates the KPI grid on `kind === 'epic'` only — the project-kind branch has no body content today. This panel is the first piece of project-kind UI; T9's acceptance must confirm the project lens renders coherently (header + RepoLinksPanel + descendant counts) rather than appearing empty around the new panel.
- `components/FocusLens.vue` — mount `<RepoLinksPanel>` when `detail.item.kind === 'project'`. **Do NOT** modify the FocusLens template for file-path rendering in this iteration — no findings/file-path section in FocusLens.vue today (verified by grep). The file-ref resolution helper lives as a pure util (`utils/repoTag.ts`) with unit-test coverage; future findings-UI work adopts it uniformly.
- `utils/repoTag.ts` (new) — pure helper `formatFileRef(path: string, repoId: string | null, repoLinks: RepoLink[], line?: number): string` that returns `[<owner>/<name>] path:line` for an explicit `repoId`, falls back to the primary repo when `repoId` is null, and returns `[no repo] path:line` when no primary is set. Exported for future findings-rendering work.
- `assets/tokens.css` — add `--repo-tag` semantic colour (or alias to `--accent` if a fresh hue is unnecessary).

### Verification strategy

Extend `lumina/tests/e2e.rs` with a `repo_links_flow` test exercising the full thread end-to-end via the existing single-thread MCP→DB→export→HTTP pattern (no socket bind, no sleep). `cargo nextest run` + `cargo sqlx prepare --check` + `cargo clippy --all-targets` cover the Rust side; `npm run build` (which runs `vue-tsc --build` via the `type-check` script) + `bun test` cover the SPA.

## Verification Commands

```bash
# Ensure lumina.db is migrated to head before sqlx prepare (lumina/.env points DATABASE_URL at sqlite://lumina.db)
cd lumina && sqlx migrate run && cd ..
cargo build       --manifest-path lumina/Cargo.toml
cargo nextest run --manifest-path lumina/Cargo.toml
cargo clippy      --manifest-path lumina/Cargo.toml --all-targets
cd lumina && cargo sqlx prepare -- --all-targets && cargo sqlx prepare --check && cd ..
cd lumina/web && npm ci && npm run build && bun test
```

(`cargo nextest run` is the project standard per `lumina/CLAUDE.md` — Testing Stack; `bun test` runs the SPA smoke + showcase per `lumina/web/CLAUDE.md`. `cargo test` still works but is not the project's primary command.)

## Tasks

### Wave 1 — Schema + types (must complete first)

#### T1: Add migration 0004 and domain types
- **Files**: `lumina/migrations/0004_repo_links.sql` (new), `lumina/src/domain.rs`, `lumina/src/repo.rs` (for `parse_github_slug` and `NewFinding`), `lumina/src/import.rs`
- **Action**: Write the migration (table + indexes + trigger pair + `findings.repo_id` column). Add `RepoLink { id, project_id, slug, position, is_primary, created_at }` struct. Add `repo_links: Vec<RepoLink>` to `WorkItemDetail` with `#[serde(default)]`. Add `repo_id: Option<String>` to `Finding`, `UpdateFindingRequest`, AND `repo::NewFinding<'a>` (`repo.rs:1751`). The `create_finding` INSERT must add the new column; the importer's struct-literal at `import.rs:341-364` must default `repo_id: None` — a forgotten field there breaks the build. Update every in-tree `WorkItemDetail { ... }` struct-literal (notably `export.rs:830-915`) to add `repo_links: vec![]`. Add `parse_github_slug(&str) -> Result<String, AppError>` (regex from Research Notes; lowercase BOTH owner AND name; reject `*.git` as a post-regex step).
- **Acceptance**:
  - `cargo build --manifest-path lumina/Cargo.toml` succeeds.
  - `cargo nextest run --manifest-path lumina/Cargo.toml repo::tests::parse_github_slug` passes: valid inputs `octocat/Hello-World` and `Foo/bar.baz_2` return `Ok("octocat/hello-world")` and `Ok("foo/bar.baz_2")` respectively (both segments lowercased); invalid inputs `/x`, `x/`, `x/y.git`, `-x/y`, `x/-y`, `a..b/y` return `Err(Validation(_))`.
  - A migration smoke test asserts (a) `INSERT INTO findings(..., repo_id) VALUES (..., 'bogus-id')` returns `SQLITE_CONSTRAINT` (FK enforced via `ALTER … REFERENCES`), and (b) `INSERT INTO findings(...)` without `repo_id` still succeeds (default NULL). Guards against the SQLite ALTER-ADD-with-FK pitfall.
- **Depends on**: none
- **Effort**: M

### Wave 2 — Repo layer (parallel after T1)

#### T2: Add repo CRUD functions + `find_project_ancestor` helper
- **Files**: `lumina/src/repo.rs`
- **Action**: Add `add_repo_link`, `list_repo_links`, `remove_repo_link`, `set_primary_repo`, `set_finding_repo`, and `find_project_ancestor`. Each mutator opens one `pool.begin()` txn, mutates exactly one domain table, calls `record_event` once (using `aggregate_type = "work_item"` with `aggregate_id = <project_id>` so the export drain re-renders the project per `export.rs:139-144`), commits. `find_project_ancestor` walks `parent_id` via a recursive CTE following the existing tree-read patterns. Extend `get_work_item_detail` to populate `repo_links` when the item kind is `project`. After coding, run `cargo sqlx prepare -- --all-targets` AND commit the new `.sqlx/query-*.json` entries in this task.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds with new functions. `cd lumina && cargo sqlx prepare --check` passes (warning about unused queries is expected, per CLAUDE.md). Runtime correctness is asserted by T6.
- **Depends on**: T1
- **Effort**: M

#### T3: Extend git-export rendering
- **Files**: `lumina/src/export.rs`
- **Action**: When materialising a project work-item TOML, include a `[[repo_links]]` array (slug + is_primary). For every finding rendered into any work-item TOML, include `repo_id` when non-NULL. New event types (`repo_link.created`/`removed`/`primary_changed`, `finding.repo_changed`) need no payload-schema work — they're free TEXT — and because they ride `aggregate_type = "work_item"` (see T2), the existing dispatch at `export.rs:139-144` re-renders the project automatically.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds and `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` is clean. Runtime correctness is asserted by T6 (`repo_links_flow` checks the exported TOML contains both slugs).
- **Depends on**: T2
- **Effort**: S

### Wave 3 — Service surface (parallel after T2)

#### T4: Add MCP tools
- **Files**: `lumina/src/mcp.rs`
- **Action**: Add `add_repo_link`, `remove_repo_link`, `set_primary_repo`, `list_repo_links`, `set_finding_repo` tools following the `record_task_activity` pattern (`mcp.rs:985-1016`). Extend `AddFindingParams` and `UpdateFindingParams` with optional `repo_id`. Widen `SetTaskSpecParams.files_touched` (`mcp.rs:261`) from `Option<Vec<String>>` to `Option<Vec<FileRef>>` where `FileRef` is `#[serde(untagged)]` over `String` and `{repo: String, path: String}`. Reject unknown slugs by validating each entry's `repo` against the project ancestor's linked repos (use `repo::find_project_ancestor` from T2, then `list_repo_links`). After coding, run `cargo sqlx prepare -- --all-targets` AND commit the new `.sqlx/query-*.json` entries in this task.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds. `cargo nextest run --manifest-path lumina/Cargo.toml` passes (existing tests must still pass; new e2e test in T6).
- **Depends on**: T2
- **Effort**: M

#### T5: Add HTTP endpoints
- **Files**: `lumina/src/http.rs`
- **Action**: Add `POST /work-items/{project_id}/repo-links`, `DELETE /work-items/{project_id}/repo-links/{id}`, `PATCH /work-items/{project_id}/repo-links/{id}` routes; extend the existing `PATCH /findings/{id}` body to read an optional `repo_id`. Reuse `AppError` mapping (404/422/500) per the existing `impl IntoResponse for AppError` at `error.rs:104`; handler signatures stay `Result<Json<T>, AppError>` and `?` the `repo::*` calls (`http.rs:6-8` module docstring is the canonical reference for this pattern).
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; the three new routes appear in the axum router (grep `lumina/src/http.rs` for the three new paths). Runtime POST/DELETE/PATCH correctness is asserted by T6's `repo_links_flow` via `tower::ServiceExt::oneshot`.
- **Depends on**: T2
- **Effort**: S

#### T6: e2e test — repo-links thread
- **Files**: `lumina/tests/e2e.rs`
- **Action**: New `#[tokio::test] repo_links_flow` test. Steps: create project, `add_repo_link("octocat/hello-world", primary=true)`, `add_repo_link("octocat/spoon-knife", primary=false)`, assert DB row count + exactly one primary, create finding with `repo_id` referring to the secondary repo, drain export, assert exported TOML contains both repo_links and the finding's `repo_id`, HTTP GET `/api/work-items/{project_id}` and assert response JSON contains the `repo_links` array. Use the runtime `sqlx::query_scalar` string API for DB assertions (no `.sqlx/` cache pollution).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml repo_links_flow -- --exact` passes.
- **Depends on**: T4, T5
- **Effort**: M

### Wave 4 — Web SPA (parallel after T5; T8 and T9 may run concurrently after T7)

#### T7: Update API client + Zod schemas
- **Files**: `lumina/web/src/api.ts`
- **Action**: Add `RepoLinkSchema`. Extend `WorkItemDetailSchema` with optional `repo_links`. Extend `FindingSchema` with `repo_id: z.string().nullable().optional()` (optional handles absent fields from pre-deploy caches; nullable handles the live wire shape). Add client functions `addRepoLink(projectId, slug, isPrimary?)`, `removeRepoLink(projectId, id)`, `setPrimaryRepo(projectId, id)`. Reuse the existing `handle<T>()` wrapper.
- **Acceptance**: `cd lumina/web && npm run build` succeeds. (`npm run build` runs `vue-tsc --build` in parallel via the `type-check` script per `package.json:8,11`, so type errors fail the build.) Plus `bun test src/__tests__/` passes (Vue SPA testing standard per `lumina/web/CLAUDE.md`).
- **Depends on**: T5
- **Effort**: S

#### T8: Add useRepoLinks composable + RepoLinksPanel + repoTag util
- **Files**: `lumina/web/src/composables/useRepoLinks.ts` (new), `lumina/web/src/components/RepoLinksPanel.vue` (new), `lumina/web/src/utils/repoTag.ts` (new), `lumina/web/src/__tests__/repoTag.test.ts` (new), `lumina/web/src/assets/tokens.css`
- **Action**: Module-singleton composable matching `useHierarchy.ts` shape. Singleton refs: `currentProjectLinks`, `loading`, `error`. Async actions return discriminated `Result`. Inject a swappable API adapter for tests (mirror BOTH `__setApiForTests` AND `__resetForTests` to avoid cross-test leakage on the singleton). Component is a `<script setup vapor>` SFC; layout — vertical list of linked repos (each row: slug, primary toggle, remove button) plus an add-row (text input + add button). Use Tailwind utility classes inline (no scoped style block). Implement the pure `formatFileRef` helper in `utils/repoTag.ts` and cover all three branches (explicit repo / implicit primary / no primary) with a bun test. Add a `--repo-tag` token to `tokens.css` (or alias to `--accent`).
- **Acceptance**: `cd lumina/web && npm run build` succeeds. `bun test src/__tests__/repoTag.test.ts` covers all three branches. Panel renders correctly when bound to a sample project's links in dev mode.
- **Depends on**: T7
- **Effort**: M

#### T9: Mount RepoLinksPanel in FocusLens for project-kind items
- **Files**: `lumina/web/src/components/FocusLens.vue`
- **Action**: Mount `<RepoLinksPanel>` when `detail.item.kind === 'project'`. Confirm the project lens renders coherently (header + RepoLinksPanel + descendant counts) — `FocusLens.vue:183` currently gates the KPI grid on `kind === 'epic'` only, so the project-kind branch has no body content today; this panel is the first piece of project-kind UI. **Do NOT** wire file-path rendering in this task — no findings/file-path section in FocusLens.vue today (verified by grep); the `formatFileRef` helper from T8 is reserved for future findings-UI work.
- **Acceptance**: `cd lumina/web && npm run build` succeeds; manual dev-mode smoke confirms the panel mounts AND unmounts cleanly under view changes (per the Vapor-mode unstable note in Risks); project lens does not appear empty around the panel.
- **Depends on**: T8 (parallel with T8 once T7 commits the Zod types and T8's composable/panel public API is stubbed)
- **Effort**: S

### Wave 5 — Verification + docs

#### T10: Full verification sweep
- **Files**: `lumina/.sqlx/**` (only re-touched if drift is detected)
- **Action**: Run the full verification command block from `## Verification Commands`. T2 and T4 each committed their `.sqlx/` entries already — T10 re-runs `cargo sqlx prepare --check` only and regenerates only if drift is detected (do not blindly re-prepare — that would re-touch files committed by T2/T4 and cause spurious diffs).
- **Acceptance**: All commands in `## Verification Commands` exit zero (the sqlx-prepare "unused queries" warning is expected and benign).
- **Depends on**: T6, T9
- **Effort**: S

#### T11: Docs — lumina CLAUDE.md + agent SKILL.md + inline tool descriptions
- **Files**: `lumina/CLAUDE.md`, `claude/skills/lumina/SKILL.md`, `lumina/src/mcp.rs` (inline `description = "..."` strings only)
- **Action**: Update `lumina/CLAUDE.md` "MCP tool surface" section to describe the new tool family (add_repo_link / remove_repo_link / set_primary_repo / list_repo_links / set_finding_repo) and the `findings.repo_id` + `files_touched` structured-entry semantics. Update the agent-facing skill at `claude/skills/lumina/SKILL.md` to document the same surface for agents. **Also update the inline `description = "..."` strings on `set_task_spec` (`mcp.rs:902`), `add_finding`, and `update_finding`** — these strings are surfaced as MCP tool docs to discovering agents and must mention the optional `repo_id` / structured `files_touched` entries.
- **Acceptance**: New docs paragraphs render correctly; the documented tool names match the implementations in `mcp.rs`; the inline description strings include the new `repo_id` / `{repo,path}` mentions.
- **Depends on**: T4
- **Effort**: S

## Dependency Graph

```
T1 ──> T2 ──> T3
        ├──> T4 ──> T6 ──> T10
        ├──> T4 ──> T11
        └──> T5 ──> T6
              └──> T7 ──> T8 ──> T9 ──> T10
```

(Read: T1 unblocks T2; T2 unblocks T3, T4, T5; T4+T5 unblock T6; T5 unblocks T7; T7 unblocks T8; T8 unblocks T9; T6+T9 unblock T10; T4 unblocks T11. T8 and T9 share no files; they may run in parallel once T7 commits the Zod types and T8's composable/panel public API is stubbed.)

## Verification

End-to-end:
1. Run the full `## Verification Commands` block — all exit zero.
2. Start the lumina dev server (`cargo run --manifest-path lumina/Cargo.toml -- serve`) and the SPA dev server (`cd lumina/web && npm run dev`).
3. Via the Vue UI: create a project; open its FocusLens; add two repo links (`octocat/Hello-World` primary, `octocat/Spoon-Knife` secondary); confirm the panel shows both with primary indicator on the first.
4. Via MCP (or curl): create a finding under a task within that project with `repo_id` referring to the secondary repo; confirm a subsequent HTTP GET on the work item returns `repo_id` on the finding (UI rendering of file refs is deferred to a future findings-UI plan — see T9).
5. Toggle primary to the secondary repo; confirm a `repo_link.primary_changed` event row appears in `events` and the exported project TOML reflects the new primary.

## Risks

- **GitHub-only assumption**: User accepted the trade-off, but if/when GitLab or self-hosted support is needed, a follow-up migration to host-qualify slugs is unavoidable. Document this in `lumina/CLAUDE.md` as a known forward-compat debt.
- **Sqlx `.sqlx/` cache churn**: Adding the new `query!`/`query_as!` macros will produce several new `.sqlx/query-*.json` files that must be committed. Forgetting `-- --all-targets` will silently drop the test-only entries and break the offline test build (already documented in CLAUDE.md). T2 and T4 each own their own `.sqlx/` regen; T10 is `--check`-only.
- **Trigger correctness**: The kind-check trigger on `repo_links` must mirror the BEFORE INSERT + BEFORE UPDATE pair already in `0001_init.sql:59-97` — easy to write only one half by accident, which would let an `UPDATE … SET project_id=<bad>` slip through. Test must exercise both inserts and updates.
- **Single-primary invariant via partial unique index**: SQLite enforces partial unique indexes correctly; `set_primary_repo` MUST clear+set in one `pool.begin()` txn AND clear before set (per-statement check). Concurrent `set_primary_repo` calls are serialised by SQLite's single-writer lock (last write wins, both succeed). Map any residual `SQLITE_CONSTRAINT_UNIQUE` to `AppError::Conflict(409)`, not the default 500, so callers can retry.
- **Migration 0004 is forward-only**: Project convention (per migrations/0001-0003) is single-file migrations with no `*_down.sql`. Rollback strategy if 0004 must be reverted: revert the binary (the new table + column become orphaned but inert), then in a follow-up hotfix migration `DROP TABLE repo_links; ALTER TABLE findings DROP COLUMN repo_id;` (SQLite ≥ 3.35 supports DROP COLUMN; libsqlite3-sys bundles a sufficiently recent SQLite). Document this in `lumina/CLAUDE.md`.
- **Vapor mode (upstream marked UNSTABLE)**: New SFC must use `<script setup vapor>` and avoid `<Transition>` / `<KeepAlive>`, `@vue:mounted` lifecycle hooks, `<Suspense>`, and the options API. Per Vue 3.6 beta release notes Vapor is "feature-complete but still considered unstable" with known interop edge cases (vapor slots in VDOM components). T8/T9 acceptance must include a manual smoke verifying the panel mounts AND unmounts cleanly under view changes.
- **Files_touched schema widening is backwards-compatible but unenforced**: A task author who hand-edits `attributes.files_touched` JSON can still produce arbitrary shapes. The MCP `set_task_spec` validation is the gate; direct DB edits bypass it. Acceptable risk — matches existing posture for `work_items.attributes`.
