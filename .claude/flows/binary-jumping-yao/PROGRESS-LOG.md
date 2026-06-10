<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Progress Log — binary-jumping-yao

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | t1-move-crate-into-server-and-create-workspace-root | 2026-06-09 | `33e079f` | workspace root + server member; SQL R100 |
| 2 | t2-carve-lumina-core-out-of-lumina-server | 2026-06-09 | `73f6e76` | core extracted; ~46-file import rewrite |
| 3 | t3-harden-the-debug-spa-path-for-cwd-independence | 2026-06-09 | `042ecb5` | assets.rs manifest-relative dev path |
| 4 | t4-add-lumina-protocol-and-lumina-companion-stub-members | 2026-06-09 | `042ecb5` | two stub members (Step-1b placeholders) |
| 5 | t6-update-claude-md-verification-commands-and-gate-paths | 2026-06-09 | `042ecb5` | workspace-aware command inventory (5 files) |
| 6 | t5-lift-profiles-dedup-deps-set-workspace-metadata | 2026-06-09 | `d144150` | profiles + [workspace.{package,dependencies}] |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E7 | Core dep set expanded beyond plan; protocol moved with jsonl_tail; two repo helpers widened to pub; tempfile is a core runtime dep | 2026-06-09 | `73f6e76` | Core also needs tokio/async-trait/notify/tracing (jsonl_tail/repo/db); protocol is leaf-clean and required by jsonl_tail/parse.rs; export.rs uses NamedTempFile; corpus_raw/now_string used cross-crate by server pty/spawn.rs. All behaviour-neutral; 475 tests green. | — |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-09 | 20 entries: status-transition × 2, verification × 11, task-completion × 6, deviation × 1 | `042ecb5`, `33e079f`, `73f6e76`, `d144150` |
