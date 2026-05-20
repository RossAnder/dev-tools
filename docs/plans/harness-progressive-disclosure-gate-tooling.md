# Plan: Propagation Gate Tooling for Harness Progressive-Disclosure

**Plan path**: `docs/plans/harness-progressive-disclosure-gate-tooling.md`
**Flow slug**: `harness-progressive-disclosure-gate-tooling`
**Created**: 2026-05-20
**Status**: Draft

## Context

The pilot run migrated `claude/commands/review.md` from 777 → 62 LOC by externalising its four shared blocks into `claude/skills/flow-contract-*/SKILL.md` and replacing inline Step-0 envelope prose with `tomlctl flow envelope build`. The mechanism is **validated live** (2026-05-20 `/review` + `/review-apply` run). The next step is propagating the pattern to the remaining 9 carriers — but `PILOT-LESSONS.md` (§10, §13, §14) is emphatic that three pieces of **gate tooling must land before any bulk propagation begins**, because the drift window between an externalised skill body and its still-embedded carrier copies opens *precisely during* multi-PR propagation, and the lessons surfaced two runtime failure classes (flag drift, fixture/manifest drift) that no review lens can catch.

This plan delivers only that gate tooling. The 9-carrier propagation (which needs 6 new skills and natural waves) is deferred to follow-up plans opened *after* this tooling lands and a smoke-run validates — confirmed with the user (scope: "gate tooling only"; enforcement: "cargo test only", no pre-commit hook changes).

The three gate tools:
1. **Fixtures-read-manifest refactor** (§14/§5) — eliminate the second hand-maintained carrier list.
2. **verify-against-skill check** (§7/§13) — detect skill-body↔carrier-copy drift during the migration window.
3. **command-lint test** (§10) — catch carrier↔CLI flag drift (the `--flow`/`--flow-override` class) at compile time.

## Scope

- **In scope**: A `tomlctl blocks verify-skills` engine + subcommand and its drift-normalisation rule; an explicit `skill =` field per externalised `[[block]]` in the manifest; refactoring the `dispatch.rs` parity fixture to read the manifest instead of hardcoding carrier lists; a `command_lint` cargo unit test feeding markdown `tomlctl` invocations to the real clap parser; docs for the new subcommand. All gated by `cargo test` (CI) — no pre-commit hook changes.
- **Out of scope**: Creating any of the 6 new propagation skills (`execution-record-schema`, `plansdirectory-prompt`, four `apply-*`); rewriting any of the 9 carriers; touching `.githooks/`; the per-carrier live smoke-run process (§11 — a propagation-PR discipline, documented but not built here); resolving the flagged `apply-rollback-protocol` divergence (a propagation-wave concern).
- **Affected areas**: `tomlctl/src/blocks.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/Cargo.toml`, `scripts/shared-blocks.toml`, `claude/skills/tomlctl/SKILL.md`, `CLAUDE.md`.
- **Estimated file count**: 7 unique files (under the 15-file guard; largest batch touches 2 files, under the 6-file batch guard).

## Research Notes

Research ran during the pilot; the actionable findings for *this* plan are codebase-internal and were re-confirmed by fresh exploration:

