<!-- Generated from execution-record.toml. Do not edit by hand. -->

# flow-tracking-overhaul — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E3 | land-cmd-flow-cmd-json-skeleton-dep-additions | 2026-05-08 | `2983cdd` | 17 files |
| E5 | implement-tomlctl-json-get-set-unset | 2026-05-08 | `5ee8c9d` | 2 files |
| E6 | implement-tomlctl-flow-active | 2026-05-08 | `5ee8c9d` | 2 files |
| E7 | implement-tomlctl-flow-find-plans | 2026-05-08 | `5ee8c9d` | 2 files |
| E8 | implement-tomlctl-flow-stale | 2026-05-08 | `5ee8c9d` | 2 files |
| E13 | phase-a-doc-sync | 2026-05-08 | `c1bad18` | 2 files |
| E14 | implement-tomlctl-flow-init | 2026-05-08 | `58d9307` | 2 files |
| E15 | implement-tomlctl-flow-ensure-artifact | 2026-05-08 | `58d9307` | 2 files |
| E16 | implement-tomlctl-flow-list | 2026-05-08 | `58d9307` | 2 files |
| E18 | implement-tomlctl-flow-resolve | 2026-05-08 | `cf03863` | 2 files |
| E19 | implement-tomlctl-flow-doctor | 2026-05-08 | `cf03863` | 2 files |
| E20 | phase-b-doc-sync | 2026-05-08 | `b8c6f8c` | 4 files |
| E21 | author-flow-bootstrap-agent | 2026-05-08 | `6aa837a` | 1 files |
| E22 | design-flow-context-template | 2026-05-08 | `0134ce4` | 1 files |
| E23 | coordinated-shared-block-rewrite | 2026-05-08 | `8ffa35f` | 11 files |
| E30 | phase-d-smoke | 2026-05-08 | `—` | 0 files |
| E31 | plansdirectory-first-use-prompt | 2026-05-08 | `494254e` | 3 files |
| E32 | phase-e-doc-sync | 2026-05-08 | `4b29c9a` | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | Stub dispatch fns omit error_format param and use anyhow::bail! rather than tagged_err(Err | 2026-05-08 | `2983cdd` | Existing Cmd::{Items,Blocks,Integrity} dispatch arms do NOT thread error_format (it is captured in main.rs and consumed only by the top-leve | — |
| E9 | P19 implemented in two commits: positive half in T2 (json refuses .toml), symmetric negati | 2026-05-08 | `5ee8c9d` | T2's owned-files list excluded cli/dispatch.rs (which contains the TOML writer arms); T2 implemented the positive half in json.rs (refuse_to | — |
| E10 | io::resolve_target does not exist; T2 composed io::guard_write_path + io::recheck_claude_c | 2026-05-08 | `5ee8c9d` | The actual surface in tomlctl/src/io.rs is guard_write_path (pre-lock, mkdir-p + canonicalise + symlink-leaf) plus recheck_claude_containmen | — |
| E11 | T3 expanded io::mutate_doc inline in active.rs to add a missing-file branch | 2026-05-08 | `5ee8c9d` | mutate_doc calls read_toml which errors kind=not_found on a missing file, but the plan also requires `add` to bootstrap a fresh registry on  | — |
| E12 | T5 used jiff::civil::Date directly; convert::maybe_date_coerce was the wrong direction (JS | 2026-05-08 | `5ee8c9d` | convert::maybe_date_coerce coerces a JSON string into a TOML datetime — wrong direction for staleness arithmetic, which needs to compare a T | — |
| E17 | T7 ships inline active-flow upsert helper (duplicates T3's mutate_active pattern) — T3 doe | 2026-05-08 | `58d9307` | T3's only public surface is dispatch(ActiveOp) which prints to stdout (would muddy T7's single-line JSON envelope). Per plan deviation proto | — |
| E24 | Per-carrier net deletion 15-43 lines (vs plan target ≥70) | 2026-05-08 | `8ffa35f` | T14's canonical Step-0 template (~44 lines after slot substitution) plus the 24-line replacement shared block adds back nearly as much as th | — |
| E25 | test-bootstrap.md grep acceptance regex semantically incorrect | 2026-05-08 | `8ffa35f` | The new replacement string '.claude/active-flow.toml' literally contains '.claude/active-flow' followed by a period (non-word char), which I | — |
| E26 | Scope expanded to sweep tdd.md L440 + plan-new.md L547/L646 + implement.md L270 stray lega | 2026-05-08 | `8ffa35f` | Three additional sites carried the legacy 5-step / .claude/active-flow prose and would have shipped contradictions with the new shared block | — |
| E41 | Added slug validation to active.rs::add/remove/touch (R15 fix from /review) | 2026-05-08 | `—` | Path-traversal threat (R15, security/warning): slug stored verbatim in registry, joined into <root>/.claude/flows/<slug>/... downstream. Mir | — |
| E42 | Added history-loss WARNING blockquote to CLAUDE.md migration section | 2026-05-08 | `—` | Architecture warning (R20): no tomlctl flow migrate command exists yet; users with in-flight flows would silently lose execution-record/ledg | — |
| E43 | Documented last-writer-wins natural-key semantics in active.rs module docstring | 2026-05-08 | `—` | DB warning (R22): registry's natural key is bare slug; find_slug_index matches slug-only; same-slug entries clobber each other regardless of | — |
| E44 | Annotated `prompt-required` source value as reserved-never-emitted in resolve.rs docstring | 2026-05-08 | `—` | Completeness warning (R27): the value is dead documentation that misleads readers about resolver behaviour. Annotated as reserved-never-emit | — |
| E45 | Added envelope.warnings entry when both --branch and --worktree are None at step-3 | 2026-05-08 | `—` | Completeness warning (R29): carriers that don't pass branch/worktree silently skip step-3 with no envelope.warnings entry. Added the warning | — |
| E46 | Replaced contradictory literal-prefix version check with regex-based semver comparison | 2026-05-08 | `—` | Completeness warning (R30): literal-prefix check would reject 0.6.0 even though parenthetical says accept. Switched to regex `tomlctl (\d+)\ | — |
| E47 | Changed gitignore-claude check from Check{ok:false} to Check::ok (warning-only) | 2026-05-08 | `—` | Completeness warning (R31): plan spec said gitignore-claude should be warning not check failure. Carriers using flow-bootstrap on a repo whe | — |
| E48 | Added stderr eprintln when ties_broken=true on branch-match path | 2026-05-08 | `—` | Completeness warning (R32): plan called for a console note when ties existed. Added eprintln warning when ties_broken=true so operator can d | — |
| E49 | Added step 3.5 'Validate required artifacts' to flow-bootstrap.md procedure | 2026-05-08 | `—` | Completeness warning (R33): missing artifacts only surfaced via warnings; carriers like /implement that need execution_record could proceed  | — |
| E50 | Aligned load_active_entries to silent-zero on read_toml Err (matches doctor.rs) | 2026-05-08 | `—` | Completeness warning (R34): corrupt registry blocked ALL resolution including step-5 fallback. Aligned resolve.rs to silent-zero with stderr | — |
| E51 | Wrapped Step 0.5 plansDirectory prompt in SHARED-BLOCK delimiters across 3 carriers + mani | 2026-05-08 | `—` | Completeness warning (R37): pre-commit hook did not enforce parity, drift would surface only at manual diff audit. Added SHARED-BLOCK:plansd | — |
| E52 | Extended read_plans_directory to accept array-of-strings shape at canonical plansDirectory | 2026-05-08 | `—` | Completeness warning (R38): users writing arrays at canonical plansDirectory key per CLAUDE.md prose found them silently ignored. Extended r | — |
| E53 | Replaced sidecar_state Option<bool> with Option<SidecarStatus> enum carrying expected/actu | 2026-05-08 | `—` | Testability warning (R47): CLAUDE.md contract promises both expected + actual hashes on mismatch (wired in maybe_verify_integrity, not in do | — |
| E54 | Replaced bare `model: haiku` alias with fully-qualified `claude-haiku-4-5` | 2026-05-08 | `—` | Package-quality warning (R50): bare alias can silently shift to a newer generation when Anthropic rotates the alias. Replaced with claude-ha | — |
| E55 | Updated implement.md cross-reference :334 → :310 in tdd.md | 2026-05-08 | `—` | Package-quality warning (R52): :334 was stale (now Phase 2: Execute header); actual line of `Extract verification commands` is :310. Updated | — |
| E56 | Updated plan-new.md cross-reference :594-602 → :606 in tdd.md | 2026-05-08 | `—` | Package-quality warning (R53): :594-602 was stale (range inside Research Notes template); ## Verification Commands heading at L606. Updated  | — |
| E57 | Corrected envelope path to parent_envelope.resolved.plan_path; collapsed redundant context | 2026-05-08 | `—` | Package-quality warning (R58): tomlctl flow resolve --json output nests plan_path under .resolved.plan_path; prose was confused between sour | — |
| E58 | Removed stale cautionary note from tdd.md Acceptance smoke-check about implement.md argume | 2026-05-08 | `—` | Package-quality warning (R60): implement.md frontmatter has been updated to `[--flow <slug>] [plan path or task description]`, so the cautio | — |
| E59 | Consolidated 3 active-flow registry parsers into flow::schema (R17 user-authorised overrid | 2026-05-08 | `—` | DB warning (R17) reopened after user-authorised override of cluster scope-confinement. Created tomlctl/src/flow/schema.rs as single source o | — |
| E60 | Removed --cwd flag from flow resolve (R26 user-authorised override) | 2026-05-08 | `—` | Architecture warning (R26) reopened after user-authorised override of public-CLI-surface gate. Removed --cwd from cli/types.rs FlowOp::Resol | — |
| E61 | R16 doc-only addition: integrity sidecar (.sha256) scope subsection added to CLAUDE.md | 2026-05-08 | `—` | Applied via /review-apply for review-ledger item R16. CLAUDE.md is in the flow's scope globs, so the doc addition is recorded as a plan dev | — |
| E62 | Added Item::validate(&JsonValue) -> Result<(), DispositionError> + DispositionError enum t | 2026-05-08 | `—` | Review finding R19 surfaced parse-time disposition-required-field validation as a correctness improvement; agent added Item::validate as | — |
| E63 | Narrowed 11/11 flow leaf modules from pub(crate) to plain mod | 2026-05-08 | `—` | Review finding R21 surfaced over-broad visibility scope; agent re-confirmed all callers are intra-flow then narrowed all 11 leaves to pl | — |
| E64 | Unified resolver step-count documentation to 6-step across resolve.rs:1 and cli/types.rs:5 | 2026-05-08 | `—` | Review finding R23 surfaced inconsistency; agent verified resolver code emits 6 distinct source paths and unified docs to 6-step; flow- | — |
| E65 | Replaced stale P19 deferred-work comment in json.rs L18-22 with cross-reference to wired r | 2026-05-08 | `—` | Review finding R28 surfaced the comment is stale (P19 already implemented at cli/dispatch.rs:274-281 and wired at L425/L454/L502); agen | — |
| E66 | doctor.rs always emits sidecar checks even when underlying TOML missing | 2026-05-08 | `—` | Review finding R35 surfaced shape inconsistency; agent normalised to always emit sidecar checks with skip-detail recording why when TOM | — |
| E67 | Check::to_json now always emits detail key (null when None) | 2026-05-08 | `—` | Review finding R41 surfaced shape-stability issue for orchestrators iterating checks; agent normalised to always emit detail (serde_jso | — |
| E68 | Added doctor_corrupt_active_flow_toml_silent_passes_stale_check integration test | 2026-05-08 | `—` | Review finding R42 surfaced gap in integration coverage; agent added the test (existing flow_resolve.rs:628 covered the resolver path o | — |
| E69 | Added step_3_binding_tie_falls_through_to_active_latest integration test | 2026-05-08 | `—` | Review finding R44 surfaced gap; agent added the integration test pinning step-3-tie -> step-4-active-latest fallthrough via the full C | — |
| E70 | Added tie_resolution_envelope_carries_canonical_5_keys shape-pin | 2026-05-08 | `—` | Review finding R45 surfaced gap; agent added shape-pin for the 5-key tie-resolution envelope (resolved/source/ties_broken/tie_candidate | — |
| E71 | README Quick tour now demonstrates orchestrator-facing flag forms | 2026-05-08 | `—` | Review finding R46 surfaced missing orchestrator-facing flag forms; agent added separate flow active touch line, retained --with-stalen | — |
| E72 | Surfaced --dry-run-without-fix UX warning via envelope warnings[] channel | 2026-05-08 | `—` | Review finding R48 surfaced UX gap; agent chose envelope-warnings channel (spec's alternative path) since doctor is JSON-only via print | — |
| E73 | Extended envelope shape-stability tests to 4 additional resolution paths | 2026-05-08 | `—` | Review finding R49 surfaced 13-key requirement is contractual across all paths; agent added scope-glob/active-binding/active-latest/bra | — |
| E74 | Expanded inline-bypass tradeoff comment in tdd.md L392 | 2026-05-08 | `—` | Review finding R51 surfaced opaque tradeoff; agent expanded the comment to enumerate bypassed gates (version-check, doctor) and articul | — |
| E75 | Corrected three -> four command count in flow-bootstrap.md:84 hard-rules block | 2026-05-08 | `—` | Review finding R54 surfaced off-by-one count typo; agent corrected three -> four to match actual list of literals (--version, flow reso | — |
| E76 | Added envelope_version: 1 to flow-bootstrap.md frontmatter | 2026-05-08 | `—` | Review finding R55 surfaced version-tracking gap; agent added envelope_version: 1 (integer for numeric comparability) with preceding YA | — |
| E77 | Removed redundant parenthetical at tdd.md L439 | 2026-05-08 | `—` | Review finding R57 surfaced redundant content noise (Step 0 guarantee already documented in SHARED-BLOCK:flow-context block at L31-72); | — |
| E78 | Added scope-rationale comments documenting carrier-list asymmetry in shared-blocks.toml | 2026-05-08 | `—` | Review finding R59 surfaced potential reader confusion; agent confirmed asymmetry is intentional (test-bootstrap is non-flow-aware per | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-08 | 79 entries: status-transition × 3, checkpoint × 1, task-completion × 18, deviation × 47, verification × 10 | 0134ce4, 2983cdd, 494254e, 4b29c9a, 58d9307, 5ee8c9d, 6aa837a, 735cdae, 8ffa35f, b8c6f8c, c1bad18, cf03863 |
