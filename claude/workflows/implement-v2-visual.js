export const meta = {
  name: 'implement-v2-visual',
  description: 'Judge the captured UI states of one v2 milestone: ui-inspector agents over groups of at most four states, each writing one note',
  whenToUse: 'Launched by the implement-v2-engine skill after a UI milestone\'s gate passes, with the slug, the milestone, the visual round, the capture directory, the chosen design and the states, controls and looks as args; not for direct use',
  phases: [
    { title: 'Inspect', detail: 'one ui-inspector per group of at most four states; each writes docs/flows/<slug>/notes/run/visual-<milestone>-<round>-<n>.md' },
  ],
}

// Inspectors write their own notes, so this returns only an index: one entry
// per asked state, clean states first and then controls, each in args order.
// Controls are spread one per group before the clean states fill the groups,
// so as many inspectors as possible face a planted defect. A state's `status`
// is `ok` when its inspector returned a verdict for it, `no-return` when the
// inspector returned nothing, `error` when it threw or returned a malformed
// object, and `missing` when its return left the state out; `verdict` is null
// unless `ok`. Nothing is retried; the engine decides what runs again.
const SCRIPT_VERSION = '1'
const GROUP_SIZE = 4
const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
const MILESTONE_ID = /^[A-Za-z0-9][A-Za-z0-9-]*$/
// A state name becomes a capture file name and an `--only` value.
const STATE = /^[A-Za-z0-9][A-Za-z0-9_.-]*$/
const ONE_LINE = /^[^\r\n]+$/
const VERDICTS = ['pass', 'fail', 'unsure']

const DEFECT_SCHEMA = {
  type: 'object',
  properties: {
    class: { type: 'string' },
    location: { type: 'string' },
    detail: { type: 'string' },
  },
  required: ['class', 'location', 'detail'],
  additionalProperties: false,
}

const RETURN_SCHEMA = {
  type: 'object',
  properties: {
    states: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          state: { type: 'string' },
          verdict: { type: 'string', enum: VERDICTS },
          defects: { type: 'array', items: DEFECT_SCHEMA },
          note: { type: 'string' },
        },
        required: ['state', 'verdict', 'defects', 'note'],
        additionalProperties: false,
      },
    },
  },
  required: ['states'],
  additionalProperties: false,
}

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const index = (states) => ({ states, errors, script_version: SCRIPT_VERSION })

// RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
const matches = (re, value) => typeof value === 'string' && re.test(value)
const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v)

const nameListProblems = (key, list, { nonEmpty }) => {
  if (!Array.isArray(list)) return [`${key} must be an array of state names`]
  if (nonEmpty && !list.length) return [`${key} must be a non-empty array of state names`]
  return list
    .filter((s) => !matches(STATE, s))
    .map((s) => `${key}: ${JSON.stringify(s)} does not match ${STATE.source}`)
}

