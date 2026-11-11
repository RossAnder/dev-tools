// Runs the saved Workflow scripts review-v2.js, plan-v2-review.js and
// implement-v2-milestone.js under stubbed runtime globals. A script's top-level
// `return` makes it unimportable as a module, so each is loaded as the body of an
// AsyncFunction whose parameters are the globals, and its `return` becomes the
// value that function resolves to.
//
// Stub semantics, matching the Workflow runtime's documented behaviour:
// - `agent(prompt, opts)` returns the canned object a test's responder picks by
//   `opts.agentType` or `opts.label`, or null for a dead agent; a responder that
//   throws makes the call reject. In deferred mode it instead returns a promise
//   the test settles by hand, and records the call as started when it is made.
// - `parallel(thunks)` resolves every thunk, turns one that throws into null,
//   and never rejects.
// - `pipeline(items, ...stages)` passes `(previous, item, index)` to each stage,
//   the item itself standing as `previous` for the first, and turns an item
//   whose stage throws into null.
// - `workflow(name, args)` runs the named loaded script with the same stubs and
//   throws on an unknown name.
// - `phase` and `log` only record.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const WORKFLOWS = join(dirname(fileURLToPath(import.meta.url)), '..')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor
const GLOBALS = ['args', 'agent', 'parallel', 'pipeline', 'phase', 'log', 'workflow']

const load = (file) => {
  const source = readFileSync(join(WORKFLOWS, file), 'utf8')
  const meta = /^export const meta\b/m
  assert.match(source, meta, `${file} must open its meta with \`export const meta\``)
  const body = source.replace(meta, 'const meta')
  assert.doesNotMatch(body, /^export\b/m, `${file} exports something besides meta, which this loader cannot strip`)
  return new AsyncFunction(...GLOBALS, body)
}

const SCRIPTS = {
  'review-v2-pipeline': load('review-v2.js'),
  'plan-v2-review': load('plan-v2-review.js'),
  'implement-v2-milestone': load('implement-v2-milestone.js'),
}

const settle = async (run) => {
  try {
    return await run()
  } catch {
    return null
  }
}

const runtime = (respond, { deferred = false } = {}) => {
  const calls = []
  const phases = []
  const logs = []
  const agent = (prompt, opts) => {
    const call = { prompt, opts, settled: false }
    calls.push(call)
    if (deferred) {
      return new Promise((resolve, reject) => {
        call.resolve = (value) => { call.settled = true; resolve(value) }
        call.reject = (error) => { call.settled = true; reject(error) }
      })
    }
    return (async () => {
      try {
        return respond(prompt, opts)
      } finally {
        call.settled = true
      }
    })()
  }
  const parallel = (thunks) => Promise.all(thunks.map(settle))
  const pipeline = (items, ...stages) => Promise.all(items.map((item, index) => settle(async () => {
    let previous = item
    for (const stage of stages) previous = await stage(previous, item, index)
    return previous
  })))
  const phase = (title) => { phases.push(title) }
  const log = (msg) => { logs.push(msg) }
  const workflow = (name, childArgs) => {
    if (!Object.prototype.hasOwnProperty.call(SCRIPTS, name)) throw new Error(`unknown workflow ${name}`)
    return SCRIPTS[name](childArgs, agent, parallel, pipeline, phase, log, workflow)
  }
  const run = (name, scriptArgs) => workflow(name, scriptArgs)
  return { run, calls, phases, logs }
}

const lineOf = (prompt, key) => {
  const m = prompt.match(new RegExp(`^${key}: (.*)$`, 'm'))
  return m ? m[1] : null
}
const idsOf = (prompt) => (lineOf(prompt, 'findings') || '').split(', ').filter(Boolean)
const labels = (calls, prefix) => calls.map((c) => c.opts.label).filter((l) => l.startsWith(prefix))
const confirmed = (finding) => ({ finding, verdict: 'confirmed' })

