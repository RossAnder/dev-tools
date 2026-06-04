# Plan: Refactor large lumina source files into cohesive submodules

**Plan path**: docs/plans/joyful-singing-crane.md
**Created**: 2026-06-03
**Status**: draft

## Context

Several `lumina/src/**` files have grown far past comfortable navigation size — `repo.rs` is **13,090 lines** and `mcp.rs` is **4,240**, with a tail of 800–1,600 line files. Large single files slow human review and degrade AI-agent navigation/editing precision. This plan is a **pure structural refactor**: split the largest files into cohesively-grouped submodule directories, preserving every behaviour, public API path, and test. No new features, no schema changes, no logic edits — only moves plus the mechanical visibility/re-export glue a split requires.

The `http/` directory already demonstrates the target pattern: a `mod.rs` that mounts per-family submodules while keeping one stable public symbol (`http::router()`).

<!-- The sections below are progress checkpoints written during planning. -->

## Exploration Notes

### repo.rs (13,090 lines; prod ~1–7735, `#[cfg(test)] mod tests` 8376–13090 ≈4,714)
Production code is already sectioned by `// ====` / `// ----` banner comments into ~18 domain clusters. Cluster → representative line ranges (from Explore agent):

- **shared/decode/validate** (header, 32–360, 461): `work_item_from_row` (151), `decode_attributes` (186), `enum_to_str` (203), `validate_entry_kind` (212), `normalise_object` (232), `validate_attributes_for_kind` (248), `validate_hierarchy_edge` (461), `parse_github_slug` (362, pub)
- **reads/hierarchy**: `list_work_items` (493), `get_work_item_detail` (533) + nested hydration `list_research_notes` (837), `list_open_questions` (874), `list_acceptance_criteria` (961), `list_activity` (981), `list_findings` (1067)
- **findings query/agg (B20)**: `QueryFindingsResult` (1129, pub enum), `query_findings` (1220), `get_story_finding_queue` (1296)
- **work-items CRUD** (1305–3104, the largest cluster ~1,800 lines): `create_work_item*` (1320/1335/1387/1432), `CreateOpts` (1381), `NewWorkItemSpec` (1589), `create_work_items` (1634), gates `enforce_closure_gate` (1735)/`enforce_epic_done_gate` (1810), `update_work_item_status` (1871), setters `set_relevance/shape/epic_plan/focus_plan/effort/complexity/closure_gate` (1914–2164), acceptance-criteria CRUD (2203–2406), `work_item_kind` (2407), `update_work_item` (2423), `append_activity` (2503), `set_work_item_attributes` (2598), `reorder_work_item` (2664), context-blocks (2691–2767), `delete_work_item` (3044)
- **findings CRUD + batch**: `update_finding` (2770), `FindingTriageUpdate` (2832), `batch_update_findings` (2865), `supersede_finding` (2957), `resolve_finding` (3000), `NewFinding` (3105), `create_finding(_tx)` (3157/3235), `finding_dedup_hash` (3300), `add_findings` (3392)
- **runs/sprints/triage (B23)**: `create_run` (3527), `create_sprint` (3601), `add_tasks_to_sprint` (3634), `record_finding_decision` (3765)
- **research notes**: 4073–4252; **open questions**: 4265–4612; **repo links**: `find_project_ancestor` (4635), `is_unique_violation` (4697), `add/list/remove/set_primary/set_finding_repo` (4725–5064)
- **risks** 5078–5395; **rejected alternatives** 5396–5697; **task dependencies** 5698–5904
- **task kind/tier/batches**: `set_task_kind` (5906), `compute_tier` (5965, pub), `compute_task_batches` (6056), `get_task_dispatch_plan` (6235), `set_task_tier` (6338)
- **team-execution work-queue** (6372–7269): `claim_next_task` (6462), `release_task` (6802), `renew_lease` (6855), `CompleteTaskResult` (6934), `complete_task` (6961)
- **sprint quiescence/readiness**: `get_sprint_quiescence` (7270), `list_open_questions_for_sprint` (7362), `get_story_readiness` (7477)
- **event infra (shared)**: `record_event` (7636), `record_inert_event` (7682) — called by EVERY mutator cluster
- **`pub mod pty`** (7707–8374): already a nested module — 14 pub fns + `Pty*` Row structs + its own tests

