<!-- This CLAUDE.md was initialised by /test-bootstrap. -->

<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack (Vue SPA)

**Framework**: bun test (Bun's built-in runner) + fast-check 3.x (property-based testing)
**Coverage tool**: bun test --coverage (built-in; emits text + lcov)
**Mutation tool**: (none — opt-in via --with-mutation; not in default CI)
**Showcase tests**: src/__tests__/showcase.test.ts
**CI workflow**: (none — deferred; re-run /test-bootstrap when ready to add)
**Bootstrapped**: 2026-05-25 via /test-bootstrap

### Prerequisite

`bun` must be on PATH (`bun --version` ≥ 1.2). Install: https://bun.sh

### Local commands

- `bun install` — install dev-deps (one-time; or whenever package.json changes)
- `bun test` — run the full TS test suite (smoke + showcase)
- `bun test src/__tests__/showcase.test.ts` — run a specific file
- `bun test --coverage` — coverage report (text by default; add `--coverage-reporter=lcov` for CI upload)
- `bun test --watch` — re-run on save

### Build discipline in flows

The repo-wide **Build discipline in multi-agent flows** rule (root `CLAUDE.md`) applies to the SPA too: a sub-agent in `/implement` / `/optimise-apply` / `/review-apply` / `/tdd` must NOT run the full `bun run build` (it composes `type-check` + a Vite production bundle) or `bun test` to self-verify. Reach for the cheap `bun run type-check` (`vue-tsc --build`) **sparingly** — at most once near the end of a cluster — and leave the bundle build and test run to the orchestrator's single `verification` pass.

### Scope and limits

Bun test runs plain TypeScript and JavaScript natively (no transpile step). Vue SFC (`.vue`) component rendering is OUT OF SCOPE for this scaffold — Bun has no native Vue compiler. The showcase covers what bun test does well: pure TS unit tests on composables (`src/composables/*.ts`), API-client wrappers (`src/api.ts`), and any pure helpers. If full Vue component-rendering tests are needed later, add Vitest + `@vue/test-utils` alongside (they coexist with bun test on the same codebase) and re-run /test-bootstrap.
<!-- TEST-BOOTSTRAP:STACK END -->
