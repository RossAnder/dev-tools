# Plan: tomlctl backlog capture

**Plan path**: `docs/plans/tomlctl-backlog-capture.md`
**Created**: 2026-09-01
**Status**: draft

## Context

Agents working a task routinely discover things that are real but out of scope: a flaky test that trips
the suite but has nothing to do with the change, a bug two directories away, a "we should do X later",
a small annoyance that needs investigation. Today every one of those dies in prose. `/implement`'s
Phase-4 `### Failed / Skipped` and `### Plan Deviations` sections are free text persisted nowhere;
`/tdd`'s deferred follow-ups are summarised and dropped; the apply-pipeline's out-of-scope branch says
in as many words "do NOT auto-invoke, report in the final summary only"; and the
`skipped <id>: requires deliberate refactor` tags an implementer emits vanish unless the orchestrator
happens to write a rejected status.

Every store that *does* exist is flow-scoped and item-scoped. A review ledger can defer an item it
already contains; it has nowhere to put an issue that was never in its item set. And nothing anywhere
answers the reverse question — an agent that has just detected an issue has no way to ask "is this
already known, and if so what did we learn last time?"

This adds a repo-scoped capture log at `.claude/backlog.toml`, written through a new `tomlctl backlog`
verb group. It is deliberately not lumina: no hierarchy, no acceptance criteria, no closure gates, no
planning detail — that stays in plan files. Capture, context, query, triage. The end state is that a
tangential discovery costs one command to record and one command to look up, current work closes off
cleanly, and a later session can pull a cluster of related items into a real scope.

Bundled with it, because the same skill documents the same CLI: the `tomlctl` skill is 869 lines
against an official 500-line ceiling and carries five confirmed drift defects. It gets split and
corrected in the same plan.

## Scope

**In scope**

- A new `tomlctl backlog` verb group: `add`, `check`, `list`, `show`, `relate`, `triage`, `cluster`,
  `compact`, `evidence`.
- The `.claude/backlog.toml` schema, its content-derived ids, its normalise-then-hash dedupe, and its
  similarity/relatedness scoring.
- Evidence artefacts as a per-item drop-box directory at `.claude/backlog-evidence/<item-id>/` —
  contents git-ignored, a tracked `.evidence` marker per directory, no per-file record — surfaced by
  `backlog show` / `backlog check` at read time and checked by a `backlog evidence {dir|audit}` verb.
- A `backlog-capture` skill and a `/backlog` sweep command.
- Capture points wired into `/implement`, `/tdd`, `/review`, `/optimise`, and the apply pipeline —
  orchestrator-written only.
- A reported-candidates block in both implementer agents.
- Splitting `claude/skills/tomlctl/SKILL.md` into `references/*.md` and fixing its drift, plus widening
  `command_lint` so the moved examples stay gated.

**Out of scope**

- Any lumina change. The backlog is not a lumina work-item feed and does not sync with one.
- Auto-generating plans, tasks, or flows from backlog items. Promotion records a link and stops.
- Fuzzy/ML similarity, embeddings, or any new crate dependency.
- Migrating existing deferred/wontfix ledger items into the backlog. `defer` and `wontfix` keep their
  current meaning for items a ledger already owns.
- Changing the review/optimise ledger schemas or the execution-record schema.
- Enumerating individual artefacts in the store. No `[[artefacts]]` array, no per-file digest,
  caption, media type or count, and no `evidence_count` field. The directory listing is computed on
  every read; any stored copy would be wrong the moment someone copies a file in by hand, which is
  the endorsed way to add one.
- Copying, deleting, renaming or interpreting artefacts. `tomlctl` creates the directory and its
  marker and reports on the result; `cp` and `rm -rf` are the capture and reclaim paths, and neither
  is worth a verb.
- Any form of large-file storage: no LFS, no external object store, no compression, no
  content-addressed blob deduplication. The git-ignore default is the whole size story.
- Secret scanning or automated redaction. `evidence audit` flags an unexpected extension, an oversize
  file, and anything git says is about to be published — it never inspects a screenshot's pixels or
  a log's text for credentials.

**Affected areas**: `tomlctl/src/`, `tomlctl/tests/`, `tomlctl/Cargo.toml`, `tomlctl/README.md`,
`claude/skills/`, `claude/commands/`, `claude/agents/`, `scripts/shared-blocks.toml`, `CLAUDE.md`,
`.gitignore`, `.claude/settings.json`

## Research Notes

### tomlctl crate architecture (`tomlctl/`, v0.5.0, edition 2024, MSRV 1.95)

- Top-level verb set is `enum Cmd` in `tomlctl/src/cli/types.rs` (1228 L):
  `Parse, Get, Set, SetJson, Validate, Items{op}, Blocks{op}, ArrayAppend, Capabilities, Integrity{op}, Flow{op}, Json{op}`.
  Hand-mirrored in `const SUBCOMMANDS` (types.rs:55) and `const FEATURES` (types.rs:22).
- `tomlctl/src/cli/dispatch.rs:471 run(cli: Cli)` — one match arm per `Cmd`; group verbs delegate
  (`Cmd::Flow{op} => crate::flow::dispatch(op)?`). `tomlctl/src/flow/dispatch.rs:11` is the template
  for a new verb group (pure fan-out to `flow::<leaf>::dispatch`). **The match is exhaustive with no
  `_` catch-all** — 12 `Cmd::` arms and nothing else — so a `Cmd` variant and its arm must land in
  the same task or the crate does not compile (E0004).
- **A generic array-of-tables engine already exists.** Every `items` op takes `--array <name>`
  (default `items`); nothing is hardcoded per ledger type. `io::items_array(doc, name)`
  (`tomlctl/src/io.rs:103`) is name-generic. Only two things are ledger-shape-aware:
  `items::Item::validate` (disposition → required-field table) and
  `dedup::FINGERPRINTED_FIELDS` = `file,summary,severity,category,symbol` (`tomlctl/src/dedup.rs:38`).
  **Corollary hazard**: an array *named* `items` in any file under `.claude/` is the default target
  of `tomlctl items add|update|apply|backfill-dedup-id`, whose `apply_dedup_id_on_add`
  (`tomlctl/src/items.rs:63`) unconditionally stamps a ledger-shaped `dedup_id`.
- Shared clap flag bundles to `#[command(flatten)]`: `ReadIntegrityArgs`, `WriteIntegrityArgs`,
  `QueryArgs` (all `--where-*` / `--select` / `--pluck` / `--lines` / `--ndjson` / `--raw`).
  Flattening these onto subcommand *variants* is the tree's existing pattern
  (`Cmd::Get { …, #[command(flatten)] integrity: ReadIntegrityArgs }`, `tomlctl/src/cli/types.rs:341`).
- Write path: `io.rs::mutate_doc` / `mutate_doc_conditional` / `mutate_doc_plan`,
  `guard_write_path` (**refuses any write outside `<repo-root>/.claude/`** unless `--allow-outside`),
  `ensure_parent_under_claude` (containment-bounded `mkdir -p`), `refuse_outside_symlink_leaf`,
  `repo_or_cwd_root()` = `TOMLCTL_ROOT` env → `git rev-parse --show-toplevel` → cwd (OnceLock-cached),
  `with_exclusive_lock`, `write_toml_with_sidecar`, `atomic_write(path, bytes: &[u8])` — the last is
  **byte-generic**, so writing a non-TOML blob needs no new machinery. Reads outside `.claude/` only
  *warn* (`io::warn_if_read_outside_claude`).
- **Several write-path helpers are module-private**, not `pub(crate)`: `read_json_arg` (:135),
  `on_missing_for` (:323), `warn_if_created` (:345), `write_envelope` (:372), `query_input_from_cli`
  (:424). Only `read_integrity_opts` (:226), `write_integrity_opts` (:266) and `seed_doc_for` (:300)
  are `pub(crate)`. `crate::flow` uses none of them — the flow group bypasses the auto-create
  machinery entirely — so a new group that *does* want auto-create must widen their visibility first.
- Auto-create on first write: `cli/dispatch.rs:282 SCHEMA_SEEDED_FLOW_FILES` + `seed_doc_for(path)`
  (basename match → `{schema_version=1, last_updated=<today>}`) + `on_missing_for` + `warn_if_created`
  + `write_envelope` → `{"ok":true,"created":<bool>,"path":"…"}`. Adding a basename is a one-line edit.
- `query::run` (`tomlctl/src/query.rs:522`) takes `&Query`, not `QueryArgs`; `Query::from_input`
  (:1690) takes a `QueryInput` that only `query_input_from_cli` builds.
- Stdin `-` sentinel: `read_json_arg` / `read_ndjson_source`, `STDIN_CONSUMED` AtomicBool (one `-` per
  invocation), TTY refusal, `MAX_STDIN_BYTES = 32 MiB` (`cli/dispatch.rs:50`).
- `integrity::sha256_hex_of_file` (`tomlctl/src/integrity.rs:68`) streams a file digest in 64 KiB
  chunks — a blob digest with no new crate.
- Output conventions: reads = pretty JSON, writes = compact one-line, exit 1 on error (clap uses 2),
  `--error-format json` → `{"error":{"kind","message","file"}}` via `errors.rs::tagged_err`.
- Constraints: `Cli`/`Cmd` are `pub(crate)` → parser tests live in-crate (`src/cli/dispatch.rs`
  `#[cfg(test)]`), not `tests/`. `tests/capabilities.rs` pins `version == "0.5.0"` (capabilities.rs:1589,
  with the rationale message at :1590) and asserts `SUBCOMMANDS`/`FEATURES` membership plus read-vs-write
  integrity-flag placement. **Those placement assertions iterate hardcoded 8-entry literal arrays**
  (`read_subs` at :26, `write_subs` at :62) that contain no `flow` subcommand and will contain no
  `backlog` one unless extended. `--no-create` is asserted in `tomlctl/tests/integration.rs`, not there.
  `tomlctl/src/capabilities.rs:41` (`build_agent_context`) *is* clap-reflected, so the `.commands`
  schema needs no hand edit. New `--json` flags are discouraged (R7 removed them from flow read verbs).
- Precedent for the size of a new verb group: the `flow` group (commit `bf6cf57`) touched
  4 src files + 5 test files. Layout to mirror: `ls tomlctl/src/flow/` (same pattern).

### Tests / gates

- `command_lint` (`tomlctl/src/cli/dispatch.rs:1843`) — extracts ` ```bash ` fenced lines starting
  with `tomlctl`, tokenises with `shell_words`, feeds `Cli::try_parse_from`; fails on
  `UnknownArgument` / `InvalidSubcommand`. Opt-out fence: ` ```bash ignore-command-lint `. Trailing
  `# → …` comments are already tolerated (`shell_words::split` strips POSIX comments; precedent at
  `claude/skills/tomlctl/SKILL.md:184-185`, and the test passes today).
  **Its scan set is narrower than it looks**: `claude/skills/tomlctl/SKILL.md` by *exact path*, then
  only `claude/skills/` entries whose directory name `starts_with("flow-contract-")`, then
  `claude/commands/*.md`, then `claude/agents/*.md`. It returns early if `claude/` is absent. So
  `claude/skills/{test-author,commit-conventions,documentation-conventions}/SKILL.md` are ungated
  today, as any new non-`flow-contract-` skill would be, and `tomlctl/README.md` is outside the tree
  entirely. Content moved into `claude/skills/tomlctl/references/*.md` silently loses gating too.
  The scan set is built inline inside `#[test] fn command_lint()` with the root fixed at
  `env!("CARGO_MANIFEST_DIR")` — there is no callable builder to test against a tempdir.
- `carrier_invokes_required_skills` (`dispatch.rs:2055`) — hardcoded `(carrier_md, [skill_names])` table;
  the derived `required` set also asserts each named `claude/skills/<name>/SKILL.md` is a file.
- `blocks_verify_reproduces_shell_hashes` (`dispatch.rs:1609`) reads `scripts/shared-blocks.toml` but
  then hardcodes `carriers_for("forbidden-working-tree-ops")` (:1661) and pins one hash — it does
  **not** cover a newly-added block.
- Pre-commit (`.githooks/pre-commit`, needs gawk) runs **three** scripts:
  `scripts/verify-shared-blocks.sh` (:11), `scripts/verify-plan-story-blocks.sh` (:12), and
  `scripts/doc-diff-gate.sh` (:15, added in `d26d436`). The last runs in `warn` mode
  (`MODE="${DOC_GATE_MODE:-warn}"`) so it cannot block a commit, but it reports on staged markdown.
  `scripts/shared-blocks.toml` carries **exactly one** block (`forbidden-working-tree-ops`, across
  `claude/agents/implement-{deep,lite}.md`); the optional `skill = "…"` field is unused, so
  `tomlctl blocks verify-skills` is vacuous. The verifier reads the manifest generically — no block
  names are hardcoded in the shell script.

### Existing ledger schema + conventions (to reuse, not reinvent)

- `claude/skills/flow-contract-ledger-schema/SKILL.md` `[[items]]`: required
  `id, file, line, severity, effort, category, summary, first_flagged, rounds, status`;
  optional `symbol, description, evidence[], related[], depends_on[], flow, fingerprint`;
  per-status `resolved`+`resolution` / `defer_reason`+`defer_trigger` / `wontfix_rationale` / `verified_note`.
  Fail-soft on unknown enum values (warn, coerce — never error).
- Dedupe, two contracts: **semantic** (same `file` AND (same non-empty `symbol` OR exact `summary`)) and
  **mechanical** `dedup_id` = `sha256(file|summary|severity|category|symbol)` truncated to 16 hex,
  auto-populated by every write funnel, `--dedupe-by dedup_id` opts in, kill switch `TOMLCTL_NO_DEDUP_ID=1`.
  Drift found: `dedup_id` is on every on-disk item but is **not** in the skill's documented field list.
  The tier-B hasher is not one function — `grep -n 'tier_b_fingerprint\|fn fingerprint' tomlctl/src/dedup.rs`
  returns seven entry points across separate TOML-side and JSON-side implementations, whose two
  `*_from_strs` bottoms take five positional `&str` params.
- Canonical two-call write idiom: `tomlctl items add <path> --json -` (heredoc) then
  `tomlctl set <path> last_updated <date>`. Never a bare filename.