**Shared private helpers used by ≥2 clusters** (must live in a shared inner module, widened to `pub(crate)`): `record_event`, `record_inert_event`, `work_item_from_row`, `decode_attributes`, `enum_to_str`, `normalise_object`, `validate_attributes_for_kind`, `validate_entry_kind`, `validate_hierarchy_edge`, `work_item_kind`, `is_unique_violation`, `create_work_item_full_tx`, `enforce_closure_gate`, `enforce_epic_done_gate`, `find_project_ancestor`, `WorkItemRow` + nested-detail `list_*` readers.

**Test module**: ONE `mod tests` block; shared fixtures `seed_chain_to_story`/`seed_chain_to_focus`/`count_work_items`/`count_events`/`count_events_for`/`count_activity`/`count_criteria`/`item_status`/`count_events_of_type`. Tests call private helpers (e.g. `finding_dedup_hash`) and use raw `sqlx::query*` assertions. Co-location works because a child `mod tests` sees its parent file's privates via `use super::*`; cross-cluster shared helpers must be `pub(crate)`.

**Doc staleness**: the `//!` header (lines 25–30) still references `query!`/`query_as!` macros and the `.sqlx/` offline cache — both removed in Part A. Fix the doc text during the split.

### mcp.rs (4,240 lines; params 152–1370, tool impl 1397–3189, helper impl 3191–3222, handler 3224–3234, service 3253–3281, tests 3283–4240)
- **~1,230 lines of `*Params` structs** + small enums (`FileRef`, `TaskActivityType` + its `impl`), groupable by the same domain families as repo.
- **ONE `#[tool_router] impl LuminaTools`** holds all **73** `#[tool]` methods. `LuminaTools` (1385) carries `tool_router: ToolRouter<Self>`; constructor `with_state` (1424) sets `tool_router: Self::tool_router()`.
- **Shared free helpers**: `app_error_to_mcp` (92; 87 calls), `json_result` (122), `structured_result` (130; 63 calls), `enum_to_str` (140). Plus method `build_subtree` (in 2nd impl) and `pool()`.
- **`ServerHandler` impl** decorated `#[tool_handler(router = self.tool_router)]` (3224). **service fns** `service`/`service_with_state` (pub) return `StreamableHttpService<…>`; `app.rs` mounts `service_with_state` at `/mcp`.
- **Count-invariant test** `create_tool_writes_rows_and_lists_domain_tools` (mcp.rs:3338) asserts `tools.tool_router.list_all().len() == 73`; `tool_annotations_match_the_spec` checks hints on all 73.
- **External `mcp::` surface**: only `service`, `service_with_state`, and `VerificationCommands` (imported by `http/structured_patches.rs`). All `*Params` are otherwise intra-module.

### domain.rs (1,591; prod 1–1425, tests 1426+) — pure data types
~15 structs + ~30 enums + request/response DTOs + aggregate read-models (`WorkItemDetail`, `StoryReadiness`, `ClaimedTask`, `SprintQuiescence`, `BatchEntry`, `NextAction`). Natural groups: enums / work-item types / findings+criteria+research / planning+execution aggregates.

### Consumers, build system, borderline files
- `lib.rs` declares `pub mod repo; pub mod mcp; pub mod domain;` — converting any to `X/mod.rs` needs **no lib.rs edit** (Rust resolves either layout).
- External callers reference `repo::X` (~41 symbols), `mcp::{service*, VerificationCommands}`, `domain::X` (~10 symbols). A `pub use submodule::*;` in each new `mod.rs` keeps every call site compiling unchanged.
- Integration tests in `lumina/tests/**` (e2e, concurrency, claim_concurrency, bulk_e2e, pty_e2e, auq_e2e, migration_000{3,4,10,11,13}, smoke, showcase) use ONLY the public crate API — no internal-path references — so they are insensitive to the split.
- **Borderline-file verdict**: SPLIT `domain.rs`; **LEAVE** `db.rs`, `export.rs`, `import.rs`, `http/pty_sessions.rs`, `http/structured_patches.rs`, `http/work_items.rs`, `pty/jsonl_tail.rs`, `pty/pty_transport.rs` (cohesive single-purpose and/or ~⅔ test code — low payoff, and `pty_transport.rs` is security-critical and easier to review whole).

