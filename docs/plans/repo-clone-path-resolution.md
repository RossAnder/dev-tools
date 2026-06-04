# Plan: Repo clone-directory & path resolution (layer 1 — path substrate)

**Plan path**: docs/plans/repo-clone-path-resolution.md
**Created**: 2026-06-02
**Status**: review-applied 2026-06-04 (11 findings merged from `/review-plan` round 1); ready for `/implement`
**Architecture**: enabling substrate for [ADR-0004](../adr/0004-harness-session-corpus.md). Prerequisite for `harness-session-corpus` (cwd→project correlation rides on this).
> Last revised: 2026-06-04
> Paths updated 2026-06-04 for the joyful-singing-crane refactor: `repo.rs` / `mcp.rs` / `domain.rs` are now submodule directories. This plan's surface lands entirely in the **repo-links family** — `repo/repo_links.rs`, `http/repo_links.rs` (unsplit), `domain/findings.rs` (the `RepoLink` struct's home) — plus a new tiny `http/settings.rs`. No MCP tool is added (Q1 resolution), so `mcp/repo_links.rs` and the `mcp/mod.rs` count-invariant are **untouched**, and no new submodule / `repo/mod.rs` re-export edits are needed (`repo/mod.rs` already `pub use repo_links::*`).

## Objective

Give each linked repo a per-machine **Clone directory** so lumina can resolve repo-relative paths to absolute and map a session's cwd to its **Project**. Add a per-machine **Clone root** default for the "offer to clone" action.

## Constraints

- **Additive, forward-only** migration — nullable `local_path` (ADD-COLUMN rule); no down-migration.
- **Single-mutation invariant** preserved for the new `set_repo_local_path` mutator (+1 `repo_links` row updated / +1 `events` row, one tx). Event routes to the owning **project**'s `work_item` aggregate — mirroring the existing `repo_link.created`/`.removed`/`.primary_changed` events — so `export.rs`'s drain re-renders the project (NOT a fresh aggregate_type the drain would skip).
- **Runtime sqlx only** (`rg` gate stays 0).
- **No new MCP tool** (Q1 resolution — "no mcp tool needed"). The whole path-substrate is operator/SPA-configured + internally-resolved. The `mcp.rs` count-invariant stays **73**; do NOT touch it or its test.
- **Single-machine-now (deliberate, per ADR-0004):** `local_path` lives on the shared `repo_links` row and `clone_root` is a per-machine env var; a per-machine path layer is explicitly deferred. Do NOT build the shared-remote per-machine layer here. (Caveat carried in Risks: `local_path` flows into the git-export project snapshot — fine on one machine, a leak to relocate when the shared-remote split lands.)
- **No canonicalisation at store time** — `local_path` may name a directory that does not yet exist (the operator records the intended clone path *before* running `git clone`). Normalise for comparison/storage but never `std::fs::canonicalize` the stored value.

## Scope

- **In**:
  - `repo_links.local_path` (nullable TEXT; NULL = "not cloned on this machine").
  - `LUMINA_CLONE_ROOT` env var + a `resolve_clone_root() -> Option<PathBuf>` resolver (mirrors `export::resolve_export_root`), surfaced read-only via `GET /api/settings`.
  - `resolve_repo_path(local_path, rel) → abs` — pure join+normalise of a repo-relative path against a clone dir.
  - `resolve_cwd_to_project(cwd) → Option<project_id>` — reverse-lookup against `local_path` prefixes (longest-prefix-wins; genuine tie → unresolved).
  - `set_repo_local_path` repo mutator + **HTTP-only** mirror (`PATCH .../repo-links/{id}/local-path`); no MCP tool.
  - SPA: per-repo-link `local_path` field on the project repo-detail + the "offer to clone → `<clone_root>/<name>`" affordance (records the binding; does not clone).
- **Out**:
  - A per-machine / shared-remote path layer (deferred per ADR-0004).
  - lumina shelling out to `git clone` (Q2: record/offer only — the operator clones out-of-band).
  - Rewriting historical `files_touched` / finding `file:line` entries.
  - `formatFileRef` (`lumina/web/src/utils/repoTag.ts`) and finding `file:line` rendering stay **relative-only** — absolute-path rendering via `local_path` is deferred to layer 2 (`harness-session-corpus`). Left deliberately untouched, not missed. *(review P11)*
  - An MCP tool for `local_path` or `clone_root`, and any settings *write* endpoint (`clone_root` is env-driven; the only settings route is the read `GET /api/settings`).
