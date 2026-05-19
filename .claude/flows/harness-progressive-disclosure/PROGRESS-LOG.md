<!-- Generated from execution-record.toml. Do not edit by hand. -->

# harness-progressive-disclosure — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E3 | create-flow-contract-flow-context-skill | 2026-05-19 | `b96e3c5` | 1 file |
| E4 | create-flow-contract-ledger-schema-skill | 2026-05-19 | `b96e3c5` | 1 file |
| E5 | create-flow-contract-vet-research-skill | 2026-05-19 | `b96e3c5` | 1 file |
| E6 | create-flow-contract-ledger-disposition-sweep-skill | 2026-05-19 | `b96e3c5` | 1 file |
| E7 | add-tomlctl-flow-envelope-build-subcommand | 2026-05-19 | `5f5ed73` | 8 files |
| E9 | rewrite-review-as-100-loc-skeleton | 2026-05-19 | `561e4ca` | 1 file |
| E10 | drop-review-from-shared-blocks-manifest | 2026-05-19 | `561e4ca` | 1 file |
| E15 | end-to-end-pilot-validation | 2026-05-19 | `f7c230f` | 0 files |
| E16 | document-pilot-lessons | 2026-05-19 | `03a7def` | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | Used 2-field skill frontmatter (name + description) instead of plan's 5-field design | 2026-05-19 | — | On-disk truth — existing skills (claude/skills/tomlctl, claude/skills/test-author) use only name + description; when_to_use content merged into description string | — |
| E8 | Used nested FlowOp::Envelope{op:EnvelopeOp::Build} instead of flat FlowOp::EnvelopeBuild | 2026-05-19 | `5f5ed73` | clap-derive renders enum variants as single kebab tokens (envelope-build); 3-word CLI spelling tomlctl flow envelope build requires nested op pattern matching existing FlowOp::Active{op:ActiveOp} | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-19 | 17 entries: status-transition × 2, deviation × 2, task-completion × 9, verification × 4 | `03a7def`, `561e4ca`, `5f5ed73`, `b96e3c5`, `f7c230f` |
