# Pilot Lessons — `/review` Progressive-Disclosure Overhaul

**Pilot date**: 2026-05-19
**Pilot carrier**: `claude/commands/review.md`
**Flow slug**: `harness-progressive-disclosure`
**Pilot commits**: `b96e3c5` (skills), `5f5ed73` (tomlctl), `561e4ca` (carrier rewrite + manifest), `f7c230f` (test-fixture follow-up)

> This document feeds the follow-up plans that propagate the pattern to the other 9 carriers (`/optimise`, `/review-apply`, `/optimise-apply`, `/plan-new`, `/plan-update`, `/implement`, `/review-plan`, `/tdd`, `/test-bootstrap`).

## Carrier LOC reduction

| Metric | Before | After | Change |
|---|---|---|---|
| `claude/commands/review.md` LOC | 777 | 62 | −92% (12.5×) |
| Shared-block carriers (count) | 9 (review, 8 others) | 8 (review removed) | — |
| `scripts/shared-blocks.toml` block entries naming `/review` | 4 | 0 | — |
| Top-level `## Step` headers | 6 + Interim | 6 + Interim | preserved |
| `<!-- SHARED-BLOCK:` marker pairs in carrier | 4 | 0 | extracted |

The 62-LOC skeleton retains:
- All six original Step headers (`Step 0` through `Step 4`) plus the `Interim checkpoint` section.
- All user-engagement gates inline, including the literal phrase "**user-engagement gate — the autonomy directive does not apply**" at Step 1 (deferred-reopen sweep) and Step 4 (disposition handler).
- Mixed-model dispatch non-negotiable (Step 2): Agents 1+3 on Opus (`flow-research-deep`), Agents 2+4+5+6 on Sonnet (`flow-research`).
- Hard caps: Security ≤ 5 findings, Agent 5 owns testing, Agent 3 owns the `db` category.
- The `## Effort: xhigh or max` directive.

## Contract skills created (descriptions verbatim)

Four `claude/skills/flow-contract-*/SKILL.md` files were created. Each carries the verbatim block body from `review.md` (stripped of `<!-- SHARED-BLOCK:NAME START/END -->` markers) under a 2-field frontmatter.

### `flow-contract-flow-context` (2946 bytes)

> Flow resolution + doctor contract for flow-bootstrap envelopes — defines how a carrier's Step-0 builds the input envelope, gates on `envelope.ok`, and binds `envelope.resolved.{slug, context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for downstream phases. Covers the no-flow fallback, doctor-fail handling, staleness reconciliation, status vocabulary, slug derivation, canonical artifact paths, and the mandatory bootstrap-summary console line format. Consult when any flow-carrying command (review, optimise, plan-new, plan-update, implement, tdd, review-plan, review-apply, optimise-apply) dispatches flow-bootstrap and needs to interpret the returned envelope correctly.

### `flow-contract-ledger-schema` (15829 bytes)

> Canonical schema for review/optimise ledgers — `[[items]]` table shape, required and optional fields, status vocabulary, vet-event log, dedupe contract, and write idioms for `review-ledger.toml` and `optimise-findings.toml` (both flow-local and flow-less `.claude/reviews/<scope>.toml` / `.claude/optimise-findings/<scope>.toml`). Embedded into review/optimise/review-apply/optimise-apply carriers; defines every field a finding must carry plus the reconciler's behaviour on schema drift. Consult before any read, write, or update against a review or optimise ledger TOML file.

### `flow-contract-vet-research` (3349 bytes)

> Universal vet-pass procedure for research-agent output — the orchestrator-side gate that distinguishes "research returned" from "research findings are trustworthy". Triages by evidence-grade, honours ESCALATE-TO-DEEP flags, drops unverified low-confidence findings, spot-checks sampled findings against cited file:line / URLs / library versions, downgrades or drops fabrications, appends a durable [[vet_events]] ledger entry, emits a mandatory per-agent console summary line, and escalates Sonnet→Opus on >30% systemic failure. Consult immediately after a flow-research or flow-research-deep agent returns, before persisting findings to any ledger or notes section.