- **Affected areas**: `lumina/migrations/`, `lumina/src/repo/repo_links.rs` (mutator + `resolve_*` + path-normalise helpers + FromRow + the one `list_repo_links` SELECT), `lumina/src/domain/findings.rs` (the `RepoLink` struct + `local_path` field), `lumina/src/http/repo_links.rs` (HTTP mirror), `lumina/src/http/settings.rs` (new, read-only), `lumina/src/http/mod.rs` (mount the settings sub-router — `pub mod settings;` + `.merge(settings::router())`; **`app.rs` is NOT touched** — it only `.nest("/api", http::router())`), `lumina/web/`, `lumina/CLAUDE.md`. (`lumina/CONTEXT.md` already carries the *Clone directory* / *Clone root* glossary terms — see ADR-0004.)

## Resolved decisions

### Grilling 2026-06-02
- Binding lives on `repo_links.local_path` (chosen over a machine-local config file or a `repo_local_paths`-by-machine table) — simplest correct thing for the single-machine reality.
- `clone_root` is a per-machine setting (e.g. `~/dev`).

### Open Design Questions — resolved 2026-06-04

**Q1 — `clone_root` storage → `LUMINA_CLONE_ROOT` env var + read-only `/api/settings`; no MCP tool, no settings table.**
Decisive fact found during grilling: lumina has **no settings table** today — every per-machine knob is an env var (`LUMINA_EXPORT_ROOT`, `LUMINA_PTY_PROJECTS_ROOT`, `LUMINA_WORKTREE_ROOT`) resolved via a `resolve_*()` precedence fn. The original lean (a lumina-owned DB setting "so the SPA can edit it") would mean building net-new settings infrastructure (migration + table + repo mutator + MCP tool + HTTP write + SPA editor) for a single advisory value. User steer: **"no mcp tool needed."** So `clone_root` mirrors `resolve_export_root` exactly: `resolve_clone_root() -> Option<PathBuf>` reading `LUMINA_CLONE_ROOT`, surfaced **read-only** at `GET /api/settings` (`{ clone_root, export_root }`). The SPA reads it to seed the clone-offer default; editing is operator-level (env/restart), identical to export root. Per-machine by construction — correct under the deferred shared-remote topology. Lowest scope: no migration, no table, no write surface.

**Q2 — lumina records/offers; it does not clone.**
Consistent with the "lumina never polices git" principle and the operator-triggered-export precedent (lumina records intent; the operator acts). The SPA "offer to clone" affordance, when a repo-link's `local_path` is NULL and `clone_root` is set, suggests the path `<clone_root>/<name>` (`name` = the repo slug's name segment) with a **"Use this path"** action that PATCHes `local_path` to the suggestion. lumina records the binding; the operator runs the actual `git clone` out-of-band. No `git clone` invocation, no shell-out, in scope.

**Q3 — cwd→project tie-break: longest-prefix-wins; genuine tie → unresolved (None).**
Standard mount-point/route resolution semantics. `resolve_cwd_to_project` loads all `(project_id, local_path)` where `local_path IS NOT NULL`, normalises cwd + each `local_path`, keeps those where cwd is **within** `local_path` at a component boundary, and returns the project of the **longest** matching `local_path`. Two refinements:
- **Component-boundary match** (not raw string prefix): cwd matches `local_path` iff `cwd == local_path` OR `cwd` starts with `local_path + separator`. So `/dev/foobar` does NOT match the clone dir `/dev/foo`.
- **Genuine ambiguity → None**: if the longest match is tied (equal length) across **two or more distinct `project_id`s** — e.g. the same clone dir linked to two projects — return `None` (unresolved), logged at `debug`. This is the safe floor for the corpus-correlation consumer (plan 2): a missing project binding degrades gracefully to "cwd-only / drop", whereas a *wrong* binding silently mis-attributes a whole session. Nesting (`/dev/mono` vs `/dev/mono/pkg/sub`) is unambiguous and resolves to the deeper repo.

## Tasks

### Phase 1: schema + domain (T1 → T2a → T2b; T2a reads the column T1 adds)

