// Bun tests for the AUQ keystroke calculator
// (`src/composables/auqKeystrokes.ts`, T7 of the lumina-interactive-prompts
// plan). The calculator is a pure function; no mocks, no module-singleton
// state, no fixtures beyond the AuqQuestion / AuqAnswer wire shapes from
// `api/pty.ts`.
//
// DSL semantics under test (verified in
// `docs/plans/lumina-interactive-prompts.preflight.md`):
//   - single-select: navigate via `down` × K from cursor=0 to the target option,
//     then `enter`.
//   - multi-select: for each selected label, navigate (`up`/`down`) from the
//     current cursor to that option's index, emit `space`. After all toggles,
//     emit `enter`. Empty selectedLabels → just `enter`.
//   - "Other": navigate `down` × N (lands on the last option in the option
//     list), emit `text:<literal>`, emit `enter`. No Tab focus token — the
//     textarea is implicitly focused on landing (E6 deviation).
//   - notes: silently ignored (E7 deferral). The calculator emits no
//     keystrokes for the `AuqAnswer.notes?` field even when set.
//   - multi-question: concatenate per-question sequences in order; the
//     picker auto-advances on `enter` (no separator token).
//
// Byte-safety rules for the `text:<literal>` body live in the Rust input
// bridge (T5). The TS calculator is byte-agnostic — it forwards the literal
// verbatim and the Rust side logs+skips on a control-byte hit. This file
// asserts that the literal IS emitted even when it contains embedded ESC.

import { describe, it, expect } from 'bun:test'

import { computeAuqKeystrokes } from '../composables/auqKeystrokes'
import type { AuqAnswer, AuqQuestion, InputFrame } from '../api/pty'

// ---------------------------------------------------------------------------
// Fixture builders.
// ---------------------------------------------------------------------------

function singleSelectQuestion(numOptions: number): AuqQuestion {
  return {
    question: `Pick one of ${numOptions}`,
    header: 'Single',
    multiSelect: false,
    options: Array.from({ length: numOptions }, (_, i) => ({
      label: `Option ${i}`,
      description: `Option ${i} description`,
    })),
  }
}

function multiSelectQuestion(numOptions: number): AuqQuestion {
  return {
    question: `Pick any of ${numOptions}`,
    header: 'Multi',
    multiSelect: true,
    options: Array.from({ length: numOptions }, (_, i) => ({
      label: `Option ${i}`,
      description: `Option ${i} description`,
    })),
  }
}

/** Build a `down` frame — the most common token in expected sequences. */
function down(): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: 'down' }
}

/** Build an `up` frame. */
function up(): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: 'up' }
}

/** Build a `space` frame. */
function space(): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: 'space' }
}

/** Build an `enter` frame. */
function enter(): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: 'enter' }
}

/** Build a `text:<literal>` frame. */
function text(literal: string): InputFrame {
  return { type: 'input', kind: 'keystroke', payload: `text:${literal}` }
}

// ---------------------------------------------------------------------------
// Single-select (Scenario 1).
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — single-select', () => {
  it('option 0 emits just enter (no navigation needed)', () => {
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 0'] }
    expect(computeAuqKeystrokes([q], [a])).toEqual([enter()])
  })

  it('option 2 emits down × 2 then enter', () => {
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 2'] }
    expect(computeAuqKeystrokes([q], [a])).toEqual([down(), down(), enter()])
  })

  it('option 3 of a 4-option picker emits down × 3 then enter', () => {
    const q = singleSelectQuestion(4)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 3'] }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      down(),
      down(),
      enter(),
    ])
  })
})