## Research Notes

### rmcp 1.7.0 — splitting `#[tool]` methods across impl blocks (DECISIVE)
- **Source**: `~/.cargo/registry/.../rmcp-macros-1.7.0/src/lib.rs:44–127` (macro doc) and `rmcp-1.7.0/src/handler/server/router/tool.rs`. Cargo.lock pins `rmcp 1.7.0`.
- **Finding (high confidence — read from the pinned crate source)**: `#[tool_router]` accepts `router = <Ident>` (names the generated fn; default `tool_router`) and `vis = <Visibility>` (default empty). `ToolRouter<S>` implements `merge(&mut self, other)` (tool.rs:419), `std::ops::Add` (`+`, 578), and `std::ops::AddAssign` (`+=`, 590). The macro doc gives the exact multi-block recipe:
  ```rust
  #[tool_router(router = tool_router_a, vis = "pub")] impl Svc { /* tools */ }
  #[tool_router(router = tool_router_b, vis = "pub")] impl Svc { /* tools */ }
  // constructor: tool_router: self::tool_router_a() + self::tool_router_b(),
  ```
- **Impact on plan**: the `mcp.rs` tool impl **can** be decomposed into per-family `#[tool_router(router = tool_router_<fam>, vis = "pub(crate)")] impl LuminaTools` blocks across submodule files, summed with `+` in `with_state`. `#[tool_handler(router = self.tool_router)]` is unchanged (reads the one combined field). The 73-count test stays green because `list_all()` runs over the combined router.

### Verification commands (from lumina/CLAUDE.md, root CLAUDE.md, nextest.toml)
- Build: `cargo build --manifest-path lumina/Cargo.toml`
- Test: `cargo nextest run --manifest-path lumina/Cargo.toml` (includes `create_tool_writes_rows_and_lists_domain_tools` ⇒ 73, e2e, concurrency)
- Lint: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- Macro-eradication gate: `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` must be `0`
- (Audit `cargo audit --file lumina/Cargo.lock` — cadence check, not gated by this refactor.)

## Scope

**In scope** — split into submodule directories (one `X/mod.rs` re-export shell + cohesive siblings, tests co-located):
- `lumina/src/repo.rs` → `repo/`
- `lumina/src/mcp.rs` → `mcp/`
- `lumina/src/domain.rs` → `domain/`
- `lumina/src/db.rs` → `db/`
- `lumina/src/import.rs` → `import/`
- `lumina/src/pty/jsonl_tail.rs` → `pty/jsonl_tail/`
- `lumina/src/pty/pty_transport.rs` → `pty/pty_transport/`
- `lumina/src/http/pty_sessions.rs` → `http/pty_sessions/`

**Out of scope** (decided — see User Decisions): `export.rs` (356 prod), `http/structured_patches.rs` (382 prod), `http/work_items.rs` (285 prod). Their *production* code is already inside the ~200–600 target; the files are long only because of co-located tests. Splitting prod that small would create sub-150-line fragments, contradicting the "don't split so finely cohesion suffers" rule. No behaviour changes anywhere; no consumer edits (all paths preserved by re-export); no schema/migration changes.

**Affected areas**: `lumina/src/repo.rs`, `lumina/src/mcp.rs`, `lumina/src/domain.rs`, `lumina/src/db.rs`, `lumina/src/import.rs`, `lumina/src/pty/`, `lumina/src/http/pty_sessions.rs`
**Estimated file count**: ~40 new files created; 8 originals converted to `mod.rs`; 0 consumer files edited.

## User Decisions

