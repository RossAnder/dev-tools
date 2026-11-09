export const meta = {
  name: 'implement-v2-milestone',
  description: 'Run the dispatchable tasks of one v2 milestone in waves: one plan-worker per task, each writing its own status and one run note',
  whenToUse: 'Launched by the implement-v2-engine skill with the slug, the milestone and its dispatchable tasks as args; not for direct use',
  phases: [
    { title: 'Run', detail: 'waves of plan-worker-lite and plan-worker-deep; each writes docs/flows/<slug>/notes/run/task-<n>.md' },
  ],
}

// A relaunch is always a fresh run, never a resume: workers write their own
// task status, so a journal hit would replay a return the store has since
// moved past. Workers write their own notes, so this returns only an index.
// A wave is every task whose needs all returned `done` and whose files overlap
// no other task in it, cut to `max_parallel`. A task behind a non-`done` return
// is reported `blocked` and never dispatched. Nothing is retried; the engine's
// drain decides what runs again. `error` is a thrown error or a bad return,
// and null otherwise.
const SCRIPT_VERSION = '1'
const MAX_PARALLEL = 8
const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
const MILESTONE_ID = /^[A-Za-z0-9][A-Za-z0-9-]*$/
const ONE_LINE = /^[^\r\n]+$/

const WORKERS = {
  lite: { agentType: 'plan-worker-lite' },
  deep: { agentType: 'plan-worker-deep' },
}

// `note` is an empty string when the worker wrote none; `handover` likewise.
const RETURN_SCHEMA = {
  type: 'object',
  properties: {
    task: { type: 'integer', minimum: 1 },
    status: { type: 'string', enum: ['done', 'failed', 'parked', 'error'] },
    note: { type: 'string' },
    files_touched: { type: 'array', items: { type: 'string' } },
    deviations: { type: 'array', items: { type: 'string' } },
    handover: { type: 'string' },
  },
  required: ['task', 'status', 'note', 'files_touched', 'deviations', 'handover'],
  additionalProperties: false,
}

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const index = (tasks) => ({ slug: a.slug || null, milestone: a.milestone || null, tasks, errors, script_version: SCRIPT_VERSION })

// RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
const matches = (re, value) => typeof value === 'string' && re.test(value)
const isId = (n) => Number.isInteger(n) && n >= 1
const hasWorker = (executor) => typeof executor === 'string' && Object.prototype.hasOwnProperty.call(WORKERS, executor)

// Kahn's algorithm over `needs`; whatever never reaches in-degree zero is on or behind a cycle.
const cycleMembers = (tasks) => {
  const indegree = new Map(tasks.map((t) => [t.id, new Set(t.needs).size]))
  const dependents = new Map(tasks.map((t) => [t.id, []]))
  tasks.forEach((t) => new Set(t.needs).forEach((n) => dependents.get(n).push(t.id)))
  const queue = tasks.filter((t) => indegree.get(t.id) === 0).map((t) => t.id)
  while (queue.length) {
    const id = queue.shift()
    dependents.get(id).forEach((d) => {
      indegree.set(d, indegree.get(d) - 1)
      if (indegree.get(d) === 0) queue.push(d)
    })
  }
  return tasks.filter((t) => indegree.get(t.id) > 0).map((t) => t.id)
}

const argProblems = () => {
  const problems = []
  if (!matches(SLUG, a.slug)) problems.push(`slug ${JSON.stringify(a.slug)} does not match ${SLUG.source}`)
  if (!matches(MILESTONE_ID, a.milestone)) problems.push(`milestone ${JSON.stringify(a.milestone)} does not match ${MILESTONE_ID.source}`)
  if (!matches(ONE_LINE, a.root)) problems.push(`root ${JSON.stringify(a.root)} must be a one-line path`)
  if (!Number.isInteger(a.max_parallel) || a.max_parallel < 1 || a.max_parallel > MAX_PARALLEL) {
    problems.push(`max_parallel ${JSON.stringify(a.max_parallel)} must be an integer from 1 to ${MAX_PARALLEL}`)
  }
  if (!Array.isArray(a.tasks) || !a.tasks.length) {
    problems.push('tasks must be a non-empty array')
    return problems
  }
  const ids = new Set()
  a.tasks.forEach((t, i) => {
    const id = t && t.id
    if (!isId(id)) problems.push(`task ${i}: id ${JSON.stringify(id)} is not a positive integer`)
    else if (ids.has(id)) problems.push(`task ${i}: id ${id} repeats, and two workers would share one row`)
    else ids.add(id)
  })
  let shaped = true
  a.tasks.forEach((t, i) => {
    const name = isId(t && t.id) ? `task ${t.id}` : `task ${i}`
    if (!hasWorker(t && t.executor)) problems.push(`${name}: executor ${JSON.stringify(t && t.executor)} is neither lite nor deep`)
    const needs = t && t.needs
    if (!Array.isArray(needs)) {
      problems.push(`${name}: needs must be an array`)
      shaped = false
    } else {
      needs.forEach((n) => {
        if (!ids.has(n)) {
          problems.push(`${name}: needs ${JSON.stringify(n)}, which is not in tasks`)
          shaped = false
        }
      })
    }
    const files = t && t.files
    if (!Array.isArray(files) || !files.length) problems.push(`${name}: files must be a non-empty array`)
    else files.forEach((f) => { if (!matches(ONE_LINE, f)) problems.push(`${name}: file ${JSON.stringify(f)} is not a one-line path`) })
  })
  if (shaped && ids.size === a.tasks.length) {
    const cycle = cycleMembers(a.tasks)
    if (cycle.length) problems.push(`needs form a cycle through tasks ${cycle.join(', ')}`)
  }
  return problems
}

