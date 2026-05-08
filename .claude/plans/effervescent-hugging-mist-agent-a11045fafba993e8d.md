# Agent 2 (Completeness & Scope) — review of `effervescent-hugging-mist.md`

Findings below cover gaps in the plan's scope: files / configs / cross-cutting concerns it should mention but doesn't.

---

## Finding 1

- **severity**: critical
- **category**: completeness
- **plan_section**: `### 9. Add 6th \`package-quality\` lens to /review [M]`
- **anchor_old**: `- **Action**: Edit existing file in 3 spots: (a) after Step 2 small-diff shortcut (~line 425), add the conditional dispatch; (b) widen ledger category enum at line 183 to include \`package-quality\`; (c) after Agent 5 (~line 532), add \`### Agent 6: Package Quality (conditional)\` subsection.`
- **anchor_new**: `- **Action**: Edit existing file in 3 spots: (a) after Step 2 small-diff shortcut (~line 425), add the conditional dispatch; (b) widen ledger category enum at line 183 to include \`package-quality\`; (c) after Agent 5 (~line 532), add \`### Agent 6: Package Quality (conditional)\` subsection. Because the \`ledger-schema\` shared block lives in four files (\`review.md\`, \`review-apply.md\`, \`optimise.md\`, \`optimise-apply.md\` per \`scripts/shared-blocks.toml\`), the line-183 edit is INSIDE the shared block — the edit MUST be applied byte-identically to all four carriers in the same commit, otherwise \`scripts/verify-shared-blocks.sh\` will reject the commit.`
- **summary**: Line 183 (`Review` category enum) is inside the `ledger-schema` shared block that is byte-identical across 4 files — the plan only mentions editing `review.md`, which would immediately break parity.
- **description**: Verified by reading `scripts/shared-blocks.toml` (lines 18-25): the `ledger-schema` block is shared across `claude/commands/{optimise,review,optimise-apply,review-apply}.md`. Verified by inspecting `claude/commands/review.md` line 183 and `claude/commands/review-apply.md` line 183 — both contain the identical line `**Review** (\`review-ledger.toml\`): \`quality\` | \`security\` | \`architecture\` | \`completeness\` | \`db\` | \`testability\` | \`verified-clean\` ...`. Editing it in only one file will trip the pre-commit hook on the next commit. Task 9 must broaden its `Files` field to all four carriers and explicitly call out the shared-block constraint.

---

## Finding 2

- **severity**: critical
- **category**: completeness
- **plan_section**: `### 10. Document new conventions in root CLAUDE.md [S]`
- **anchor_old**: `- **Detail**: (a) "Per-command sub-directory convention" — document that \`claude/commands/<name>/\` is the canonical location for per-command supplementary files (references, fixtures, etc.) when a command outgrows a single \`.md\`. Cite \`claude/commands/test-bootstrap/references/\` as the first instance. (b) "Testing-discipline commands" — one-paragraph entry per: \`/test-bootstrap\` (when to use, what it does), \`/tdd\` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the \`test-author\` skill (model-discoverable, no manual invocation). NO build/test/lint/audit commands need to be added (no new binaries — evalctl was dropped).`
- **anchor_new**: `- **Detail**: (a) "Per-command sub-directory convention" — document that \`claude/commands/<name>/\` is the canonical location for per-command supplementary files (references, fixtures, etc.) when a command outgrows a single \`.md\`. Cite \`claude/commands/test-bootstrap/references/\` as the first instance. (b) "Testing-discipline commands" — one-paragraph entry per: \`/test-bootstrap\` (when to use, what it does), \`/tdd\` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the \`test-author\` skill (model-discoverable, no manual invocation). (c) **Update the hardcoded trigger-list sentence in the existing "Developer setup" section** — the prose currently reads "...claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md..."; insert \`tdd\` into that brace expansion since Task 8 widens the manifest to include it. NO build/test/lint/audit commands need to be added (no new binaries — evalctl was dropped).`
- **summary**: Plan adds `tdd.md` to the parity-checked set but doesn't update the brace-expansion trigger list inside `CLAUDE.md` line 11 — that prose drifts the moment Task 8 lands.
- **description**: `CLAUDE.md` line 11 says verbatim: "The hook currently triggers on staged changes to `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md`". After Task 8 widens `shared-blocks.toml` to include `tdd.md`, the hook will *also* trigger on `tdd.md` edits (because `verify-shared-blocks.sh` reads the manifest, not a hardcoded list — verified by reading `scripts/verify-shared-blocks.sh` lines 42-55 which iterate the manifest). Task 10 must update this prose, otherwise the documentation contradicts the hook's actual behaviour.

