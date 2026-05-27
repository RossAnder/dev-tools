// Open-questions wire wrappers (migration 0003).
//
// Filled in by T11a of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrappers over the five
// axum routes added by Phase-2 task T5 (`lumina/src/http/open_questions.rs`):
//   * POST  /work-items/{story_id}/open-questions                — add question; 201 + { id }
//   * POST  /open-questions/{id}/options                          — add option; 201 + { id }
//   * POST  /work-items/{task_id}/block-on-question/{question_id} — block; 201 + { ok }
//   * PATCH /work-items/{task_id}/enabling-option/{option_id}    — set branch; 200 + { ok }
//   * POST  /open-questions/{id}/resolve                          — resolve; 200 + { ok }
//
// Schemas: `OpenQuestion` / `QuestionOption` (and their zod schemas) are still
// declared inline in `./work-items` (T7 deferred the move to a future cleanup
// to keep wave-1 parallel agents off that file). We RE-EXPORT them here so
// `@/api` consumers see them via either entry — and so a future cleanup can
// flip the source-of-truth without churning call sites.

import * as z from 'zod'

import { API_BASE, handle } from './http'

// Re-exports — see file-level comment. The inline-declared schemas live in
// `./work-items` for now; this barrel-style re-export means `@/api` consumers
// can already import them from the per-family file (forward-compat with the
// future move).
export {
  OpenQuestionSchema,
  QuestionOptionSchema,
  type OpenQuestion,
  type QuestionOption,
} from './work-items'

/** Response shape of `POST /api/work-items/{story_id}/open-questions`. */
const AddOpenQuestionResponseSchema = z.object({ id: z.string() })

/** Response shape of `POST /api/open-questions/{id}/options`. */
const AddQuestionOptionResponseSchema = z.object({ id: z.string() })

/** Response shape of the three `{ ok: true }` routes. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/**
 * Body accepted by `POST /api/open-questions/{id}/options`.
 *
 * The server's `AddQuestionOptionBody` carries `#[serde(default, alias = "kind")]`
 * on `detail`, so legacy callers may pass `{ kind }` instead. We surface only
 * the canonical `detail` field here; legacy callers should migrate.
 */
export interface AddQuestionOptionBody {
  label: string
  detail?: string
}

/**
 * `POST /api/work-items/{story_id}/open-questions` — add an open question to a
 * story. The repo rejects a non-story target with `Validation` (→ 422).
 * Returns the new `open_questions.id`.
 */
export async function addOpenQuestion(
  storyId: string,
  question: string,
): Promise<{ id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(storyId)}/open-questions`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ question }),
      },
    ),
    AddOpenQuestionResponseSchema,
  )
}

/**
 * `POST /api/open-questions/{id}/options` — add an answer option to an open
 * question. Returns the new `question_options.id`. `detail` is the canonical
 * key (the server accepts `kind` as an alias for legacy callers — prefer
 * `detail` going forward).
 */
export async function addQuestionOption(
  questionId: string,
  body: AddQuestionOptionBody,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/open-questions/${encodeURIComponent(questionId)}/options`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    AddQuestionOptionResponseSchema,
  )
}

/**
 * `POST /api/work-items/{task_id}/block-on-question/{question_id}` — block a
 * task on an open question (sets the task's `blocked_by_question_id` FK and
 * `status=blocked`). No body. Returns `{ ok: true }`.
 */
export async function blockTaskOnQuestion(
  taskId: string,
  questionId: string,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(taskId)}/block-on-question/${encodeURIComponent(questionId)}`,
      {
        method: 'POST',
        body: '',
      },
    ),
    OkResponseSchema,
  )
}

/**
 * `PATCH /api/work-items/{task_id}/enabling-option/{option_id}` — tie an
 * exclusive-branch task to a question option (so resolution either unblocks or
 * cancels it). No body. Returns `{ ok: true }`.
 */
export async function setEnablingOption(
  taskId: string,
  optionId: string,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(taskId)}/enabling-option/${encodeURIComponent(optionId)}`,
      {
        method: 'PATCH',
        body: '',
      },
    ),
    OkResponseSchema,
  )
}

/**
 * `POST /api/open-questions/{id}/resolve` — resolve an open question by picking
 * an option: unblocks the chosen branch's tasks (blocked→todo) and cancels the
 * other branches' exclusive tasks. One event for the whole resolution. Returns
 * `{ ok: true }`.
 */
export async function resolveOpenQuestion(
  questionId: string,
  chosenOptionId: string,
  by?: string,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(`${API_BASE}/open-questions/${encodeURIComponent(questionId)}/resolve`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(by === undefined ? { chosen_option_id: chosenOptionId } : { chosen_option_id: chosenOptionId, by }),
    }),
    OkResponseSchema,
  )
}