- **Manifest has no `skill =` field today** — the skill path is currently implicit (`claude/skills/flow-contract-<block>/SKILL.md`). §13 requires it explicit because the block→skill name mapping is non-derivable in general (block `vet-flow-research` → skill `flow-contract-vet-research`).
- **`scripts/verify-shared-blocks.sh` parses the manifest line-by-line with awk** (`^name = "`, `^files = \[`, `in_files`). A scalar `skill = "..."` line placed *outside* the `files = [...]` array is ignored by this parser — so adding the field requires **no shell-script change** (verified against the awk logic in `verify-shared-blocks.sh:42-55`). The field must NOT sit inside the `files` array.
- **`extract_block(contents, name) -> Option<Vec<u8>>`** (`tomlctl/src/blocks.rs:35`) already normalises CRLF→LF via `str::lines()` and excludes the markers — reuse it for the carrier side of verify-skills. `scan_block_names` (`blocks.rs:68`) is also reusable.
- **The drift-normalisation surface is tiny.** Diffing the live `flow-contract-vet-research` SKILL.md body against the `vet-flow-research` block embedded in `optimise.md`, the *only* divergent line is the cross-reference (skill: `` `flow-contract-ledger-schema` skill → Vet event log section ``; carrier: `` `SHARED-BLOCK:ledger-schema` → `Vet event log` ``). §12 rewrote the whole sentence, not just the token — so the normalisation must **drop** contract-cross-reference lines from both sides, not canonicalise them. Procedure lines referencing `flow-research-deep` do **not** match a `flow-contract-` pattern, so they are not falsely dropped.
- **`Cli` is `pub(crate)`** (`tomlctl/src/cli/types.rs:70`, re-exported `pub(crate)` at `cli/mod.rs:29`) — integration tests in `tomlctl/tests/` cannot reach it. The command-lint test **must be a unit test in `src/`**. `src/cli/dispatch.rs` already hosts the `blocks_verify_reproduces_shell_hashes` unit test using the `env!("CARGO_MANIFEST_DIR") → parent() → repo_root` path pattern (`dispatch.rs:1395`) — the command-lint test shares that harness and file.
- **No existing `try_parse_from` idiom** — all `tomlctl/tests/` use `assert_cmd::Command` (subprocess). The command-lint test introduces the first in-process `Cli::try_parse_from` usage; it lives in-crate so it can.
- **Quote-aware tokenisation is required** for command-lint: `--worktree "$(git rev-parse --show-toplevel)"` naively whitespace-split yields a bogus `--show-toplevel)"` token that clap rejects as an unknown flag (false positive). Use `shell-words` (add as dev-dependency) so quoted args stay single tokens; `$(...)`/`<placeholder>` values are fine because clap validates *structure* (flags/subcommands), not values.
- **Existing fixture arrays to replace** (`dispatch.rs:1400-1424`): `flow_context_eight`, `ledger_schema_three`, `execution_record_four`, `apply_pair`, with pinned per-block hash constants (`dispatch.rs:1545-1603`). The hashes stay (block content is unchanged); only the carrier-list *source* moves to the manifest.

### Sources
- `docs/plans/harness-progressive-disclosure/PILOT-LESSONS.md` §§5, 7, 10, 11, 12, 13, 14
- `tomlctl/src/blocks.rs`, `tomlctl/src/cli/{types,dispatch}.rs`, `scripts/{shared-blocks.toml,verify-shared-blocks.sh}` (fresh exploration 2026-05-20)

## User Decisions

> Phase 4 gate — answers captured 2026-05-20. Authoritative.

### Q1 — Plan scope
**Chosen: Gate tooling only.** This plan covers the three gate tools + manifest field + docs (~7 files, fully cargo/parity-verifiable). The 9-carrier propagation becomes follow-up plans opened after this tooling lands and a smoke-run validates. Matches the pilot's own gating discipline (§13: "verify-against-skill should gate propagation, not trail it").
> Prompted by: scope exceeds the 15-file guard and bundles a refactor with new tooling; §13 forces tooling-before-propagation.

### Q2 — Enforcement surface
**Chosen: cargo test only.** The two new checks live as cargo tests gated by CI's `cargo test`. The pre-commit hook keeps running only the existing bash parity script. Avoids touching the supply-chain-sensitive awk hook (CLAUDE.md "Supply-chain note"; §13 argues against adding fragile logic there).
> Prompted by: CLAUDE.md flags `.githooks/` + `verify-shared-blocks.sh` as supply-chain-sensitive; §13 recommends a tested Rust pass over awk-hook extension.