---

## Finding 3

- **severity**: warning
- **category**: completeness
- **plan_section**: `### 9. Add 6th \`package-quality\` lens to /review [M]`
- **anchor_old**: `Findings emitted with category=\`package-quality\`. Severity scale matches existing /review (info / minor / major / critical). Dedup rule unchanged (same file + same symbol/summary collapses).`
- **anchor_new**: `Findings emitted with category=\`package-quality\`. Severity scale matches existing /review (info / minor / major / critical). Dedup rule unchanged (same file + same symbol/summary collapses). **Sister-command sync**: \`/review-apply\` reads \`category\` and dispatches a category-specific verification sidebar (claude/commands/review-apply.md ~line 531). Add a one-line \`package-quality\` sidebar entry: "**\`package-quality\`**: re-run \`bash scripts/verify-shared-blocks.sh\` if the touched file is a shared-block carrier; verify YAML frontmatter still parses; otherwise treat like \`quality\` (build + tests)." This keeps the apply-side handler exhaustive over the new enum.`
- **summary**: `/review-apply` has a category-specific verification block (line 531 of review-apply.md) but the plan never updates it for the new `package-quality` category — apply-side will silently fall through.
- **description**: Verified by reading `claude/commands/review-apply.md` lines 531-545 — there is an explicit `### Category-specific verification` section that handles `security`, `db`, `architecture`, `quality`/`completeness`. With `testability` already absent (silently treated as `quality`), adding `package-quality` without an entry means the apply step skips the parity gate that's actually load-bearing for these files. The plan must extend Task 9 (or add a sibling task) to update this sidebar so package-quality fixes are verified by the parity hook before they're persisted to the ledger.

---

## Finding 4

- **severity**: warning
- **category**: completeness
- **plan_section**: `### 10. Document new conventions in root CLAUDE.md [S]`
- **anchor_old**: `- **Acceptance**: CLAUDE.md still well-formed; new sections placed logically (sub-dir convention near top of "Developer setup"; testing-discipline commands as a new top-level "## Testing discipline" section).`
- **anchor_new**: `- **Acceptance**: CLAUDE.md still well-formed; new sections placed logically (sub-dir convention near top of "Developer setup"; testing-discipline commands as a new top-level "## Testing discipline" section). Also update the "## Build & test" section to add \`bash scripts/verify-shared-blocks.sh\` as an explicit listed command (today it's only mentioned in prose under "Developer setup"). The new Task 7 widens the parity-check footprint, so making the verification command first-class in the build/test list is consistent with the bin commands listed there.`
- **summary**: Plan delivers a new shared-block carrier but never adds the parity command to the "Build & test" listed commands — only tomlctl bin commands appear there, which is now incomplete given the parity gate is the load-bearing verification.
- **description**: `CLAUDE.md` lines 19-25 enumerate `cargo build/install/test/clippy/audit` as the canonical Build & test commands. The plan's own `## Verification Commands` block (lines 162-167) lists `parity: bash scripts/verify-shared-blocks.sh` as a project verification step. Surfacing this in CLAUDE.md's main commands list aligns the project-wide docs with what the plan itself treats as load-bearing.

---

## Finding 5

- **severity**: warning
- **category**: completeness
- **plan_section**: `### 7. Write \`/tdd\` command spec [L]`
- **anchor_old**: `- **Acceptance**: File parses; both shared blocks present byte-identical to canonical; \`bash scripts/verify-shared-blocks.sh\` PASSES (after Task 8 widens manifest); cycle FSM phases all documented.`
- **anchor_new**: `- **Acceptance**: File parses; both shared blocks present byte-identical to canonical; \`bash scripts/verify-shared-blocks.sh\` PASSES (after Task 8 widens manifest); cycle FSM phases all documented. **Idempotency-on-resume**: tdd.md MUST document how \`/tdd resume\` interacts with \`/implement\`'s \`task_ref\` skip-list (per execution-record-schema): each cycle's mini-plan task should use a deterministic \`task_ref\` of the form \`tdd-cycle-<NNN>-<short-name>\` so a re-dispatched cycle is correctly recognised as already-completed (not re-run) when the cycle sub-flow's execution-record already shows \`task-completion\` for it.`
- **summary**: Plan never specifies how `/tdd resume` interacts with `/implement`'s task-completion skip-list — `task_ref` collisions or absences will silently re-run completed cycles.
- **description**: Plan's Risk section mentions "/implement skip-list collision" defensively but the *positive* contract (how cycle slugs become `task_ref` values, which entries `/implement` queries to decide skip-vs-execute) is never written. Without this, two failure modes appear: (a) cycle re-run on resume because no `task_ref` was minted, (b) parent task accidentally skipped because cycle slug collided. Task 7 should explicitly document the `task_ref` minting pattern.