// Verifiers confirm every finding they are given; checkers refute unless the test
// says otherwise; a cell returns the test's canned return for its id.
const reviewResponder = (cellReturns, { checker, verifier } = {}) => (prompt, opts) => {
  if (opts.label.startsWith('cell ')) {
    const ret = cellReturns[opts.label.split(' ')[1]]
    if (ret instanceof Error) throw ret
    return ret === undefined ? { status: 'covered', findings: [] } : ret
  }
  const ids = idsOf(prompt)
  if (opts.agentType === 'review-checker') {
    return checker ? checker(ids) : { verdicts: ids.map((finding) => ({ finding, verdict: 'refuted' })) }
  }
  if (opts.agentType === 'review-verifier') {
    if (verifier) return verifier(ids, prompt, opts)
    return /^batch: /m.test(prompt) ? { verdicts: ids.map(confirmed) } : confirmed(ids[0])
  }
  throw new Error(`no canned return for ${opts.agentType} (${opts.label})`)
}

const SCHEMAS = { cell_return: { type: 'object' }, verdict_return: { type: 'object' } }

const reviewArgs = (extra) => ({
  run: 'U1',
  wave: 1,
  depth: 'standard',
  pack_path: '/runs/U1/pack.md',
  cells: [{ id: 'C1', agentType: 'review-finder' }],
  store: { slug: 'demo' },
  schemas: SCHEMAS,
  ...extra,
})

const finding = (id, fields) => ({ id, route: 'verifier', kind: 'defect', severity: 'major', ...fields })

test('review-v2: a wave with a cell, a panel, a minor batch and a checker pool with one reroute counts every launch', async () => {
  const rt = runtime(reviewResponder(
    {
      C1: {
        status: 'covered',
        findings: [
          finding('F2', { route: 'panel' }),
          finding('F3', { severity: 'minor' }),
          finding('F4', { severity: 'minor' }),
          finding('F5', { route: 'checker', kind: 'cleanup' }),
        ],
      },
    },
    { checker: (ids) => ({ verdicts: ids.map((f) => ({ finding: f, verdict: 'unsettled' })) }) },
  ))
  const out = await rt.run('review-v2-pipeline', reviewArgs())
  // cell 1 + panel 3 + minor batch 1 + checker 1 + reroute 1
  assert.equal(out.agents, 7)
  assert.equal(rt.calls.length, out.agents)
  assert.equal(labels(rt.calls, 'panel F2').length, 3)
  assert.deepEqual(labels(rt.calls, 'verify C1 minor'), ['verify C1 minor x2'])
  assert.deepEqual(labels(rt.calls, 'check batch'), ['check batch 1 x1'])
  assert.deepEqual(labels(rt.calls, 'verify F5'), ['verify F5'])
  assert.equal(out.verified, 4)
  assert.deepEqual(out.errors, [])
  assert.deepEqual(out.cells, [{ id: 'C1', status: 'covered', findings: 4, escalated: 0 }])
})

test('review-v2: an escalation is capped at 300 characters', async () => {
  const long = 'x'.repeat(400)
  const rt = runtime(reviewResponder({ C1: { status: 'covered', findings: [], escalate: [long, 'short', { lens: 'auth' }] } }))
  const out = await rt.run('review-v2-pipeline', reviewArgs())
  assert.equal(out.escalations.length, 3)
  assert.equal(out.escalations[0].text, 'x'.repeat(300))
  assert.equal(out.escalations[1].text, 'short')
  assert.equal(out.escalations[2].text, '{"lens":"auth"}')
  assert.ok(out.escalations.every((e) => e.cell === 'C1'))
})