### `flow-contract-ledger-disposition-sweep` (3865 bytes)

> Ledger disposition sweep procedure for /review and /optimise — read-only orphan surfacing (file orphans, symbol orphans), duplicate-finding detection, and the disposition-sweep workflow that surfaces stale or no-longer-applicable items without auto-transitioning them. Defines how the sweep walks open `[[items]]`, batches Glob/Grep lookups for efficiency, and reports findings to the console for user disposition. Consult during the disposition-sweep phase of /review or /optimise, or any time a ledger needs orphan/duplicate triage.

Total skill body bytes externalised from `/review`: **25,989 bytes** (~26 KB).

## tomlctl subcommand validation

`tomlctl flow envelope build` works as designed. Verified by:

- 4 cargo integration tests in `tomlctl/tests/flow_envelope.rs` — minimal-args, all-fields-set, invalid-command rejection, invalid-artifact rejection. All pass.
- Manual smoke test: `tomlctl flow envelope build --command review --branch main --worktree "$(git rev-parse --show-toplevel)" --cwd "$(pwd)"` emits valid JSON with all 8 fields (`command`, `flow_override`, `path_args`, `branch`, `worktree`, `cwd`, `require_artifacts`, `staleness_threshold`).
- `claude/skills/tomlctl/SKILL.md` updated with an "Envelope construction — flow envelope build" section advertising the subcommand and example invocation.

**Implementation-level deviation (logged in execution record E8)**: clap-derive renders enum variants as single kebab-case tokens, so a flat `FlowOp::EnvelopeBuild` would yield CLI spelling `tomlctl flow envelope-build` (2-word). To get the documented 3-word `tomlctl flow envelope build` spelling, the deep agent introduced a nested `EnvelopeOp::Build` enum mirroring the existing `FlowOp::Active { op: ActiveOp }` pattern. This adaptation is consistent with the codebase's existing nested-subcommand idiom and was the right call.

## Skill-invocation mechanism — UNVALIDATED in this pilot

**Critical caveat**: the most important risk this pilot was meant to test — *does the model reliably load a skill body when the carrier's prose says "Invoke the `flow-contract-vet-research` skill"?* — was **not validated** during this `/implement` run. The reason is mechanical: the orchestrator (this `/implement` session) cannot itself dispatch the `/review` slash command — slash commands fire from the user's interactive session, not from sub-agent contexts. The skeleton has been written; the validation requires a separate human-initiated `/review` invocation in a fresh session.

What we DO know works:
- The skills exist at the conventional `claude/skills/<name>/SKILL.md` path.
- The frontmatter parses (2 fields: `name`, `description`) — matches the on-disk convention used by `tomlctl/SKILL.md` and `test-author/SKILL.md`.
- The skill bodies are byte-identical to the pre-extraction shared-block content (modulo marker comments).
- The descriptions are concrete and specific, naming the contract surface and the trigger condition ("Consult when…") — model-discoverable on natural-language matching.

What we DON'T know yet — and propagation MUST NOT proceed until validated:
- Whether a markdown directive like "Invoke the `flow-contract-vet-research` skill to load the vet-pass procedure" actually causes the model to load the skill body. The natural-language form is *plausible* but is not a documented invocation contract.
- Whether the loaded skill body lands in the carrier session's working context (and not, say, summarised away).
- Whether the model honours skill loading at the *correct phase boundary* — e.g. loading the vet skill BEFORE persisting findings, not AFTER.

**Required pre-propagation validation step**: the user must run `/review` against a controlled fixture (or recent git changes) and observe whether:

1. Step 0 dispatches `flow-bootstrap` with the envelope produced by `tomlctl flow envelope build`.
2. Step 1 loads `flow-contract-flow-context` and `flow-contract-ledger-schema` skill bodies in time to inform the ledger setup.
3. Step 2.5 loads `flow-contract-vet-research` and applies the vet procedure to research-agent output.
4. The disposition sweep (Step 1) loads `flow-contract-ledger-disposition-sweep` and surfaces orphans correctly.
5. Output behaviour matches the pre-overhaul `/review` (ledger entries with the same field set, same sample-size discipline on vet, same dispatch model).