### Phase 5 outcome
**Skipped.** Phase 4 answers introduced no unresearched topic — both decisions are strategic/codebase-internal, fully covered by exploration and PILOT-LESSONS. No library or API was introduced.

## Approach

### A. Manifest `skill =` field (T1)

Add one scalar `skill = "claude/skills/flow-contract-<block>/SKILL.md"` line to each of the **four already-externalised** `[[block]]` entries (`flow-context`, `ledger-schema`, `vet-flow-research`, `ledger-disposition-sweep`). Place it immediately after `name = "..."`, before `files = [...]`, so the awk parser in `verify-shared-blocks.sh` ignores it. Blocks not yet externalised (`execution-record-schema`, `plansdirectory-prompt`, the `apply-*` family, `forbidden-working-tree-ops`) get **no** `skill` field — verify-skills skips any block lacking one. Extend the top-of-file migration comment to document the field's meaning.

### B. verify-skills engine + normalisation (T2 engine, T3 variant, T4 route/test)

New `tomlctl blocks verify-skills [--manifest <path>]` subcommand (`BlocksOp::VerifySkills`, renders kebab `verify-skills`). Engine `blocks::verify_skills(manifest_path) -> Result<SkillDriftReport>` in `blocks.rs`:

For each `[[block]]` carrying a `skill` field AND a non-empty `files` list:
1. Read the skill file; **strip leading YAML frontmatter** (first `---` line through the next `---`, plus one trailing blank line) to get the skill body.
2. For each carrier in `files`, `extract_block(carrier_contents, block_name)` to get the embedded copy.
3. Apply the **normalisation rule** to both skill body and each carrier copy, then SHA256-compare. On mismatch, record `{block, skill, carrier, first_differing_line}`.

A block whose `files` list is empty (all carriers migrated) is skipped — there is no embedded copy to compare against; guard against file-not-found on a missing skill too.