test('review-v2: batch_defects batches two singles and keeps a same_as row single', async () => {
  const findings = [finding('F1'), finding('F2'), finding('F3', { same_as: true })]
  const batched = runtime(reviewResponder({ C1: { status: 'covered', findings } }))
  const out = await batched.run('review-v2-pipeline', reviewArgs({ batch_defects: true }))
  assert.equal(out.agents, 3)
  const batch = batched.calls.find((c) => c.opts.label === 'verify C1 defect x2')
  assert.ok(batch, 'one defect batch for the cell')
  assert.deepEqual(idsOf(batch.prompt), ['F1', 'F2'])
  assert.deepEqual(labels(batched.calls, 'verify F'), ['verify F3'])
  assert.equal(out.verified, 3)

  const unbatched = runtime(reviewResponder({ C1: { status: 'covered', findings } }))
  const plain = await unbatched.run('review-v2-pipeline', reviewArgs())
  assert.deepEqual(labels(unbatched.calls, 'verify ').sort(), ['verify F1', 'verify F2', 'verify F3'])
  assert.equal(plain.agents, 4)
})

const reproduceArgs = (units) => ({
  mode: 'reproduce',
  run_dir: '/runs/apply',
  store: { slug: 'demo' },
  flow: 'demo',
  milestone: 'D',
  round: 1,
  units,
})

const reproduceResponder = (checkerVerdicts) => (prompt, opts) => {
  const ids = idsOf(prompt)
  if (opts.agentType === 'review-checker') {
    return { reproductions: ids.filter((f) => checkerVerdicts[f]).map((f) => ({ finding: f, verdict: checkerVerdicts[f] })) }
  }
  if (opts.agentType === 'review-verifier') {
    return { task: lineOf(prompt, 'task'), reproductions: ids.map((f) => ({ finding: f, verdict: 'gone' })) }
  }
  throw new Error(`no canned return for ${opts.agentType}`)
}

test('review-v2 reproduce: a checker unit goes to the checker; an unsettled checker finding goes to a verifier', async () => {
  const rt = runtime(reproduceResponder({ F1: 'gone', F2: 'unsettled', F3: 'gone' }))
  const out = await rt.run('review-v2-pipeline', reproduceArgs([
    { task: 'fix-a', files: ['a.rs'], findings: ['F1', 'F2'], route: 'checker' },
    { task: 'fix-b', files: ['b.rs'], findings: ['F3'], route: 'checker' },
    { task: 'fix-c', files: ['c.rs'], findings: ['F4'] },
  ]))
  const checks = rt.calls.filter((c) => c.opts.agentType === 'review-checker')
  assert.equal(checks.length, 1)
  assert.deepEqual(idsOf(checks[0].prompt), ['F1', 'F2', 'F3'])
  assert.deepEqual(labels(rt.calls, 'reproduce fix').sort(), ['reproduce fix-a', 'reproduce fix-c'])
  assert.equal(out.agents, 3)
  assert.deepEqual(out.units.map((u) => [u.task, u.status]), [['fix-a', 'ok'], ['fix-b', 'ok'], ['fix-c', 'ok']])
  assert.deepEqual(out.units[1].reproductions, [{ finding: 'F3', verdict: 'gone' }])
  assert.deepEqual(out.errors, [])
})

test('review-v2 reproduce: an unsettled checker finding goes to a verifier', async () => {
  const rt = runtime(reproduceResponder({ F1: 'unsettled' }))
  const out = await rt.run('review-v2-pipeline', reproduceArgs([
    { task: 'fix-a', files: ['a.rs'], findings: ['F1'], route: 'checker' },
  ]))
  assert.deepEqual(rt.calls.map((c) => c.opts.agentType), ['review-checker', 'review-verifier'])
  assert.equal(out.units[0].status, 'ok')
  assert.ok(rt.logs.some((l) => l.includes('fix-a: F1 not settled gone by the checker')))
})