- Repo-scoped (non-flow) stores live at `.claude/<name>.toml`: `active-flow.toml` (+ `.sha256`),
  `commit-conventions.toml` (deliberately no sidecar). Scope-keyed ledgers get a subdirectory
  (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`).
- `.claude/settings.json` pre-approves `Bash(tomlctl *)` and denies all three `--allow-outside` forms
  (three glob spellings, because one does not cover flag-at-end and flag-in-middle), so a new **verb
  under `tomlctl`** inherits the permission allow-list for free; a separate binary would not. That
  three-spelling deny is the reusable precedent for gating any new dangerous flag.

### lumina's model — what to borrow vs omit

- `Relevance` = `active | backlog | deferred | rejected` (defaults to `backlog` on create) — this **is**
  the triage axis. `Origin` = `plan|implement|review|optimise|tdd|human|none` — provenance, i.e. "which
  agent minted this". `Effort` = `s|m|l`. `Severity` = `critical|major|minor|suggestion`.
  Findings carry `confidence` (high/medium/low) and a `triage_state` (default `pending`) with
  `finding_decisions.decision ∈ spawn_task|spawn_story|defer|dismiss|resolve` and a `superseded_by` self-FK.
- **Omit**: the `kind` hierarchy + `parent_id`/`position`, `closure_gate`, acceptance criteria,
  `tier`/`complexity`/`task_kind`/`lane`, question/option links, sprints/runs/worktrees,
  research-note lifecycle. Those are exactly the "full planning detail" this store must not carry.

### Harness surface

- **The repository is PUBLIC.** `gh repo view --json visibility` returns
  `{"isPrivate":false,"visibility":"PUBLIC"}` for `RossAnder/dev-tools`, and `git ls-files .claude | wc -l`
  returns 314. There is no LFS configuration (`git config --get-regexp '^lfs'` exits 1; `.gitattributes`
  holds one `*.sql text eol=lf` line and no merge driver). Everything tracked under `.claude/` today is
  human-authored or human-reviewed; this plan is the first to write there automatically.
- `claude/` is the source of truth; `.claude/` holds runtime state + two repo-local skills.
  **No symlinks** — publication to `~/.claude/{skills,commands,agents}/` is a manual byte-copy.
  One file already drifted there: `claude/skills/flow-contract-apply-pipeline/SKILL.md`.
- Skill frontmatter in this repo uses exactly `name` + `description`. `disable-model-invocation` is
  banned **by convention**. The only mechanical gate — `scripts/verify-plan-story-blocks.sh` invariant 4 —
  sets `SKILLS="$PLUGIN/skills"` (:46) and scans ONLY `claude/plugins/lumina-story-blocks/skills/*/`,
  so `claude/skills/*` including any new skill is ungated and the convention is prose-only there.
- Only two skills use the `references/*.md` progressive-disclosure pattern
  (`commit-conventions`, `documentation-conventions`); all `flow-contract-*` skills and `tomlctl`
  are single-file. Derive the flow-contract count with `ls -d claude/skills/flow-contract-*/ | wc -l`.
- All six flow agents (`flow-bootstrap`, `implement-{deep,lite}`, `research-{deep,lite}`, `verification`)
  hold `Bash`, so any of them *could* call a new CLI verb. Five hold `Skill`; `flow-bootstrap` does not.
  Caveat: `research-*` hold Bash for "non-mutating verification only — never change the tree", and
  `verification` is "no interpretation, no retry" — minting from those two would contradict their
  stated contracts as written.
- `claude/agents/flow-bootstrap.md`'s step-2 pre-flight gate halts when the installed `tomlctl` is
  below **0.5**, with the required version spelled out in two error literals. A version bump that does
  not raise that gate leaves a stale binary passing pre-flight and failing at the new verb.

### Candidate capture points (where a tangential issue dies in prose today)

| Carrier | Location | Current disposition |
| --- | --- | --- |
| `claude/commands/implement.md` | `### Failed / Skipped` (:91), `### Plan Deviations` (:94) | free prose in the Phase-4 report — **not persisted anywhere** |
| `claude/commands/implement.md` | Phase 2 bullet **3c. Minting a task the plan lacks** (:60) | closest existing analogue; mints a plan task + `type=deviation` E-entry, forces `/plan-update reformat` |
| `claude/commands/tdd.md` | Cycle FSM REFACTOR (:65), Cycle decision (:67) | "deferred follow-ups" summarised in prose only |
| `flow-contract-apply-pipeline` | Step 6 Plan-deviation follow-up (SKILL.md:550-552) | in-scope → prose instructing "auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument `deviation`"; **out-of-scope → explicitly "do NOT auto-invoke", final summary only**. Note there is no `Skill("plan-update","deviation")` literal anywhere in that file; the only such literal is `Skill("plan-update", "status")` at :563. |
| `flow-contract-apply-constraints` | SKILL.md:12,15,16 | `skipped <id>: <reason>` strings that die in prose unless the orchestrator writes a rejected status |
| `claude/commands/{review,optimise}.md` | Step 1 prior-findings load | deferred-reopen sweep + `tomlctl items orphans` — surface-only |

**The gap**: every existing mechanism is *flow-scoped and item-scoped*. There is no store for an issue
tangential to the current ledger's item set, and no cross-flow "is this already known?" query.
`tomlctl items find-duplicates` accepts multiple ledgers (emits `source_file`) and is the nearest primitive.

**Carrier vocabularies differ** — do not assume `/review`'s syntax holds for `/optimise`.
`claude/commands/review.md` carries `defer R{n} — reason — trigger` (2 occurrences) and
`wontfix R{n} — rationale`. `claude/commands/optimise.md:39` states its disposition vocabulary is
`open` / `deferred` / `applied` / `wontapply` with `O{n}` ids and no `verified-clean` counterpart;
neither `defer R{n}` nor `wontfix` appears there at all.

### `tomlctl` skill — current state (workstream B)

`claude/skills/tomlctl/SKILL.md` — **56,096 bytes / 869 lines**, single file, no sub-files.
Next-largest skill is `flow-contract-apply-pipeline` at 36,883 B; median ≈ 8 KB.

Confirmed drift:

1. `:133` documents `{"version":"0.4.0", …}`; `tomlctl/Cargo.toml` is `0.5.0`. The same stale sample
   also sits at `tomlctl/README.md:209`.
2. `:851-859` is **one fence carrying two `blocks verify` invocations**, both dead. `:853-855` targets
   `claude/commands/{optimise,review,optimise-apply,review-apply}.md` with
   `--block flow-context --block ledger-schema`; `:858` is `tomlctl blocks verify claude/commands/*.md`.
   **No file under `claude/commands/` carries a `SHARED-BLOCK` marker any more** —
   `grep -rl 'SHARED-BLOCK' claude/` returns only the two implement agents and SKILL.md itself — and
   neither named block exists on the agent files, which carry only `forbidden-working-tree-ops`. So
   retargeting the *files* is not enough; the `--block` names must change too, and both invocations
   need fixing.
3. `:863-869` documents `blocks verify-skills` as a live drift check; it is now vacuous. The string
   `blocks verify-skills` appears **3 times** in the file (heading :863, prose :865, fence body :868).
4. `claude/skills/flow-contract-ledger-schema/SKILL.md`'s own `description` claims identical copies are
   "still embedded in the optimise/review-apply/optimise-apply carriers pending migration" — false
   (`grep -rl 'SHARED-BLOCK' claude/` returns 0 command carriers).
5. `## Advanced / maintenance` (:845) claims the `blocks` verbs are "kept for hook/script authors";
   the hooks call the **bash** scripts, not `tomlctl blocks`.

Bloat hot-spots: `### Patch an existing item` (49 L), `## Common recipes` (46 L, re-derivable),
`### Append a single new item` (42 L), 12 near-identical `--dry-run` preview blocks (one per verb),
12 `--verify-integrity` mentions plus an 18-line matrix, and cross-domain duplication with
`flow-contract-ledger-schema` (the `--array rollback_events` explanation, the `-` stdin rule,
the two-call write pattern).

**Live intra-document anchors are load-bearing.** `grep -c '](#' claude/skills/tomlctl/SKILL.md`
returns 16, of which only `#constraints-and-gotchas` and `#flow-bootstrap-agent-entrypoint` (an
explicit `<a id=…>` inside the retained Quick Reference) target sections the split keeps. The rest
point at sections the split moves out. Two cross-file deep links also exist:
`tomlctl/README.md:59` → `…/SKILL.md#query-items-full-query-surface` and
`tomlctl/README.md:61` → `…/SKILL.md#stdin-input-for-large-json-payloads`.

### Verification commands observed

See `## Verification Commands` below. Both drift gates (`command_lint`, `carrier_invokes_required_skills`)
are in-crate unit tests in the single file `tomlctl/src/cli/dispatch.rs` (:1843, :2055). `scripts/` also
holds `doc-diff-gate.sh` (invoked by the pre-commit hook) and `templates/flow-context.md`; the two
`verify-*.sh` hook verifiers are markdown-block parity checks and are unaffected by adding a CLI verb.

### Vet pass

Run per `flow-contract-vet-research`. Console lines:
`vet: Agent-1 (skill-authoring) — 4 sampled, 0 dropped, 2 downgraded`
`vet: Agent-2 (backlog prior art) — 4 sampled, 0 dropped, 1 downgraded`

### Skill authoring (Agent-1, spot-checked against the live page 2026-09-01)

- **R1 — The 500-line ceiling is official, and it is a line count.** "Keep SKILL.md body under 500 lines
  for optimal performance. If your content exceeds this, split it into separate files."; checklist item
  "SKILL.md body is under 500 lines". Grade A —
  <https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices> (§Progressive
  disclosure patterns, §Token budgets, §Checklist), verified verbatim.
  **Impact**: `claude/skills/tomlctl/SKILL.md` at 869 lines is 1.74× the ceiling — the split is
  justified by documented guidance, not taste.
- **R2 — Bundled files cost zero context until read.** "No context penalty for large files: Reference
  files, data, or documentation don't consume context tokens until actually read"; only metadata is
  pre-loaded at startup. Grade A, same page (§Runtime environment). The finer per-level token figures
  (~100 tok/skill metadata, "under 5k tokens" for the body) come from the overview page and were **not**
  re-verified — treated as directional.
  **Impact**: moving the flag reference into `references/` is a real saving, not a nominal one.
- **R3 — References must be exactly one level deep, with a ToC over 100 lines.** "Keep references one
  level deep from SKILL.md"; "For reference files longer than 100 lines, include a table of contents at
  the top." Grade A, verified verbatim. The documented pattern is domain-partitioned
  `reference/<domain>.md` linked directly from SKILL.md, plus a `grep -i` hint for search.
  **Impact**: split by verb domain; never link reference→reference; every reference file gets `## Contents`.
- **R4 — `description` ≤ 1,024 chars, third person.** "Maximum 1,024 characters"; "Always write in third
  person". Grade A, verified verbatim. Measured: the current tomlctl description is 693 chars,
  third-person, and already carries "Use this for any TOML mutation in a flow command".
  **Impact**: leave the description alone during the split.
- **R5 — Spec frontmatter is `name` + `description` (+ `license`, `compatibility`, `metadata`,
  `allowed-tools`); there is no `version` key.** Grade A for the two required fields (verified on the
  best-practices page); the wider allowed-key list and the Claude Code extension fields come from
  <https://code.claude.com/docs/en/skills> and were **not** re-verified — downgraded to B.
  **Impact**: do not introduce a `version:` key when splitting.
- **R6 — Documented anti-patterns.** The page's own "Anti-patterns to avoid" section names exactly two:
  Windows-style backslash paths, and offering too many options instead of one default + escape hatch.
  Adjacent guidance (time-sensitive statements → a collapsed `<details>` "Old patterns" block;
  consistent terminology; concrete over abstract examples; avoid deeply nested references) lives under
  §Content guidelines / §Progressive disclosure / the checklist, **not** under "Anti-patterns".
  *Downgraded*: Agent-1 attributed all of these to the anti-patterns section — content correct,
  attribution wrong. Grade A for the content.
  **Impact**: exhaustive CLI flag tables are **not** a documented anti-pattern. The documented treatment
  is "Bundle comprehensive resources … no context penalty until accessed" — so the flag tables move to
  `references/`; they do not get deleted.
- **R7 — Tooling.** `claude --version` = 2.1.252 locally (verified). `claude plugin` exists; its command
  list includes `details`, `disable`, `enable`, `eval`, `init|new`.
  *Downgraded*: Agent-1 asserted "There is no `claude plugin eval`" — **falsified**, it is present in
  2.1.252. The `claude plugin validate <dir>` claim is plausible (docs-cited) but was not confirmed in
  the truncated local help output, so the plan must confirm it before relying on it as a gate.
  Nothing official enforces the 500-line ceiling — a repo-side check is required if we want it gated.

### Backlog / issue-capture prior art (Agent-2)

- **R8 — Hash IDs, not counters, for multi-agent appends.** beads README, verified verbatim:
  "Zero Conflict: Hash-based IDs (`bd-a1b2`) prevent merge collisions in multi-agent/multi-branch
  workflows." Grade A — <https://raw.githubusercontent.com/steveyegge/beads/main/README.md>.
  **Impact**: the highest-signal finding. Every existing tomlctl ledger uses monotonic
  `R{n}`/`O{n}`/`E{n}` ids allocated by `items next-id`; two agents in two worktrees both mint `B7`, and
  the merge is a silent duplicate id rather than a conflict. Surfaced to the user as a decision.
  **Scope note**: the guarantee is that ids do not collide between *different* issues. It is not a
  claim that git will merge two branch-side appends cleanly — see Risks.
- **R9 — Typed relations beat status overloading.** beads README, verified verbatim: "Graph Links:
  `relates-to`, `duplicates`, `supersedes`, and `replies-to` for knowledge graphs." Grade A.
  **Impact**: model "already known" as an explicit `duplicate_of` field, not a `status = "duplicate"`.
- **R10 — beads' status/type vocabulary is wider than open/closed.** 7 statuses
  (`open`, `in_progress`, `blocked`, `deferred`, `closed`, `pinned`, `hooked`), 9+ types
  (`bug`, `feature`, `task`, `epic`, `chore`, `decision`, `message`, `molecule`, `gate`). Invariant:
  closed ⇒ `ClosedAt` set; non-closed ⇒ unset. Grade B (deepwiki over the repo; **not** in the README —
  confirmed absent there by direct fetch).
  **Impact**: borrow the terminal-timestamp invariant as a validator rule; do not borrow the width.
- **R11 — todo.txt: one required field, free-text tag namespaces, no status enum.** Required: the
  description. Optional `(A)` priority, creation date, `+project`, `@context`, open-ended `key:value`.
  Completion is a leading `x` plus a date. Grade A — <https://github.com/todotxt/todo.txt>.
  **Impact**: the strongest argument for free `tags[]` over a fixed `category` enum — an enum forces a
  triage decision at capture time, which is exactly what capture-first defers.
- **R12 — Backlog.md's MVP field set**: `id`, `title`, `status`, `labels`, `dependencies`, `milestone`,
  acceptance criteria; one `.md` per task; `--json` for scripts. Grade A —
  <https://raw.githubusercontent.com/MrLesk/Backlog.md/main/README.md>.
  **Impact**: confirms the intersection worth keeping — id, title, status, labels, created. Drop
  `milestone` and acceptance criteria: those are the planning-system fields this store must not carry.
- **R13 — Rot control is compaction, not expiry.** beads summarises old closed issues to save context
  ("Semantic 'memory decay' summarizes old closed tasks"). Grade A for the feature's existence (README,
  verified verbatim); *downgraded* on the exact command — Agent-2 cited `bd admin compact --days 90`
  from the FAQ and the README carries no syntax. A `last_seen` / `seen_count` bump on re-capture is
  **UNVERIFIED as prior art** — no surveyed system does it.
- **R14 — Dedupe: normalise, then hash.** The cheap standard pipeline is lowercase → strip
  punctuation/stopwords → exact hash; simhash + Hamming ≤ k only when near-duplicates are needed.
  Grade C — <https://github.com/seomoz/simhash-py>.
  **Impact**: v1 = normalised-summary SHA-256 as the dedupe key, mirroring tomlctl's existing
  `dedup_id`. Trigram Jaccard over normalised summaries as a *warning* only, never an auto-merge.
- **R15 — clap resolves to 4.6.6** (`tomlctl/Cargo.lock`, verified locally). Two claims Task 5 relies
  on: `value_delimiter` is opt-in (clap 4 does not split on commas), so prefer a repeated `--tag`
  (`Vec<String>`, implicit Append) over `num_args = 1..`, which can swallow a following positional.
  Grade A — <https://docs.rs/clap/latest/clap/_derive/_cookbook/git_derive/index.html>.
  *Trimmed*: the cookbook's `args_conflicts_with_subcommands` / `flatten_help` idiom is an `Args`
  struct combining a nested subcommand with *default* flattened args for a bare-parent invocation.
  Task 5 declares a plain nested group with no default-arg fallback, structurally identical to the
  existing `Cmd::Flow { op }` / `Cmd::Items { op }`, where neither attribute applies — follow Task 5's
  Detail, not the cookbook shape. Separately, `#[group(...)]` is an `Args`/`Parser` **struct**
  attribute; Subcommand variants take only `skip`, `flatten`, `external_subcommand`, so an `ArgGroup`
  must live on a flattened struct.