If (1)-(5) all hold: propagation proceeds. If any step fails, the fallback documented in plan Risk 1 applies — replace the natural-language skill-invocation directive with an explicit `Skill` tool dispatch line in the carrier (an extra ~5 LOC per invocation, still well under the 100-LOC ceiling).

### Update — VALIDATED LIVE (2026-05-20)

The above caveat was written from the `/implement` session, which structurally could not fire `/review`. The mechanism has since been validated by a real human-initiated run: on **2026-05-20** a live `/review harness-progressive-disclosure` invocation (followed by `/review-apply`) executed end-to-end. The orchestrator successfully loaded the `flow-contract-flow-context`, `flow-contract-ledger-schema`, `flow-contract-ledger-disposition-sweep`, and `flow-contract-vet-research` skill bodies at their correct phase boundaries (Step 0, Step 1, Step 2.5). The natural-language "Invoke skill `<name>`" directive does cause the model to load the skill body into the carrier session's working context at the right phase — the #1 risk this pilot was meant to test is now answered: **the mechanism works.**

Caveat narrows but does not vanish: the mechanism is **validated-once**, not **continuously testable**. The CLI toolchain (cargo, `verify-shared-blocks.sh`) cannot exercise skill loading — only a human-driven `/review` run can. Before broad propagation, still add an automated fixture-based smoke check (or at minimum a documented manual re-validation step per propagation PR) so a future skill-loading regression is caught mechanically rather than discovered in production. The original §"Skill-invocation mechanism" caveat above is retained as the historical record of what was unknown at pilot time.

## Recommended changes for propagation

Based on observations during this pilot, the propagation plans for the remaining 9 carriers should adopt the following refinements:

### 1. Skill frontmatter convention is fixed

Use the **2-field schema** (`name` + `description`) verbatim. Do not include `when_to_use`, `user-invocable`, or `disable-model-invocation` fields — these were a research-derived hypothesis that does not match the on-disk reality (logged as deviation E2). The `description` field should be a single long line combining what the contract defines and when to consult it. Aim for ~500–1500 chars per skill; the existing `tomlctl/SKILL.md` and `test-author/SKILL.md` precedents normalise long descriptions.

### 2. Use `tomlctl flow envelope build` in every Step 0

The subcommand is now part of `tomlctl`'s public surface and documented in `claude/skills/tomlctl/SKILL.md`. Each migrated carrier replaces its inline ~15-line envelope-template prose with one bash invocation, passing `--command <carrier-name>` and the standard branch/worktree/cwd args. For carriers that take path arguments (`/review`, `/optimise`), use the repeatable `--path-arg <p>` flag.

### 3. Preserve existing top-level header structure (don't refactor)

The plan's "Required phase structure" listed `## Phase 1`–`## Phase 5`, but `/review` actually uses `## Step N` headers exclusively (no `## Phase` at all). The deep agent correctly honoured the "preserve every existing header" rule over the "typical structure" suggestion (logged via the agent's deviation note inline in its T6 return). Each carrier's existing header structure is the reviewer's grep anchor — preserve names and numbering verbatim.

**Implication for propagation**: each carrier's existing top-level headers should be enumerated up-front (e.g. `grep -n "^## " claude/commands/<carrier>.md`) and the propagation task should explicitly list "preserve every one verbatim" rather than imposing a generic phase template. Plan-side `## Phase N` boilerplate in propagation tasks should be removed.

### 4. Skill invocations bind at the actual phase where the contract is consulted

The plan sketch placed `flow-contract-ledger-disposition-sweep` at "Phase 4 (Disposition sweep)" but in `/review`'s actual flow the orphan-surfacing sweep runs at Step 1 (before agent dispatch) — Step 4 in the carrier is the *user-disposition-reply handler*, a different phase. The deep agent placed the skill at Step 1 correctly. **For each propagation: read the current carrier carefully and bind each skill invocation to the phase where the contract actually fires**, not where a plan template suggests it might fit.