test('review-v2: a dead cell agent fills a no-return row and a thrown one an error row', async () => {
  const rt = runtime(reviewResponder({ C1: null, C2: new Error('boom') }))
  const out = await rt.run('review-v2-pipeline', reviewArgs({
    cells: [{ id: 'C1', agentType: 'review-finder' }, { id: 'C2', agentType: 'review-sweeper' }],
  }))
  assert.deepEqual(out.cells, [
    { id: 'C1', status: 'no-return', findings: 0 },
    { id: 'C2', status: 'error', findings: 0 },
  ])
  assert.ok(out.errors.some((e) => e.startsWith('cell C1: agent returned nothing')))

  const repro = runtime(() => null)
  const unit = await repro.run('review-v2-pipeline', reproduceArgs([{ task: 'fix-a', files: [], findings: ['F1'] }]))
  assert.equal(unit.units[0].status, 'no-return')
  assert.ok(unit.errors.some((e) => e.includes('F1 unchecked')))
})

const LEDGER = 'ledger: .claude/flows/demo/review-v2.toml'
const planArgs = (extra) => ({
  slug: 'demo',
  run: 'U1',
  mode: 'full',
  depth: 'standard',
  run_dir: '/runs/U1',
  ledger_line: LEDGER,
  schemas: SCHEMAS,
  ...extra,
})
const recheckArgs = (extra) => planArgs({ mode: 'recheck', depth: 'quick', targets: ['D3'], ...extra })

// The lead composes one review-finder cell and settles with the test's return;
// everything else falls through to the review responder.
const planResponder = ({ survey, baselines, settle: settleRet, verifier } = {}) => {
  const review = reviewResponder({}, { verifier })
  return (prompt, opts) => {
    if (opts.agentType === 'review-surveyor') return survey
    if (opts.agentType === 'verification') return baselines(prompt)
    if (opts.agentType === 'plan-review-lead') {
      if (opts.label.startsWith('lead compose')) return { wave: 1, cells: [{ id: 'C1', agentType: 'review-finder' }] }
      if (opts.label.startsWith('lead settle')) return settleRet || { next: null, unverified: [] }
      if (opts.label === 'lead audit') return { sampled: 2, overturned: [], contested: [] }
    }
    return review(prompt, opts)
  }
}

test('plan-v2-review: baseline keys match, an unknown key is an error and a none line passes through', async () => {
  const rt = runtime(planResponder({
    survey: {
      digest: 'profile: plan',
      verification: [
        { key: 'tests', command: 'cargo test' },
        { key: 'build', command: 'cargo build' },
        { key: 'lint', command: 'none — no linter configured' },
      ],
    },
    baselines: () => ({
      baselines: [
        { key: 'tests', outcome: 'pass', log: '/logs/tests.log' },
        { key: 'bogus', outcome: 'pass', log: '/logs/bogus.log' },
      ],
    }),
  }))
  const out = await rt.run('plan-v2-review', planArgs())
  const verification = rt.calls.find((c) => c.opts.agentType === 'verification')
  assert.match(verification.prompt, /^- cargo test$/m)
  assert.doesNotMatch(verification.prompt, /no linter/)
  const compose = rt.calls.find((c) => c.opts.label.startsWith('lead compose')).prompt
  assert.match(compose, /^ {2}tests: \/logs\/tests\.log$/m)
  assert.match(compose, /^ {2}build: none — the verification agent returned no log$/m)
  assert.match(compose, /^ {2}lint: none — no linter configured$/m)
  assert.ok(out.errors.some((e) => e.includes('unknown or repeated key "bogus"')))
  assert.ok(out.errors.some((e) => e.startsWith('baseline build: verification returned no log')))
})