#### T1: Add the `local_path` column
- **Files**: `lumina/migrations/0014_repo_local_path.sql` (new)
- **Action**: `ALTER TABLE repo_links ADD COLUMN local_path TEXT;` — nullable, no default (ADD-COLUMN rule; SQLite accepts a nullable add with no default). Add a one-line SQL comment: per-machine absolute clone directory; NULL = not cloned on this machine.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml` migration test passes against a fresh DB; `ALTER` applies on top of 0013 with no error. `0014` is the next free number (latest on disk is `0013_team_execution.sql`).
- **Blocked-by**: none

#### T2a: `RepoLink.local_path` field + FromRow + SELECT plumbing
*(review P8: split out from the old T2 so the mechanical schema-plumbing gates separately from the algorithmic path fns in T2b.)*
- **Files**: `lumina/src/domain/findings.rs` (the `RepoLink` struct), `lumina/src/repo/repo_links.rs` (FromRow + the `list_repo_links` SELECT)
- **Action**:
  1. *(review P3)* Add `pub local_path: Option<String>` to `RepoLink` (place before `created_at`; all-scalar struct, so the export tables-last rule is unaffected), carrying `#[serde(skip_serializing_if = "Option::is_none")]` to match **every** sibling Option field in `domain/findings.rs` (e.g. `repo_id` at l.79) — this is load-bearing for the common NULL (uncloned) case under `toml::to_string_pretty` export (`export.rs:278`), and is **not** caught by `cargo build`. Add a doc-comment.
  2. *(review P2)* In the `impl FromRow for RepoLink` (repo_links.rs:111), add `local_path: row.try_get("local_path")?` **and add `Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>` to the where-clause** — this bound is REQUIRED (not optional), mirroring the `PendingEvent` FromRow at `export.rs:72` which restates the `Option<String>` bound even though it already bounds `String`; the generic-over-`R` recipe does not infer the Option bound from the String bound.
  3. Add `local_path` to the **one** `SELECT … FROM repo_links` that maps to `RepoLink` (`list_repo_links`, repo_links.rs:139–150). This is the sole `RepoLink` read-site — `reads.rs:101` detail-folding and the export path both funnel through `list_repo_links`, so this single edit covers detail + export.
- **Acceptance**: `cargo build` clean; `rg 'FROM repo_links'` shows the `local_path` column present in the `list_repo_links` SELECT and absent from the non-`RepoLink` helper SELECTs (position/primary/ownership lookups stay as-is); `GET /api/work-items/{project_id}` detail includes `local_path` on each repo-link (NULL until set).
- **Blocked-by**: T1

