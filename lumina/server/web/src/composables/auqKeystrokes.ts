/**
 * Pure-TS calculator: AUQ answer → InputFrame[] of `kind: 'keystroke'` frames.
 *
 * The DSL tokens emitted here are translated to raw PTY bytes by lumina's
 * Rust input bridge (lumina/src/pty/pty_transport.rs, T5). The verified
 * DSL→byte mapping lives in `docs/plans/lumina-interactive-prompts.preflight.md`
 * and was empirically pinned via `script --log-in` captures of claude-code
 * v2.1.141 on 2026-05-28.
 *
 * Notes (`AuqAnswer.notes`) are silently ignored: claude-code v2.1.141's AUQ
 * picker does not expose a notes-focus keystroke (deferred per plan Risks §3
 * / execution-record E7).
 *
 * "Other" textbox is implicitly focused on selection — no Tab focus token
 * is required (corrected per execution-record E6).
 */

import type { InputFrame, AuqQuestion, AuqAnswer } from '../api/pty'

/** Build one keystroke InputFrame with the given DSL token payload. */
function keystroke(token: string): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: token }
}

/**
 * Compute the keystroke-frame sequence that drives claude's TUI AUQ picker
 * through `answers` (in array order) against the originating `questions`.
 *
 * Each answer's `questionIndex` is a 0-based index into `questions`. The
 * cursor is assumed to reset to option 0 at each question boundary (the
 * picker auto-advances on `enter`, fresh picker per question — see preflight
 * Scenario 6).
 *
 * Throws `TypeError` on caller bugs:
 *   - `questionIndex` out of range against `questions`.
 *   - Single-select label not found in the question's option list.
 *
 * Multi-select labels not found in the option list are likewise rejected
 * with `TypeError` (caller invariant: the picker SFC only emits known labels).
 */
export function computeAuqKeystrokes(
  questions: AuqQuestion[],
  answers: AuqAnswer[],
): InputFrame[] {
  const frames: InputFrame[] = []

  for (const answer of answers) {
    const qi = answer.questionIndex
    if (!Number.isInteger(qi) || qi < 0 || qi >= questions.length) {
      throw new TypeError(
        `computeAuqKeystrokes: questionIndex ${String(qi)} out of range [0, ${questions.length})`,
      )
    }
    const question = questions[qi]!

    // "Other" branch — `otherText` set means the user picked the free-text row.
    // "Other" is rendered as the LAST option (index = question.options.length).
    if (answer.otherText !== undefined) {
      const otherIndex = question.options.length
      for (let i = 0; i < otherIndex; i++) {
        frames.push(keystroke('down'))
      }
      frames.push(keystroke(`text:${answer.otherText}`))
      frames.push(keystroke('enter'))
      continue
    }

    // Multi-select branch — toggle each selected label via space, submit via enter.
    // An empty selectedLabels array means the user toggled nothing → just enter.
    if (question.multiSelect) {
      let cursor = 0
      for (const label of answer.selectedLabels) {
        const target = question.options.findIndex((opt) => opt.label === label)
        if (target < 0) {
          throw new TypeError(
            `computeAuqKeystrokes: multi-select label ${JSON.stringify(label)} not found in question[${qi}].options`,
          )
        }
        const delta = target - cursor
        if (delta > 0) {
          for (let i = 0; i < delta; i++) frames.push(keystroke('down'))
        } else if (delta < 0) {
          for (let i = 0; i < -delta; i++) frames.push(keystroke('up'))
        }
        cursor = target
        frames.push(keystroke('space'))
      }
      frames.push(keystroke('enter'))
      continue
    }

    // Single-select branch — exactly one label expected.
    if (answer.selectedLabels.length !== 1) {
      throw new TypeError(
        `computeAuqKeystrokes: single-select question[${qi}] requires exactly one selectedLabel, got ${answer.selectedLabels.length}`,
      )
    }
    const label = answer.selectedLabels[0]!
    const target = question.options.findIndex((opt) => opt.label === label)
    if (target < 0) {
      throw new TypeError(
        `computeAuqKeystrokes: single-select label ${JSON.stringify(label)} not found in question[${qi}].options`,
      )
    }
    for (let i = 0; i < target; i++) frames.push(keystroke('down'))
    frames.push(keystroke('enter'))
  }

  return frames
}