test('plan-v2-review: the agent total adds the child pipeline\'s agents', async () => {
  const plan = planResponder({
    survey: { digest: 'profile: plan', verification: [{ key: 'tests', command: 'cargo test' }] },
    baselines: () => ({ baselines: [{ key: 'tests', outcome: 'pass', log: '/logs/tests.log' }] }),
  })
  // The child's cell returns one major defect, so the wave costs a cell and a verifier.
  const rt = runtime((prompt, opts) => (opts.label === 'cell C1 review-finder'
    ? { status: 'covered', findings: [finding('F1')] }
    : plan(prompt, opts)))
  const out = await rt.run('plan-v2-review', planArgs())
  assert.equal(out.waves.length, 1)
  assert.equal(out.waves[0].agents, 2)
  // surveyor, verification, compose, settle and audit, plus the wave's two
  assert.equal(out.agents, 7)
  assert.equal(rt.calls.length, out.agents)
  assert.deepEqual(out.audit, { sampled: 2, overturned: [], contested: [] })
  assert.deepEqual(out.errors, [])
})

test('plan-v2-review: reverify skips an id the lead already returned unverified', async () => {
  const rt = runtime(planResponder({ settle: { next: null, unverified: ['F2'], reverify: ['F2', 'F3'] } }))
  const out = await rt.run('plan-v2-review', recheckArgs())
  const again = rt.calls.filter((c) => c.opts.label.startsWith('reverify'))
  assert.equal(again.length, 1)
  assert.deepEqual(idsOf(again[0].prompt), ['F3'])
  assert.deepEqual(out.unverified, ['F2'])
  assert.deepEqual(out.errors, [])
})

test('plan-v2-review: a reverify_single id gets its own dispatch while the rest share one batch', async () => {
  const rt = runtime(planResponder({ settle: { next: null, unverified: [], reverify: ['F1', 'F3', 'F4'], reverify_single: ['F1'] } }))
  const out = await rt.run('plan-v2-review', recheckArgs())
  const again = rt.calls.filter((c) => c.opts.label.startsWith('reverify'))
  assert.deepEqual(again.map((c) => c.opts.label).sort(), ['reverify F1', 'reverify x2'])
  const single = again.find((c) => c.opts.label === 'reverify F1')
  assert.deepEqual(idsOf(single.prompt), ['F1'])
  assert.doesNotMatch(single.prompt, /^batch: /m)
  const batch = again.find((c) => c.opts.label === 'reverify x2')
  assert.deepEqual(idsOf(batch.prompt), ['F3', 'F4'])
  assert.match(batch.prompt, /^batch: reverify-1$/m)
  assert.deepEqual(out.unverified, [])
  assert.deepEqual(out.errors, [])
})

test('plan-v2-review: an id the reverify batch omits counts as unverified', async () => {
  const rt = runtime(planResponder({
    settle: { next: null, unverified: [], reverify: ['F3', 'F4'] },
    verifier: (ids, prompt) => (/^batch: /m.test(prompt) ? { verdicts: [confirmed('F3')] } : confirmed(ids[0])),
  }))
  const out = await rt.run('plan-v2-review', recheckArgs())
  assert.deepEqual(out.unverified, ['F4'])
  assert.ok(out.errors.some((e) => e.startsWith('reverify batch: no verdict returned for F4')))
})

const milestoneArgs = (tasks, extra) => ({ slug: 'demo', milestone: 'D', root: '/repo', max_parallel: 4, tasks, ...extra })
const task = (id, files, fields) => ({ id, executor: 'lite', needs: [], files, ...fields })
const workerReturn = (id, status = 'done', extra) => ({
  task: id,
  status,
  note: `docs/flows/demo/notes/run/task-${id}.md`,
  files_touched: [],
  deviations: [],
  handover: '',
  ...extra,
})

// Lets every queued microtask and the pool's Promise.race run to quiescence.
const tick = () => new Promise((resolve) => setImmediate(resolve))
const taskOf = (call) => Number(call.opts.label.match(/^task (\d+) /)[1])
const started = (rt) => rt.calls.map(taskOf)
const inFlight = (rt) => rt.calls.filter((c) => !c.settled).map(taskOf)
const finish = async (rt, id, status = 'done', extra) => {
  const call = rt.calls.find((c) => taskOf(c) === id && !c.settled)
  assert.ok(call, `task ${id} is in flight`)
  call.resolve(workerReturn(id, status, extra))
  await tick()
}