**Normalisation rule** (a tested pure function, pinned by unit-test fixtures):
1. (skill side only) frontmatter strip per B.1.
2. Split into lines (CRLF→LF via `str::lines()`, reusing `extract_block`'s convention).
3. **Drop contract-cross-reference lines** from both sides — a line is dropped if it matches any of: contains `SHARED-BLOCK:`; contains the backtick-quoted token `` `flow-contract- ``; matches the embedder-list sentence pattern (`[Ee]mbedded (verbatim )?into … carriers?`). These are exactly the §12 lines that legitimately diverge between standalone-skill and in-carrier contexts.
4. Trim per-line trailing whitespace; drop trailing blank lines.
5. SHA256 the resulting joined line sequence; compare.

This makes the check pass green at introduction (the sole divergent line in every live pair is a dropped cross-reference). Unit-test fixtures encode: (a) identical bodies → ok; (b) bodies differing only on a cross-reference line → ok (line dropped); (c) bodies differing on a substantive line → drift reported.

JSON output mirrors `blocks_verify`'s shape (`{ok, blocks:[{name, skill, drift:[{carrier, line}]}]}`); exit 1 on drift.

### C. Fixtures-read-manifest refactor (T4)

Refactor the `blocks_verify_reproduces_shell_hashes` unit test (`dispatch.rs:1385`) so the carrier lists come from `scripts/shared-blocks.toml` rather than the hardcoded `flow_context_eight` / `ledger_schema_three` / `execution_record_four` / `apply_pair` arrays. Parse the manifest with the `toml` crate (already a tomlctl dependency), build `block_name → Vec<PathBuf>` at runtime, and assert each block's computed hash equals its pinned constant (the hash constants stay — block content is unchanged). Keep the existing graceful-skip when `claude/commands/*.md` are absent. This kills the §14 "two hand-maintained copies" drift root cause: the manifest becomes the single source the test reads.

### D. command-lint test (T5)

New `command_lint` unit test in `dispatch.rs` (must be in-crate; `Cli` is `pub(crate)`):
1. Resolve `repo_root` via the existing `env!("CARGO_MANIFEST_DIR") → parent()` pattern; glob `claude/skills/tomlctl/SKILL.md`, `claude/skills/flow-contract-*/SKILL.md`, `claude/commands/*.md` (graceful skip if absent).
2. For each file, scan fenced ```` ```bash ```` blocks. **Opt-out**: skip any block whose opening fence carries the info-string token `ignore-command-lint` (for deliberately illustrative/partial snippets).
3. For each line inside a bash block, if (after trimming leading whitespace, and after taking the segment beginning at `tomlctl` when the line is a `… | tomlctl …` pipe) the line starts with `tomlctl`, tokenise it with `shell_words::split` (quote-aware). Join `\`-continued lines first.
4. `Cli::try_parse_from(tokens)` and assert the result is not a clap `ErrorKind::UnknownArgument` / `InvalidSubcommand` / `UnknownArgument`-class error. Other error kinds (e.g. missing required positional, since placeholder values are absent) are tolerated — the lint validates flag/subcommand *structure*, not full argument satisfaction. (Decision: match specifically on the unknown-flag/unknown-subcommand kinds to avoid false positives from required-arg omissions.)
5. On any structural failure, fail with the offending file, line, and clap error.

Add `shell-words` to `tomlctl/Cargo.toml` `[dev-dependencies]`. **Discovery loop**: running this test against the repo for the first time may surface real drift in existing markdown (the whole point). Each hit is either fixed (real drift) or, if genuinely illustrative, marked with the `ignore-command-lint` fence token — never silenced by weakening the matcher.

### E. Docs (T6)

Add a "Drift checks" subsection to `claude/skills/tomlctl/SKILL.md` documenting `tomlctl blocks verify-skills` (purpose + one example invocation — which the command-lint test will then validate). Add a bullet to `CLAUDE.md` "Build & test" noting that `cargo test` now also gates skill-body↔carrier drift and carrier↔CLI flag drift, and that these are CI-gated (not pre-commit).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
parity: bash scripts/verify-shared-blocks.sh
```

## Tasks

### Batch 1 (parallel — independent files)

#### 1. Add `skill =` field to externalised blocks in the manifest [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: —
- **Action**: Add `skill = "claude/skills/flow-contract-<block>/SKILL.md"` to the four externalised `[[block]]` entries (`flow-context`, `ledger-schema`, `vet-flow-research`, `ledger-disposition-sweep`), placed between `name` and `files`. Extend the top-of-file migration comment to explain the field (set when a block is externalised; consumed by `tomlctl blocks verify-skills`).
- **Detail**: Do NOT add the field to non-externalised blocks. The field MUST sit outside the `files = [...]` array so `verify-shared-blocks.sh`'s awk parser ignores it.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0; manifest parses as valid TOML (`tomlctl parse scripts/shared-blocks.toml`); the four entries each carry a `skill` line, no others do.

#### 2. Implement verify-skills engine + normalisation in blocks.rs [M]
- **Files**: `tomlctl/src/blocks.rs`
- **Depends on**: —
- **Action**: Add (a) a frontmatter-strip helper; (b) a `normalise_block(lines) -> Vec<String>` pure function implementing the drop-cross-reference-lines + whitespace rule from Approach B; (c) `verify_skills(manifest_path: &Path) -> Result<SkillDriftReport>` that reads the manifest, and for each block with a `skill` field + non-empty `files`, compares normalised skill body vs each carrier's `extract_block` output. Reuse `extract_block`. Add unit tests for `normalise_block` covering: identical bodies ok; cross-reference-only divergence ok; substantive divergence reported.
- **Detail**: `SkillDriftReport { ok: bool, report: serde_json::Value }` mirroring `BlocksReport`. Skip blocks with empty `files` (all migrated) and guard missing skill files. Report the first differing normalised-line index per (block, carrier).
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml blocks::` passes including the new `normalise_block` fixture tests; `cargo clippy` clean.

#### 3. Add `BlocksOp::VerifySkills` variant in types.rs [S]
- **Files**: `tomlctl/src/cli/types.rs`
- **Depends on**: —
- **Action**: Add `VerifySkills { #[arg(long)] manifest: Option<PathBuf> }` to the `BlocksOp` enum (renders `tomlctl blocks verify-skills`; `--manifest` defaults to `scripts/shared-blocks.toml` when omitted, resolved in dispatch). Doc-comment the variant.
- **Detail**: Mirror the existing `BlocksOp::Verify` doc-comment style. Default-path resolution lives in dispatch, not in the clap default, so the test can pass an explicit path.
- **Acceptance**: `cargo build --manifest-path tomlctl/Cargo.toml` compiles; `tomlctl blocks verify-skills --help` lists the subcommand and `--manifest`.

### Batch 2 (sequential — same file, after Batch 1)

#### 4. Route verify-skills + refactor parity fixture to read the manifest [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/Cargo.toml`
- **Depends on**: 1, 2, 3
- **Action**: (a) Route `BlocksOp::VerifySkills { manifest }` to `blocks::verify_skills` (default path `scripts/shared-blocks.toml`), print JSON, exit 1 on drift — mirror `blocks_dispatch`'s `Verify` arm. (b) Refactor `blocks_verify_reproduces_shell_hashes` to parse `scripts/shared-blocks.toml` for carrier lists instead of the hardcoded `flow_context_eight`/`ledger_schema_three`/`execution_record_four`/`apply_pair` arrays; keep pinned hash constants and the graceful-skip-on-absent-files guard. (c) Add a `verify_skills_clean` unit test calling `blocks::verify_skills` against the real repo manifest and asserting `ok == true` (the introduction-time green check, §13).
- **Detail**: Use the `toml` crate to parse the manifest; build `block_name → Vec<PathBuf>` keyed off `cmd_dir`/repo-root. The hash constants do not change. If (c) reports drift at introduction, the divergent line is either added to the normalisation exclusion set (legitimate §12 reference divergence) or fixed as real drift — do not weaken the comparison to force green.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes (incl. refactored fixture + `verify_skills_clean`); `tomlctl blocks verify-skills` against this repo exits 0 and emits JSON parseable by `jq`; `cargo clippy` clean.

#### 5. Add the command-lint unit test [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/Cargo.toml`
- **Depends on**: 4
- **Action**: Add `shell-words` to `[dev-dependencies]`. Add a `command_lint` unit test per Approach D: glob the three markdown sets, extract ```` ```bash ```` blocks (honouring the `ignore-command-lint` fence opt-out), tokenise `tomlctl` lines quote-aware via `shell_words::split`, feed to `Cli::try_parse_from`, and assert no unknown-flag/unknown-subcommand error. Fix any real drift the test surfaces in existing markdown; mark genuinely illustrative snippets with `ignore-command-lint`.
- **Detail**: Join `\`-continued lines; for `… | tomlctl …` pipes take the segment from `tomlctl`. Match specifically on `clap::error::ErrorKind::{UnknownArgument, InvalidSubcommand}` to avoid false positives from omitted required positionals. Reuse the `dispatch.rs` repo-root resolution.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes against the repo; a deliberately-broken local edit (`tomlctl flow --bogus-flag`) makes it fail (verify, then revert); `cargo clippy` clean.

### Batch 3 (sequential — after Batch 2)

#### 6. Document the new subcommand and the gate [S]
- **Files**: `claude/skills/tomlctl/SKILL.md`, `CLAUDE.md`
- **Depends on**: 5
- **Action**: Add a "Drift checks" subsection to `tomlctl/SKILL.md` documenting `tomlctl blocks verify-skills` (1-sentence purpose + one example invocation). Add a `CLAUDE.md` "Build & test" bullet noting `cargo test` now gates skill-body↔carrier drift (`blocks verify-skills`) and carrier↔CLI flag drift (`command_lint`), CI-only.
- **Detail**: The SKILL.md example invocation will be validated by the Task 5 command-lint test on the next `cargo test`, so it must parse cleanly.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` still green (validates the new SKILL.md example); both files contain the new content referencing `verify-skills` and `command_lint` by exact name.

## Dependency Graph

```
Batch 1 (parallel):  T1 (manifest), T2 (blocks.rs engine), T3 (types.rs variant)
Batch 2 (sequential): T4 (dispatch route + fixture refactor + verify_skills_clean) ← T1,T2,T3
                      T5 (command_lint test) ← T4            [both edit dispatch.rs → sequential]
Batch 3 (sequential): T6 (docs) ← T5
```

T1/T2/T3 are independent files. T4 and T5 both edit `dispatch.rs`, so they run sequentially even though logically distinct.

## Verification

Gate acceptance (run all):
- `cargo build --manifest-path tomlctl/Cargo.toml` → clean build
- `cargo test --manifest-path tomlctl/Cargo.toml` → all pass, incl. `normalise_block` fixtures, refactored `blocks_verify_reproduces_shell_hashes`, `verify_skills_clean`, `command_lint`
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` → no new warnings
- `bash scripts/verify-shared-blocks.sh` → exit 0 (manifest `skill =` field does not break the awk parser)
- Manual: `tomlctl blocks verify-skills | jq .` → exits 0, valid JSON; `tomlctl blocks verify-skills --help` lists the subcommand
- Manual negative check: temporarily edit one embedded carrier block's substantive line → `tomlctl blocks verify-skills` exits 1 and names the carrier; revert

## Risks

1. **Normalisation under- or over-drops lines** — if the cross-reference exclusion patterns are too broad they mask real drift; too narrow they false-positive at introduction. *Mitigation*: patterns calibrated against all four live skill/carrier pairs; `normalise_block` pinned by unit-test fixtures; the `verify_skills_clean` test asserts green against the real repo so any miscalibration surfaces immediately.
2. **command-lint false positives from shell syntax** — `$(...)`, heredocs, multi-line invocations. *Mitigation*: quote-aware `shell_words` tokenisation; `ignore-command-lint` fence opt-out for genuinely partial snippets; match only on unknown-flag/unknown-subcommand error kinds (not missing-positional).
3. **Manifest `skill =` field breaks the awk parser** — `verify-shared-blocks.sh` is hand-rolled awk. *Mitigation*: field placed outside the `files` array where the parser ignores it; verified against the awk logic; T1 acceptance re-runs the script.
4. **Fixture refactor changes test behaviour subtly** — reading the manifest could pick up a block the hardcoded arrays omitted (the very drift §14 cites). *Mitigation*: this is the intended fix; if a previously-untested block appears, add its pinned hash constant; `cargo test` surfaces the mismatch with the remediation message already built into the fixture.
5. **`shell-words` adds a dependency** — minor supply-chain surface. *Mitigation*: dev-dependency only (not in the shipped binary); widely-used, small crate. If undesirable, a hand-rolled quote-aware splitter is a fallback (note in PR).

## Next Steps (post-approval)

- At Phase 9 bootstrap, relocate this file to `docs/plans/harness-progressive-disclosure-gate-tooling.md` and use slug `harness-progressive-disclosure-gate-tooling` (the `clever-waddling-turtle` placeholder was forced by plan mode).
- After this tooling lands: open the first **propagation** plan (Wave 1 — extract `flow-contract-execution-record-schema` + `flow-contract-plansdirectory-prompt`, migrate plan-new/plan-update/review-plan/implement/tdd), gated by a live smoke-run per §11. The `apply-*` wave and `optimise`/`test-bootstrap` follow.