// ---------------------------------------------------------------------------
// Multi-select (Scenario 2).
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — multi-select', () => {
  it('toggles via space; submits via enter (single label at option 0)', () => {
    const q = multiSelectQuestion(3)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 0'] }
    // cursor=0, want 0: 0 nav frames, then space, then enter.
    expect(computeAuqKeystrokes([q], [a])).toEqual([space(), enter()])
  })

  it('toggles [0, 2, 3] in order with correct cursor delta (4-option multi)', () => {
    // cursor starts 0: space (toggle 0)
    // cursor 0 → 2: down × 2, then space (toggle 2)
    // cursor 2 → 3: down × 1, then space (toggle 3)
    // submit: enter
    const q = multiSelectQuestion(4)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 0', 'Option 2', 'Option 3'],
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      space(),
      down(),
      down(),
      space(),
      down(),
      space(),
      enter(),
    ])
  })

  it('handles backwards navigation via up when labels are picked in descending index order', () => {
    // selectedLabels=[Option 2, Option 0] for a 3-option multi:
    //   cursor=0 → 2: down × 2, space
    //   cursor=2 → 0: up × 2, space
    //   enter
    const q = multiSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 2', 'Option 0'],
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      down(),
      space(),
      up(),
      up(),
      space(),
      enter(),
    ])
  })

  it('empty selectedLabels emits just enter (no toggles)', () => {
    const q = multiSelectQuestion(3)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: [] }
    expect(computeAuqKeystrokes([q], [a])).toEqual([enter()])
  })

  it('repeated label at current cursor does not emit a redundant nav token', () => {
    // selectedLabels=[Option 1, Option 1] for a 3-option multi:
    //   cursor=0 → 1: down, space (toggle 1 on)
    //   cursor=1 → 1: 0 nav frames, space (toggle 1 off)
    //   enter
    const q = multiSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 1', 'Option 1'],
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      space(),
      space(),
      enter(),
    ])
  })
})

// ---------------------------------------------------------------------------
// "Other" (Scenario 3, E6 deviation applied — no Tab focus token).
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — Other', () => {
  it('navigates to last option (down × N), emits text:<literal>, then enter', () => {
    // For a 3-option question, "Other" is the implicit 4th row (index 3).
    // Cursor=0 → 3 requires down × 3.
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: 'pistachio',
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      down(),
      down(),
      text('pistachio'),
      enter(),
    ])
  })

  it('first-colon split is preserved in the DSL payload (text:vanilla:chocolate)', () => {
    // The DSL token is `text:vanilla:chocolate`; the Rust parser performs the
    // first-colon split and the literal body is `vanilla:chocolate`.
    // The calculator forwards the literal as-is.
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: 'vanilla:chocolate',
    }
    const frames = computeAuqKeystrokes([q], [a])
    // The text frame's payload must be exactly `text:vanilla:chocolate` —
    // the colon in the literal is preserved verbatim.
    const textFrame = frames.find(
      (f) => f.type === 'input' && f.kind === 'keystroke' && f.payload.startsWith('text:'),
    )
    expect(textFrame).toBeDefined()
    if (textFrame && textFrame.type === 'input' && textFrame.kind === 'keystroke') {
      expect(textFrame.payload).toBe('text:vanilla:chocolate')
    }
  })

  it('empty otherText emits text: + enter (calculator does not validate body)', () => {
    // Empty literal — the calculator is byte-agnostic and emits the empty
    // text frame; the Rust input bridge decides whether to skip / accept.
    const q = singleSelectQuestion(2)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: '',
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      down(),
      text(''),
      enter(),
    ])
  })

  it('literal containing embedded ESC is forwarded as-is (Rust bridge filters)', () => {
    // E6 / E7 + byte-safety: byte filtering is the Rust side's job
    // (`pty_transport.rs`, T5). The TS calculator forwards the literal
    // verbatim and the Rust side logs+skips on hit. The frame IS emitted.
    const q = singleSelectQuestion(2)
    const literalWithEsc = 'foo\x1bbar'
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: literalWithEsc,
    }
    const frames = computeAuqKeystrokes([q], [a])
    expect(frames).toEqual([
      down(),
      down(),
      text(literalWithEsc),
      enter(),
    ])
  })

  it('"Other" on a 1-option question still navigates down once to land on Other', () => {
    // Smallest case: 1 real option + "Other" as the implicit last row.
    const q = singleSelectQuestion(1)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: 'freeform',
    }
    expect(computeAuqKeystrokes([q], [a])).toEqual([
      down(),
      text('freeform'),
      enter(),
    ])
  })
})

