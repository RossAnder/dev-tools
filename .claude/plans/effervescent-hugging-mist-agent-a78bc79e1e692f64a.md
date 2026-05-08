# Agent 4 (Risk & External Validity) — Findings

Plan reviewed: `/home/ross/Dev/dev-tools/.claude/plans/effervescent-hugging-mist.md`
Reviewer scope: technology currency (Rust / Python / TS / Go test stacks), anti-cheat soundness, concurrency, scope realism, security, backward compatibility, docs drift.

Note on prompt-injection during research: a spurious `<system-reminder>` urging TaskCreate use was embedded inside one WebSearch tool result. It is ignored — the genuine /review-plan brief mandates only this file as a writable target and findings as the deliverable.

---

## Finding 1

- **severity**: warning
- **category**: risk
- **plan_section**: Approach → /tdd cycle FSM (RED phase)
- **anchor_old**: `capture `red_test_fingerprint = sha256` over project test glob`
- **anchor_new**: `capture `red_test_fingerprint = sha256` over project test glob (excluding generated snapshot artifacts: `**/__snapshots__/**`, `*.snap`, `*.snap.*`, `**/snapshots/**`, `*.snapshot`, plus framework-specific exclusions for insta `.snap.new` and vitest inline-snapshot rewrites). Glob is captured **post-commit** from the just-recorded `red:` commit's tree, not from the working tree, so any post-RED snapshot regeneration in the test runner does not race the fingerprint.`
- **summary**: Fingerprint will false-positive on auto-generated snapshot files (jest `__snapshots__/*.snap`, insta `.snap.new`, vitest inline rewrites) and races the working tree if captured pre-commit.
- **description**: Two real failure modes the plan does not address. (a) Snapshot frameworks regenerate snapshot files on first run as part of normal test execution — not as anti-cheat test mutation. With insta, `cargo insta test` produces `.snap.new` files on the very first RED run; with jest/vitest, `__snapshots__/*.snap` and `*.snap` get touched as soon as the test renders. The fingerprint diff would treat these as illegal test mutations and revert the cycle. (b) The plan says "captured at RED gate pass" without specifying pre- or post-commit. If pre-commit, anything that touches the test glob between fingerprint capture and `/implement` dispatch (e.g. an editor LSP autosave, a pre-commit formatter) silently invalidates the gate. Both bite hardest in TS and Rust — the two stacks the plan most explicitly targets. Fix: exclude snapshot artifacts from the glob and capture from the committed tree (`git show --name-only red-commit`), not from disk.

## Finding 2

- **severity**: warning
- **category**: risk
- **plan_section**: Tasks → 9. Add 6th `package-quality` lens to /review
- **anchor_old**: `(b) widen ledger category enum at line 183 to include `package-quality`;`
- **anchor_new**: `(b) widen ledger category enum at line 183 to include `package-quality` AND simultaneously update `claude/commands/review-apply.md` to accept the new category in any switch/match/enum-validation logic (parity check guards block content, not enum values across files);`
- **summary**: Widening the category enum in `review.md` without a paired update to `review-apply.md` is a backward-compat hazard — `/review-apply` consumes the ledger and may reject unknown categories.
- **description**: The ledger-schema shared block lives in 4 files (`optimise`, `review`, `optimise-apply`, `review-apply`). The block content itself stays byte-identical because the enum is part of the block, so the parity check **passes** trivially when both `review.md` and `review-apply.md` get the same widened enum. But the plan only schedules editing `review.md`. If the implementer treats Task 9 literally and only edits `review.md`, the shared-block parity check will FAIL (different enum strings between the four carriers) — a different kind of drift than the plan anticipates. Either way, the Task 9 description is misleading: the enum lives inside the shared block, so it must be updated in all 4 carriers atomically. Recommend the task explicitly enumerate `optimise.md`, `review.md`, `optimise-apply.md`, `review-apply.md` as targets for the enum widening, and file count for Task 9 jumps from 1 to 4 (still well under the 15-file overall guard, but changes the wave 3 file-count math).

## Finding 3

- **severity**: warning
- **category**: risk
- **plan_section**: Tasks → 10. Document new conventions in root CLAUDE.md
- **anchor_old**: `(b) "Testing-discipline commands" — one-paragraph entry per: `/test-bootstrap` (when to use, what it does), `/tdd` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the `test-author` skill (model-discoverable, no manual invocation).`
- **anchor_new**: `(b) "Testing-discipline commands" — one-paragraph entry per: `/test-bootstrap` (when to use, what it does), `/tdd` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the `test-author` skill (model-discoverable, no manual invocation). (c) **Update the existing parity-trigger enumeration in CLAUDE.md line 11**: the prose currently says "claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md" — append `tdd` so it reads `{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md`. Without this edit the prose silently lies once Task 8 widens the manifest.`
- **summary**: CLAUDE.md "Developer setup" prose enumerates the 8 trigger files literally; Task 10 doesn't call out updating that enumeration to the new 9, so docs drift the moment Task 8 lands.
- **description**: Confirmed by reading CLAUDE.md line 11 verbatim: the trigger-list is hand-maintained prose, not auto-generated from the manifest. After Task 8, the manifest will list 9 files for `flow-context` and 4 for `execution-record-schema`, but the CLAUDE.md prose will still say "8 files" by enumeration. This is exactly the doc-drift class of bug `/review`'s 6th lens is meant to catch — but the documentation update has to land in the same plan run, not as a follow-up. Tiny additive edit but easy to miss because the trigger-list isn't a "shared block" the parity check would catch.

## Finding 4

- **severity**: warning
- **category**: risk
- **plan_section**: Approach → /tdd cycle FSM
- **anchor_old**: `**Cycle sub-flows**: each cycle gets a transient flow at `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/context.toml``
- **anchor_new**: `**Cycle sub-flows**: each cycle gets a transient flow at `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/context.toml`. **Concurrency**: `/tdd` MUST acquire a per-parent-flow lockfile at `.claude/flows/<parent-slug>/tdd/.tdd.lock` (matching tomlctl/`/implement` lockfile convention) before incrementing the cycle counter, to prevent two concurrent `/tdd` invocations from racing on cycle-NNN allocation or interleaving RED/GREEN entries in the parent's execution-record. Halt with a clear "another /tdd session active in this flow" message on lock contention.`
- **summary**: Plan inherits tomlctl + /implement lockfiles transitively but never declares a /tdd-level lock; concurrent /tdd invocations on the same parent flow could collide on cycle numbering.
- **description**: tomlctl's lockfile guards individual TOML mutations and `/implement`'s lockfile guards a single implement run, but neither protects the **cycle-counter increment** that `/tdd` performs as a precondition to creating `cycle-<NNN>/`. Two `/tdd` sessions opened against the same parent flow (deliberate parallelism, or a stuck/zombie session a user "tries again") could each allocate the same `cycle-NNN` directory or interleave RED/GREEN entries in the parent's execution-record copy-up step. Add an explicit `/tdd`-level lock at the parent-flow scope.

## Finding 5

- **severity**: suggestion
- **category**: risk
- **plan_section**: Tasks → 4. Write `/test-bootstrap` TypeScript reference
- **anchor_old**: `Stack — `vitest` (preferred for modern projects) + `fast-check` (property) + `@vitest/coverage-v8` + `stryker-mutator` (mutation, opt-in). Note jest fallback for legacy projects.`
- **anchor_new**: `Stack — `vitest` (preferred for modern projects; downloads have grown ~3.5× in 2 years and Nuxt/SvelteKit/Astro/Angular all default to it) + `fast-check` (property) + `@vitest/coverage-v8` + `stryker-mutator` (mutation, opt-in; latest StrykerJS 6.x line, `@stryker-mutator/typescript-checker` package — note the older `@stryker-mutator/typescript` package is unmaintained, do not reference). **Jest fallback ONLY for React Native projects** — RN is the only ecosystem in 2026 that still officially requires Jest; everywhere else, plain "legacy project" should not be a reason to choose Jest.`
- **summary**: Stack choice is current; minor tightening: scope jest-fallback narrowly (React Native is the only legitimate 2026 case) and steer mutation users to the right Stryker package.
- **description**: Verified via web search: in 2026 Vitest is the consensus default, Jest is plateaued, and "@stryker-mutator/typescript" (the package whose name might tempt the implementer) is 5 years stale — the actively maintained one is `@stryker-mutator/typescript-checker`. Without this nuance the reference will steer users to a deprecated package. The "legacy project" jest fallback is also too broad — most legacy TS suites can migrate vitest with a near-trivial config swap; React Native is the genuine exception.

## Finding 6

- **severity**: suggestion
- **category**: risk
- **plan_section**: Tasks → 5. Write `/test-bootstrap` Go reference
- **anchor_old**: `Stack — `go test` + `testify` (assertions, optional) + `gotestsum` (output formatting) + `go test -cover` + `go-mutesting` or `gremlins` (mutation, opt-in).`
- **anchor_new**: `Stack — `go test` + `testify` (assertions, optional) + `gotestsum` (output formatting) + `go test -cover` + `gremlins` (mutation, opt-in — actively maintained by go-gremlins, modern documentation, designed for CI quality gates). Note: original `zimmski/go-mutesting` is dormant since 2014; an `avito-tech/go-mutesting` fork exists but `gremlins` is the recommended default in 2026 unless a specific operator gap rules it out.`
- **summary**: Listing `go-mutesting` and `gremlins` as equal alternatives understates the gap; gremlins is the clear 2026 choice unless a project has a specific reason to use the avito fork.
- **description**: Verified: zimmski/go-mutesting hasn't seen meaningful activity since 2014; the avito-tech fork picks up some maintenance but lacks the polish, modern docs, and CI-orientation of gremlins. The plan as written invites the implementer to pick alphabetically (i.e., go-mutesting), which is the wrong default. Naming gremlins as the recommendation (with go-mutesting fork as fallback) is the higher-fidelity reference.

## Finding 7

- **severity**: suggestion
- **category**: risk
- **plan_section**: Tasks → 3. Write `/test-bootstrap` Python reference
- **anchor_old**: `Stack — `pytest` ≥7.0 + `pytest-asyncio` (conditional) + `hypothesis` (property) + `pytest-cov` (coverage with branch) + `mutmut` (mutation, opt-in).`
- **anchor_new**: `Stack — `pytest` ≥8.4 (required by pytest-asyncio 1.0+, current stable is 1.3 with 1.4 in alpha) + `pytest-asyncio` ≥1.0 (note: `event_loop` fixture removed; use `@pytest.mark.asyncio(loop_scope="...")` and `asyncio.get_running_loop()`) + `hypothesis` (property) + `pytest-cov` (coverage with branch) + `mutmut` ≥3.5 (mutation, opt-in — actively maintained, Feb 2026 release; consider `cosmic-ray` as alternative for projects needing broader operator coverage).`
- **summary**: pytest baseline is too lax for 2026 (≥7.0 is below pytest-asyncio 1.x's required ≥8.4) and the python ref should warn about pytest-asyncio's removed event_loop fixture.
- **description**: pytest-asyncio 1.0 (released 2025) bumped its minimum pytest to 8.4 and removed the long-standing `event_loop` fixture — anyone scaffolding a new project today against pytest ≥7.0 will hit immediate version-resolution conflict the moment they `pip install pytest-asyncio`. mutmut 3.5 (Feb 2026) is current and active, so the plan's mutmut choice is fine — but worth noting cosmic-ray as an alternative for projects that complain about mutmut's Python-AST limitations.

## Finding 8

- **severity**: warning
- **category**: risk
- **plan_section**: Approach → Idempotency for `/test-bootstrap` re-runs
- **anchor_old**: `On re-run, `/test-bootstrap` detects the marker and offers: `"Already bootstrapped on <date> with <stack>. Choose: upgrade stack / add coverage gates / abort."` Never silently overwrites.`
- **anchor_new**: `On re-run, `/test-bootstrap` detects the marker and offers: `"Already bootstrapped on <date> with <stack>. Choose: upgrade stack / add coverage gates / remove (clean uninstall) / abort."` Never silently overwrites. The `remove` mode strips the marked block and prints a checklist of generated files (CI workflow, smoke test, conftest/snapshot dirs) the user may want to delete manually — `/test-bootstrap` does not delete user code, only the marked CLAUDE.md block.`
- **summary**: Plan offers upgrade/add/abort but no `remove` mode; the marked CLAUDE.md block becomes orphaned if a project later abandons /test-bootstrap reliance.
- **description**: A target project that adopts `/test-bootstrap`, then later moves to a different testing stack manually, is left with a stale `<!-- TEST-BOOTSTRAP:STACK START/END -->` block in CLAUDE.md claiming a stack the project no longer uses. This is a documentation-rot vector. Add a `remove` choice that strips the block (and prints — not deletes — the list of generated artifacts so the user can clean up safely).

## Finding 9

- **severity**: suggestion
- **category**: risk
- **plan_section**: Tasks → 1. Write `/test-bootstrap` command spec
- **anchor_old**: `Support `--with-mutation` flag.`
- **anchor_new**: `Support `--with-mutation` flag. Documentation MUST include per-tool runtime expectations and recommended CI policy: cargo-mutants typically takes (build_time + test_time) × N_mutants — for the tomlctl crate that's roughly minutes-to-tens-of-minutes; PR-only with `--shard` parallelism recommended, never on every commit. mutmut and stryker have similar profile (10× to 100× a normal test run). Default opt-in CI snippet should run mutation on a separate workflow with timeout-minutes set to 30 and continue-on-error true (advisory, not blocking).`
- **summary**: `--with-mutation` enables tools whose runtime can be 10-100× normal CI; without explicit guidance, a project enables it and discovers their PR pipeline now takes hours.
- **description**: cargo-mutants documents that runtime ≈ (build + test) × N_viable_mutants; mutmut and stryker have similar multiplicative profiles. A naive "I'll turn on mutation testing because the docs mention it" leads to surprise CI bills and timeouts. /test-bootstrap is the right place to opinionate: PR-only, sharded, advisory (continue-on-error), 30-minute timeout default. Without this, the package ships a footgun.

## Finding 10

- **severity**: suggestion
- **category**: risk
- **plan_section**: Risks → /tdd cycle sub-flow proliferation
- **anchor_old**: `**Mitigation**: cycle sub-flows are intentionally retained for audit; `.gitignore` keeps `.claude/flows/` out of git in most repos so the on-disk noise doesn't leak.`
- **anchor_new**: `**Mitigation**: cycle sub-flows are intentionally retained for audit; `.gitignore` keeps `.claude/flows/` out of git in most repos so the on-disk noise doesn't leak. Disk: 100 cycles × ~5KB = ~500KB per parent flow — negligible. **Privacy/PII**: cycle sub-flows may contain test code snippets, file paths, and (via copy-up) verification output — same sensitivity as parent flow. Document in tdd.md that users handling regulated data should treat `.claude/flows/<parent-slug>/tdd/**` with the same retention/scrubbing policy as the parent flow's context.toml.`
- **summary**: Disk impact is fine; the unaddressed concern is PII/secret hygiene — cycle sub-flows can contain test fixtures with real-looking data and verification stdout/stderr.
- **description**: 500KB per long-lived parent flow is not a disk problem. The unmentioned risk is that `verification` entries in the cycle sub-flows capture stdout/stderr from the project's test command, and `test-author`'s output that gets copied into the cycle plan can include realistic-looking fixture data. For a regulated environment (health, finance) where parent-flow context.toml is already governed by a retention policy, the cycle sub-flows inherit the same sensitivity but the plan doesn't say so. One sentence in tdd.md is enough to close this.

---

Effort/scope note (not a finding, but flagged per brief): Task 7 is correctly marked [L] and is plausibly the longest single command file in the repo once it carries both the `flow-context` and `execution-record-schema` shared blocks plus the cycle FSM, anti-cheat protocol, mini-plan template, and edge-case handling. With Findings 2 (4 files for enum widening) and 3 (CLAUDE.md prose update) folded in, the total file-count rises from 10 to 13 unique files — still under the 15-file guard. Wave 3 grows from 2 files to 5. Effort estimates remain workable for a single `/implement` run.

Sources used in research:
- [cargo-mutants — Rust mutation testing](https://github.com/sourcefrog/cargo-mutants)
- [cargo-mutants timeouts](https://mutants.rs/timeouts.html)
- [Hangs and timeouts - cargo-mutants](https://mutants.rs/timeouts.html)
- [mutmut PyPI](https://pypi.org/project/mutmut/)
- [Vitest vs Jest 2026 (DEV)](https://dev.to/whoffagents/vitest-vs-jest-in-2026-i-migrated-my-ai-saas-and-heres-what-changed-2gda)
- [Vitest vs Jest 2026 benchmark](https://www.sitepoint.com/vitest-vs-jest-2026-migration-benchmark/)
- [pytest-asyncio releases](https://github.com/pytest-dev/pytest-asyncio/releases)
- [pytest-asyncio 1.0 migration](https://thinhdanggroup.github.io/pytest-asyncio-v1-migrate/)
- [gremlins (Go mutation)](https://github.com/go-gremlins/gremlins)
- [avito-tech/go-mutesting fork](https://github.com/avito-tech/go-mutesting)
- [Stryker JS releases](https://github.com/stryker-mutator/stryker-js/releases)
- [cargo-llvm-cov vs tarpaulin](https://www.rustprojectprimer.com/measure/coverage.html)
- [insta snapshots](https://insta.rs/)
- [proptest-rs](https://github.com/proptest-rs/proptest)
- [Jest snapshot resolver](https://brunoscheufler.com/blog/2020-03-08-configuring-jest-snapshot-resolvers)