### Escalation not taken

Agent-2 returned `ESCALATE-TO-DEEP` for clustering captured items into candidate work scopes: no
documented non-ML algorithm exists in any surveyed tracker, and it "needs design reasoning, not
fetch-and-summarise". That is orchestrator work, so it is resolved in Design rather than by a second
research dispatch. Shared-file-path and shared-tag co-occurrence remain hypotheses, explicitly unsourced.

### Measured baseline for the skill split

Taken 2026-09-01 over `claude/skills/tomlctl/SKILL.md`:
**869 lines**, **56,096 bytes**, **50** ` ```bash ` fences, **86** lines beginning `tomlctl ` (all 86
unique under `sort -u`), **12** `--dry-run` mentions, **12** `--verify-integrity` mentions, **16**
`](#…)` intra-document links. (Agent-2 reported 14 `--dry-run` blocks; the measured figure is 12 — the
plan uses the measured value.) Re-derive rather than trust these at execution time; the commands are
`awk 'END{print NR}'`, `wc -c`, and `grep -c` against the paths named.

### Phase 5 outcome

Skipped. Every term the Phase 4 answers introduced — trigram, Jaccard, normalise-then-hash, `duplicates`,
`supersedes`, `relates-to`, clap `flatten` — already appears in Research Notes above. The one genuinely
uncovered topic, clustering, was explicitly escalated by Agent-2 as design reasoning rather than a
research gap, and is resolved in Approach.

## User Decisions

1. **ID scheme → hash-derived `B-a1b2c3d4`.** Prompted by R8 (beads README, verified verbatim: hash IDs
   "prevent merge collisions in multi-agent/multi-branch workflows") against the observed fact that every
   existing tomlctl ledger allocates monotonic ids via `items next-id`, and this repo runs parallel
   agents in worktrees. Accepted cost: ids are not age-sortable, and `items next-id` is bypassed for this
   store.
2. **CLI depth → full verb group including `cluster` and `compact`.** Prompted by the exploration finding
   that `items` already has a generic `--array` engine (so a zero-Rust option existed) set against
   `items add` accepting only `--json`, plus the recorded Windows heredoc unreliability.
3. **Writer → orchestrator-only, automatic at defined seams.** Prompted by the `tools:` audit: all six
   flow agents hold `Bash`, but `research-*` are contracted to "non-mutating verification only — never
   change the tree" and `verification` is "no interpretation, no retry". Sub-agents report candidates;
   the orchestrator writes.
4. **Skill rework → split into `references/` and fix the drift.** Prompted by R1 (documented 500-line
   ceiling, verified verbatim) against the measured 869 lines, and by the five confirmed drift defects.
5. **Clustering → both views, plus first-class relatedness.** The user's answer redirected the question:
   *"I'm thinking both but we must also consider facilitating 'related' and dedup capabilities for agents
   detecting issues and finding truly related items and being able to detect if the issue is already
   wholely documented and logged."* This is treated as the load-bearing requirement, not a preference
   between two clustering keys — see Approach, which promotes `backlog check` to the primary verb and
   adds a third clustering view over the relation graph.
6. **Promotion → record the link, do not generate.** Keeps the store a log rather than a second planning
   system.
7. **Checkpoint cadence → milestones.**
8. **Evidence → a per-item companion directory, and the directory is authoritative.** The user's words:
   *"If I have a UI bug that I want to store screenshots showing the bug, I would want a companion
   evidence folder for the backlog that I could store the evidence content in and link in the backlog
   record"*, refined to *"perhaps a folder named for the finding id and all added there rather than
   enumerating all items in the finding. Makes adding items manually a simpler matter."* So the
   directory IS the record: a file dropped in counts, `show` lists the directory at read time, and
   the store gains no new field at all — not a count, not a boolean, not a path, since a bare `cp`
   invalidates all three. Accepted cost: nothing is enforced at capture time, because any gate inside
   a `tomlctl` verb is bypassed by the `cp` the design endorses. The model is detection
   (`evidence audit`) rather than prevention, plus the one control git enforces regardless of how a
   file arrived — the ignore rule.

## Approach

### The store

A single repo-wide `.claude/backlog.toml`, tracked in git (`.claude/` is not gitignored — 314 files are
already tracked), following the `.claude/active-flow.toml` / `.claude/commit-conventions.toml`
precedent for non-flow-scoped state. It sits inside `io::guard_write_path`'s `<repo-root>/.claude/`
containment, so it needs no `--allow-outside` — which repo policy denies anyway — and
`.claude/settings.json` already pre-approves `Bash(tomlctl *)`, so the new verbs inherit the permission
allow-list for free.

**The repository is public** (`gh repo view --json visibility` → `PUBLIC`, checked 2026-09-01), and this
is the first store written *automatically* at five carrier seams rather than by a reviewed human edit.
Every minted `summary`, `context` and `evidence` string is therefore published. Captures must carry no
secrets, no customer data, and no verbatim error output quoting paths outside the repo; the
`backlog-capture` skill states that rule, and the orchestrator reviews minted rows before the commit
train stages them.

Item shape, one `[[items]]` row per capture. **The array is named `backlog`, not `items`** — an array
called `items` in a file under `.claude/` is the default target of `tomlctl items add|update|apply`,
whose `apply_dedup_id_on_add` (`tomlctl/src/items.rs:63`) would stamp a ledger-shaped `dedup_id` over
`FINGERPRINTED_FIELDS` and silently break the `id = "B-" + dedup_id[..8]` invariant:

```toml
[[backlog]]
id = "B-a1b2c3d4"          # "B-" + first 8 hex of dedup_id, widening on collision
kind = "flaky-test"        # bug|flaky-test|debt|direction|annoyance|question|other
summary = "pty_readiness_probe flakes on slow CI"
area = "lumina/server/tests/pty_readiness_probe.rs"   # repo-relative file or dir prefix; "" if none
tags = ["ci", "windows", "conpty"]
status = "open"            # open|promoted|dismissed|resolved
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "a1b2c3d4e5f60718"
origin = "implement"       # the command or agent that minted it
flow = "lumina-pty-service"                            # flow slug at mint time, optional
context = "Only reproduces when the readiness gate races the first prompt write."
evidence = ["lumina/server/tests/pty_readiness_probe.rs:88"]   # path:line into tracked source
related = ["B-7f0e2d91"]
```

Terminal states add their companion fields, validated before write: `promoted` → `promoted` (date) +
`promoted_to`; `dismissed` → `dismissed` (date) + `dismiss_reason`; `resolved` → `resolved` (date) +
`resolution`. `open` may carry `reopen_rationale` and nothing else terminal. The invariant borrowed
from R10 is enforced both ways — a terminal status must carry its date, and `open` must carry none of
them. `duplicate_of` and `supersedes` are separate typed fields rather than statuses, per R9. Ids are
unique across `[[backlog]]` ∪ `[[compacted]]`, enforced by the validator — see Risks for why that
matters on merge.

The taxonomy is a small fail-soft `kind` enum plus unbounded free `tags[]`. The enum is what
`--count-by kind` and bulk sweeps rely on; `other` is the coercion target so an unrecognised kind
warns rather than errors, matching the ledger schema's fail-soft rule. Anything the enum did not
anticipate lands in `tags` rather than being forced into a wrong bucket (R11).

### Identity and dedupe

`dedup_id = sha256(kind | area | normalise(summary))` truncated to 16 hex; `id = "B-" + dedup_id[..8]`,
widening to 10 then 12 hex only on a genuine collision with a *different* `dedup_id`. The widening
tie-break is a total order over `dedup_id` — the lexicographically smaller keeps the short id, the
other widens — not insertion order, because an order-dependent tie-break makes two worktrees assign
the pair in opposite directions, which is the exact divergence decision 1 exists to remove.

Because the id is derived from content, re-minting the same discovery in a second worktree produces the
*same* id. Note precisely what that does and does not buy: `.gitattributes` configures no TOML merge
driver and none is set (`git config --get-regexp 'merge\.'` is empty), so git merges this file as text
and auto-collapses only byte-identical additions. Two worktrees agreeing on `id` will still differ on
`created`, `last_seen`, `origin`, `flow` and the agent-authored `context`, so the merge yields a
conflict hunk or two rows under one id. The id-uniqueness validator rule and `check`'s `duplicate-id`
verdict are what make that visible rather than silent.

`normalise` is the R14 pipeline: lowercase → non-alphanumeric to space → collapse whitespace → drop a
pinned stopword list → single-space join. It is byte-oriented and uses `u8::to_ascii_lowercase`, never
`str::to_lowercase` — the two differ on non-ASCII input (a curly quote, an em dash, an accented
identifier) and every stored `dedup_id` is frozen on whichever is chosen. It is deterministic and its
exact definition is load-bearing — see Risks.

The existing tier-B hasher is generalised to take a field list rather than gaining a second
implementation. `dedup::FINGERPRINTED_FIELDS` and its documented byte-identity contract are left
untouched; the backlog passes its own list. This is a wider surface than it sounds: seven entry points
across separate TOML-side and JSON-side implementations, whose two `*_from_strs` bottoms take five
positional `&str` params, so taking a field list changes their arity at every site.

### Answering "is this already known?"

`backlog check` is the primary verb, not `add`. An agent that has detected something runs `check` first
and gets a graded verdict plus the matching items' stored `context`, which is the "how do I proceed
around it" half of the requirement:

| Verdict | Test | What the agent does |
| --- | --- | --- |
| `duplicate` | `dedup_id` equality | Do not mint. `backlog add` will bump `seen_count` instead. |
| `previously-resolved` | `dedup_id` hit in the `[[compacted]]` array | Do not mint; read the recorded resolution. |
| `likely-duplicate` | char-trigram Jaccard ≥ 0.75 | Read the candidate; mint only if genuinely distinct. |
| `related` | word Jaccard ≥ 0.35, **or** ≥2 shared `area` path components, **or** ≥2 shared tags | Mint, and pass `--related <id>` so the edge is recorded. |
| `novel` | none of the above | Mint. |

Both thresholds are named constants with a written justification and a CLI override, not bare numbers.
`check` resolves the `duplicate`, `previously-resolved` and `duplicate-id` verdicts through a single
`dedup_id`-keyed map built once per invocation over both arrays, so only the fallback similarity path
pays per-candidate trigram cost; it scans the `[[compacted]]` array as well as `[[backlog]]`, so folding
an old resolved item away never loses the "we already solved this" answer.

`add --on-duplicate bump` (the default) is the second half: re-detecting a known issue increments
`seen_count`, refreshes `last_seen`, and unions `tags`/`evidence` without touching `summary` or
`status`. That turns repeat detection into a usable signal — an item seen nine times is real, one seen
once eight months ago is noise — which is also the rot-control mechanism (R13 documents compaction but
nothing surveyed does re-confirmation counting, so this is our own design, flagged as such).

### Relations and clustering

`relate <a> --to <b> --as {relates-to|duplicates|supersedes}` writes typed edges: `relates-to` is
symmetric; `duplicates` sets `a.duplicate_of` and dismisses `a` with the companion reason; `supersedes`
sets `a.supersedes` and dismisses `b`.

`cluster --by all` (the default) emits three independent views rather than one blended score:

- **area** — longest common repo-path prefix, collapsing upward until a group reaches `--min-size`.
  Deterministic, explainable, and maps onto how a work session is actually scoped.
- **tags** — items sharing ≥ `--min-shared-tags` tags, with overlapping groups merged transitively.
  Catches cross-cutting themes a path prefix misses.
- **relations** — connected components over the `related` / `duplicate_of` / `supersedes` edge set.
  This view only exists because decision 5 made relations first-class.