// ---------------------------------------------------------------------------
// Notes deferral (E7).
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — notes (E7 deferral)', () => {
  it('notes field is silently ignored — same frames as notes-undefined', () => {
    const q = singleSelectQuestion(3)
    const baseAnswer: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 1'],
      otherText: undefined,
    }
    const withNotes: AuqAnswer = {
      ...baseAnswer,
      notes: 'some user note that should not produce keystrokes',
    }
    expect(computeAuqKeystrokes([q], [withNotes])).toEqual(
      computeAuqKeystrokes([q], [baseAnswer]),
    )
  })

  it('notes alongside "Other" otherText is also ignored', () => {
    const q = singleSelectQuestion(2)
    const baseAnswer: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: [],
      otherText: 'custom',
    }
    const withNotes: AuqAnswer = {
      ...baseAnswer,
      notes: 'should not produce keystrokes',
    }
    expect(computeAuqKeystrokes([q], [withNotes])).toEqual(
      computeAuqKeystrokes([q], [baseAnswer]),
    )
  })
})

// ---------------------------------------------------------------------------
// Multi-question (Scenario 6).
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — multi-question', () => {
  it('emits per-question sequences concatenated with no separator', () => {
    // 2-question AUQ, both single-select 3-option pickers, both answered
    // with Option 2. Cursor resets to 0 between questions (the picker
    // auto-advances on `enter`). Expected: down,down,enter,down,down,enter.
    const q0 = singleSelectQuestion(3)
    const q1 = singleSelectQuestion(3)
    const a0: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 2'] }
    const a1: AuqAnswer = { questionIndex: 1, selectedLabels: ['Option 2'] }
    expect(computeAuqKeystrokes([q0, q1], [a0, a1])).toEqual([
      down(),
      down(),
      enter(),
      down(),
      down(),
      enter(),
    ])
  })

  it('mixes a single-select + a multi-select question across two answers', () => {
    const q0 = singleSelectQuestion(3) // pick Option 1: down + enter
    const q1 = multiSelectQuestion(3) // pick [Option 0, Option 2]: space, down×2, space, enter
    const a0: AuqAnswer = { questionIndex: 0, selectedLabels: ['Option 1'] }
    const a1: AuqAnswer = {
      questionIndex: 1,
      selectedLabels: ['Option 0', 'Option 2'],
    }
    expect(computeAuqKeystrokes([q0, q1], [a0, a1])).toEqual([
      down(),
      enter(),
      space(),
      down(),
      down(),
      space(),
      enter(),
    ])
  })

  it('answers can be supplied for a subset of questions (only matching question advanced)', () => {
    // 3 questions, only one answer provided (questionIndex=1).
    // The calculator emits only that question's frames — the caller is
    // responsible for matching answers to questions.
    const q0 = singleSelectQuestion(2)
    const q1 = singleSelectQuestion(3)
    const q2 = singleSelectQuestion(2)
    const a1: AuqAnswer = { questionIndex: 1, selectedLabels: ['Option 1'] }
    expect(computeAuqKeystrokes([q0, q1, q2], [a1])).toEqual([
      down(),
      enter(),
    ])
  })
})

// ---------------------------------------------------------------------------
// Edge cases at the question/answer boundary.
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — empty inputs', () => {
  it('empty answers array returns empty frame list', () => {
    const q = singleSelectQuestion(3)
    expect(computeAuqKeystrokes([q], [])).toEqual([])
  })

  it('empty questions + empty answers returns empty frame list', () => {
    expect(computeAuqKeystrokes([], [])).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// Caller-bug surface — TypeError on invalid input.
// ---------------------------------------------------------------------------

describe('computeAuqKeystrokes — caller bugs', () => {
  it('throws TypeError on out-of-bounds questionIndex (positive)', () => {
    const q = singleSelectQuestion(2)
    const a: AuqAnswer = { questionIndex: 5, selectedLabels: ['Option 0'] }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on negative questionIndex', () => {
    const q = singleSelectQuestion(2)
    const a: AuqAnswer = { questionIndex: -1, selectedLabels: ['Option 0'] }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on non-integer questionIndex', () => {
    const q = singleSelectQuestion(2)
    const a: AuqAnswer = { questionIndex: 1.5, selectedLabels: ['Option 0'] }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on single-select label not in question.options', () => {
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Not A Real Option'],
    }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on multi-select label not in question.options', () => {
    const q = multiSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 0', 'Phantom Label'],
    }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on single-select with zero selectedLabels', () => {
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = { questionIndex: 0, selectedLabels: [] }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })

  it('throws TypeError on single-select with multiple selectedLabels', () => {
    const q = singleSelectQuestion(3)
    const a: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 0', 'Option 1'],
    }
    expect(() => computeAuqKeystrokes([q], [a])).toThrow(TypeError)
  })
})
