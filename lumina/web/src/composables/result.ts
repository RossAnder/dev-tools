/**
 * Discriminated-union return type for composable mutators. Lets call sites
 * narrow on success/failure without coupling to the singleton `error` ref
 * (which is still set as a side effect for the UI's error-banner subscription).
 *
 * Defaults `E = string` because most composables surface a flat error message
 * to the singleton; mutators that carry richer error shapes (e.g.
 * useTaskDependencies' `CycleOrError`) override `E` at the call site.
 */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }
