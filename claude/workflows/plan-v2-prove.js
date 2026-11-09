export const meta = {
  name: 'plan-v2-prove',
  description: 'Cold-read the milestones of a v2 plan: one plan-cold-reader per milestone, each writing one note',
  whenToUse: 'Launched by the plan-v2-engine skill at its Prove step with the slug and the milestones to cold-read as args; not for direct use',
  phases: [
    { title: 'Cold read', detail: 'one plan-cold-reader per milestone; each writes docs/flows/<slug>/notes/cold-read-<milestone>.md' },
  ],
}

// Agents write their own notes, so this returns only an index. A milestone
// prompt is built from `args` alone, so a same-args relaunch hits the journal up
// to the first failed agent, then reruns it and every agent started after it. A
// failed read is reported rather than retried; the engine decides what runs again.
const SCRIPT_VERSION = '1'
const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
const MILESTONE_ID = /^[A-Za-z0-9][A-Za-z0-9-]*$/

// An agent that wrote nothing returns `note` as an empty string.
const RETURN_SCHEMA = {
  type: 'object',
  properties: {
    unactionable: { type: 'integer', minimum: 0 },
    false_premises: { type: 'integer', minimum: 0 },
    note: { type: 'string' },
  },
  required: ['unactionable', 'false_premises', 'note'],
  additionalProperties: false,
}

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const index = (reads) => ({ slug: a.slug || null, reads, errors, script_version: SCRIPT_VERSION })

// RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
const matches = (re, value) => typeof value === 'string' && re.test(value)

const argProblems = () => {
  const problems = []
  if (!matches(SLUG, a.slug)) problems.push(`slug ${JSON.stringify(a.slug)} does not match ${SLUG.source}`)
  if (!Array.isArray(a.milestones) || !a.milestones.length) {
    problems.push('milestones must be a non-empty array')
    return problems
  }
  // Ids are compared case-folded because notes on a case-insensitive filesystem would collide.
  const seenIds = new Set()
  const seenTasks = new Map()
  a.milestones.forEach((m, i) => {
    const id = m && m.id
    if (!matches(MILESTONE_ID, id)) problems.push(`milestone ${i}: id ${JSON.stringify(id)} does not match ${MILESTONE_ID.source}`)
    else if (seenIds.has(id.toLowerCase())) problems.push(`milestone ${i}: id ${id} repeats, and two agents would share one note`)
    else seenIds.add(id.toLowerCase())
    const tasks = m && m.tasks
    if (!Array.isArray(tasks) || !tasks.length) {
      problems.push(`milestone ${i}: tasks must be a non-empty array`)
      return
    }
    tasks.forEach((n) => {
      if (!Number.isInteger(n) || n < 1) problems.push(`milestone ${i}: task ${JSON.stringify(n)} is not a positive integer`)
      else if (seenTasks.has(n)) problems.push(`milestone ${i}: task ${n} is already in milestone ${seenTasks.get(n)}`)
      else seenTasks.set(n, i)
    })
  })
  return problems
}

const problems = argProblems()
if (problems.length) {
  problems.forEach(fail)
  fail('no agent launched')
  return index([])
}

const notePath = (id) => `docs/flows/${a.slug}/notes/cold-read-${id}.md`

const fetchLines = (n) => `- task ${n}: \`tomlctl plan view --slug ${a.slug} --for worker --task ${n}\`, then \`tomlctl tasks show ${n} --slug ${a.slug} --select id,files\``

const milestonePrompt = (m) => `milestone: ${m.id}, tasks ${m.tasks.join(', ')}
note: ${notePath(m.id)}

Fetch each task with these commands, run as given:
${m.tasks.map(fetchLines).join('\n')}

Read the milestone as a worker and write your note at that path, as your procedure says. If a file is already at that path, write nothing and return \`note\` as an empty string. Return only \`{unactionable, false_premises, note}\`.`

// `note` is compared by suffix because an agent may report the path it resolved.
const namesNote = (returned, expected) => {
  const path = returned.replace(/\\/g, '/')
  return path === expected || path.endsWith(`/${expected}`)
}

const toEntry = (m, ret) => {
  const expected = notePath(m.id)
  if (!ret) {
    fail(`milestone ${m.id}: agent returned nothing; check notes/ for what it wrote`)
    return { id: m.id, status: 'no-return', unactionable: null, false_premises: null, note: null }
  }
  const counts = { unactionable: ret.unactionable, false_premises: ret.false_premises }
  if (!ret.note) {
    fail(`milestone ${m.id}: agent wrote no note`)
    return { id: m.id, status: 'no-note', ...counts, note: null }
  }
  if (!namesNote(ret.note, expected)) {
    fail(`milestone ${m.id}: agent returned note ${ret.note}; expected ${expected}`)
    return { id: m.id, status: 'mismatch', ...counts, note: ret.note }
  }
  return { id: m.id, status: 'ok', ...counts, note: expected }
}

phase('Cold read')
// A throw inside a thunk resolves to null here, where a directly awaited one would fail the whole run.
const returns = await parallel(a.milestones.map((m) => () => agent(milestonePrompt(m), {
  agentType: 'plan-cold-reader',
  schema: RETURN_SCHEMA,
  label: `milestone ${m.id}`,
  phase: 'Cold read',
})))

return index(a.milestones.map((m, i) => toEntry(m, returns[i])))