### 5. Test-fixture carrier lists in tomlctl need updating per migration

The hardcoded carrier lists in `tomlctl/src/cli/dispatch.rs` (the `blocks_verify_reproduces_shell_hashes` test at line ~1390) duplicate the manifest's carrier lists and must be updated in lockstep with `scripts/shared-blocks.toml`. For each propagation:
- Remove the migrating carrier from the relevant `flow_context_*`, `ledger_schema_*`, `execution_record_*`, or `apply_pair` arrays in the test.
- Rename the variable to reflect the new length (e.g. `flow_context_seven` → `flow_context_six` when the second carrier migrates).
- The pinned hash constants stay the same — block content in the remaining carriers is byte-identical, so the parity hash doesn't change.

If a propagation PR forgets this, `cargo test` fails with a clear "block X missing from <carrier>" panic. The fix is mechanical (5–10 line edit in `dispatch.rs`); the verification agent surfaces it cleanly.

### 6. Migration policy in `scripts/shared-blocks.toml` is human-readable

The top-of-file comment added in this pilot ("Carriers are progressively externalising shared blocks…") gives future maintainers context for shrinking `files` arrays. Each propagation PR should:
- Remove the migrating carrier from the relevant `files = [...]` arrays.
- Add a per-block comment line `# <carrier>.md migrated to claude/skills/<skill>/SKILL.md (<date>).` immediately above the modified `files` line, mirroring the pattern this pilot established.
- When a block's `files` array becomes empty (all carriers migrated), delete the entire `[[block]]` entry — the parity contract is satisfied by the skill body alone.

### 7. Doctor-of-skills check (recommended addition)

Currently nothing in CI verifies that the externalised skill body matches what other carriers' embedded copies still contain. During the migration window (one carrier migrated, others still embed the same block) there is a real risk of drift: a fix to one embedded copy doesn't ripple to the skill body, and vice versa. **Recommended one-off CI/local check**:

```
For each (skill, block) pair where the manifest still lists ≥ 1 carrier embedding the block:
  Compute SHA256(skill body, normalised) and SHA256(carrier's embedded block).
  Fail loudly if they differ.
```

This check is mechanical and fast; it can be a 30-line Rust subcommand on tomlctl (`tomlctl blocks verify-against-skill <block-name> <skill-path>`) or a 20-line bash addition to `scripts/verify-shared-blocks.sh`. Add it before the next propagation PR lands so the verification budget catches drift caused by mid-migration patches.

### 8. Don't allow stray uncommitted state to leak into pilot commits

During this pilot, commit `561e4ca` (T6+T7) bundled in some pre-existing uncommitted state (`precious-frolicking-steele/` flow files, `commit-conventions/` skill files) from a prior session. The cause was undetermined; possibly a hook or parallel session, possibly a misapplied `git add`. The work is correct but provenance is muddier than it should be. For future propagation PRs: explicitly verify `git diff --cached` matches the expected file set before each `git commit`; if untracked items appear, unstage them with `git restore --staged <path>` rather than letting them ride along.

### 9. Live `/review` validation gates propagation

Per §"Skill-invocation mechanism" above: do not start propagating the pattern until a human-driven `/review` run confirms skill bodies load when invoked via natural-language directive. This is the cheapest single validation; the entire propagation plan is contingent on it. If natural-language directives don't reliably load skills, the workaround (explicit `Skill` tool dispatch in carrier prose) is straightforward and adds ~5 LOC per carrier — still well under the 100-LOC ceiling.

## Lessons from the live pilot run (2026-05-20)

These emerged from the first end-to-end human-driven `/review harness-progressive-disclosure` + `/review-apply` run (the validation recorded in §"Skill-invocation mechanism — VALIDATED LIVE"). They are distinct from the build-time lessons above because most were **invisible until the carrier actually executed** — the review lenses cannot catch them because they read code, they don't run commands. They sharpen §1, §5, §7, and §9.

### 10. Carrier↔CLI flag drift is its own failure class — lint it at compile time

