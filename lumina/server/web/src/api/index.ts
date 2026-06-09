// Barrel re-export for the lumina wire layer.
//
// Created by T7 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md) when the monolithic
// `lumina/web/src/api.ts` was split into per-family modules under
// `lumina/web/src/api/`. Every existing consumer's `import { ... } from
// '@/api'` continues to resolve through this barrel without edits.
//
// Phase-5 family stubs (acceptance-criteria, research-notes, risks,
// rejected-alternatives, task-deps, open-questions, findings, activity,
// context-blocks, scalars, structured-patches, readiness) are pre-declared
// here so subsequent Phase-5 tasks (T8a/T8b/T9/T10/T11a/T11b) only fill in
// their owned file body — this barrel file is touched ONCE (by T7) and NOT
// re-edited.

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
