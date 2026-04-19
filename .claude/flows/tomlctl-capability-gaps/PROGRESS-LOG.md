<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl-capability-gaps — Progress Log

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E1 | items-next-id-infer-from-file | 2026-04-18 | `f9b43db` | 3 files |
| E3 | items-add-dedupe-by | 2026-04-18 | `6457f9f` | 5 files |
| E5 | dedup-id-auto-populate | 2026-04-18 | `8f171f9` | 5 files |
| E6 | dry-run-compute-apply-split | 2026-04-18 | `2378a01` | 4 files |
| E7 | items-backfill-dedup-id | 2026-04-18 | `923f420` | 3 files |
| E9 | error-format-json | 2026-04-18 | `9900443` | 8 files |
| E11 | strict-read | 2026-04-18 | `ebbfb12` | 3 files |
| E12 | count-distinct-shape | 2026-04-18 | `66436fd` | 3 files |
| E13 | lines-ndjson-pluck | 2026-04-18 | `64cb17c` | 3 files |
| E14 | raw-emitter | 2026-04-18 | `77467a8` | 3 files |
| E15 | capabilities-version-bump | 2026-04-18 | `c24dd83` | 3 files |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | Added walk_json_path in convert.rs and mutate_doc_conditional in io.rs rather than extracting from --where | 2026-04-18 | `6457f9f` | --where predicates use flat tbl.get(key) lookups today (no nested path support at all); no walker existed to extract. walk_json_path is new, documented inline. mutate_doc_conditional added so dedupe-skip keeps ledger mtime stable. | — |
| E4 | Tier-B fingerprint field order is file\|summary\|severity\|category\|symbol, not file\|symbol\|summary\|severity\|category as plan prose stated | 2026-04-18 | `8f171f9` | Actual inline tier-B code uses file\|summary\|severity\|category\|symbol; byte-identity with pre-refactor output is the stronger constraint. Kept actual order, documented in FINGERPRINTED_FIELDS rustdoc and README. | — |
| E8 | anyhow chain().find_map(downcast_ref) doesn't work; used Error::downcast_ref directly. Also moved message inside TaggedError to preserve byte-identical text mode. | 2026-04-18 | `9900443` | Trait-object downcast doesn't see anyhow's internal context wrappers — chain().find_map returns None. anyhow's inherent Error::downcast_ref method understands its own context. Separately, .context(TaggedError) inserts a ': ' separator that breaks byte-identical text mode; putting the message inside TaggedError.message and emitting it verbatim via Display avoids the artifact. | — |
| E10 | Plan assumed items list <missing> returns [] today; T8's read_toml tagging already errors kind=not_found. Only items next-id has a genuine silent-default fast path. | 2026-04-18 | `ebbfb12` | Empirical check showed T8's tagging in read_toml makes every read subcommand except items next-id error on missing file already. Rewrote test (a) to pin next-id's fast path and added a benign-no-op sweep across other read arms. README wording reflects actual behaviour. | — |

## Deferrals

_None._

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-04-18 | 20 entries: task-completion × 11, deviation × 4, verification × 4, status-transition × 1 | `f9b43db`, `6457f9f`, `8f171f9`, `2378a01`, `923f420`, `9900443`, `ebbfb12`, `66436fd`, `64cb17c`, `77467a8`, `c24dd83` |