const problems = argProblems()
if (problems.length) {
  problems.forEach(fail)
  fail('no agent launched')
  return index([])
}

const notePath = (id) => `docs/flows/${a.slug}/notes/run/task-${id}.md`

// Paths are compared slash-normalised and case-folded, since two spellings of
// one file on a case-insensitive filesystem are one file.
const fileKey = (f) => f.replace(/\\/g, '/').toLowerCase()

const workerPrompt = (t, siblings) => `slug: ${a.slug}
task: ${t.id}
root: ${a.root}
note: ${notePath(t.id)}
siblings: ${siblings.length ? siblings.join(', ') : 'none'}

Fetch your task with these commands, run as given:
- \`tomlctl plan view --slug ${a.slug} --for worker --task ${t.id}\`
- \`tomlctl tasks show ${t.id} --slug ${a.slug}\`

Carry out this one task as your procedure says, write your note at that path, and return only \`{task, status, note, files_touched, deviations, handover}\`.`

// `note` is compared by suffix because a worker may report the path it resolved.
const namesNote = (returned, expected) => {
  const path = returned.replace(/\\/g, '/')
  return path === expected || path.endsWith(`/${expected}`)
}

const entry = (id, status, fields) => ({ id, status, note: null, error: null, deviations: [], handover: '', ...fields })

const toEntry = (t, ret) => {
  const expected = notePath(t.id)
  if (ret && ret.thrown !== undefined) {
    fail(`task ${t.id}: worker failed: ${ret.thrown}; its row may still read in-progress`)
    return entry(t.id, 'error', { error: ret.thrown })
  }
  if (!ret) {
    fail(`task ${t.id}: worker returned nothing; its row may still read in-progress`)
    return entry(t.id, 'no-return', {})
  }
  if (ret.task !== t.id) {
    const error = `returned task ${ret.task}; expected ${t.id}`
    fail(`task ${t.id}: worker ${error}`)
    return entry(t.id, 'mismatch', { error })
  }
  const fields = { deviations: ret.deviations, handover: ret.handover }
  if (!ret.note) {
    const error = 'wrote no note'
    fail(`task ${t.id}: worker ${error}`)
    return entry(t.id, ret.status, { ...fields, error })
  }
  if (!namesNote(ret.note, expected)) {
    const error = `returned note ${ret.note}; expected ${expected}`
    fail(`task ${t.id}: worker ${error}`)
    return entry(t.id, ret.status, { ...fields, note: ret.note, error })
  }
  return entry(t.id, ret.status, { ...fields, note: expected })
}

// The schema forbids extra keys, so a real return never carries `thrown`.
const caught = async (run) => {
  try {
    return await run()
  } catch (e) {
    return { thrown: String((e && e.message) || e) }
  }
}

const results = new Map()
let pending = a.tasks.slice()

// Marks every pending task behind a non-`done` result, to a fixpoint, since a
// blocked task blocks its own dependents in turn.
const settleBlocked = () => {
  let changed = true
  while (changed) {
    changed = false
    pending = pending.filter((t) => {
      const behind = t.needs.filter((n) => results.has(n) && results.get(n).status !== 'done')
      if (!behind.length) return true
      const named = behind.map((n) => `${n} (${results.get(n).status})`).join(', ')
      log(`task ${t.id}: blocked behind task ${named}; not dispatched`)
      results.set(t.id, entry(t.id, 'blocked', {}))
      changed = true
      return false
    })
  }
}

const nextWave = () => {
  const ready = pending.filter((t) => t.needs.every((n) => results.has(n)))
  const wave = []
  const held = new Set()
  for (const t of ready) {
    if (wave.length === a.max_parallel) break
    const keys = t.files.map(fileKey)
    if (keys.some((k) => held.has(k))) continue
    keys.forEach((k) => held.add(k))
    wave.push(t)
  }
  return wave
}

phase('Run')
let waveNo = 0
settleBlocked()
while (pending.length) {
  const wave = nextWave()
  if (!wave.length) {
    fail(`no task of ${pending.map((t) => t.id).join(', ')} could start; none was dispatched`)
    pending.forEach((t) => results.set(t.id, entry(t.id, 'blocked', {})))
    break
  }
  waveNo += 1
  log(`wave ${waveNo}: tasks ${wave.map((t) => t.id).join(', ')}`)
  const inWave = new Set(wave.map((t) => t.id))
  pending = pending.filter((t) => !inWave.has(t.id))
  // Each thunk catches its own throw so the entry can name it; an uncaught one would resolve to null.
  const returns = await parallel(wave.map((t) => () => {
    const worker = WORKERS[t.executor]
    const siblings = wave.filter((s) => s.id !== t.id).flatMap((s) => s.files)
    return caught(() => agent(workerPrompt(t, siblings), {
      agentType: worker.agentType,
      schema: RETURN_SCHEMA,
      label: `wave ${waveNo} · task ${t.id} (${t.executor})`,
      phase: 'Run',
    }))
  }))
  wave.forEach((t, i) => results.set(t.id, toEntry(t, returns[i])))
  settleBlocked()
}

return index(a.tasks.map((t) => results.get(t.id)))