const argProblems = () => {
  const problems = []
  if (!matches(SLUG, a.slug)) problems.push(`slug ${JSON.stringify(a.slug)} does not match ${SLUG.source}`)
  if (!matches(MILESTONE_ID, a.milestone)) problems.push(`milestone ${JSON.stringify(a.milestone)} does not match ${MILESTONE_ID.source}`)
  if (!Number.isInteger(a.round) || a.round < 1) problems.push(`round ${JSON.stringify(a.round)} is not a positive integer`)
  if (!matches(ONE_LINE, a.capture_dir)) problems.push(`capture_dir ${JSON.stringify(a.capture_dir)} must be a one-line path`)
  if (!matches(ONE_LINE, a.design)) problems.push(`design ${JSON.stringify(a.design)} must be a one-line path`)
  const stateProblems = nameListProblems('states', a.states, { nonEmpty: true })
  const controlProblems = nameListProblems('controls', a.controls, { nonEmpty: false })
  problems.push(...stateProblems, ...controlProblems)
  if (stateProblems.length || controlProblems.length) {
    if (!Array.isArray(a.looks)) problems.push('looks must be an array')
    return problems
  }
  const seen = new Set()
  ;[...a.states, ...a.controls].forEach((s) => {
    if (seen.has(s)) problems.push(`state ${s} is named twice across states and controls`)
    seen.add(s)
  })
  if (!Array.isArray(a.looks)) {
    problems.push('looks must be an array')
    return problems
  }
  a.looks.forEach((l, i) => {
    if (!isObject(l)) { problems.push(`looks[${i}] is not an object`); return }
    const taskOk = (Number.isInteger(l.task) && l.task >= 1) || matches(ONE_LINE, l.task)
    if (!taskOk) problems.push(`looks[${i}].task ${JSON.stringify(l.task)} is neither a positive integer nor a one-line ref`)
    if (!Number.isInteger(l.accept) || l.accept < 0) problems.push(`looks[${i}].accept ${JSON.stringify(l.accept)} is not a non-negative integer`)
    if (typeof l.behaviour !== 'string' || !l.behaviour.trim()) problems.push(`looks[${i}].behaviour must be a non-empty string`)
    if (!Array.isArray(l.states) || !l.states.length) {
      problems.push(`looks[${i}].states must be a non-empty array of state names`)
    } else {
      l.states.forEach((s) => {
        if (!seen.has(s)) problems.push(`looks[${i}].states names ${JSON.stringify(s)}, which is in neither states nor controls`)
      })
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

const units = [
  ...a.states.map((state) => ({ state, control: false })),
  ...a.controls.map((state) => ({ state, control: true })),
]

const groupCount = Math.ceil(units.length / GROUP_SIZE)
const groups = Array.from({ length: groupCount }, () => [])
a.controls.forEach((state, i) => groups[i % groupCount].push({ state, control: true }))
let next = 0
a.states.forEach((state) => {
  while (groups[next].length >= GROUP_SIZE) next += 1
  groups[next].push({ state, control: false })
})

const notePath = (n) => `docs/flows/${a.slug}/notes/run/visual-${a.milestone}-${a.round}-${n}.md`

// A prompt must not show which states are controls: no flag, states in name
// order, and a control carrying the looks of the clean states on its surface
// (the name up to its last '-'), or every look when none shares that surface.
// A look's number is its 1-based position in `a.looks`, the same in every prompt.
const surfaceOf = (name) => {
  const cut = name.lastIndexOf('-')
  return cut < 0 ? name : name.slice(0, cut)
}
const numbered = a.looks.map((look, i) => ({ k: i + 1, look }))
const looksOf = (u) => {
  if (!u.control) return numbered.filter(({ look }) => look.states.includes(u.state)).map(({ k }) => k)
  const surface = surfaceOf(u.state)
  const sameSurface = (s) => a.states.includes(s) && surfaceOf(s) === surface
  if (!a.states.some(sameSurface)) return numbered.map(({ k }) => k)
  return numbered.filter(({ look }) => look.states.some(sameSurface)).map(({ k }) => k)
}
const byName = (x, y) => (x.state < y.state ? -1 : x.state > y.state ? 1 : 0)

const inspectorPrompt = (group, n) => {
  const lines = [...group].sort(byName).map((u) => ({ state: u.state, ks: looksOf(u) }))
  const carried = new Set(lines.flatMap(({ ks }) => ks))
  const looks = numbered.filter(({ k }) => carried.has(k))
  return `slug: ${a.slug}
milestone: ${a.milestone}
round: ${a.round}
capture_dir: ${a.capture_dir}
design: ${a.design}
note: ${notePath(n)}
states:
${lines.map(({ state, ks }) => `- ${state}: looks ${ks.length ? ks.join(', ') : 'none'}`).join('\n')}
looks:
${looks.length ? looks.map(({ k, look }) => `- ${k}: ${JSON.stringify({ task: look.task, accept: look.accept, behaviour: look.behaviour })}`).join('\n') : '- none'}

Judge these states as your procedure says, write your note at that path, and return only \`{states}\`, one entry per state above in its order.`
}

// `note` is compared by suffix because an inspector may report the path it resolved.
const namesNote = (returned, expected) => {
  const path = returned.replace(/\\/g, '/')
  return path === expected || path.endsWith(`/${expected}`)
}

const entry = (u, status, fields) => ({
  state: u.state,
  control: u.control,
  status,
  verdict: null,
  defects: [],
  note: null,
  error: null,
  ...fields,
})

// The schema forbids extra keys, so a real return never carries `thrown`.
const caught = async (run) => {
  try {
    return await run()
  } catch (e) {
    return { thrown: String((e && e.message) || e) }
  }
}

const toEntries = (group, n, ret) => {
  const expected = notePath(n)
  const label = `group ${n} (${group.map((u) => u.state).join(', ')})`
  if (ret && ret.thrown !== undefined) {
    fail(`${label}: inspector failed: ${ret.thrown}`)
    return group.map((u) => entry(u, 'error', { error: ret.thrown }))
  }
  if (!ret) {
    fail(`${label}: inspector returned nothing`)
    return group.map((u) => entry(u, 'no-return', {}))
  }
  if (!Array.isArray(ret.states)) {
    const error = 'returned no states array'
    fail(`${label}: inspector ${error}`)
    return group.map((u) => entry(u, 'error', { error }))
  }
  const asked = new Set(group.map((u) => u.state))
  const byState = new Map()
  ret.states.forEach((s) => {
    const name = s && s.state
    if (!asked.has(name)) fail(`${label}: inspector returned state ${JSON.stringify(name)}, which it was not asked`)
    else if (byState.has(name)) fail(`${label}: inspector returned state ${name} twice; the first kept`)
    else byState.set(name, s)
  })
  return group.map((u) => {
    const s = byState.get(u.state)
    if (!s) {
      const error = 'inspector returned no entry for this state'
      fail(`state ${u.state}: ${error}`)
      return entry(u, 'missing', { error })
    }
    if (!VERDICTS.includes(s.verdict) || !Array.isArray(s.defects)) {
      const error = `returned verdict ${JSON.stringify(s.verdict)} or defects not an array`
      fail(`state ${u.state}: inspector ${error}`)
      return entry(u, 'error', { error })
    }
    const fields = { verdict: s.verdict, defects: s.defects }
    if (typeof s.note !== 'string' || !s.note) {
      const error = 'wrote no note'
      fail(`state ${u.state}: inspector ${error}`)
      return entry(u, 'ok', { ...fields, error })
    }
    if (!namesNote(s.note, expected)) {
      const error = `returned note ${s.note}; expected ${expected}`
      fail(`state ${u.state}: inspector ${error}`)
      return entry(u, 'ok', { ...fields, note: s.note, error })
    }
    return entry(u, 'ok', { ...fields, note: expected })
  })
}

phase('Inspect')
const returns = await Promise.all(groups.map((group, i) => caught(() => agent(inspectorPrompt(group, i + 1), {
  agentType: 'ui-inspector',
  schema: RETURN_SCHEMA,
  label: `inspect ${i + 1} x${group.length}`,
  phase: 'Inspect',
}))))

const results = new Map()
groups.forEach((group, i) => toEntries(group, i + 1, returns[i]).forEach((e) => results.set(e.state, e)))

return index(units.map((u) => results.get(u.state)))
