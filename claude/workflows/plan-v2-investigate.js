export const meta = {
  name: 'plan-v2-investigate',
  description: 'Answer the investigation lenses of a v2 plan: one plan-investigator per lens, each writing one note',
  whenToUse: 'Launched by the plan-v2-engine skill at its Investigate step with the slug and the lenses still missing a note as args; not for direct use',
  phases: [
    { title: 'Investigate', detail: 'one plan-investigator per lens; each writes docs/flows/<slug>/notes/<lens>.md' },
  ],
}

// Agents write their own notes, so this returns only an index. A lens prompt is
// built from `args` alone, so a same-args relaunch hits the journal up to the
// first failed agent, then reruns it and every agent started after it. A failed
// lens is reported rather than retried; the engine decides what runs again.
const SCRIPT_VERSION = '1'
const MAX_LENSES = 6
const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
const LENS_ID = /^[a-z0-9][a-z0-9-]*$/

// An agent that wrote nothing returns `note` as an empty string.
const RETURN_SCHEMA = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    verdict: { type: 'string', pattern: '^[^\\r\\n]+$' },
    note: { type: 'string' },
  },
  required: ['id', 'verdict', 'note'],
  additionalProperties: false,
}

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const index = (notes) => ({ slug: a.slug || null, notes, errors, script_version: SCRIPT_VERSION })

// RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
const matches = (re, value) => typeof value === 'string' && re.test(value)

const argProblems = () => {
  const problems = []
  if (!matches(SLUG, a.slug)) problems.push(`slug ${JSON.stringify(a.slug)} does not match ${SLUG.source}`)
  if (!Array.isArray(a.lenses) || !a.lenses.length) {
    problems.push('lenses must be a non-empty array')
    return problems
  }
  if (a.lenses.length > MAX_LENSES) problems.push(`${a.lenses.length} lenses exceeds the limit of ${MAX_LENSES}`)
  const seen = new Set()
  a.lenses.forEach((lens, i) => {
    const id = lens && lens.id
    if (!matches(LENS_ID, id)) problems.push(`lens ${i}: id ${JSON.stringify(id)} does not match ${LENS_ID.source}`)
    else if (seen.has(id)) problems.push(`lens ${i}: id ${id} repeats, and two agents would share one note`)
    seen.add(id)
    const question = lens && lens.question
    if (typeof question !== 'string' || !question.trim() || /[\r\n]/.test(question)) {
      problems.push(`lens ${i}: question must be one non-empty line, because a later line could pose as a \`note:\` line`)
    }
  })
  return problems
}

const problems = argProblems()
if (problems.length) {
  problems.forEach(fail)
  fail('no agent launched')
  return index([])
}

const notePath = (id) => `docs/flows/${a.slug}/notes/${id}.md`

const lensPrompt = (lens) => `lens: ${lens.id}
question: ${lens.question}
note: ${notePath(lens.id)}

Answer the question and write your note at that path, as your procedure says. If a file is already at that path, write nothing, return \`note\` as an empty string and say so in \`verdict\`. Return only \`{id, verdict, note}\`.`

// `note` is compared by suffix because an agent may report the path it resolved.
const namesNote = (returned, expected) => {
  const path = returned.replace(/\\/g, '/')
  return path === expected || path.endsWith(`/${expected}`)
}

const toEntry = (lens, ret) => {
  const expected = notePath(lens.id)
  if (!ret) {
    fail(`lens ${lens.id}: agent returned nothing; check notes/ for what it wrote`)
    return { id: lens.id, status: 'no-return', verdict: null, note: null }
  }
  if (!ret.note) {
    fail(`lens ${lens.id}: agent wrote no note: ${ret.verdict}`)
    return { id: lens.id, status: 'no-note', verdict: ret.verdict, note: null }
  }
  if (ret.id !== lens.id || !namesNote(ret.note, expected)) {
    fail(`lens ${lens.id}: agent returned id ${ret.id} and note ${ret.note}; expected ${expected}`)
    return { id: lens.id, status: 'mismatch', verdict: ret.verdict, note: ret.note }
  }
  return { id: lens.id, status: 'ok', verdict: ret.verdict, note: expected }
}

phase('Investigate')
// A throw inside a thunk resolves to null here, where a directly awaited one would fail the whole run.
const returns = await parallel(a.lenses.map((lens) => () => agent(lensPrompt(lens), {
  agentType: 'plan-investigator',
  schema: RETURN_SCHEMA,
  label: `lens ${lens.id}`,
  phase: 'Investigate',
})))

return index(a.lenses.map((lens, i) => toEntry(lens, returns[i])))