The thin carrier said `tomlctl … --flow <slug>` but the actual flag is `--flow-override` (review-ledger R1). This is a *guaranteed* runtime failure on a supported input path, and it broke on the very first live invocation — yet no review lens flagged it, because lenses don't execute the command. Hand-written `tomlctl …` invocations in carrier/skill prose are unverified against the real CLI.

**Resolution — compile-time command-lint beats `--help` advice.** Advising authors to run `tomlctl <subcmd> --help` is a manual, per-author, runtime check that depends on diligence — exactly what failed here. Instead add a `cargo test` that:
1. Globs the markdown carrying `tomlctl …` invocations — the `tomlctl/SKILL.md` (the authoritative guidance), the `flow-contract-*` skills, and the carriers.
2. Extracts each command line (fenced ` ```bash ` blocks; split on `|` for piped stdin).
3. Feeds the tokens to the **real `Cli` clap parser** via `try_parse_from` / `try_get_matches_from` (which validates structure without executing) and asserts no unknown-subcommand / unknown-flag error.

Because it validates against the *same clap surface the binary uses*, flag-name drift becomes **impossible to commit** — the pre-commit/CI test fails. This is what makes `tomlctl/SKILL.md` authoritative in the strong sense: not "we declare it authoritative" but "it is mechanically guaranteed to match the binary." Precedent already exists — `dispatch.rs:1395` reads `claude/commands/*.md` from the test crate for the parity fixtures, so the machinery is in place.

- **Boundary**: this validates *structure* (the flag/subcommand exists — the R1 class), NOT *semantics* (that the flag does what the prose claims). Semantic drift still needs the per-carrier smoke-run (§11). The two are complementary.
- **Convention needed**: validate only fenced `bash` blocks with lines beginning `tomlctl`, and provide an opt-out marker for deliberately illustrative/partial snippets (else false positives). Placeholder *values* (`<ledger>`, `R{n}`, `$ARGUMENTS`) are fine — clap doesn't validate arg values at parse time, only flag/subcommand structure.
- Demote `--help` to an *authoring aid* (look up a flag while writing), never the enforcement mechanism.
- This test could share its markdown-extraction harness with the §7 verify-against-skill check — both are "Rust tests that read the markdown and assert it matches the source of truth."

### 11. "Skills load" ≠ "carrier is correct" — smoke-run each migrated carrier

The skill-invocation mechanism worked perfectly (§VALIDATED LIVE), yet the same carrier still shipped two runtime bugs: R1 (flag drift) and R3 (flow mis-resolution — `active-latest` silently resolved the wrong flow when given a bare arg matching a flow slug). Confirming that skill bodies *load* is necessary but not sufficient for carrier correctness.

**Sharpens §9**: the propagation gate must be a live, end-to-end run *of each migrated carrier*, not a one-time mechanism check. §9's single `/review` run validated the *mechanism* (does natural-language skill invocation work?) — it does not transfer correctness to the other 9 carriers. Each breakout PR needs its own smoke-run, and §10's command-lint should run in CI for all of them continuously.

### 12. Post-extraction reference-rewrite is a mandatory extraction step

Verbatim block extraction reliably leaves two classes of stale prose inside the new standalone skill (review-ledger R9/R10/R11): (a) in-block cross-references in the old `SHARED-BLOCK:<name>` marker notation, which resolve to nothing once the block is loaded standalone; and (b) "embedded verbatim into review.md, …" embedder-list prose that is wrong the moment the block leaves a carrier. Every future breakout produces these.

**Bake into the extraction procedure** (don't discover them via a later review round): after stripping the markers, (a) rewrite each in-block cross-ref from `SHARED-BLOCK:X` to "the `flow-contract-X` skill"; (b) correct any "embedded into <carriers>" sentence to name the skill as the canonical source and list only the carriers *still* embedding the block pending their own migration.

**Interaction with §7**: this divergence between the skill body and the still-embedded carrier copies is *legitimate and expected* on these specific reference lines (the standalone and in-carrier contexts genuinely differ). That is precisely why the §7 verify-against-skill check must **normalise these reference lines before comparison**, or it will false-positive on exactly the lines this step is supposed to fix.

### 13. verify-against-skill (§7) should gate propagation, not trail it

The drift window between a skill body and the still-embedded carrier copies opens *precisely during* the multi-PR propagation (one carrier migrated, others still embed the same block). Discovering drift after the fact is too late. §7 was written as a "recommended addition" to land "before the next propagation PR"; this run reinforces that it should be a **hard prerequisite landed before the bulk breakouts begin** (review-ledger R5, skipped as needing a dedicated plan).

R5's analysis sharpens the §7 design: skill bodies use YAML frontmatter (not `SHARED-BLOCK` markers), only 4 of 11 blocks are externalised, and the block→skill name mapping is **non-derivable** (e.g. block `vet-flow-research` → skill `flow-contract-vet-research`). So the check needs (a) an explicit `skill = "claude/skills/flow-contract-<X>/SKILL.md"` field per `[[block]]` in the manifest, and (b) a defined frontmatter-strip + whitespace-normalisation rule (which must also normalise the §12 reference lines). Implement it as a **tested `tomlctl blocks verify` Rust pass**, not an extension of the unsandboxed awk hook — the supply-chain sensitivity of the pre-commit hook (per CLAUDE.md) argues against adding fragile new logic there. Bodies were confirmed byte-identical at pilot time, so the check passes cleanly at introduction.

### 14. Scope the next breakouts: ledger-schema already serves R+O; execution-record is a separate breakout

The `flow-contract-ledger-schema` skill already defines **both** `R{n}` (review) and `O{n}` (optimise) IDs and both category vocabularies, and its read/write contract names `review-ledger.toml` *and* `optimise-findings.toml`. So `/optimise`, `/review-apply`, and `/optimise-apply` can reference it the moment they migrate — **no new ledger skill is needed** for them.

The `E`-prefixed **execution-record** contract is a *different* schema — `execution-record-schema`, still an embedded block carried by `/implement`, `/plan-new`, `/plan-update`, `/tdd`, not externalised in this pilot. It needs its own `flow-contract-execution-record` breakout. And R4 showed `execution-record-schema`'s parity is enforced by **two hand-maintained copies** — the manifest *and* the Rust fixtures in `dispatch.rs` — which had **already drifted** (the fixtures omitted `tdd.md` while the manifest included it). When externalising it, fix the root cause: make the Rust fixtures **read the manifest** rather than re-encoding the carrier list (the same single-source principle as §10 and §13). Until then, every carrier add/remove must touch both copies in lockstep (§5).

## Quick-glance verification snapshot

```
cargo build  — pass
cargo test   — pass (461 tests across 16 suites; one transient flake on flow_active::remove_then_list_shows_zero_entries cleared on re-run, suspected parallel-execution / global-state issue unrelated to this work)
cargo clippy — pass (1 pre-existing warning in capabilities.rs:65, not introduced)
parity       — pass (shared-block parity OK across 8 remaining carriers + 2 agents)
review.md    — 62 LOC, 7 headers preserved, 0 SHARED-BLOCK markers, 4 skill references, envelope-build present
skill files  — 4 created, frontmatter parses, body byte-lengths verified
```

## Cache-behaviour observations — NOT MEASURED

Plan Task 9's acceptance called for a "cache-behaviour observations (token counts before/after)" section, and Risk 3 proposed validating cache behaviour during Task 8 by inspecting the API response's `cache_*` fields. **No token-count data was captured during this pilot.** The before/after cache-read / cache-write figures were not recorded, so the cache-thrash hypothesis (Risk 3) remains untested empirically — it rests only on the architectural argument that skill bodies load into conversation context after the final cache breakpoint, not into the cached system prompt.

This gap is stated explicitly rather than left silently missing. Before broad propagation, a future run should capture `cache_creation_input_tokens` / `cache_read_input_tokens` from the API response across a `/review` invocation (and ideally a back-to-back `/plan-new` to observe command-swap behaviour) so the caching claim is backed by measurement rather than reasoning alone.