- **Q (scope)** → *"repo + mcp + domain + borderline files."* Applied with judgment: split the 8 files above; leave `export.rs` / `http/structured_patches.rs` / `http/work_items.rs` (prod already in-range, length is test-driven). _Prompting finding: borderline-file assessment — prod-vs-test split per file._ The user may override the three exclusions if they want symmetry.
- **Q (mcp tools)** → *"Split tools by family too."* Use rmcp 1.7.0's documented multi-router pattern: per-family `#[tool_router(router = tool_router_<fam>, vis = "pub(crate)")] impl LuminaTools`, summed with `+` in the constructor. _Prompting finding: rmcp-macros-1.7.0/src/lib.rs:95–121 + ToolRouter `Add` impl._
- **Q (test placement)** → *"Co-locate per submodule."* Each submodule carries its own `#[cfg(test)] mod tests` (`use super::*` reaches its file's privates); shared fixtures move to a `#[cfg(test)] pub(crate) mod test_support`. _Prompting finding: tests call private helpers (`finding_dedup_hash`) + shared fixtures (`seed_chain_to_story`)._

## Approach

**One uniform mechanical pattern per module** (the `http/` directory is the precedent):

1. **Convert** `X.rs` → `X/mod.rs` via `git mv` (a file and its dir cannot coexist; the rename leaves the tree compiling because Rust resolves `pub mod X;` to either layout — `lib.rs`/parent `mod.rs` need NO edit).
2. **Carve** cohesive clusters out of `X/mod.rs` into sibling files. Each sibling starts with the minimal `use super::*;` / `use crate::…;` header it needs, holds its cluster's items **and its co-located tests**, and `X/mod.rs` gains `mod <sib>;` + `pub use <sib>::*;`.
3. **Preserve the public surface**: every externally-referenced `repo::X` / `mcp::X` / `domain::X` symbol keeps its `pub` and is re-exported by `pub use <sib>::*;` in `mod.rs`, so all (un-edited) call sites and integration tests compile unchanged.
4. **Shared substrate**: helpers used by ≥2 clusters move to a shared inner module (`repo/shared.rs` + `repo/events.rs`; `mcp` keeps `app_error_to_mcp`/`json_result`/`structured_result`/`enum_to_str` in `mcp/mod.rs`). Private helpers reached by a sibling or a sibling's tests widen from private → `pub(crate)` (a visibility-only change, behaviour-preserving — explicitly permitted).
5. **Co-located tests**: per-submodule `#[cfg(test)] mod tests`; shared fixtures in a `pub(crate)` `test_support` module.

**mcp tool-family split** uses the rmcp recipe verbatim:
```rust
// mcp/findings.rs
#[tool_router(router = tool_router_findings, vis = "pub(crate)")]
impl LuminaTools { /* #[tool] fns + their *Params + tests */ }
// mcp/mod.rs constructor:
tool_router: Self::tool_router_reads() + Self::tool_router_work_items() + … + Self::tool_router_team_execution(),
```
`#[tool_handler(router = self.tool_router)]` is unchanged. During the carve chain, `mcp/mod.rs` keeps a shrinking default `#[tool_router] impl` holding the not-yet-carved tools, and the constructor sums `Self::tool_router() + <carved families>` so `list_all().len() == 73` stays green at **every** step; the final carve empties and removes the default block.

**Sequencing — why intra-module is a chain but inter-module is parallel:** carving repeatedly edits the one `X/mod.rs` (remove code + add `mod`/`pub use`/router term), which is a same-file mutation → the carves for a module must run **sequentially**. Different modules touch disjoint files and each preserves its public surface, so `repo`, `mcp`, and the six single-task modules are **mutually independent** and run **concurrently**. Every task is designed to leave the whole crate compiling + `nextest` green (so `/implement` can verify after each).

**Recommended file layout** (implementer keeps each prod body ~200–600 lines; fold a trivially-small cluster, e.g. context-blocks ~80 lines, into an adjacent sibling rather than make a sub-150-line file):
- `repo/`: `mod.rs`, `shared.rs`, `events.rs`, `test_support.rs`, `reads.rs`, `findings_query.rs`, `work_items.rs`, `work_items_meta.rs` (setters/attrs/activity/context), `acceptance_criteria.rs` (AC CRUD), `findings.rs`, `runs_sprints.rs`, `research_notes.rs`, `open_questions.rs`, `repo_links.rs`, `risks.rs`, `rejected_alternatives.rs`, `task_dependencies.rs`, `task_graph.rs`, `team_execution.rs`, `readiness.rs`, `pty.rs`
- `mcp/`: `mod.rs` (struct/constructors/`build_subtree`/`ServerHandler`/`service*`/shared free helpers/router-sum), `reads.rs`, `work_items.rs`, `planning.rs`, `findings.rs`, `runs_sprints.rs`, `repo_links.rs`, `risks_alts.rs`, `task_graph.rs`, `team_execution.rs`
- `domain/`: `mod.rs`, `enums.rs`, `work_items.rs`, `findings.rs`, `planning.rs`
- `db/`: `mod.rs` (init/connect/begin_write), `erased.rs` (`Backend`/`Args`/`AnyPool`/`AnyRow` + impls), `client.rs` (`DbClient`/`DbTx`/`Scalar` traits + `tx_query_*`/`scalar_*`/`tx_scalar_*` + `args!`)
- `import/`: `mod.rs` (pipeline), `schema.rs` (TOML DTOs)
- `pty/jsonl_tail/`: `mod.rs` (tail/bind/drain runtime), `parse.rs` (record types + `parse_line` + mappers)
- `pty/pty_transport/`: `mod.rs` (`Transport` impl + spawn — security-critical, kept whole), `config.rs` (`no_auq_system_prompt`/mcp-url/mcp-config/`translate_keystroke_dsl`)
- `http/pty_sessions/`: `mod.rs` (router + CRUD handlers), `ws.rs` (WebSocket), `ask.rs` (`answer_question` + keystrokes)

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings
```
Additional gates (run after each task and at the end):
- Tool-count invariant: `create_tool_writes_rows_and_lists_domain_tools` (inside `nextest`) asserts exactly **73**.
- Macro-eradication: `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` must print `0`.

## Tasks

> Pattern is identical across tasks: `git mv` (foundation only) or carve clusters per the Approach; preserve every `pub` symbol via `pub use`; co-locate tests; widen shared helpers to `pub(crate)` as needed. **Acceptance for every task** = `cargo build`, `cargo nextest run`, `cargo clippy --all-targets` all green + tool-count 73 + macro-gate 0 (commands above), PLUS a per-task structural check that the carve landed (each new sibling is non-trivial and the carved symbols are gone from `mod.rs`, e.g. `rg -c '<representative fn>' <module>/mod.rs` -> 0). Without it a no-op carve that leaves bodies in `mod.rs` passes every check above.

### Phase A — repo/ chain (sequential)

#### R0: Establish repo/ shell + shared substrate
- **Files**: `lumina/src/repo.rs`→`lumina/src/repo/mod.rs`, `repo/shared.rs`, `repo/events.rs`, `repo/test_support.rs`
- **Depends on**: none
- **Action**: `git mv repo.rs repo/mod.rs`. Move shared helpers (`work_item_from_row`, `WorkItemRow`, `decode_attributes`, `enum_to_str`, `normalise_object`, `validate_attributes_for_kind`, `validate_entry_kind`, `validate_hierarchy_edge`, `work_item_kind`, `is_unique_violation`, `check_plan_field_len`, `validate_plan_field_constraints`, `parse_github_slug`, `create_work_item_full_tx`, `enforce_closure_gate`, `enforce_epic_done_gate`, `find_project_ancestor`, nested-detail `list_*` readers) into `shared.rs`; move `record_event`/`record_inert_event` into `events.rs`; move test fixtures (`seed_chain_to_story`, `seed_chain_to_focus`, `count_*`, `item_status`) into `#[cfg(test)] pub(crate) mod test_support` (`test_support.rs`). Widen moved helpers to `pub(crate)` (keep `parse_github_slug` `pub`, re-export it). Add `mod shared; mod events; #[cfg(test)] mod test_support;` + `use` glue in `mod.rs`. **Fix the stale `//!` doc** (remove `query!`/`.sqlx/` references; state queries are runtime via the `DbClient`/`DbTx` seam).
- **Detail**: mod.rs still holds every cluster; they now reference `shared::*`/`events::*`. Preserve the single-mutation-path invariant text in the doc.
- **Acceptance**: build/nextest/clippy green; 73; macro-gate 0.

#### R1: Carve reads + findings-query
- **Files**: `repo/mod.rs`, `repo/reads.rs`, `repo/findings_query.rs`
- **Depends on**: R0
- **Action**: move `list_work_items`/`get_work_item_detail` (+ nested hydration call sites) into `reads.rs`; `QueryFindingsResult`/`query_findings`/`get_story_finding_queue` into `findings_query.rs`. Re-export both via `pub use`.

#### R2: Carve work-items CRUD + meta/lifecycle
- **Files**: `repo/mod.rs`, `repo/work_items.rs`, `repo/work_items_meta.rs`, `repo/acceptance_criteria.rs` (context-blocks at repo.rs:2691-2767 is ~80 lines and `append_activity` ~95 lines — both <150, so fold both into `work_items_meta.rs`)
- **Depends on**: R1
- **Action**: `work_items.rs` = `create_work_item*`/`CreateOpts`/`NewWorkItemSpec`/`create_work_items`/`update_work_item`/`update_work_item_status`/gates/`delete_work_item`; `work_items_meta.rs` = setters (`set_relevance`/`set_shape`/`set_effort`/`set_complexity`/`set_closure_gate`/`set_epic_plan`/`set_focus_plan`/`set_work_item_attributes`/`reorder_work_item`) + activity + context-blocks; `acceptance_criteria.rs` = AC CRUD. **L effort** (largest cluster ~1,800 prod + tests).

#### R3: Carve findings + runs/sprints + research-notes
- **Files**: `repo/mod.rs`, `repo/findings.rs`, `repo/runs_sprints.rs`, `repo/research_notes.rs`
- **Depends on**: R2
- **Action**: findings CRUD/batch/`finding_dedup_hash`/`NewFinding`/`FindingTriageUpdate` → `findings.rs`; runs/sprints/`record_finding_decision` → `runs_sprints.rs`; research notes → `research_notes.rs`.

#### R4: Carve open-questions + repo-links + risks + alternatives
- **Files**: `repo/mod.rs`, `repo/open_questions.rs`, `repo/repo_links.rs`, `repo/risks.rs`, `repo/rejected_alternatives.rs`
- **Depends on**: R3

#### R5: Carve task-graph + team-execution + readiness + pty
- **Files**: `repo/mod.rs`, `repo/task_dependencies.rs`, `repo/task_graph.rs`, `repo/team_execution.rs`, `repo/readiness.rs`, `repo/pty.rs`
- **Depends on**: R4
- **Action**: move the existing `pub mod pty { … }` block verbatim into `repo/pty.rs` as `pub mod pty;` (preserves `repo::pty::*`). **In `repo/mod.rs` declare `pub mod pty;` — NOT `pub use pty::*`** (27 nested `repo::pty::FOO` call sites across `pty/{queue,emit,spawn,supervisor}.rs` + `http/pty_sessions.rs` need the module path; this is the one deviation from the uniform `pub use <sib>::*` step). **L effort**. After this, `mod.rs` is the re-export shell + `shared`/`events`/`test_support` only.

### Phase B — mcp/ chain (sequential, parallel to Phase A)

#### M0: Convert mcp.rs → mcp/mod.rs
- **Files**: `lumina/src/mcp.rs`→`lumina/src/mcp/mod.rs`
- **Depends on**: none
- **Action**: `git mv mcp.rs mcp/mod.rs` only (no content change). **S effort.** Verify green/73.

#### M1: Carve reads + work-items tool families
- **Files**: `mcp/mod.rs`, `mcp/reads.rs`, `mcp/work_items.rs`
- **Depends on**: M0
- **Action**: move read tools (+ their `*Params`) into `reads.rs` with `#[tool_router(router = tool_router_reads, vis = "pub(crate)")]`; work-item CRUD + story-plan/task-spec/context/activity tools into `work_items.rs`. Move shared test fixtures (`seed_chain_to_story` at mcp.rs:3290 and any cross-family helpers from the single `mod tests` block at mcp.rs:3284) into a `#[cfg(test)] pub(crate) mod test_support` (`mcp/test_support.rs`); only family-local tests move with their tools. Keep the shrinking default `#[tool_router] impl` in `mod.rs`; set constructor `tool_router: Self::tool_router() + Self::tool_router_reads() + Self::tool_router_work_items()`. Keep `app_error_to_mcp`/`structured_result`/`json_result`/`enum_to_str`/`build_subtree`/`new`/`with_state`/`pool`/`ServerHandler`/`service*` in `mod.rs`.

#### M2: Carve planning + findings + runs/sprints tool families
- **Files**: `mcp/mod.rs`, `mcp/planning.rs`, `mcp/findings.rs`, `mcp/runs_sprints.rs`
- **Depends on**: M1
- **Action**: planning = relevance/effort/complexity/closure-gate/shape/epic-plan/focus-plan + acceptance-criteria + research-notes + open-questions tools; findings = finding CRUD/batch/query/finding-queue/`set_finding_repo`; runs_sprints = runs/sprints/triage + batch `create_work_items`/`add_findings`. Extend constructor sum. **L effort.**

#### M3: Carve repo-links + risks/alts + task-graph + team-execution; finalize
- **Files**: `mcp/mod.rs`, `mcp/repo_links.rs`, `mcp/risks_alts.rs`, `mcp/task_graph.rs`, `mcp/team_execution.rs`
- **Depends on**: M2
- **Action**: carve the remaining families; remove the now-empty default `#[tool_router] impl` from `mod.rs` and drop the `Self::tool_router() +` term so the constructor sums only the family routers. **L effort.** Confirm `list_all().len() == 73`.

### Phase C — single-task modules (each one task; all independent; parallel to A, B, and each other)

#### D1: Split domain.rs → domain/
- **Files**: `domain.rs`→`domain/mod.rs`, `domain/enums.rs`, `domain/work_items.rs`, `domain/findings.rs`, `domain/planning.rs`
- **Depends on**: none
- **Action**: `git mv` then carve: enums → `enums.rs`; `WorkItem`/activity/detail/requests → `work_items.rs`; `Finding`/`AcceptanceCriterion`/`ResearchNote`/`Risk`/`RejectedAlternative`/`OpenQuestion`/etc. → `findings.rs`; read-models (`StoryReadiness`/`ClaimedTask`/`SprintQuiescence`/`BatchEntry`/`NextAction`/`NewRun`/`NewSprint`/…) → `planning.rs`. `pub use *` all from `mod.rs`.

#### DB1: Split db.rs → db/
- **Files**: `db.rs`→`db/mod.rs`, `db/erased.rs`, `db/client.rs`
- **Depends on**: none
- **Action**: `mod.rs` = `init`/`connect_in_memory`/`begin_write`/`is_in_memory` (**fix the stale `//!` header** — db.rs:8-11 still claims `query!`/`query_as!` need the `.sqlx` cache, false post-Part-A; state queries are runtime via the `DbClient`/`DbTx` seam); `erased.rs` = `Backend`/`Args`/`AnyPool`/`AnyRow` + their impls (incl. `DbClient for AnyPool`/`SqlitePool`/`Arc<AnyPool>`); `client.rs` = `DbClient`/`DbTx`/`Scalar` traits + `decode_row`/`tx_query_one`/`tx_query_opt`/`tx_query_all` + the `scalar_one`/`scalar_opt`/`scalar_all`/`tx_scalar_one`/`tx_scalar_opt` family (db.rs:746-807, ~46 call sites in repo.rs) + the `#[macro_export] args!` macro (db.rs:214) reachable as `crate::args!`. Preserve `db::X` paths.

#### J1: Split pty/jsonl_tail.rs → pty/jsonl_tail/
- **Files**: `pty/jsonl_tail.rs`→`pty/jsonl_tail/mod.rs`, `pty/jsonl_tail/parse.rs`
- **Depends on**: none
- **Action**: `parse.rs` = record types (`JsonlRecord`/`UserMessage`/`AssistantMessage`/blocks/`JsonlRecordParsed`) + `parse_line` + `map_record_to_typed` + `sanitise_cwd`/`resolve_projects_root`; `mod.rs` = `bind_jsonl_path`/`tail`/`drain_and_broadcast` runtime.

#### PS1: Split http/pty_sessions.rs → http/pty_sessions/
- **Files**: `http/pty_sessions.rs`→`http/pty_sessions/mod.rs`, `http/pty_sessions/ws.rs`, `http/pty_sessions/ask.rs`
- **Depends on**: none
- **Action**: `mod.rs` = `router()` + CRUD handlers (list/get/spawn/messages/queue/input/batch/delete/patch); `ws.rs` = WebSocket upgrade + frame stream; `ask.rs` = `answer_question` + keystrokes. `router()` stays the single public symbol.

#### PT1: Split pty/pty_transport.rs → pty/pty_transport/
- **Files**: `pty/pty_transport.rs`→`pty/pty_transport/mod.rs`, `pty/pty_transport/config.rs`
- **Depends on**: none
- **Action**: `config.rs` = `no_auq_system_prompt`/`lumina_ask_mcp_url`/`ask_mcp_config_json`/`translate_keystroke_dsl`; `mod.rs` = `PtyTransport` + `Transport` impl (keep the security-critical `bypassPermissions`/`skipDangerousModePermissionPrompt` spawn line and its CLAUDE.md-referenced comment intact). **S/M effort.**

#### I1: Split import.rs → import/
- **Files**: `import.rs`→`import/mod.rs`, `import/schema.rs`
- **Depends on**: none
- **Action**: `schema.rs` = TOML DTOs (`ContextDoc`/`Artifacts`/`ExecutionRecord`/`ExecItem`/`Ledger`/`LedgerItem`); `mod.rs` = `import_flow`/`ensure_scaffold`/`resolve_artifact`/`ImportSummary`. **S/M effort.**

## Dependency Graph

```
Independent roots (run concurrently, ≤4 agents/batch): R0, M0, D1, DB1, J1, PS1, PT1, I1
repo chain:  R0 → R1 → R2 → R3 → R4 → R5
mcp chain:   M0 → M1 → M2 → M3
domain/db/jsonl_tail/pty_sessions/pty_transport/import: single tasks (D1, DB1, J1, PS1, PT1, I1), no successors
```
No two concurrent tasks share a file (each module owns its own `mod.rs` and siblings; no consumer or parent `mod.rs`/`lib.rs` edits). `/implement` batches the roots ≤4 at a time, then advances each chain as its predecessor lands.

## Verification

- [ ] `cargo build --manifest-path lumina/Cargo.toml` — clean.
- [ ] `cargo nextest run --manifest-path lumina/Cargo.toml` — all green, incl. `create_tool_writes_rows_and_lists_domain_tools` (73), `tool_annotations_match_the_spec`, e2e, concurrency, claim_concurrency, bulk_e2e, migration_000{3,4,10,11,13}.
- [ ] `cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings` — no new warnings.
- [ ] `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` → `0`.
- [ ] `git diff --stat` review: only moves + `mod`/`pub use`/visibility glue; **no consumer file (`app.rs`, `lib.rs`, `http/*` besides pty_sessions, other `pty/*`) changed**; no logic diffs (spot-check a few moved fns are byte-identical bodies).
- [ ] `cargo build` of `tomlctl` is unaffected (separate crate).

## Risks

- **Same-file carve races** → mitigated by the per-module sequential chains in the Dependency Graph; concurrent tasks are cross-module only.
- **mcp router-sum drift / 73-count regression** → each mcp carve keeps the shrinking default block + grows the constructor sum so `list_all()` stays 73 at every step; M3 verifies after removing the empty block. The `create_tool_writes_rows_and_lists_domain_tools` test (mcp.rs:3338, NOT `tool_surface_is_sound`) is the guard — but `ToolRouter::merge` is HashMap-keyed by tool name, so a duplicate-name collision overwrites silently and KEEPS the count at 73. Add a name-uniqueness assertion (collect `list_all()` names into a set; assert the set has 73 entries) so a count-preserving overwrite is caught.
- **Test private-access breakage after co-location** → a sibling's `mod tests` sees only its own file's privates (via `super`) + `pub(crate)` shared items; if a moved test reaches a *cousin* cluster's private helper, widen that helper to `pub(crate)` (visibility-only) or keep that test beside the helper. `nextest` catches misses immediately.
- **Accidental behaviour change during a move** → enforced by "moves only" discipline + the `git diff` body-identity spot-check in Verification; the full `nextest` suite (incl. the +1 work_items/+1 events invariant tests and e2e) is the backstop.
- **Doc-comment staleness carried forward** → R0 fixes the `repo` `//!` `.sqlx`/macro references, and DB1 likewise fixes the `db` `//!` (db.rs:8-11), as part of each move.
- **Large plan (~16 tasks / ~40 files)** → tasks are uniform and mechanical; **commit after each green task** (one commit per R*/M*/D*/etc. task) so rollback is `git revert <task-commit>` and the moves-only `git diff --stat` review is per-task, not one 40-file blob; recommend `/review-plan` then `/implement` (which parallelises the independent roots). If preferred, the eight modules could instead be run as per-module sub-plans, but one plan is simpler given the uniform pattern.

