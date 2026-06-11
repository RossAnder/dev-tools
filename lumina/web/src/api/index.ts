// Barrel re-export for the lumina wire layer.
//
// Created by T7 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md) when the monolithic
// `lumina/web/src/api.ts` was split into per-family modules under
// `lumina/web/src/api/`. Every existing consumer's `import { ... } from
// '@/api'` continues to resolve through this barrel without edits.
//
// New per-family api modules are appended here as they land (e.g. the
// vectorized-brewing-boole slice's `execution`, then Wave 2a's
// `sprints`/`worktrees`). The Wave-1 stream plumbing (`./ws-core`,
// `./stream`) is deliberately NOT barreled — those are internal modules that
// composables import directly by path.

export * from './http'
export * from './wire-enums'
export * from './work-items'
export * from './repo-links'
export * from './scalars'
export * from './structured-patches'
export * from './readiness'
export * from './acceptance-criteria'
export * from './research-notes'
export * from './risks'
export * from './rejected-alternatives'
export * from './task-deps'
export * from './open-questions'
export * from './findings'
export * from './activity'
export * from './context-blocks'
export * from './pty'
export * from './execution'
export * from './sprints'
export * from './worktrees'