#### T2b: path-resolution functions
*(review P8: the algorithmic half; P5/P6 pin the previously-underspecified cross-OS path logic; P4 adds the soft-delete guard.)*
- **Files**: `lumina/src/repo/repo_links.rs` (new fns; auto-re-exported by `pub use repo_links::*` in `repo/mod.rs`)
- **Action**:
  1. *(review P5)* Add a private `normalise_path_for_compare(p: &str) -> String` with this **pinned ordered algorithm** (do NOT reuse `pty::jsonl_tail::parse::sanitise_cwd` — it slugifies separators, replacing *every* non-alphanumeric byte incl. `/`,`\`,`:` with `-`; only its verbatim-prefix-strip idea transfers): (1) strip a leading Windows verbatim prefix — both the plain `\\?\` form **and** the verbatim-UNC `\\?\UNC\` form (the UNC strip must yield a leading double-separator `\\…`, not a bare `UNC\…`); (2) replace `\` → `/`; (3) strip a single trailing `/` but **never** reduce a root (`C:/` or `/`) to empty; (4) on `cfg(windows)` **only**, lowercase via `to_ascii_lowercase`. **No `canonicalize`** (the path may not exist). See the host-keyed-case-fold caveat in Risks. *(Revised post-review — execution-record E15/E16, findings R4/R5/R7: a repeated-separator collapse preserving a UNC leading `//` was inserted between steps (2) and (3); the algorithm was split into `normalise_path_structural` (steps 1–3, **case-preserved**, used for storage and as the comparison base) and `normalise_path_for_compare` (= structural + step (4) case-fold, used only by the matchers) so storage keeps operator casing while comparison stays host-case-folded.)*
  2. *(review P6)* Add `pub fn resolve_repo_path(local_path: &str, rel: &str) -> PathBuf` — pure: normalise `local_path`; lexically normalise `rel`, **cancelling** every `..` component (popping the last pushed component, clamped at the base — review R5/E15; was "drop every `..`") so the result can never ascend above `local_path` (**CLAMP, not error** — the `-> PathBuf` signature has no error channel); **ignore an absolute `rel`** (`PathBuf::join` silently REPLACES the base on an absolute arg — guard `Path::new(rel).is_absolute()` and treat as relative-to-root); join onto the normalised `local_path`. Document: a `..`-escaping or absolute `rel` is clamped to `local_path`, never escapes. No DB.
  3. Add `pub fn select_longest_prefix_project(cwd: &str, candidates: &[(String /*project_id*/, String /*local_path*/)]) -> Option<String>` — pure Q3 algorithm (component-boundary match, longest-prefix-wins, tie-across-distinct-projects → None). Keep DB-free for direct unit testing.
  4. *(review P4)* Add `pub async fn resolve_cwd_to_project(db: &impl DbClient, cwd: &str) -> Result<Option<String>, AppError>` — thin wrapper: `SELECT rl.project_id, rl.local_path FROM repo_links rl JOIN work_items w ON w.id = rl.project_id WHERE rl.local_path IS NOT NULL AND w.deleted_at IS NULL` (the `deleted_at IS NULL` guard excludes soft-deleted projects so a cwd never binds to a tombstoned project — precedent `findings_query.rs:187`), hand to `select_longest_prefix_project`.
- **Acceptance**: `cargo build` clean; the four fns are `pub`-exported and reachable at `crate::repo::*`. **Do not defer all correctness to T5**: land a smoke assertion with the fns that `resolve_repo_path` clamps a `../../etc` rel to within `local_path` and `select_longest_prefix_project` returns `None` on a distinct-project tie (full coverage in T5).
- **Blocked-by**: T2a

### Phase 2: mutator + HTTP surface (after Phase 1)

#### T3: `set_repo_local_path` mutator + HTTP mirror + settings read endpoint
- **Files**: `lumina/src/repo/repo_links.rs`, `lumina/src/http/repo_links.rs`, `lumina/src/http/settings.rs` (new), `lumina/src/http/mod.rs` *(review P1: `app.rs` removed — it is not touched)*
- **Action**:
  1. `pub async fn set_repo_local_path(db: &impl DbClient, repo_link_id: &str, local_path: Option<&str>) -> Result<(), AppError>` in repo_links.rs, following the `set_finding_repo`/`remove_repo_link` idiom: resolve the owning `project_id` BEFORE the tx (NotFound if the id is absent); when `local_path` is `Some`, **validate it is absolute against the *normalised* form** *(review P5)* — require a drive-anchored (`^[A-Za-z]:/` after `normalise_path_for_compare`) or `/`-rooted path, rejecting relative with `AppError::Validation` (do NOT gate on raw `Path::is_absolute`, which rejects `\dev\foo` and `C:foo` on Windows and differs by host OS — validating the normalised form keeps a Linux-CI executor and a Windows operator consistent) — then store the **case-preserved structural** value (the raw input is trimmed first — review R13; the stored form is `normalise_path_structural`, NOT the case-folded compare form — review R7/E16); one `db.begin()` tx → `UPDATE repo_links SET local_path = $2 WHERE id = $1` (rows_affected==0 ⇒ NotFound on a lost race) → `record_event(tx, "work_item", project_id, "repo_link.local_path_changed", {id, project_id, local_path})` → commit. `Some` sets, `None` clears.
  2. HTTP route `PATCH /work-items/{project_id}/repo-links/{id}/local-path`, body `{ "local_path": string | null }`, delegating to `repo::set_repo_local_path`. Add as a sub-path (do NOT fold into the existing `PATCH .../repo-links/{id}`, which hard-guards `is_primary=true`). Returns `{ "ok": true }`. (The `project_id` segment is structural REST clarity, consistent with the existing repo-link routes; the mutator resolves the project from the row.)
  3. `resolve_clone_root() -> Option<PathBuf>` reading `LUMINA_CLONE_ROOT` (mirror `export::resolve_export_root`'s `var_os` shape) — home it in the new `http/settings.rs`.
  4. `GET /api/settings` handler in `http/settings.rs` returning `{ "clone_root": Option<String>, "export_root": String }` from `resolve_clone_root()` + `export::resolve_export_root()`. Read-only; no DB hit. *(review P1)* Mount the `settings::router()` sub-router in `http/mod.rs` ONLY — add `pub mod settings;` (with the other `pub mod` decls) and `.merge(settings::router())` inside `http::router()`. `app.rs` is NOT touched (it only `.nest("/api", http::router())`). Register the route as `GET /settings` inside the sub-router — paths there are relative to the `/api` nest, so registering `/api/settings` would resolve to `/api/api/settings`.
- **Acceptance**: set→read round-trip via HTTP (PATCH then `GET /api/work-items/{project_id}` shows the value); clearing with `{"local_path": null}` returns the repo-link to NULL; PATCH an absent id → 404; PATCH a relative path → 422; `GET /api/settings` returns the env value (and `null` when `LUMINA_CLONE_ROOT` is unset); exactly one `events` row per successful PATCH on the project aggregate. `mcp.rs` count test still reports **73** (unchanged).
- **Blocked-by**: T2b

### Phase 3: SPA + tests + docs (after Phase 2; T4/T5/T6 parallel-safe — disjoint files)

#### T4: SPA repo-detail `local_path` field + "offer to clone" affordance
*(review P7: concrete files named; the Zod-schema edit is mandatory or `.parse()` silently strips `local_path`.)*
- **Files**:
  - `lumina/web/src/components/panels/ReposPanel.vue` — the repo-link row component; adds the `local_path` field + the clone-offer affordance.
  - `lumina/web/src/api/repo-links.ts` — **dual edit**: extend `interface RepoLink` with `local_path: string | null` AND `RepoLinkSchema` (Zod) with `local_path: z.string().nullable()` (a missing schema field makes Zod `.parse()` silently drop `local_path` even when the server returns it); add a `setRepoLocalPath(projectId, id, localPath | null)` wrapper mirroring `setPrimaryRepo`.
  - `lumina/web/src/composables/useRepoLinks.ts` — extend the `Api` adapter type **and** both the production adapter and the `__setApiForTests` double with `setRepoLocalPath`.
  - `lumina/web/src/composables/useSettings.ts` (new) — module-singleton composable (NOT Pinia, NOT vue-router) fetching `GET /api/settings`, exposing `cloneRoot`.
  - `lumina/web/src/__tests__/repoTag.test.ts` — add `local_path` to the `link()` factory (the field is required-nullable, mirroring the Rust `Option<String>`, so the factory type-errors without it).
- **Action**:
  1. Fetch `GET /api/settings` once via `useSettings.ts`, exposing `cloneRoot`.
  2. On each repo-link row, render `local_path` (editable text field) with a PATCH to `…/repo-links/{id}/local-path` on commit, and a clear action (PATCH `null`).
  3. "Offer to clone": when a repo-link's `local_path` is NULL and `cloneRoot` is set, show the suggested path `<cloneRoot>/<name>` where `name = slug.split('/')[1]` (GitHub slugs are exactly `<owner>/<name>`, lowercased at store per migration 0004) with a **"Use this path"** button that PATCHes `local_path` to the suggestion. Make clear in copy that lumina records the path; the operator runs `git clone` themselves. When `cloneRoot` is unset, fall back to a manual entry field.
- **Acceptance**: `cd lumina/web && bun run build` succeeds (it runs `vue-tsc` type-check, so a missing TS/Zod field fails it); `cd lumina/web && bun test` green (incl. the updated `repoTag.test.ts` factory); manual check — a project with a linked repo and `LUMINA_CLONE_ROOT` set shows the suggested path and binds it on click; a project with NULL clone_root shows the manual field.
- **Blocked-by**: T3

#### T5: Tests
- **Files**: `lumina/src/repo/repo_links.rs` (`#[cfg(test)] mod tests`), `lumina/tests/` (cross-cutting e2e)
- **Action**: rstest-parameterised unit tests for the pure fns (co-located): `normalise_path_for_compare` cases (plain `\\?\` strip, verbatim-UNC `\\?\UNC\` strip, `\`→`/`, trailing-slash, root-not-emptied, `cfg(windows)` case-fold, and a **verbatim-vs-bare cross-match** — a `\\?\C:\dev\foo` cwd must match a stored `C:/dev/foo`); `resolve_repo_path` round-trips + `..`-clamp + absolute-`rel`-ignored; `select_longest_prefix_project` (nested → deeper wins, exact match, component-boundary `foo` vs `foobar` non-match, no-match → None, tie-across-distinct-projects → None). DB-backed tests: `set_repo_local_path` set→read-back, `None` clear, NotFound on absent id, relative-path → Validation, one event on the project aggregate; `resolve_cwd_to_project` end-to-end against a small seeded DB (NULL `local_path` rows excluded; **a soft-deleted project's row excluded** — P4 guard; longest-prefix resolution). Cross-cutting HTTP round-trip (PATCH local-path → detail read; `GET /api/settings`) in `lumina/tests/`.
- **Acceptance** *(review P9)*: `cargo nextest run --manifest-path lumina/Cargo.toml` green; `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest --fail-under-lines 80 --fail-under-regions 70` passes (the project gate), AND the new files (the `repo_links.rs` additions, `http/settings.rs`) meet the per-file `--fail-under-file-lines 90` target.
- **Blocked-by**: T3

#### T6: Docs
- **Files**: `lumina/CLAUDE.md`
- **Action**: extend the § *Project↔repo-links* / § *HTTP routes* notes — new nullable `repo_links.local_path` column (migration 0014), the `set_repo_local_path` mutator + `PATCH …/repo-links/{id}/local-path` route, the `resolve_repo_path`/`resolve_cwd_to_project`/`select_longest_prefix_project` fns, the `LUMINA_CLONE_ROOT` env var + read-only `GET /api/settings`, and an explicit note that **the MCP tool count stays 73** (no MCP tool added). Record the shared-remote export-leak + host-keyed case-fold caveats (see Risks). CONTEXT.md already carries the *Clone directory*/*Clone root* terms (ADR-0004) — verify, no new prose needed.
- **Acceptance**: `rg 'local_path|/api/settings|LUMINA_CLONE_ROOT' lumina/CLAUDE.md` shows the new entries; the "73 tools" claim is reaffirmed (not bumped).
- **Blocked-by**: T3

## Dependency Graph

```
T1 ─→ T2a ─→ T2b ─→ T3 ─┬─→ T4
                        ├─→ T5
                        └─→ T6
```

## Verification

- `cargo build --manifest-path lumina/Cargo.toml`
- `cargo nextest run --manifest-path lumina/Cargo.toml`
- `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` = **0**
- `mcp.rs` count-invariant test asserts **73** (unchanged — no MCP tool added)
- `cd lumina/web && bun run build` (+ `bun test`)

## Risks

- **Path-normalisation across OSes** (Windows verbatim `\\?\` / `\\?\UNC\`, trailing slashes, separators, case-fold, symlinks). Mitigation: reuse **only the verbatim-prefix-strip idea** from `sanitise_cwd` (NOT its separator-slugifying body — see T2b's pinned algorithm) plus the `resolve_projects_root` env-precedence precedent in `pty/jsonl_tail/parse.rs`; the pure `normalise_path_for_compare`/`resolve_repo_path`/`select_longest_prefix_project` fns are unit-tested across the OS edge cases incl. verbatim-vs-bare cross-match (T5). **Symlinks are not resolved** (no store-time canonicalise, since the dir may not exist yet) — a `local_path` pointing through a symlink that differs from a session's realpath cwd will not match; accepted for v1, documented.
- **Host-keyed case-fold** (deferred-topology caveat) *(review P10)*: `normalise_path_for_compare` folds case on `cfg!(windows)` of the lumina **host**, not the path's own filesystem. Correct single-machine-now (the host FS is the only one in play). Under the deferred shared-remote topology a Linux-hosted lumina holding a Windows clone path would under-fold, and a Windows-hosted lumina would over-fold a case-sensitive Linux path — colliding `/dev/Foo` and `/dev/foo` (two distinct real dirs) → wrong project attribution, the exact harm Q3's tie→None guards against. Revisit with the per-machine path layer.
- **Shared-remote export leak** (deferred-topology caveat): `local_path` lives on the shared `repo_links` row and flows into the git-export project snapshot. On one machine that is the operator's own path in their own export trail — fine. Under a future shared-remote lumina the absolute machine path would leak across machines; that is exactly the per-machine path layer ADR-0004 defers, and relocating `local_path` there is the documented follow-up. Do not paper over it here.
- **Ambiguous cwd→project** (same clone dir linked to two projects): resolved to `None` rather than a guess (Q3) — safe for the correlation floor but means such a session binds by cwd only / may be dropped by plan 2. Acceptable and logged.