test('implement-v2-milestone: a short task starts while a long file-disjoint task is in flight', async () => {
  const rt = runtime(null, { deferred: true })
  const done = rt.run('implement-v2-milestone', milestoneArgs([
    task(1, ['long.rs']),
    task(2, ['short.rs']),
    task(3, ['next.rs'], { needs: [2] }),
  ]))
  await tick()
  assert.deepEqual(started(rt), [1, 2])
  await finish(rt, 2)
  assert.deepEqual(inFlight(rt).sort(), [1, 3])
  await finish(rt, 3)
  await finish(rt, 1)
  const out = await done
  assert.deepEqual(out.tasks.map((t) => [t.id, t.status]), [[1, 'done'], [2, 'done'], [3, 'done']])
})

test('implement-v2-milestone: an after task starts once its target is done', async () => {
  const rt = runtime(null, { deferred: true })
  const done = rt.run('implement-v2-milestone', milestoneArgs([task(1, ['a.rs']), task(2, ['b.rs'], { after: [1] })]))
  await tick()
  assert.deepEqual(started(rt), [1])
  await finish(rt, 1)
  assert.deepEqual(inFlight(rt), [2])
  assert.match(rt.calls[1].prompt, /^after-premises: /m)
  await finish(rt, 2)
  const out = await done
  assert.deepEqual(out.tasks.map((t) => t.status), ['done', 'done'])
})

test('implement-v2-milestone: an after task is blocked when its target fails', async () => {
  const rt = runtime(null, { deferred: true })
  const done = rt.run('implement-v2-milestone', milestoneArgs([task(1, ['a.rs']), task(2, ['b.rs'], { after: [1] })]))
  await tick()
  await finish(rt, 1, 'failed')
  const out = await done
  assert.deepEqual(started(rt), [1])
  assert.deepEqual(out.tasks.map((t) => [t.id, t.status]), [[1, 'failed'], [2, 'blocked']])
})

test('implement-v2-milestone: two tasks sharing a file are never in flight together', async () => {
  const rt = runtime(null, { deferred: true })
  // One file, spelled with a different case and separator.
  const done = rt.run('implement-v2-milestone', milestoneArgs([
    task(1, ['src/Lib.rs']),
    task(2, ['src\\lib.rs', 'b.rs']),
    task(3, ['c.rs']),
  ]))
  const overlap = () => assert.ok(!(inFlight(rt).includes(1) && inFlight(rt).includes(2)), 'tasks 1 and 2 overlap')
  await tick()
  overlap()
  assert.deepEqual(started(rt), [1, 3])
  await finish(rt, 3)
  overlap()
  assert.deepEqual(inFlight(rt), [1])
  await finish(rt, 1)
  overlap()
  assert.deepEqual(inFlight(rt), [2])
  await finish(rt, 2)
  const out = await done
  assert.deepEqual(out.tasks.map((t) => t.status), ['done', 'done', 'done'])
})

test('implement-v2-milestone: needs_files reaches the index', async () => {
  const rt = runtime((prompt, opts) => {
    const id = Number(opts.label.match(/^task (\d+) /)[1])
    return id === 1 ? workerReturn(1, 'failed', { needs_files: ['other.rs'] }) : workerReturn(id)
  })
  const out = await rt.run('implement-v2-milestone', milestoneArgs([task(1, ['a.rs']), task(2, ['b.rs'])]))
  assert.equal(out.tasks[0].status, 'failed')
  assert.deepEqual(out.tasks[0].needs_files, ['other.rs'])
  assert.deepEqual(out.tasks[1].needs_files, [])
  assert.deepEqual(rt.phases, ['Run'])
})

test('workflow stub throws on an unknown name', async () => {
  const rt = runtime(() => null)
  await assert.rejects(async () => rt.run('no-such-workflow', {}), /unknown workflow/)
})