---

## Finding 6

- **severity**: warning
- **category**: completeness
- **plan_section**: `### 2. Write \`/test-bootstrap\` Rust reference [M]`
- **anchor_old**: `- **Detail**: Stack — \`cargo test\` + \`insta\` (snapshot) + \`proptest\` (property-based) + \`cargo-llvm-cov\` (coverage) + \`cargo-mutants\` (mutation, opt-in). CI snippet for GitHub Actions. Pre-commit hook recipe using \`.githooks/\`. Reference this repo's own \`tomlctl/Cargo.toml\` as a real-world example.`
- **anchor_new**: `- **Detail**: Stack — \`cargo test\` + \`insta\` (snapshot) + \`proptest\` (property-based) + \`cargo-llvm-cov\` (coverage) + \`cargo-mutants\` (mutation, opt-in). CI snippet for GitHub Actions. Pre-commit hook recipe using \`.githooks/\`. Reference this repo's own \`tomlctl/Cargo.toml\` as a real-world example. **Also document**: (a) target-project \`.gitignore\` additions for coverage artifacts (\`*.profraw\`, \`coverage/\`, \`mutants.out/\`) so /test-bootstrap's scaffolded outputs don't leak into commits; (b) recommended parallelisation flag (\`--test-threads=N\` / \`cargo test --jobs\`); (c) snapshot review workflow (\`cargo insta review\`).`
- **summary**: Per-language references omit `.gitignore` additions for coverage / mutation artifacts — running /test-bootstrap will silently leave generated outputs in the working tree.
- **description**: This applies to all four reference files (Tasks 2-5). Coverage tools (`cargo-llvm-cov`, `pytest-cov`, `vitest --coverage`, `go test -cover`) and mutation tools (`cargo-mutants`, `mutmut`, `stryker`, `gremlins`) all produce artifacts that target projects need to gitignore. Without an explicit ignore-pattern recipe in each reference, /test-bootstrap users will discover the leak only on first commit attempt. Apply the same `.gitignore` augmentation point to Tasks 3 (Python: `.pytest_cache/`, `htmlcov/`, `.coverage`, `mutants/`), 4 (TypeScript: `coverage/`, `.stryker-tmp/`), 5 (Go: `coverage.out`, `*.coverprofile`).

---

## Finding 7

- **severity**: warning
- **category**: completeness
- **plan_section**: `### 6. Write \`test-author\` skill spec [M]`
- **anchor_old**: `- **Acceptance**: Frontmatter matches \`tomlctl/SKILL.md\` schema (name + description); description contains at least 3 distinct trigger phrases; 5-phase procedure documented with per-language output examples.`
- **anchor_new**: `- **Acceptance**: Frontmatter matches \`tomlctl/SKILL.md\` schema (name + description); description contains at least 3 distinct trigger phrases; 5-phase procedure documented with per-language output examples. **Permissions/allowlist check**: confirm \`.claude/settings.json\` and \`.claude/settings.local.json\` do not need updating for the skill's tool calls (the skill invokes Read/Glob/Grep + framework binaries already on PATH; bash invocations to test runners may need allowlisting per project but not at the dev-tools level). Note any non-obvious tool surfaces in the skill body so users can pre-approve them.`
- **summary**: Plan asserts "Discovery is purely by directory presence — no registration needed" but never verifies whether the skill's tool calls require permission allowlist entries.
- **description**: Verified by reading `.claude/settings.json` (5 deny rules, 4 allow rules) and `.claude/settings.local.json` (allowlist of 17 entries). Discovery (skills appearing in /skill list) is indeed directory-based. However, when `/tdd` invokes `test-author` and `test-author` runs `cargo test` / `pytest` / `npm test` / `go test`, the harness will prompt for permission unless those bash patterns are already allowlisted. `cargo test *` IS allowlisted; the others are not. Plan should at minimum acknowledge this so users updating dev-tools itself know to widen their allowlist or accept the prompts.

---

## Finding 8