Each group carries `{key, reason, size, item_ids, kinds, areas}` so the output is directly usable as a
scope proposal for `/plan-new`. No documented prior art exists for any of this (Agent-2's escalation) —
these are chosen for explainability over cleverness, and none of them needs tuning to be useful.

### Evidence artefacts

`evidence[]` holds `path:line` pointers into tracked source. A screenshot of a misaligned checkout
button, a flamegraph, a captured stdout log, a HAR from a failing request — none of those are
pointers, and none can live in a TOML field.

**The directory is the record.** `.claude/backlog-evidence/<item-id>/` is the evidence set for that
item. Drop files in; that is the whole interface. Nothing in `.claude/backlog.toml` enumerates them,
because the moment a record lists filenames, the plain `cp` that makes this feature worth having
becomes a two-step operation that silently drifts on step one being skipped. A per-file row with a
digest and a caption is wrong the instant someone does the thing we are asking them to do, and a
stale digest is worse than no digest. So the item gains **no new fields at all** — not a count, not
a boolean, not an `evidence_dir` path. All three would be invalidated by a bare `cp`; the count and
the boolean by an addition, and the path by being derivable. `backlog show` lists the directory at
read time instead, which is always right by construction.

The corollary is that **the filename is the caption.** It is the only per-artefact metadata a manual
drop can carry, and the only one that cannot go stale, so `backlog-capture` teaches
`checkout-total-overlap-1280.png` over `Screenshot 2026-09-01 at 14.02.png`.

**Naming a specific artefact inline is supported and expected** — what is rejected is a *maintained
index*, not a reference. A bare filename appearing in `context` prose or as an `evidence[]` entry
(`checkout-total-overlap-1280.png`, resolved relative to the item's evidence directory, as distinct
from the `path:line` form which is a pointer into tracked source) says "this file shows the thing I
am describing" at the point where it is useful. That is not bookkeeping: it is written once by the
author who is looking at the file, it sits beside the sentence it belongs to, and adding a second
file later does not oblige anyone to update it. A count would be wrong after the next `cp`; a
sentence saying "the overlap is visible in `checkout-total-overlap-1280.png`" stays true. The one
way such a reference can rot is the file being renamed or removed, which `evidence audit` reports as
`referenced-missing` — cheap to check precisely because a reference is a filename and the directory
listing is a filename set.

The directory sits under `.claude/`, so `io::guard_write_path` (`tomlctl/src/io.rs:929`) admits it
with no `--allow-outside` — which repo policy denies anyway — and `ensure_parent_under_claude`
(`tomlctl/src/io.rs:985`) already `mkdir -p`s missing intermediates whose nearest existing ancestor
is under `.claude/`, so a single guarded write both creates and validates the path.

**Contents ignored, directory tracked.** This repository is public (`gh repo view --json visibility`
→ `PUBLIC`, 314 files already tracked under `.claude/`) with no LFS configured
(`git config --get-regexp '^lfs'` exits 1; `.gitattributes` holds one `*.sql text eol=lf` line). A
tracked screenshot is published irrevocably into every clone forever, and a HAR publishes
`Authorization` headers and session cookies verbatim — so the contents are ignored by default. But
an ignored directory is invisible to the next clone, and the store's whole value is cross-session
recall, so each directory carries one tracked four-line marker file, `.evidence`, written once at
creation and never updated:

```gitignore
/.claude/backlog-evidence/*/*
!/.claude/backlog-evidence/*/.evidence
```

Verified 2026-09-01 in a throwaway `git init`: under those two rules `git check-ignore -q` reports
`B-a1b2c3d4/shot.png` ignored and `B-a1b2c3d4/.evidence` trackable, and `git add -A` stages only the
marker. The marker is the entirety of the tracked footprint. It costs one file per item that has
evidence at all, it is written by `tomlctl` rather than by hand, and a later `cp` never touches it:

```text
B-a1b2c3d4  checkout total overlaps the confirm button below 1400px
Evidence for this backlog item. Files here are git-ignored by default. Publish
one deliberately with `git add -f <file>`, after checking it for credentials,
personal data and session tokens. This repository is public.
```

Its summary line cannot go stale: `add --on-duplicate bump` leaves `summary` untouched by contract.

Publication is `git add -f`, not a `tomlctl` flag. `-f` is git's own existing "I know this is
ignored" override, it is universally understood, and it is not covered by the pre-approved
`Bash(tomlctl *)` — so an agent cannot reach it without a prompt, while a human in their own
terminal can. That is strictly better than inventing a `--publish` flag and then denying it.

**`show` reports three states, and only one of them is a finding.** Directory absent — the common
case, most items have no evidence, reported as `evidence: null` and never flagged. Directory present
with files — `evidence: {dir, files: [{name, bytes}]}`. Directory present with only the marker —
`evidence: {dir, files: []}`, which is genuinely ambiguous between "another clone holds these" and
"they were deleted", and deliberately not disambiguated, because the reader's action is identical
either way: the bytes are not here, ask whoever captured them. The marker is what makes that state
distinguishable from "no evidence was ever captured", which is the only distinction that changes
what a reader does.

**Auditing runs the other way round.** `items orphans` (`tomlctl/src/orphans.rs:3-8`) walks the
*store* and asks whether each `file` still exists. That model inverts here: every file in an
evidence directory is unregistered by design, so "a file nobody registered" is not an orphan, it is
the only way evidence exists. `backlog evidence audit` therefore walks the *filesystem* and asks
whether the store owns each directory, plus questions about policy and about inline references that
no registration could answer. Seven classes: `unowned` (a directory whose name matches no id in
`[[backlog]]` or `[[compacted]]` — the sole true orphan, and the one a typo'd hand-created path
produces); `no-marker` (files but no `.evidence`, so the directory is invisible cross-clone);
`oversize`; `disallowed-extension`; `referenced-missing` (an item's `context` or `evidence[]` names
a bare filename that is not in its directory — the one way an inline reference rots); `tracked` (a
file `git check-ignore` says is *not* ignored — always reported, never a failure, because it
doubles as the pre-push review of exactly what is about to become public); and `empty` (marker only
— informational, the expected state in a fresh clone). `--strict` exits 1 on the first five and
never on the last two. Note `referenced-missing` fires only when the directory exists: an item whose
prose names a file in a clone that never received the bytes is the `empty` case, not a defect.

**Two ops, both about the directory, neither of them a copy.** `backlog evidence dir <id>` resolves
the id against the store, creates the directory and its marker if absent, and prints the path.
`backlog evidence audit` is the read-only scan. There is deliberately no copy verb: with a drop-box
directory, any allowlist enforced at copy time is bypassed by the `cp` we are explicitly endorsing,
so pretending to enforce it would be self-deceiving. The controls that survive are the ones git
enforces regardless of how a file arrived — the ignore rule — plus after-the-fact detection. There
is likewise no prune verb: `rm -rf .claude/backlog-evidence/B-a1b2c3d4/` is a command every user
already has, and a delete verb whose target directory name comes from agent-influenced input would
be the highest-blast-radius thing in the group for no gain.

`evidence dir` earns its place on one argument. Ids widen: `derive_id` goes 8→10→12 hex on
collision, so the correct directory for an item may be `B-a1b2c3d4e5`, and a human who eyeballs the
8-hex prefix from a `list` output and `cd`s into `B-a1b2c3d4` creates a directory owned by nobody
that `audit` will later report as `unowned`. Resolving the id against the store, rather than
deriving the path by hand, is the entire job.

**Widening cannot strip a directory of its owner.** Task 4's rule widens the *incoming* item when an
existing item already holds that id with a different `dedup_id`; the item already on disk keeps its
id unchanged. So a directory is never renamed and there is no rename path to specify — the hazard is
entirely on the creation side, and `evidence dir`'s store lookup is the mitigation.
`relate --as duplicates` and `--as supersedes` dismiss an item but do not delete it, so its id stays
in `[[backlog]]` and its directory stays owned; nothing moves. `show` on the survivor already
renders its one-hop neighbourhood (Task 8), and each neighbour carries its own evidence listing, so
the dismissed duplicate's screenshots remain one hop away. `compact` preserves `id` in the
`[[compacted]]` row for Task 7's benefit, and `audit` reads both arrays, so a compacted item's
directory stays owned too.

**Windows.** Directory names are `B-` plus 8-12 lowercase hex characters, derived wholly from the
id — no sanitisation is needed, no two can collide case-insensitively, none can equal a reserved
device name, and the longest path is 45 characters from the repo root. Filenames inside are the
author's; the skill recommends lowercase-hyphen ASCII and warns that `: ? * " < > |` and trailing
dots or spaces cannot be checked out on Windows, which matters only for a file about to be
published — which is exactly what the `tracked` class surfaces.

### Where captures come from

Sub-agents never write the store. `implement-deep` and `implement-lite` gain a shared block instructing
them to report tangential discoveries under a fixed heading in their return payload; the orchestrator
runs `check` then `add`. That preserves the `research-*` and `verification` agents' read-only contracts
verbatim, keeps all writes serialised on one process, and matches the existing "lead is sole committer"
pattern. Seams wired: `/implement` Phase 4, `/tdd`'s REFACTOR follow-ups, `/review` and `/optimise`
Step 1 and Step 3/4, and the apply-pipeline's Step 6 out-of-scope branch.

### The skill split

`claude/skills/tomlctl/SKILL.md` goes from 869 lines to a navigational body under 500, with the full
reference surface moved verbatim into four `references/*.md` files, each linked exactly once from
SKILL.md and never from each other (R3), each with a `## Contents` ToC. The flag tables are *moved,
not deleted* — R6 is explicit that comprehensive reference material belongs in bundled files, which
cost nothing until read (R2). The 12 near-identical `--dry-run` preview blocks collapse to one section
plus a one-line mention per verb. The `description` frontmatter is left alone (693 chars, third person,
already compliant) and no `version:` key is introduced (R5).

Two hazards, both encoded as task ordering. First, `command_lint` pushes
`claude/skills/tomlctl/SKILL.md` by exact path (`tomlctl/src/cli/dispatch.rs:1859`) and otherwise reads
only `claude/skills/flow-contract-*/SKILL.md`, `claude/commands/*.md` and `claude/agents/*.md` — so
moving fences into `references/` would silently drop them out of CLI-drift gating, and a new
non-`flow-contract-` skill is never gated at all. The scan set is therefore widened **before** anything
moves and **before** the new skill is authored. Second, the split moves the targets of 13 of the file's
16 intra-document anchors and of both `tomlctl/README.md` deep links; every one is retargeted or
demoted to prose in the same milestone, and a heading-conservation check on the destructive task is
what proves nothing was dropped rather than moved.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

`cargo test` is what runs the three drift gates — `command_lint`, `carrier_invokes_required_skills` and
`blocks_verify_reproduces_shell_hashes` are in-crate unit tests in `tomlctl/src/cli/dispatch.rs`
(:1843, :2055, :1609), not CI-only scripts. Prefix a full verification pass with `CARGO_INCREMENTAL=0`
per the repo's build-tuning note.

Additional checks, not keyed above because they are not the build/test/lint triple:

- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC check.
- `bash scripts/verify-shared-blocks.sh` and `bash scripts/verify-plan-story-blocks.sh` — the two
  markdown-parity hook verifiers. `.githooks/pre-commit` also runs `scripts/doc-diff-gate.sh`, which is
  in `warn` mode and cannot block, but which will report on the several hundred staged markdown lines
  Milestone 3 adds — expect the noise rather than diagnosing it mid-run.
- Skill size gate: `awk 'END{print NR}' claude/skills/tomlctl/SKILL.md` must print ≤ 500, and the same
  for `claude/skills/backlog-capture/SKILL.md`. Nothing official enforces the ceiling (R7), so Task 21
  adds a repo-side latch alongside its `command_lint` edit rather than leaving this a manual assertion;
  `claude plugin validate <path>` exists in 2.1.252 but its help text describes it as validating "a
  plugin or marketplace", so do **not** rely on it as a bare skills-directory gate.
- Round-trip smoke, run by hand in a scratch clone (it writes `.claude/backlog.toml`):
  `tomlctl backlog check --summary "flaky pty readiness probe"` → expect `novel`;
  `tomlctl backlog add --summary "flaky pty readiness probe" --kind flaky-test --area lumina/server/tests/`
  → expect `action:"added"`; re-run the same `add` → expect `action:"bumped"` and `seen_count:2`;
  `tomlctl backlog evidence dir <id>` then `cp shot.png "$(that path)"` then
  `tomlctl backlog show <id>` → expect `shot.png` in `files` and the `.evidence` marker excluded;
  `tomlctl backlog cluster --by all` → expect three keyed views.
- **Manual publication step, not a task**: `claude/` is the source of truth and publication to
  `~/.claude/{skills,commands,agents}/` is a manual byte-copy. After this plan lands, copy every changed
  file across. Note `claude/skills/flow-contract-apply-pipeline/SKILL.md` is *already* byte-divergent
  from its published copy, independent of this work.

## Execution Policy

- **Checkpoints**: `milestones` — four, each a valid topological cut and a buildable increment.
- **Checkpoint after**: tasks 2, 4, 5, 27, 28; task 14; tasks 22, 23; tasks 16, 17, 18, 19, 26.
  A marker closes on the *dependency closure* of each task it names, so every group-closing task is
  named explicitly. Tasks 16-19 are graph sinks off Task 15 and lie in no other task's closure; Tasks
  22 and 23 close the reference-extraction cut, which is the last state at which
  `claude/skills/tomlctl/SKILL.md` is still intact and every reference file exists — the destructive
  Task 24 then lands against a gate-verified tip.
- **Max parallel agents**: 8 — Milestone 2's eight file-disjoint leaves (Tasks 6-12, 29) dispatch in
  one frontier, which is exactly the contract ceiling.
- **Commit granularity**: `per-task`.

## Tasks

**Milestone 1 — crate foundations**

### 1. Create the backlog module skeleton [L]
- **Files**: `tomlctl/src/main.rs`, `tomlctl/src/backlog/mod.rs`, `tomlctl/src/backlog/schema.rs`,
  `tomlctl/src/backlog/normalise.rs`, `tomlctl/src/backlog/ids.rs`, `tomlctl/src/backlog/dispatch.rs`,
  `tomlctl/src/backlog/add.rs`, `tomlctl/src/backlog/check.rs`, `tomlctl/src/backlog/query.rs`,
  `tomlctl/src/backlog/relate.rs`, `tomlctl/src/backlog/triage.rs`, `tomlctl/src/backlog/cluster.rs`,
  `tomlctl/src/backlog/compact.rs`, `tomlctl/src/backlog/evidence.rs`,
  `tomlctl/src/backlog/evidence_ops.rs`, `tomlctl/src/cli/dispatch.rs`
- **Depends on**: none
- **Action**: add `mod backlog;` to `tomlctl/src/main.rs`; create `tomlctl/src/backlog/mod.rs`
  declaring all thirteen leaf modules; create every other listed `backlog/` file containing only its
  module doc comment. In `tomlctl/src/cli/dispatch.rs`, widen `read_json_arg` (:135),
  `on_missing_for` (:323), `warn_if_created` (:345), `write_envelope` (:372) and
  `query_input_from_cli` (:424) from bare `fn` to `pub(crate) fn` — nothing else in that file changes.
- **Detail**: this is deliberately one task despite the file count — a module declared in `mod.rs`
  without its file is E0583, a leaf file not declared in `mod.rs` is never compiled at all, and the
  five helpers above are needed by Tasks 6, 8, 9, 10 and 12, none of which can own
  `tomlctl/src/cli/dispatch.rs` without colliding with Tasks 13, 16 and 21. Doing the visibility
  widening once, here, is what keeps the leaf frontier file-disjoint. Note `crate::flow` uses none of
  those helpers, so the `flow` group is not a usable precedent for the auto-create path. Every listed
  file other than `tomlctl/src/main.rs`, `tomlctl/src/backlog/mod.rs` and `tomlctl/src/cli/dispatch.rs`
  is an empty placeholder; the real content lands in Tasks 2, 3, 4, 6-12, 13, 28 and 29, each
  owning exactly one of them. Mirror the layout of `tomlctl/src/flow/` (`ls tomlctl/src/flow/`, same
  pattern).
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` exits 0;
  `tomlctl/src/backlog/` contains exactly the fourteen listed `.rs` files; and
  `grep -c 'pub(crate) fn on_missing_for\|pub(crate) fn write_envelope\|pub(crate) fn read_json_arg\|pub(crate) fn warn_if_created\|pub(crate) fn query_input_from_cli' tomlctl/src/cli/dispatch.rs`
  returns 5 (it returns 0 before the change).

### 2. Implement the backlog schema vocabulary and validator [M]
- **Files**: `tomlctl/src/backlog/schema.rs`
- **Depends on**: 1
- **Action**: define the field-name constants (including the array names `backlog`, `compacted` and
  `artefacts`), the `kind` vocabulary (`bug|flaky-test|debt|direction|annoyance|question|other`), the
  `status` vocabulary (`open|promoted|dismissed|resolved`), the per-status required-field clusters,
  `COMPACTED_FIELDS`, `validate(&JsonValue) -> Result<(), BacklogError>`, and the shared
  `backlog_path() -> PathBuf` helper resolving `io::repo_or_cwd_root().join(".claude/backlog.toml")`,
  mirroring `tomlctl/src/flow/active.rs:57`. Every other leaf calls those helpers rather than
  rebuilding the path or re-deriving a field list.
- **Detail**: mirror the shape of `items::Item::validate` (`tomlctl/src/items.rs:1767`) — a
  status → required-fields match — but write a separate validator rather than widening that one, which
  documents itself as mirroring `claude/commands/review.md`'s disposition table. Enforce the invariant
  both directions: a terminal status must carry its date field non-empty, and `open` must carry none of
  `promoted`/`dismissed`/`resolved` (it may carry `reopen_rationale`, which Task 10 writes and Task 5
  supplies via a required `--rationale` on `--reopen`). Enforce id-uniqueness across `[[backlog]]` ∪
  `[[compacted]]`. `COMPACTED_FIELDS` is
  `{id, dedup_id, summary, kind, area, status, terminal_date, terminal_reason, context, compacted_on}` —
  `dedup_id` and `context` are there because Task 7's `previously-resolved` verdict reads them, and
  pinning the list here is what stops Tasks 7 and 12 disagreeing about the row shape. **No evidence
  field is added here** — not a count, not a boolean, not a directory path; the directory is the
  record and `show` lists it at read time, so any stored copy would be wrong after the next `cp`. If
  a later revision reaches for `evidence_count` in this file, that is the signal the design has
  slipped back into enumeration. Unknown `kind` coerces to `other` with a stderr warning, never an
  error, matching the ledger schema's fail-soft rule. Reuse `items::is_empty_json` semantics for
  "missing".
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::schema` passes, with in-file
  cases covering each status's required cluster; rejection of an `open` item carrying a `resolved`
  date; rejection of `resolved` without `resolution`; acceptance of an `open` item carrying
  `reopen_rationale`; rejection of two rows sharing an `id` across `[[backlog]]` and `[[compacted]]`;
  and unknown-kind coercion returning `Ok` with `other`.

### 3. Implement summary normalisation and similarity scoring [M]
- **Files**: `tomlctl/src/backlog/normalise.rs`
- **Depends on**: 1
- **Action**: implement `normalise(&str) -> String`, `word_tokens(&str) -> BTreeSet<String>`,
  `char_trigrams(&str) -> BTreeSet<String>`, and `jaccard(&BTreeSet, &BTreeSet) -> f64`. Pin the
  stopword list as a documented `const`. Expose `SIMILARITY_STRONG = 0.75` (char-trigram) and
  `SIMILARITY_RELATED = 0.35` (word) as named constants.
- **Detail**: the R14 pipeline — lowercase, replace every non-alphanumeric byte with a space, collapse
  runs of whitespace, drop stopwords, join with single spaces. The pipeline is byte-oriented and must
  use `u8::to_ascii_lowercase`, never `str::to_lowercase`: the two differ on non-ASCII input and every
  stored `dedup_id` is frozen on the choice. No regex is used; the crate's `regex` dependency is in any
  case compiled without Unicode classes (`tomlctl/Cargo.toml:43`). Every constant carries a comment
  justifying its value — these two thresholds decide whether an agent mints a duplicate.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::normalise` passes, with cases
  asserting `normalise(normalise(x)) == normalise(x)`; that three punctuation/case/whitespace variants
  of one summary normalise to the same string; that a summary containing a non-ASCII character (a
  curly apostrophe) normalises identically under repeated application, pinning the ASCII path;
  `jaccard(a, a) == 1.0`; and `jaccard` of two disjoint sets `== 0.0`.

### 4. Implement content-derived ids and the backlog fingerprint [M]
- **Files**: `tomlctl/src/backlog/ids.rs`, `tomlctl/src/dedup.rs`
- **Depends on**: 1, 3
- **Action**: in `tomlctl/src/dedup.rs`, generalise the tier-B hasher into a function taking an explicit
  field list, leaving `FINGERPRINTED_FIELDS` and its byte-identity contract unchanged. In
  `tomlctl/src/backlog/ids.rs`, add `BACKLOG_FINGERPRINT_FIELDS = ["kind", "area", "summary"]` where
  `summary` is hashed through `normalise`, plus `dedup_id(item) -> String` (16 hex) and
  `derive_id(dedup_id, existing) -> String` returning `"B-" + &dedup_id[..8]`, widening to 10 then 12
  hex when an existing item already holds that id with a *different* `dedup_id`.
- **Detail**: the surface is wider than one function — seven entry points across separate TOML-side and
  JSON-side implementations (`tier_b_fingerprint` :179, `_table` :200, `_json` :221,
  `fingerprint_from_strs` :239, `fingerprint_bytes_from_strs` :251, `tier_b_fingerprint_bytes` :285,
  `tier_b_fingerprint_bytes_json` :659), and the two `*_from_strs` bottoms take five positional `&str`
  params, so taking a field list changes their arity at every site. The comment on
  `FINGERPRINTED_FIELDS` (`tomlctl/src/dedup.rs:38`) states the field order is load-bearing for
  byte-identity with pre-refactor tier-B output — preserve it exactly; the ledger path keeps calling
  with the same list in the same order. The widening tie-break is a total order over `dedup_id` (the
  lexicographically smaller keeps the short id), never insertion order, so two worktrees resolve a
  collision identically.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml dedup` and
  `cargo test --manifest-path tomlctl/Cargo.toml backlog::ids` both pass, and the five existing tier-B
  tests (`tomlctl/src/dedup.rs:824, :873, :897, :922, :949`) still pass unmodified. Cases: the tier-B
  fingerprint for a fixture ledger item equals the hex captured from HEAD **before** editing — for
  `file="src/a.rs" summary="dup-summary" severity="warning" category="quality" symbol=""` that is
  `95953a6bf4f9bfb7` (`printf 'src/a.rs|dup-summary|warning|quality|' | sha256sum | cut -c1-16`,
  measured 2026-09-01), plus one real on-disk `dedup_id` re-derived from an existing
  `.claude/**/*.toml` row; two summaries differing only in punctuation and case yield the same
  `dedup_id` and the same id; a forced 8-hex collision against a different `dedup_id` widens to 10, and
  widens the same way regardless of which of the two is inserted first.

### 5. Declare the `backlog` CLI surface [L]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/backlog/dispatch.rs`,
  `tomlctl/tests/capabilities.rs`
- **Depends on**: 1
- **Action**: add `Backlog { #[command(subcommand)] op: BacklogOp }` to `Cmd`; define `BacklogOp` with
  `Add`, `Check`, `List`, `Show`, `Relate`, `Triage`, `Cluster`, `Compact`, and
  `Evidence { #[command(subcommand)] op: EvidenceOp }` with `Dir` and `Audit`, and their flags;
  add the `Cmd::Backlog { op } => crate::backlog::dispatch(op)?` arm to `run`
  (`tomlctl/src/cli/dispatch.rs:471`) with a `bail!`-stub `pub(crate) fn dispatch` in
  `tomlctl/src/backlog/dispatch.rs`; add `"backlog"` to `SUBCOMMANDS` and `backlog_capture`,
  `backlog_check`, `backlog_cluster`, `backlog_compact`, `backlog_evidence` to `FEATURES`; update the
  exhaustive expected list and both integrity-placement arrays in `tomlctl/tests/capabilities.rs`.
- **Detail**: **the variant and its match arm must land together.** `run`'s match on `cli.cmd` is
  exhaustive with 12 arms and no `_` catch-all, so a `Cmd` variant without an arm is E0004 and every
  later task's `cargo test` acceptance breaks; Task 13 replaces the stub with the real fan-out. Flatten
  `WriteIntegrityArgs` onto `Add`/`Relate`/`Triage`/`Compact` and `ReadIntegrityArgs` onto
  `Check`/`List`/`Show`/`Cluster` and `Evidence`'s `Audit`. Flatten `QueryArgs` onto `List`.
  `Evidence`'s `Dir` takes **neither** bundle: it writes no TOML, so it has no sidecar to refresh and
  no document to verify, and giving it `WriteIntegrityArgs` would hand it `--allow-outside`, the one
  flag repo policy denies. It takes `<id>` plus `--no-create`; `Audit` takes `--strict` and
  `--max-bytes`. The two placement guards (`tomlctl/tests/capabilities.rs:26` `read_subs` and `:62`
  `write_subs`) iterate hardcoded 8-entry path arrays containing no `flow` subcommand — a new
  subcommand trips neither by default, so add `["backlog","evidence","audit","--help"]` and the other
  new read paths to `read_subs` in this task or they are ungated; `--no-create` coverage lives in
  `tomlctl/tests/integration.rs`, not there. Use repeated `--tag` / `--evidence` / `--related` as
  `Vec<String>` (implicit Append) rather than `value_delimiter`, per R15 — a `num_args` range can
  swallow a following positional. Name `check`'s threshold overrides `--similarity-strong` and
  `--similarity-related`, never `--related`, which is already an id list on `add`. Declare `triage`'s
  four mode flags in a dedicated `#[derive(Args)] #[group(required = true, multiple = false)] struct
  TriageMode` and `#[command(flatten)]` it in — `#[group(...)]` is an `Args`/`Parser` struct attribute,
  not a Subcommand-variant attribute — with `--rationale` required on `--reopen`. Do **not** add an
  `--evidence-file` flag to `backlog add`, any copy flag anywhere, or a `--json` output flag; output
  is always JSON, and evidence arrives by `cp`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes;
  `cargo run --manifest-path tomlctl/Cargo.toml -- backlog --help` lists all nine ops and
  `backlog evidence --help` lists two; and `cargo build --manifest-path tomlctl/Cargo.toml` succeeds
  (it fails with E0004 if the match arm is omitted). **Measured break set** (checked 2026-09-01, not
  predicted): `tomlctl/tests/capabilities.rs` carries an exhaustive list "duplicated from
  `cli::FEATURES`" around :1527 ending in `assert_eq!(features.len(), expected.len())` at :1560, which
  fails on any `FEATURES` addition, and a literal `"0.5.0"` assertion at :1589 which does **not** fail
  here (the version bump is Task 25). `tomlctl/tests/integration.rs` contains **no** subcommand-
  enumeration assertion, despite the comment at `tomlctl/src/cli/types.rs:54` claiming one — do not go
  looking for it. Any additional failure is stop-and-report.

### 27. Add the evidence-directory ignore rules [S]
- **Files**: `.gitignore`
- **Depends on**: none
- **Action**: append the two rules that ignore an evidence directory's contents while keeping the
  directory itself tracked through its marker file, under a comment naming why (a public repo with
  no LFS): `/.claude/backlog-evidence/*/*` then `!/.claude/backlog-evidence/*/.evidence`.
- **Detail**: order and anchoring both matter. The exclude pattern has four segments, so it matches
  files *inside* an item directory but not the item directory itself — git only permits re-including
  a path whose parent directory is not excluded, and this is what makes the negation legal. The
  leading `/` anchors both at the repo root so a nested `.claude/` in a worktree is unaffected. Land
  this before Task 29 creates a directory; landing it after leaves a window in which a captured
  screenshot is stageable. No `.claude/settings.json` change is needed: `evidence dir` and
  `evidence audit` are both harmless under the existing `Bash(tomlctl *)` allow entry, and the
  publication path is `git add -f`, which is not pre-approved for agents at all.
- **Acceptance**: from the repo root,
  `git check-ignore -q .claude/backlog-evidence/B-a1b2c3d4/shot.png` exits 0 and
  `git check-ignore -q .claude/backlog-evidence/B-a1b2c3d4/.evidence` exits 1. Both exit 1 before
  the change — measured 2026-09-01 — so the first assertion fails if the rules are missing and the
  second fails if the negation is dropped or mis-anchored.

### 28. Implement the evidence directory policy module [M]
- **Files**: `tomlctl/src/backlog/evidence.rs`
- **Depends on**: 1, 2
- **Action**: implement the pure resolution and policy surface — `EVIDENCE_ROOT_NAME`,
  `MARKER_NAME = ".evidence"`, `EVIDENCE_EXTENSIONS: &[&str]`, `EVIDENCE_MAX_BYTES` (2 MiB);
  `evidence_root() -> PathBuf`; `dir_for(item_id) -> PathBuf`;
  `resolve_id(&TomlValue, &str) -> Result<String>`; `marker_text(id, summary) -> String`;
  `list_dir(&Path) -> Result<Option<Vec<(String, u64)>>>` returning `None` for an absent directory
  and `Some(files)` — marker excluded — otherwise; and
  `referenced_names(item) -> BTreeSet<String>`, extracting bare filenames (no `/`, no `:line` suffix)
  from an item's `context` and `evidence[]` so `audit` can check them.
- **Detail**: `evidence_root` is `schema::backlog_path()`'s parent joined with the root name (Task 2),
  never a rebuilt path. `resolve_id` looks the id up in both the `backlog` and `compacted` arrays and
  errors `kind=not_found` on a miss, which is the whole reason `evidence dir` exists — ids widen
  8→10→12 hex (Task 4), so a hand-derived path is silently wrong while a resolved one cannot be.
  `list_dir` distinguishing absent from empty is what lets `show` report "no evidence" separately
  from "captured, not in this clone"; conflating them loses the only distinction that changes a
  reader's action. `marker_text` emits the four fixed lines; it is written once at directory creation
  and never rewritten, so the summary it embeds is safe — `add --on-duplicate bump` leaves `summary`
  untouched by contract. `referenced_names` is deliberately conservative: a token containing `/` is a
  repo path, and one matching `…:<digits>` is the `path:line` source-pointer form, so neither is an
  evidence reference. `EVIDENCE_EXTENSIONS` is advisory only — nothing enforces it at write time,
  because the endorsed capture path is a plain `cp` — and exists solely so `audit` can flag a `.pem`
  or an extensionless file sitting in an evidence directory.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::evidence` passes, with cases
  asserting `dir_for("B-a1b2c3d4")` ends `.claude/backlog-evidence/B-a1b2c3d4` and contains no
  backslash; `resolve_id` returning `Ok` for an id present only in `[[compacted]]` and `Err` with
  `kind=not_found` for an absent one; `list_dir` returning `None` for a missing path, `Some(vec![])`
  for a directory holding only `.evidence`, and a one-entry vector excluding the marker for a
  directory holding the marker plus one file; `marker_text` beginning with the item id and containing
  the literal `git add -f`; and `referenced_names` extracting `shot.png` from a `context` sentence
  while ignoring both `src/a.rs:88` and `lumina/web/x.vue`.

<!-- CHECKPOINT 1 — foundations compile and unit-test; evidence ignore rules in place -->

**Milestone 2 — the verb group**

### 6. Implement `backlog add` [M]
- **Files**: `tomlctl/src/backlog/add.rs`
- **Depends on**: 2, 4, 5
- **Action**: build an item from the flags, compute `dedup_id` and `id`, set `created`/`last_seen` to
  today and `seen_count = 1`, validate, and write through `io::mutate_doc` with `on_missing_for` so the
  file auto-creates; bump the document's `last_updated`. Implement
  `--on-duplicate {bump|skip|fail|add}` (default `bump`). Accept `--json -` as an alternative full
  payload. Support `--dry-run`.
- **Detail**: `bump` increments `seen_count`, sets `last_seen`, and unions `tags` and `evidence`, leaving
  `summary` and `status` untouched; it emits `{"ok":true,"action":"bumped","id":…,"seen_count":N}` while
  a fresh mint emits `"action":"added"`. Resolve the path with `schema::backlog_path()` (Task 2) and
  target the `backlog` array, not `items`. The write-path helpers this needs — `on_missing_for`,
  `read_json_arg`, `write_envelope` — were made `pub(crate)` by Task 1; do not re-widen them here.
  Route `--dry-run` through the existing `compute_*_mutation` / `build_dry_run_plan_envelope` pattern so
  the file and sidecar stay byte-identical.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::add` passes, with cases for a
  fresh mint returning `action:"added"`; the identical summary re-minted returning `action:"bumped"` with
  `seen_count == 2` and a unioned tag set; `--on-duplicate fail` erroring with `kind=validation`; and
  `--dry-run` leaving the document unchanged.

### 7. Implement `backlog check` [M]
- **Files**: `tomlctl/src/backlog/check.rs`
- **Depends on**: 2, 3, 4, 5
- **Action**: implement the read-only pre-mint gate. Take `--summary` (required) plus optional `--area`,
  `--kind`, `--tag`, and rank every item in both the `backlog` and `compacted` arrays, emitting
  `{"verdict":…,"candidates":[…]}` sorted by score descending and capped by `--limit` (default 5).
- **Detail**: verdict ladder exactly as tabulated in Approach — `duplicate` on `dedup_id` equality;
  `previously-resolved` on a `dedup_id` hit inside `[[compacted]]`; `duplicate-id` when two stored rows
  share an `id` with different `dedup_id`s (the merge artefact named in Risks); `likely-duplicate` at
  char-trigram Jaccard ≥ `SIMILARITY_STRONG`; `related` at word Jaccard ≥ `SIMILARITY_RELATED` or ≥2
  shared `area` path components or ≥2 shared tags; otherwise `novel`. Build a single `dedup_id`-keyed
  map over both arrays once per invocation so the first three verdicts are O(1) lookups and only the
  fallback path pays per-candidate trigram cost. Each candidate carries
  `{id, summary, score, reason, status, seen_count, context, evidence_files}` — `context` is what makes
  a hit actionable rather than merely informative, and `evidence_files` is the count of non-marker files
  in the candidate's `.claude/backlog-evidence/<id>/` directory, stat'ed at read time for the returned
  candidates only and never stored, so a manual `cp` cannot make it wrong. Expose
  `--similarity-strong` and `--similarity-related` to override the thresholds.
  Reading `[[compacted]]` — whose shape is `schema::COMPACTED_FIELDS` from Task 2, including `dedup_id`
  and `context` — is what stops Task 12 from destroying the "we already solved this" answer.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::check` passes, with cases for
  a punctuation-and-case-only rephrasing returning `duplicate`; a near-paraphrase returning
  `likely-duplicate`; an unrelated summary under the same directory returning `related` with
  `reason == "area"`; a `[[compacted]]` fixture row built from `schema::COMPACTED_FIELDS` returning
  `previously-resolved` with its `context` present in the candidate; a `duplicate` hit whose evidence
  directory holds a marker plus two files reporting `evidence_files: 2`, and the same item with no
  directory reporting `0`; two stored rows sharing an id returning `duplicate-id`; and an empty store
  returning `novel` with an empty candidate list.

### 8. Implement `backlog list` and `backlog show` [M]
- **Files**: `tomlctl/src/backlog/query.rs`
- **Depends on**: 2, 5, 28
- **Action**: `list` delegates to `query::run` over the store's `backlog` array with `QueryArgs`
  flattened, adding the convenience filters `--status`, `--kind`, `--tag` (AND across repeats),
  `--open`, `--area-prefix`, and `--has-evidence`. `show <id>` emits the item, its one-hop relation
  neighbourhood, and its evidence directory listing.
- **Detail**: build the `Query` via `query_input_from_cli` (`tomlctl/src/cli/dispatch.rs:424`, made
  `pub(crate)` by Task 1) — `query::run` (`tomlctl/src/query.rs:522`) takes `&Query`, not `QueryArgs`.
  `--area-prefix` is the one predicate the generic engine cannot express — match on repo-path component
  boundaries, so `lumina/server` matches `lumina/server/pty/x.rs` but not `lumina/server-extras/y.rs`.
  `show` must include reverse edges (items whose `related` / `duplicate_of` / `supersedes` point *at*
  this id), because an agent asking "what do I need to know" needs both directions in one call, and
  because the neighbourhood is how a `duplicates`-dismissed item's artefacts stay reachable from the
  survivor — `relate` moves no files. **Evidence is computed by reading the directory**, never from a
  stored field, via `evidence::list_dir` (Task 28), and resolves to exactly one of three shapes:
  `null` when the directory is absent (the common case, not a finding); `{dir, files: []}` when only
  the `.evidence` marker is present, which means "captured, but not in this clone or since deleted"
  and is deliberately not disambiguated further, because the reader's action is the same either way;
  and `{dir, files: [{name, bytes}]}` otherwise, with the marker excluded from `files`. Each
  neighbour in the neighbourhood carries the same shape.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::query` passes, with cases for
  `--area-prefix lumina/server` matching `lumina/server/pty/x.rs` and rejecting both `lumina/web/y.ts`
  and `lumina/server-extras/z.rs`; `show` on an item returning a peer that points at it but which it
  does not itself list; `show` returning `evidence: null` for an item with no directory, `files: []`
  for a marker-only directory, and a two-entry `files` array — marker excluded — for a directory
  holding the marker and two files; and an unknown id erroring with `kind=not_found`.

### 9. Implement `backlog relate` [S]
- **Files**: `tomlctl/src/backlog/relate.rs`
- **Depends on**: 2, 5
- **Action**: implement `relate <a> --to <b> --as {relates-to|duplicates|supersedes}`. `relates-to`
  appends symmetrically to both items' `related`; `duplicates` sets `a.duplicate_of = b` and transitions
  `a` to `dismissed` with `dismiss_reason = "duplicate of <b>"`; `supersedes` sets `a.supersedes = b`
  and dismisses `b` with the mirrored reason.
- **Detail**: reject self-edges and unknown ids before any write. Every form is idempotent — a re-run
  adds no second entry and does not re-dismiss. Both transitions must produce an item that passes
  `schema::validate`, which means writing the terminal date alongside the reason. Delete no evidence:
  a dismissed item keeps its own directory, reachable from the survivor through `show`'s one-hop
  neighbourhood.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::relate` passes, with cases for
  the symmetric `relates-to` write; `duplicates` producing an item that `schema::validate` accepts and
  leaving the dismissed item's evidence directory on disk; a self-edge erroring with `kind=validation`;
  and a re-run leaving the document byte-identical.

### 10. Implement `backlog triage` [M]
- **Files**: `tomlctl/src/backlog/triage.rs`
- **Depends on**: 2, 5
- **Action**: implement `triage <id>...` taking exactly one of `--promote --to <ref>`,
  `--dismiss --reason <r>`, `--resolve --resolution <r>`, or `--reopen --rationale <r>`. Set the status,
  its companion field, and today's terminal date; validate before writing. Accept repeated ids for bulk
  sweeps.
- **Detail**: `--to` accepts a flow slug or a repo-relative plan path and is stored verbatim in
  `promoted_to` — nothing is generated, per decision 6. `--reopen` clears the terminal date and
  companion, returns the status to `open`, and records `reopen_rationale` from its required
  `--rationale`, which Task 5 declares and Task 2's validator admits as the one companion an `open`
  item may carry. The four mode flags arrive as the flattened `TriageMode` `ArgGroup` from Task 5, so
  passing two is a parse-time error rather than a priority ladder. Delete no evidence — reclaiming disk
  is a manual `rm -rf` of the item's evidence directory, never a side effect of a triage transition.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::triage` passes, with cases for
  each transition writing its companion and passing `schema::validate`; a transition missing its
  companion erroring with `kind=validation`; `--reopen` clearing the terminal date and storing
  `reopen_rationale`; two mode flags together failing at clap with `ErrorKind::ArgumentConflict`; and a
  bulk `--dismiss` over three ids applying to all three.

### 11. Implement `backlog cluster` [L]
- **Files**: `tomlctl/src/backlog/cluster.rs`
- **Depends on**: 2, 5
- **Action**: implement `--by {area|tags|relations|all}` (default `all`), emitting each view as a
  separately-keyed array. Restrict to `open` items unless `--all-statuses`.
- **Detail**: *area* — group by longest common repo-path prefix, collapsing a prefix upward until its
  group reaches `--min-size` (default 2); items with an empty `area` land in an `unscoped` group.
  *tags* — group items sharing at least `--min-shared-tags` (default 2) tags, then merge overlapping
  groups transitively. *relations* — connected components over the union of `related`, `duplicate_of`
  and `supersedes` edges. Every group emits `{key, reason, size, item_ids, kinds, areas}`. Keep the
  three implementations independent; do not blend them into a single score.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::cluster` passes, with cases
  for three items under `lumina/server/pty/` grouping at that prefix rather than at `lumina/`; two items
  sharing exactly one tag not grouping at the default `--min-shared-tags`; a three-item `relates-to`
  chain landing in one component; and `--by area` emitting only the area view.

### 12. Implement `backlog compact` [M]
- **Files**: `tomlctl/src/backlog/compact.rs`, `tomlctl/src/flow/stale.rs`
- **Depends on**: 2, 5
- **Action**: move terminal items whose terminal date is older than `--older-than` (default `90d`) out of
  `[[backlog]]` and into `[[compacted]]`, carrying exactly `schema::COMPACTED_FIELDS`. Emit
  `{"ok":true,"compacted":N,"remaining":M}`. Support `--dry-run`.
- **Detail**: parse the duration by promoting `flow::stale::parse_threshold`
  (`tomlctl/src/flow/stale.rs:177`) to `pub(crate)` and reusing it — it is a deliberately-local
  `<n>{s|m|h|d|w}` parser returning `std::time::Duration`, **not** `jiff` (`grep -c jiff
  tomlctl/src/flow/stale.rs` returns 0; that file's module doc at :19-21 says so outright). Reusing it
  keeps `--older-than` and `flow stale --threshold` on one grammar; writing a second parser would ship
  two. No new dependency either way. Never touch an `open` item regardless of age; age-out is for
  decided work only. The compacted row carries `id`, `dedup_id` and `context` because Task 7's
  `previously-resolved` verdict reads all three — and because `backlog evidence audit` resolves
  directory ownership against `[[compacted]]` as well as `[[backlog]]`, so dropping the id would make
  every compacted item's evidence directory report as `unowned`. `compact` must not touch, move or
  delete an evidence directory: reclaiming that disk is a manual `rm -rf`, never a side effect of
  ageing a row out.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::compact` passes, with cases
  for an `open` item dated five years ago being left untouched; a `resolved` item exactly at the
  boundary being excluded and one day past it being included; `--dry-run` leaving the document and its
  sidecar byte-identical; the compacted row containing every `schema::COMPACTED_FIELDS` key including
  `dedup_id` and `context`; and a compacted item's `.claude/backlog-evidence/<id>/` directory
  surviving the compaction with its files intact.

### 29. Implement `backlog evidence dir` and `backlog evidence audit` [M]
- **Files**: `tomlctl/src/backlog/evidence_ops.rs`
- **Depends on**: 5, 28
- **Action**: implement `evidence dir <id> [--no-create]`, which resolves the id, creates the
  directory and its marker when absent, and emits `{"ok":true,"dir":…,"created":bool,"files":N}`; and
  read-only `evidence audit [--strict] [--max-bytes N]`, which walks `.claude/backlog-evidence/` and
  emits one record per finding across seven classes.
- **Detail**: `dir` is the only writer and it writes exactly one file — the marker, via
  `io::guard_write_path(&marker, false)` (which also performs the containment-bounded `mkdir -p`,
  `tomlctl/src/io.rs:985`) then `io::atomic_write`. It touches no TOML, takes no integrity flags, and
  must leave `.claude/backlog.toml` and its sidecar byte-identical. `--no-create` resolves and prints
  without creating, erroring `kind=not_found` if the directory is absent. `audit` walks the
  filesystem and asks whether the store owns each directory — the inverse of `items orphans`
  (`tomlctl/src/orphans.rs:3-8`), because here an unregistered file is the normal case, not an error.
  Classes: `unowned` (directory name in neither array), `no-marker` (files present, marker absent),
  `oversize`, `disallowed-extension`, `referenced-missing` (a name from `evidence::referenced_names`
  is not in the item's directory — checked only when the directory exists and is non-empty, so a
  clone that never received the bytes reports `empty` rather than a wall of false positives),
  `tracked` (`git check-ignore` reports the file as not ignored), and `empty` (marker only).
  `--strict` exits 1 on the first five and never on `tracked` or `empty` — `tracked` is a deliberate
  `git add -f` and doubles as the pre-push review of what is about to become public; `empty` is the
  expected state in a fresh clone. Batch the git query through one `git check-ignore --stdin` child
  process, shelling out the way `io::repo_or_cwd_root` already does (`tomlctl/src/io.rs:1176`), and
  degrade to a single `git-unavailable` note rather than an error when git is missing, mirroring that
  function's fallback. Never delete, never move, never rename.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::evidence_ops` passes with
  `TOMLCTL_ROOT` pointed at a `git init`-ed tempdir carrying Task 27's two ignore rules — the `git`
  init is load-bearing, since `check-ignore` cannot answer outside a repo. Cases: `dir` on a minted
  item creating the directory, writing a marker whose first line begins with the id, reporting
  `created:true`, and leaving `.claude/backlog.toml` and its `.sha256` byte-identical; a second `dir`
  call reporting `created:false` and not rewriting the marker (compare bytes and mtime); `dir` on an
  unknown id erroring `kind=not_found` with no directory created; `dir --no-create` on an absent
  directory erroring rather than creating; `audit` reporting `unowned` for a hand-made `B-deadbeef`
  directory and `--strict` exiting 1; the same run exiting 0 once that directory is removed; `audit`
  reporting `no-marker` for a directory holding one file and no marker; `disallowed-extension` for a
  `.pem` and `oversize` for a file one byte over `--max-bytes`; `referenced-missing` for an item
  whose `context` names `shot.png` when its non-empty directory holds only `other.png`, with
  `--strict` exiting 1, and no such record once `shot.png` is present; `tracked` for a file
  force-added to the index, with `--strict` still exiting 0; and `empty` for a marker-only directory,
  likewise exiting 0.

### 13. Wire the backlog group into dispatch and the auto-create seed [S]
- **Files**: `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 5, 6, 7, 8, 9, 10, 11, 12, 29
- **Action**: replace Task 5's stub with `pub(crate) fn dispatch(op: BacklogOp) -> Result<()>` as a pure
  fan-out to the nine leaves, the `Evidence { op }` arm fanning out again to `dir` and `audit`,
  mirroring `tomlctl/src/flow/dispatch.rs:11`; add `"backlog.toml"` to `SCHEMA_SEEDED_FLOW_FILES`
  (`tomlctl/src/cli/dispatch.rs:282`).
- **Detail**: the `Cmd::Backlog` match arm already exists — Task 5 added it, because `run`'s match is
  exhaustive with no catch-all. What lands here is the real fan-out behind it and the seed-table entry,
  which is what makes a first write produce `{schema_version = 1, last_updated = <today>}` rather than
  an empty table. Key insertion order is load-bearing there.
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` exits 0, and
  `cargo test --manifest-path tomlctl/Cargo.toml seed_doc_for` passes with a new case asserting
  `backlog.toml` seeds `schema_version` before `last_updated`.

### 14. Add black-box CLI tests for the backlog group [M]
- **Files**: `tomlctl/tests/backlog_write.rs`, `tomlctl/tests/backlog_read.rs`
- **Depends on**: 13
- **Action**: cover the group end-to-end with `assert_cmd`, using `tomlctl/tests/common/mod.rs` helpers
  and `TOMLCTL_ROOT` pointed at a tempdir.
- **Detail**: `backlog_write.rs` walks mint → bump → relate → triage → compact, asserting the sidecar is
  written and `--verify-integrity` passes afterwards, and using `assert_sidecar_matches` to prove
  `--dry-run` byte-identity. It also runs `evidence dir` on a minted item and asserts the printed path
  exists, holds a `.evidence` marker whose first line begins with the item id, and that
  `.claude/backlog.toml` is byte-identical afterwards — `evidence dir` writes no TOML.
  `backlog_read.rs` covers `check` verdicts, `list` filters, `show` neighbourhoods (including all
  three evidence-listing shapes) and `cluster` views, plus `evidence audit --strict` exiting 1 on a
  hand-made unowned directory and 0 once it is removed, plus `--error-format json` envelope shape on
  an unknown id. These live in `tests/` rather than in-file
  because `Cli` and `Cmd` are `pub(crate)` — the leaf tasks' own tests are unit tests for exactly that
  reason.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_write --test backlog_read`
  passes.

<!-- CHECKPOINT 2 — the CLI group is complete and black-box tested -->

**Milestone 3 — harness adoption and the skill split**

### 21. Widen `command_lint`'s scan set and latch the skill ceiling [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 14
- **Action**: extract the inline scan-set construction inside `#[test] fn command_lint()`
  (`tomlctl/src/cli/dispatch.rs:1855-1888`) into `fn command_lint_scan_set(claude_dir: &Path) -> Vec<PathBuf>`
  taking the root as a parameter, then extend it to read **every** `claude/skills/*/SKILL.md` (dropping
  the `flow-contract-` prefix filter) and `claude/skills/*/references/*.md`. Add a sibling
  `#[test] fn skill_bodies_under_line_ceiling()` asserting every `claude/skills/*/SKILL.md` is ≤ 500
  lines, reusing the same graceful `claude/`-absent skip.
- **Detail**: the extraction is not optional — the current code hard-binds `repo_root` from
  `env!("CARGO_MANIFEST_DIR")`, so the acceptance's temp-directory test is impossible against it, and
  writing into the live repo tree from a test is not acceptable. Use std `read_dir`; the existing code
  comments note there is no `glob` crate in the dependency tree and std is sufficient. Keep the
  existing `claude/` self-skip so a packaged checkout without the harness tree still passes. Dropping
  the prefix filter is what makes Task 15's new skill gated at all and closes the same hole for
  `test-author`, `commit-conventions` and `documentation-conventions`. The line-ceiling latch is what
  turns Milestone 3 from a one-time cleanup into a maintained invariant; nothing official enforces the
  ceiling (R7). This must land before Tasks 15, 22 and 23 create or move any fence.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes — note this newly
  brings `claude/skills/commit-conventions/references/{detection,generation-runbook}.md` and the three
  currently-ungated `SKILL.md` files into scope for the first time; if any line fails, fix the doc line
  rather than narrowing the glob. Plus a new unit test asserting `command_lint_scan_set` includes files
  it writes to a temporary `claude/skills/x/SKILL.md` and `claude/skills/x/references/y.md` and
  excludes `claude/skills/x/templates/z.md` — assert on the non-prefixed `x` directory, not a
  `flow-contract-x` one, so the test fails if the prefix filter is left in place. Plus
  `cargo test --manifest-path tomlctl/Cargo.toml skill_bodies_under_line_ceiling`, which **fails at
  this point** against `claude/skills/tomlctl/SKILL.md` at 869 lines and is expected to; Task 24 is
  what turns it green, and Checkpoint 3 is placed after Task 23 rather than here for that reason.

### 15. Author the `backlog-capture` skill [M]
- **Files**: `claude/skills/backlog-capture/SKILL.md`
- **Depends on**: 14, 21
- **Action**: write a model-invocable skill covering when to mint and when not to; the mandatory
  `backlog check` before `backlog add` discipline and how to act on each of the verdicts; the kind
  and status vocabularies; the orchestrator-only writer rule and how a sub-agent surfaces a candidate
  instead; the evidence-directory convention and its publication discipline; and the CLI idioms.
- **Detail**: the evidence section is a hygiene contract, and it is the part most likely to cause real
  harm if it is vague. State plainly: run `tomlctl backlog evidence dir <id>` and copy into the path
  it prints — never hand-derive it, because ids widen and a typo'd directory is owned by nothing;
  name the file for what it shows, since the filename is the only caption a manual drop carries, and
  reference it by that bare filename in the item's `context` prose where it clarifies a sentence;
  this repository is public and the directory's contents are git-ignored precisely so a screenshot is
  not published by accident; publishing one is `git add -f <file>`, a deliberate human act after
  checking it for credentials, personal data, session tokens and a visible username in a path; a HAR
  or a network `.json` dump carries `Authorization` headers verbatim and should essentially never be
  published; and a file left ignored is invisible in every other clone, so the item's `context`
  prose — not the picture — has to carry the finding. Frontmatter carries `name` and `description` only — third person, stating what it
  does and when to use it, under 1,024 characters, no `version` key (R4, R5). Every `tomlctl` example
  goes in a ` ```bash ` fence; Task 21 is what makes `command_lint` actually gate this file, which is
  why this task depends on it. Follow `documentation-conventions`: this skill owns the capture
  *discipline*; the CLI flag reference belongs in `claude/skills/tomlctl/references/backlog.md`
  (Task 23) and must not be restated here.
- **Acceptance**: `awk 'END{print NR}' claude/skills/backlog-capture/SKILL.md` prints ≤ 500;
  `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes with this file now inside the
  scan set (introduce a deliberate `tomlctl backlog --bogus-flag` fence locally and confirm the test
  fails, then remove it — a gate that cannot fail is not a gate); and
  `grep -c 'git add -f' claude/skills/backlog-capture/SKILL.md` returns ≥ 1.

### 16. Add the `/backlog` sweep command [M]
- **Files**: `claude/commands/backlog.md`, `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 15, 21
- **Action**: write a multi-step carrier for human-driven triage — pre-flight, `cluster --by all`,
  present the clusters and take dispositions via `AskUserQuestion`, apply them with `backlog triage`,
  run `backlog evidence audit` and surface anything it reports, then summarise. Register it in
  the `carrier_invokes_required_skills` expected table (`tomlctl/src/cli/dispatch.rs:2055`).
- **Detail**: `Depends on: 21` is not about content — it is because Task 21 also edits
  `tomlctl/src/cli/dispatch.rs`, and `/implement` frontier-schedules from these edges, so without the
  edge both tasks claim the same file in one frontier. As a multi-step carrier it must invoke
  `flow-contract-task-visibility` and mint run-scoped task entries with the
  `<slug> /backlog · <ref> — <title>` subject prefix, and it must degrade silently when the task tools
  are absent. It also invokes `backlog-capture`. Both names must appear in the file for the gate to
  pass.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml carrier_invokes_required_skills` and
  `cargo test --manifest-path tomlctl/Cargo.toml command_lint` both pass.

### 17. Wire capture points into `/implement` and `/tdd` [S]
- **Files**: `claude/commands/implement.md`, `claude/commands/tdd.md`
- **Depends on**: 15
- **Action**: in `claude/commands/implement.md`, at the Phase-4 `### Failed / Skipped` and
  `### Plan Deviations` sections, have the orchestrator run `backlog check` then `backlog add` for each
  out-of-scope discovery and list the minted ids in the report. In `claude/commands/tdd.md`, do the same
  for the REFACTOR-phase deferred follow-ups.
- **Detail**: state the orchestrator-only writer rule explicitly in both. Do not disturb the existing
  `type=deviation` / `type=deferral` execution-record paths — a plan deviation is still a deviation; the
  backlog is only for what falls *outside* the plan's item set. `--origin implement` / `--origin tdd`
  on every mint.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` and
  `cargo test --manifest-path tomlctl/Cargo.toml carrier_invokes_required_skills` pass, and in both
  files `grep -c 'backlog check' <file>` is ≥ `grep -c 'backlog add' <file>` — the mechanical form of
  "every mint is preceded by a check", replacing a judgement about what "the same step" means.

### 18. Wire capture points into `/review` and `/optimise` [S]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`
- **Depends on**: 15
- **Action**: extend Step 1's prior-findings load to also run `backlog list --open` so known issues
  arrive with their context, and add a disposition path in Step 3/4 for observations that fall outside
  the ledger's scope entirely — mint to the backlog rather than forcing a terminal disposition on an
  item the ledger never owned.
- **Detail**: leave each carrier's existing disposition syntax byte-unchanged; those remain correct for
  items the ledger already owns, and the two carriers do **not** share a vocabulary.
  `claude/commands/review.md` carries `defer R{n} — reason — trigger` (2 occurrences, measured
  2026-09-01) and `wontfix R{n} — rationale`. `claude/commands/optimise.md` carries neither string: per
  its own line 39 the vocabulary is `open` / `deferred` / `applied` / `wontapply` with `O{n}` ids and no
  `verified-clean` counterpart. The new path is only for findings that were never ledger items.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes;
  `grep -c 'defer R{n}' claude/commands/review.md` returns 2 (its pre-change value); and
  `grep -c 'wontapply' claude/commands/optimise.md` returns its pre-change value, recorded before
  editing.

### 19. Close the apply-pipeline out-of-scope gap [S]
- **Files**: `claude/skills/flow-contract-apply-pipeline/SKILL.md`
- **Depends on**: 15
- **Action**: at Step 6's plan-deviation follow-up, give the out-of-scope branch — currently
  "do NOT auto-invoke, report in the final summary only" — a backlog mint, and route the
  `skipped <id>: …` skip tags into backlog items carrying the skip reason as `context`.
- **Detail**: leave the in-scope branch untouched. That branch is **prose**, not a `Skill(...)` literal:
  it reads "auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument …"
  (:550-552). The only `Skill("plan-update", …)` literal in the file is `Skill("plan-update", "status")`
  at :563, which belongs to a different step. This file is already byte-divergent from its published
  copy under `~/.claude/skills/` — that predates this work and is resolved by the manual publication
  step, not here.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes, and
  `grep -c 'auto-invoke the `plan-update` skill via the `Skill` tool' claude/skills/flow-contract-apply-pipeline/SKILL.md`
  still returns 1 — the in-scope branch untouched.

### 20. Add the candidate-capture block to both implementer agents [S]
- **Files**: `claude/agents/implement-deep.md`, `claude/agents/implement-lite.md`,
  `scripts/shared-blocks.toml`
- **Depends on**: 15
- **Action**: add a byte-identical `<!-- SHARED-BLOCK:backlog-candidates START -->` / `END` block to both
  implementer agents instructing them to report tangential discoveries under a fixed heading in their
  return payload and never to write the store themselves, and register the block in
  `scripts/shared-blocks.toml`.
- **Detail**: the manifest currently carries exactly one block (`forbidden-working-tree-ops`) across
  these same two files; follow its entry shape. `scripts/verify-shared-blocks.sh` reads the manifest
  generically — no block names are hardcoded there — so registering the new block is all that is
  required. The block's content must be byte-identical in both carriers or the pre-commit hook rejects
  the commit.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0, and
  `cargo test --manifest-path tomlctl/Cargo.toml blocks_verify_reproduces_shell_hashes` still passes.
  Note that Rust test hardcodes `carriers_for("forbidden-working-tree-ops")`
  (`tomlctl/src/cli/dispatch.rs:1661`) and does **not** cover a newly-added block, so the shell verifier
  is the only real gate here and the manifest and the two carriers must land in the same commit.

### 22. Extract the tomlctl read, query and write references [M]
- **Files**: `claude/skills/tomlctl/references/query.md`, `claude/skills/tomlctl/references/write.md`
- **Depends on**: 21
- **Action**: copy the full read+query surface (`## Read operations`, `### Strict reads (--strict-read)`,
  the `items` query section and its filters, projection, shaping, aggregation, output shapes,
  single-item fetch, find-duplicates and orphans subsections) into `query.md`, and the full write
  surface (auto-create, set, set-json, add, add-many, update, remove, apply, next-id, array-append,
  backfill-dedup-id, stdin, `## Dedup fingerprint contract`, `### Regenerate a missing sidecar —
  integrity refresh`, `## Common recipes`) into `write.md`. Give each a `## Contents` table of contents.
- **Detail**: copy verbatim, and leave `claude/skills/tomlctl/SKILL.md` untouched — Task 24 removes the
  originals, so this task and Task 23 stay file-disjoint and the content is briefly duplicated in
  between. The section lists above are exhaustive on purpose: cross-referencing Task 24's keep-list
  against `grep -n '^##' claude/skills/tomlctl/SKILL.md` showed six sections previously assigned to
  neither reference file, which Task 24 would have deleted rather than moved — `## Common recipes`,
  `## Read operations`, `### Strict reads`, `### Verify shared-block parity across markdown files`,
  `### Regenerate a missing sidecar`, and `## Dedup fingerprint contract`. Two of them
  (`#dedup-fingerprint-contract`, `#strict-reads---strict-read`) are live link targets. Copying rather
  than deleting is the documented treatment: R6 is explicit that comprehensive reference material
  belongs in bundled files rather than being cut. Both files will exceed 100 lines, which is what
  triggers the ToC requirement (R3). Collapse the 12 near-identical `--dry-run` preview blocks
  (measured) into one `## Dry-run` section in `write.md` plus a one-line mention per verb; that is
  deduplication, not deletion, and every distinct invocation must survive.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes with the moved
  fences now in scope, and every heading in the copy-list above appears in exactly one of the two new
  files (`grep -h '^#\{2,3\} ' claude/skills/tomlctl/references/{query,write}.md | sort` contains each).

### 23. Extract the flow and maintenance references and document the backlog group [M]
- **Files**: `claude/skills/tomlctl/references/flow.md`, `claude/skills/tomlctl/references/backlog.md`
- **Depends on**: 20, 21
- **Action**: copy the flow subcommand surface, the `--verify-integrity` support matrix, sidecar
  semantics, the error-format section, `### Verify shared-block parity across markdown files` and the
  Advanced/maintenance section into `flow.md`; write `backlog.md` documenting the nine new verbs. Give
  both a `## Contents` ToC.
- **Detail**: leave `claude/skills/tomlctl/SKILL.md` untouched — Task 24 removes the originals. Fix
  three drift defects while copying. (a) **Both** `blocks verify` invocations in the single fence at
  SKILL.md:851-859 are dead: the first targets four `claude/commands/*.md` files that carry no
  `SHARED-BLOCK` marker, and the second is `tomlctl blocks verify claude/commands/*.md` with the same
  problem. Retarget both at `claude/agents/implement-deep.md` and `claude/agents/implement-lite.md`,
  the only files that do, **and change `--block flow-context --block ledger-schema` to the block names
  those files actually carry** — `forbidden-working-tree-ops` plus `backlog-candidates` once Task 20
  lands, which is what makes this task's `Depends on: 20` load-bearing. A `--block` naming an absent
  marker is a non-zero exit. (b) Move `blocks verify-skills` into a collapsed `<details>` "Old
  patterns" block noting it is vacuous now that no surviving block sets the manifest's `skill` field;
  that is the documented treatment for superseded material (R6). All three occurrences (heading, prose,
  fence) move together and the fence must survive, so `command_lint` keeps gating the verb. (c) Do not
  copy defect #5 forward: SKILL.md:845's "Kept documented for hook/script authors" is false — the
  pre-commit hook invokes the bash scripts, not `tomlctl blocks`. `backlog.md` documents the two
  two-op `evidence` subgroup, the directory-as-record convention and why no field enumerates the
  files, the two `.gitignore` rules and the tracked `.evidence` marker, `git add -f` as the
  publication path, the convention for naming a file inline in `context` prose, and the seven
  `evidence audit` classes with which five `--strict` fails on. Per the Risks section it also states
  that the normaliser's definition is frozen without a `schema_version` bump, and that evidence
  directory names derive from the id rather than from any normalised text, so a normaliser change
  cannot orphan a directory.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes; running the
  corrected examples from the repo root —
  `tomlctl blocks verify claude/agents/implement-deep.md claude/agents/implement-lite.md --block forbidden-working-tree-ops --block backlog-candidates`
  and the `--block`-less second form over the same two files — each exit 0 and emit `"ok":true`; and
  `grep -c 'blocks verify-skills' claude/skills/tomlctl/references/flow.md` returns 3 (heading, prose
  and fence, all inside the one `<details>` block).

<!-- CHECKPOINT 3 — references extracted; SKILL.md still intact, the last recoverable state -->

### 25. Bump the tomlctl version and its pinned assertion [S]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/tests/capabilities.rs`, `claude/agents/flow-bootstrap.md`
- **Depends on**: 14
- **Action**: bump `version` from `0.5.0` to `0.6.0`; update the literal assertion and its rationale
  message at `tomlctl/tests/capabilities.rs:1589-1590`; raise `claude/agents/flow-bootstrap.md`'s
  step-2 pre-flight gate from `≥0.5` to `≥0.6` and update both error literals that spell the version.
- **Detail**: a new top-level verb group is a feature, and the repo's install note says to rerun
  `cargo install --path tomlctl` on a version bump. Raising the pre-flight gate is not optional: it is
  the only thing that stops an operator running a stale `0.5.0` binary from passing pre-flight cleanly
  and then failing at every `backlog check` / `backlog add` seam Tasks 17-19 wire in. That exact
  failure is on record for the previous bump
  (`.claude/flows/flow-tracking-overhaul/plan-review-findings.toml:169`). Put the
  `cargo install --path tomlctl` line in this task's commit message. The capabilities assertion is a
  literal string with a bespoke failure message, so it fails loudly rather than silently.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes;
  `grep -c '"0\.5\.0"' tomlctl/tests/capabilities.rs` returns 0; and
  `grep -c '0\.5' claude/agents/flow-bootstrap.md` returns 0.

### 24. Rewrite the tomlctl SKILL.md body [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 22, 23, 25
- **Action**: delete the sections Tasks 22 and 23 copied out, and reduce the body to overview,
  when-to-use, Quick Reference, install and capabilities, constraints/gotchas/permissions, and one-level
  links to the four reference files. Fix the version drift and retarget every surviving in-page anchor.
- **Detail**: read the version out of `tomlctl/Cargo.toml` rather than transcribing a remembered value —
  the current text says `0.4.0`, the tree said `0.5.0` at planning time, and Task 25 moves it to
  `0.6.0`, which is why this task depends on it. Link each reference file exactly once and never link
  reference-to-reference (R3). **The anchors are the trap**: `grep -c '](#' claude/skills/tomlctl/SKILL.md`
  returns 16, of which only `#constraints-and-gotchas` and `#flow-bootstrap-agent-entrypoint` (an
  explicit `<a id=…>` inside the retained Quick Reference) target sections that survive. Every other
  one — including `#dedup-fingerprint-contract`, `#strict-reads---strict-read`,
  `#auto-create-on-first-write`, `#sidecar-files`, `#output-shapes---raw----lines----ndjson`,
  `#advanced--maintenance`, `#stdin-input-for-large-json-payloads`, `#--verify-integrity-support-matrix`,
  `#envelope-construction--flow-envelope-build`, `#render-progress-logmd--flow-render-progress-log` and
  the filters anchor — must become either a cross-file link into the right `references/*.md` or plain
  prose. Re-derive the list rather than trusting this one. Leave the `description` frontmatter
  byte-unchanged: at 693 characters, third person, with a "Use this for…" trigger, it is already
  compliant (R4). Do not add a `version:` key (R5).
- **Acceptance**: capture `grep -h '^#\{2,3\} ' claude/skills/tomlctl/SKILL.md | sort` **before**
  editing; afterwards every captured heading appears at least once across the post-change SKILL.md plus
  `claude/skills/tomlctl/references/*.md`, with any deliberate drop named in the task record.
  `awk 'END{print NR}' claude/skills/tomlctl/SKILL.md` prints ≤ 500 and
  `cargo test --manifest-path tomlctl/Cargo.toml skill_bodies_under_line_ceiling` (Task 21) now passes.
  `grep -c '0\.4\.0' claude/skills/tomlctl/SKILL.md` returns 0 and
  `grep -o '"version":"[^"]*"' claude/skills/tomlctl/SKILL.md` equals the `version` in
  `tomlctl/Cargo.toml` (compare with `tomlctl json get`/`grep` rather than by eye).
  `grep -c '^### Patch an existing item' claude/skills/tomlctl/SKILL.md` returns 0.
  `grep -h '^tomlctl ' claude/skills/tomlctl/SKILL.md claude/skills/tomlctl/references/*.md | sort -u | wc -l`
  equals **86** — the pre-change value measured 2026-09-01; a dropped invocation lowers it and a
  mangled copy raises it. Every remaining `](#…)` in SKILL.md resolves to a heading still present in
  SKILL.md (`grep -o '](#[^)]*)' …` cross-checked against `grep '^#' …`).
  `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes, and each of the four
  `references/*.md` files is linked exactly once from SKILL.md while no reference file links to another.

### 26. Update the repo documentation [S]
- **Files**: `tomlctl/README.md`, `CLAUDE.md`
- **Depends on**: 24, 25
- **Action**: add the `backlog` group to the README's Quick tour fence; fix `tomlctl/README.md:209`'s
  stale `"version": "0.4.0"` sample to match `tomlctl/Cargo.toml`; retarget both cross-file deep links
  Task 22 breaks — `:59`'s `…/SKILL.md#query-items-full-query-surface` at
  `claude/skills/tomlctl/references/query.md` and `:61`'s
  `…/SKILL.md#stdin-input-for-large-json-payloads` at `claude/skills/tomlctl/references/write.md`; and
  add a short `## Backlog capture` section to `CLAUDE.md` naming `.claude/backlog.toml`, the evidence
  directory, the orchestrator-only writer rule, and `backlog-capture` as the skill that owns the detail.
- **Detail**: one fact, one file — `CLAUDE.md` points at the skill and must not restate the item schema,
  the verdict ladder, the artefact row shape, or the flag surface. Keep it to a handful of lines,
  matching the density of the neighbouring sections. `tomlctl/README.md:250`'s "added in 0.4.0" is
  historical prose and correctly stays. Note `command_lint`'s scan set is rooted at `claude/`
  (`tomlctl/src/cli/dispatch.rs:1851`) and does **not** read `tomlctl/README.md`, so the README fences
  are not gated — verify each new line by hand against `tomlctl backlog --help`.
- **Acceptance**: `grep -c 'dedup_id\|seen_count\|EVIDENCE_MAX_BYTES' CLAUDE.md` returns 0 (it returns 0
  before the change too — this guards against an implementer restating the schema, not against
  inaction); `grep -c '0\.4\.0' tomlctl/README.md` returns 0; `grep -c 'SKILL.md#' tomlctl/README.md`
  returns 0 (both deep links retargeted at `references/`); and every `tomlctl backlog …` line added to
  the README parses — check by pasting each into `Cli::try_parse_from` via
  `cargo run --manifest-path tomlctl/Cargo.toml -- <line> --help`, since `command_lint` will not.

<!-- CHECKPOINT 4 — harness adoption and skill split complete -->

## Dependency Graph

Checkpoint markers only; the authoritative edges are each task's **Depends on** line. A marker closes
on the *dependency closure* of every task it names, which is why several markers name more than one.

- **Checkpoint 1** — after tasks 2, 4, 5, 27, 28. Closure: tasks 1-5, 27, 28. Task 5 ships the
  `Cmd::Backlog` match arm alongside the variant, so the tip compiles; tasks 2 and 4 are named because
  neither lies in `closure(5)`, and task 4 sits one dependency level deeper than task 5.
- **Checkpoint 2** — after task 14. Closure: tasks 1-14, 28, 29 (task 13 depends on 29 and task 14 on
  13, so the evidence leaf is inside this cut; task 27 closed at Checkpoint 1 and nothing depends on
  it, which is why it is named there rather than here).
- **Checkpoint 3** — after tasks 22, 23. Closure adds tasks 15, 20, 21, 22, 23. The last state at which
  `claude/skills/tomlctl/SKILL.md` is still intact and all four reference files exist — the destructive
  task 24 then lands against a gate-verified tip.
- **Checkpoint 4** — after tasks 16, 17, 18, 19, 26. Closure: everything. Tasks 16-19 are graph sinks
  off task 15 and lie in no other task's closure, so they are named explicitly; without that they would
  be committed only by the final Phase-3 train.

## Risks

- **Scope is 40 unique files across 30 tasks, above the ~25-file guidance.** Flagged deliberately: the
  two workstreams were bundled by request, and they are genuinely coupled — the tomlctl skill is the
  document that has to describe the new verb group, so splitting them would mean editing the same
  SKILL.md twice. The natural seam if you would rather split is Checkpoint 2: Milestones 1-2 ship the
  CLI standalone, and Milestone 3 becomes its own plan. Nothing in Milestone 3 changes the crate's
  behaviour except Tasks 21 and 25.
- **`.claude/` is tracked in a PUBLIC repository, and this is the first store written there
  automatically.** Verified 2026-09-01: `gh repo view --json visibility` returns `PUBLIC` and
  `git ls-files .claude | wc -l` returns 314. Every minted `summary`, `context` and `evidence` string
  is published, at five carrier seams, with no human in the loop. A published screenshot or HAR is
  worse still — irrevocable, scraped within minutes, and in every clone forever with no LFS configured
  (`git config --get-regexp '^lfs'` exits 1) to keep it out of the pack. Mitigation — the
  `backlog-capture` skill states the redaction rule, the orchestrator reviews minted rows before the
  commit train stages them, evidence contents are git-ignored by two rules verified to work
  (Task 27), publication requires `git add -f`, which is not pre-approved for agents, and
  `evidence audit`'s `tracked` class enumerates every evidence file currently headed for the next
  push. None of that inspects content: a redacted screenshot is the author's responsibility, and
  Task 15's skill text, not the code, is the real control.
- **Nothing enforces the extension allowlist or the size ceiling at capture time.** The endorsed path
  is a bare `cp`, so any gate applied inside a `tomlctl` verb is bypassed by the very workflow the
  design asks for. The model is therefore detection, not prevention, and detection only fires when
  someone runs `evidence audit`. Mitigation — `/backlog` (Task 16) runs it, and the git-ignore default
  means an undetected bad file is a local-disk problem rather than a repository one. The honest
  residual: a `.env` copied into an evidence directory is invisible until an audit, and harmless only
  for as long as nobody force-adds it.
- **The evidence directory is invisible in every other clone, and the marker is a thin substitute.** A
  teammate on a fresh clone sees an empty directory and a four-line marker naming the item — not the
  screenshot that made the finding legible. That is the price of the ignore default, paid on the
  common path. Mitigation — `show` distinguishes "no evidence" from "captured, not here" so the gap is
  legible rather than silent, and the skill instructs that the item's `context` prose carries the
  finding. If that proves inadequate the lever is the prose, not flipping the default.
- **A hand-derived directory path is silently unowned.** Ids widen 8→10→12 hex on collision, so
  `B-a1b2c3d4` may not be the correct directory for the item whose `list` row starts with those eight
  characters, and a `cp` into the wrong name succeeds and is never read by anything. Mitigation —
  `backlog evidence dir <id>` resolves against the store and is the documented way to obtain the path;
  `evidence audit`'s `unowned` class catches the mistake after the fact. It cannot catch the inverse,
  where a file lands in a real but *different* item's directory. Widening itself is not a hazard: it
  lengthens only the incoming item's id, so an existing directory is never orphaned by it.
- **Nothing bounds the evidence tree's growth.** `compact` deliberately deletes nothing,
  `triage --dismiss` deletes nothing, and there is no prune verb — so a five-year-old dismissed item's
  screenshots persist until a human runs `rm -rf`. Accepted: the alternative is a delete verb whose
  target directory name derives from agent-influenced input, which is more blast radius than the
  problem justifies. The failure is a slow local-disk creep, never a repository one, precisely because
  the contents are ignored.
- **The two `.gitignore` rules are a negation, and negations are fragile.** Re-including `.evidence` is
  legal only because the exclude pattern matches files inside an item directory rather than the
  directory itself; a well-meant simplification to `/.claude/backlog-evidence/` excludes the parent and
  makes the negation a no-op, at which point every marker vanishes from the index and the cross-clone
  signal is silently gone. Mitigation — Task 27's acceptance asserts both directions with
  `git check-ignore -q`, and `evidence audit`'s `no-marker` class surfaces the resulting state.
- **The normaliser's definition is load-bearing and effectively frozen.** Every stored `dedup_id` and
  every derived `id` depends on it byte-for-byte, including the choice of `u8::to_ascii_lowercase` over
  `str::to_lowercase`. Changing the stopword list or the punctuation rule later silently re-partitions
  the store: old items stop matching new detections of the same issue. Mitigation — treat a normaliser
  change as a `schema_version` bump requiring a rehash pass, and say so in `references/backlog.md`.
  There is no rehash verb in this plan. An artefact's `sha256` is over raw bytes and is unaffected.
- **Content-derived ids do not make a git merge converge.** `.gitattributes` configures no TOML merge
  driver and `git config --get-regexp 'merge\.'` is empty, so this file merges as plain text and git
  auto-collapses only byte-identical additions. Two worktrees minting the same discovery agree on `id`
  but differ on `created`, `last_seen`, `origin`, `flow` and `context`, so the merge yields a conflict
  hunk or two rows under one id. R8's guarantee is that ids do not collide between *different* issues;
  it is not a claim about text merges. Mitigation — `schema::validate` enforces id-uniqueness across
  `[[backlog]]` ∪ `[[compacted]]` and `check` emits a `duplicate-id` verdict, so the condition is
  visible rather than silent. No verb repairs it automatically.
- **Content-derived ids are not age-sortable and lose the `items next-id` affordance.** Accepted with
  decision 1. Anything wanting chronological order sorts on `created`. A 32-bit id prefix collides at
  roughly 65k items by the birthday bound — far beyond this store's scale, and handled deterministically
  by widening under a total order on `dedup_id` so two worktrees resolve it identically. The widening
  path must actually be tested (Task 4) or it will be the code that has never run; the 10→12 tier is
  unreachable at 40 bits and is tested only for its tie-break, not its collision.
- **Similarity thresholds are judgement calls with no prior art.** 0.75 and 0.35 are chosen, not
  derived. Too high and agents mint duplicates; too low and `check` cries wolf and gets ignored, which
  is the worse failure. They are overridable per-invocation, but the defaults are what will actually be
  used — expect to retune after real usage rather than treating the first values as settled.
- **`[[compacted]]` grows monotonically and `check` reads it on every mint.** Nothing prunes it, and
  `check` runs before every `add` at five automatic seams. Mitigation — the `dedup_id`-keyed map built
  once per invocation makes the three exact verdicts O(1) so only the fallback similarity path pays
  per-candidate trigram cost. Whether this is ever hot is unmeasured; at a few hundred rows it is
  irrelevant.
- **Auto-capture can turn the store into a graveyard.** `seen_count` / `last_seen` and `compact` are the
  countermeasures, but neither is proven — R13 confirms compaction as prior art while the
  re-confirmation counter is our own invention with no surveyed precedent. If the store stops being
  useful, the first thing to check is whether `check` is being run before `add`.
- **Moving fences out of SKILL.md silently drops CLI-drift gating unless Task 21 lands first, and Task
  21 is itself a change to the gate.** The dependency is encoded, but it is the kind of ordering that
  survives a plan and dies in a rushed rebase. `command_lint` will not complain about a fence it never
  sees. Task 21 also newly pulls previously-unlinted files into scope, so expect it to surface
  pre-existing doc-line defects; fix the doc line rather than narrowing the glob.
- **Task 24 is the only destructive edit and its acceptances are mostly negative.** A wrong deletion is
  recoverable — `git show <sha>^:claude/skills/tomlctl/SKILL.md` — but it is only *detectable* through
  the heading-conservation capture the acceptance now requires, because the `^tomlctl ` invocation
  count covers 86 of 869 lines. Checkpoint 3 sits immediately before it for exactly this reason.
- **`Item::validate` carries `#[allow(dead_code)]`.** Observed at `tomlctl/src/items.rs:1745`. Whether
  the ledger validator is actually wired into every write funnel is unverified; the backlog validator
  must be called explicitly from each write path rather than assumed to run.
- **Publication to `~/.claude/` is manual and already drifted.**
  `claude/skills/flow-contract-apply-pipeline/SKILL.md` differs from its published copy today. Everything
  in Milestone 3 is inert until copied across, so a "it doesn't work" report after this lands is most
  likely an uncopied file rather than a defect.
