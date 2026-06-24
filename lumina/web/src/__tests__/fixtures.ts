// Shared test fixtures for the Vue SPA test suite.
//
// Keep this module's exports stable: every test file imports the WorkItemDetail
// builder from here, so a schema addition that breaks the shape must be
// reflected here (and the corresponding `__resetForTests` call survives the
// migration).

/**
 * Build a minimal WorkItemDetail object with sensible defaults; per-test
 * overrides via `overrides` (shallow merge — the typical override target is
 * one of the nested arrays like `acceptance_criteria` or `findings`).
 *
 * Mirrors the wire shape returned by `GET /api/work-items/{id}` BEFORE the
 * boundary boolean-normalises `acceptance_criteria[].checked`. Tests that
 * exercise the post-normalisation path build their own `acceptance_criteria`
 * override.
 */
export function workItemDetail(
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    item: {
      id: 's-1',
      kind: 'story',
      parent_id: null,
      title: 's',
      body: null,
      status: 'open',
      position: 0,
      attributes: null,
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      shape: null,
      plan_epoch: 0,
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-25T00:00:00Z',
    },
    children: [],
    findings: [],
    context_blocks: [],
    activity: [],
    acceptance_criteria: [],
    research_notes: [],
    open_questions: [],
    repo_links: [],
    risks: [],
    rejected_alternatives: [],
    task_dependencies: [],
    story_files_footprint: [],
    task_research_links: [],
    ...overrides,
  }
}