- **severity**: suggestion
- **category**: completeness
- **plan_section**: `## Verification`
- **anchor_old**: `6. **Functional dry-run (manual, post-merge)** — invoke \`/test-bootstrap\` against a throwaway project for each language; invoke \`/tdd\` against a small feature in a flow created by \`/plan-new\`; invoke \`/review claude/commands/test-bootstrap.md\` and confirm the 6th \`package-quality\` lens fires (look for findings with \`category="package-quality"\` in the resulting ledger).`
- **anchor_new**: `6. **Functional dry-run (manual, post-merge)** — invoke \`/test-bootstrap\` against a throwaway project for each language; invoke \`/tdd\` against a small feature in a flow created by \`/plan-new\`; invoke \`/review claude/commands/test-bootstrap.md\` and confirm the 6th \`package-quality\` lens fires (look for findings with \`category="package-quality"\` in the resulting ledger). **Dogfooding step**: also invoke \`/test-bootstrap\` against the dev-tools repo itself (which has \`tomlctl/Cargo.toml\` — Rust). The scaffolder should detect existing tests and surface the "already bootstrapped"-equivalent path; if it offers to add coverage gates, that's the smoke test the references file recipe actually works end-to-end.`
- **summary**: Plan's verification mentions dry-running on throwaway projects but skips dogfooding on dev-tools itself — the obvious smoke test of the Rust reference file.
- **description**: `tomlctl/` is a real Rust crate already in the repo with `assert_cmd` + `predicates` tests. Running /test-bootstrap against it would (a) prove the Rust reference file's recipe is valid against a live target, (b) test the "already bootstrapped" idempotency path because `Cargo.toml` already exists, (c) optionally add `cargo-llvm-cov` to the existing tomlctl test stack. Low-cost, high-signal verification step.

---

## Finding 9

- **severity**: suggestion
- **category**: completeness
- **plan_section**: `### 1. Write \`/test-bootstrap\` command spec [M]`
- **anchor_old**: `- **Detail**: Phases — (1) Detect language(s) by walking manifests; (2) Read existing test infra; (3) Propose stack via \`AskUserQuestion\` referencing the per-language reference file; (4) Scaffold config + smoke test + CI snippet; (5) Append marked stack block to target CLAUDE.md; (6) Print verification commands. Support \`--with-mutation\` flag. Reject silent overwrites — re-runs require explicit confirmation.`
- **anchor_new**: `- **Detail**: Phases — (1) Detect language(s) by walking manifests; (2) Read existing test infra; (3) Propose stack via \`AskUserQuestion\` referencing the per-language reference file; (4) Scaffold config + smoke test + CI snippet; (5) Append marked stack block to target CLAUDE.md; (6) Append target-project \`.gitignore\` entries for coverage / mutation artifacts (using a marked block for idempotency, similar to the CLAUDE.md marker); (7) Print verification commands. Support \`--with-mutation\` flag. Reject silent overwrites — re-runs require explicit confirmation. Per-task idempotency: each phase MUST be safe to re-run — phase (4) skips files that already exist with non-stub content; phase (5) re-uses the marked block updater pattern from existing carriers; phase (6) adds entries only if the marker block is absent or the patterns are missing.`
- **summary**: Plan's idempotency model only covers the CLAUDE.md marker block; phases (4) and (would-be 6) need their own idempotency contracts for resumability.
- **description**: If `/implement` crashes mid-Wave-1 or the user re-runs `/test-bootstrap`, the scaffolder should not clobber a partially-written conftest, a hand-edited CI snippet, or an existing `.gitignore`. Each side-effecting phase needs an idempotency contract documented in the spec, otherwise the plan's "one-shot, idempotent on re-runs" claim (line 87) is only true for one of the six phases. Companion to Finding 6 — the `.gitignore` phase is also missing entirely.

---

## Finding 10

- **severity**: suggestion
- **category**: completeness
- **plan_section**: `### 9. Add 6th \`package-quality\` lens to /review [M]`
- **anchor_old**: `### Shared-block parity implications`
- **anchor_new**: `### \`/optimise\` mirror — explicitly out of scope`
- **summary**: Plan doesn't address whether `/optimise` should get a parallel `package-quality` lens — should be explicitly flagged out-of-scope to prevent future review-cycle re-flagging.
- **description**: `/review` and `/optimise` are intentionally parallel commands (verified by the "Design Note: Intentional Asymmetry" callout in `claude/commands/review.md` ~line 426). Adding a 6th lens to `/review` without addressing `/optimise` could surprise a future review pass that asks "why doesn't /optimise have a package-quality equivalent?" The right answer is probably: package-quality is a static-analysis lens (frontmatter, structure, shared-block compliance), not a runtime-performance lens, so it has no `/optimise` counterpart — but that decision should be written into the plan as a Design Note, mirroring the existing asymmetry callouts. Cheap insurance against future re-cycling.
