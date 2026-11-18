// Runs the saved Workflow script implement-v2-visual.js under stubbed runtime
// globals, loaded as an AsyncFunction body the way harness.test.mjs loads the
// others. The loader is repeated here because importing a test file would
// register its tests a second time under `node --test`.
//
// `agent(prompt, opts)` returns what the test's responder gives for the call,
// null standing for a dead agent; a responder that throws makes the call reject.

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
  return { source, script: new AsyncFunction(...GLOBALS, body) }
}

const { source: SOURCE, script: VISUAL } = load('implement-v2-visual.js')

const runtime = (respond) => {
  const calls = []
  const phases = []
  const logs = []
  const agent = async (prompt, opts) => {
    calls.push({ prompt, opts })
    return respond(prompt, opts)
  }
  const unused = () => { throw new Error('implement-v2-visual must not call this global') }
  const phase = (title) => { phases.push(title) }
  const log = (msg) => { logs.push(msg) }
  const run = (scriptArgs) => VISUAL(scriptArgs, agent, unused, unused, phase, log, unused)
  return { run, calls, phases, logs }
}

const lineOf = (prompt, key) => {
  const m = prompt.match(new RegExp(`^${key}: (.*)$`, 'm'))
  return m ? m[1] : null
}

// The `- <state>: looks <k>, <k>` lines of a prompt, as [state, look numbers] pairs.
const statesOf = (prompt) => [...prompt.matchAll(/^- ([^ :\n]+): looks (none|\d+(?:, \d+)*)$/gm)]
  .map((m) => [m[1], m[2] === 'none' ? [] : m[2].split(', ').map(Number)])
const looksBlock = (prompt) => prompt.split(/^looks:\n/m)[1].split('\n\n')[0]
// The `- <k>: {...}` lines of a prompt's looks block, as [k, look] pairs.
const looksOf = (prompt) => looksBlock(prompt).split('\n')
  .map((l) => l.match(/^- (\d+): (\{.*\})$/))
  .filter(Boolean)
  .map((m) => [Number(m[1]), JSON.parse(m[2])])
const shown = ({ task, accept, behaviour }) => ({ task, accept, behaviour })

const visualArgs = (extra) => ({
  slug: 'demo',
  milestone: 'B',
  round: 1,
  capture_dir: 'target/ui-capture/B-1/',
  design: 'docs/flows/demo/design/ui-B.md',
  states: ['wide', 'narrow'],
  controls: ['planted'],
  looks: [],
  ...extra,
})

// Each inspector passes every clean state and fails every control, writing the
// note its prompt names. The prompt does not say which states are controls, so
// the test passes its own control list.
const judging = (controls, verdictOf = (state, control) => (control ? 'fail' : 'pass')) => (prompt) => ({
  states: statesOf(prompt).map(([state]) => [state, controls.includes(state)]).map(([state, control]) => ({
    state,
    verdict: verdictOf(state, control),
    defects: control ? [{ class: 'overlap', location: '#panel', detail: 'two panels overlap' }] : [],
    note: lineOf(prompt, 'note'),
  })),
})

test('implement-v2-visual: the script names its meta and reads no clock or randomness', () => {
  assert.match(SOURCE, /name: 'implement-v2-visual'/)
  assert.doesNotMatch(SOURCE, /Date\.now|Math\.random|new Date\(\)/)
})

test('implement-v2-visual: ten states and two controls make three groups of at most four, a control in two of them', async () => {
  const states = Array.from({ length: 10 }, (_, i) => `s${i}`)
  const controls = ['c0', 'c1']
  const rt = runtime(judging(controls))
  const out = await rt.run(visualArgs({ states, controls }))
  assert.equal(rt.calls.length, 3)
  assert.ok(rt.calls.every((c) => c.opts.agentType === 'ui-inspector'))
  const groups = rt.calls.map((c) => statesOf(c.prompt))
  assert.ok(groups.every((g) => g.length >= 1 && g.length <= 4))
  assert.deepEqual(groups.map((g) => g.filter(([s]) => controls.includes(s)).length), [1, 1, 0])
  assert.deepEqual(groups.flat().map(([s]) => s).sort(), [...states, 'c0', 'c1'].sort())
  assert.deepEqual(rt.calls.map((c) => lineOf(c.prompt, 'note')), [
    'docs/flows/demo/notes/run/visual-B-1-1.md',
    'docs/flows/demo/notes/run/visual-B-1-2.md',
    'docs/flows/demo/notes/run/visual-B-1-3.md',
  ])
  assert.deepEqual(out.states.map((s) => s.state), [...states, 'c0', 'c1'])
  assert.ok(out.states.every((s) => s.status === 'ok' && s.error === null))
  assert.deepEqual(out.states.filter((s) => s.verdict === 'fail').map((s) => s.state), ['c0', 'c1'])
  assert.deepEqual(out.errors, [])
  assert.equal(out.script_version, '1')
  assert.deepEqual(rt.phases, ['Inspect'])
})

test('implement-v2-visual: a prompt carries the launch fields and only the looks naming its states', async () => {
  const looks = [
    { task: 3, accept: 0, behaviour: 'the list scrolls, the header stays', states: ['wide'] },
    { task: 4, accept: 1, behaviour: 'never shown', states: ['other'] },
  ]
  const states = ['wide', 'a', 'b', 'c', 'other']
  const rt = runtime(judging([]))
  await rt.run(visualArgs({ states, controls: [], looks }))
  assert.equal(rt.calls.length, 2)
  const [first, second] = rt.calls.map((c) => c.prompt)
  assert.equal(lineOf(first, 'capture_dir'), 'target/ui-capture/B-1/')
  assert.equal(lineOf(first, 'design'), 'docs/flows/demo/design/ui-B.md')
  assert.equal(lineOf(first, 'round'), '1')
  assert.deepEqual(statesOf(first), [['a', []], ['b', []], ['c', []], ['wide', [1]]])
  assert.deepEqual(statesOf(second), [['other', [2]]])
  assert.deepEqual(looksOf(first), [[1, shown(looks[0])]])
  assert.deepEqual(looksOf(second), [[2, shown(looks[1])]])
})

test('implement-v2-visual: a prompt whose states carry no look prints an empty looks block', async () => {
  const rt = runtime(judging(['planted']))
  await rt.run(visualArgs({ looks: [] }))
  assert.equal(rt.calls.length, 1)
  assert.deepEqual(statesOf(rt.calls[0].prompt), [['narrow', []], ['planted', []], ['wide', []]])
  assert.equal(looksBlock(rt.calls[0].prompt), '- none')
})

test('implement-v2-visual: a control carries every look and no prompt says which states are controls', async () => {
  const looks = [
    { task: 5, accept: 0, behaviour: 'the panel wraps its labels', states: ['web-01', 'web-03'] },
    { task: 6, accept: 1, behaviour: 'the status bar stays one row', states: ['tui-01'] },
  ]
  const controls = ['web-02', 'tui-02', 'odd']
  const rt = runtime(judging(controls))
  const out = await rt.run(visualArgs({ states: ['web-01', 'web-03', 'tui-01'], controls, looks }))
  const carried = new Map()
  rt.calls.forEach(({ prompt }) => {
    assert.doesNotMatch(prompt, /control/)
    const lines = statesOf(prompt)
    const names = lines.map(([s]) => s)
    assert.deepEqual(names, [...names].sort())
    lines.forEach(([s, ks]) => carried.set(s, ks))
    assert.doesNotMatch(looksBlock(prompt), /"states"/)
    const wanted = [...new Set(lines.flatMap(([, ks]) => ks))].sort((x, y) => x - y)
    assert.deepEqual(looksOf(prompt), wanted.map((k) => [k, shown(looks[k - 1])]))
  })
  assert.deepEqual(Object.fromEntries(carried), {
    'web-01': [1],
    'web-03': [1],
    'tui-01': [2],
    'web-02': [1],
    'tui-02': [2],
    odd: [1, 2],
  })
  assert.deepEqual(out.states.filter((s) => s.verdict === 'fail').map((s) => s.state), controls)
  assert.deepEqual(out.states.filter((s) => s.control).map((s) => s.state), controls)
  assert.deepEqual(out.errors, [])
})

test('implement-v2-visual: a dead inspector\'s states come back no-return and a thrown one\'s error', async () => {
  const states = ['s1', 's2', 's3', 's4', 's5', 's6']
  const rt = runtime((prompt) => {
    const n = lineOf(prompt, 'note')
    if (n.endsWith('-1.md')) return null
    if (n.endsWith('-2.md')) throw new Error('boom')
    return judging(['c'])(prompt)
  })
  const out = await rt.run(visualArgs({ states, controls: ['c'] }))
  assert.equal(rt.calls.length, 2)
  const byGroup = rt.calls.map((c) => statesOf(c.prompt).map(([s]) => s))
  const statusOf = (s) => out.states.find((e) => e.state === s)
  byGroup[0].forEach((s) => {
    assert.equal(statusOf(s).status, 'no-return')
    assert.equal(statusOf(s).verdict, null)
    assert.equal(statusOf(s).note, null)
  })
  byGroup[1].forEach((s) => {
    assert.equal(statusOf(s).status, 'error')
    assert.equal(statusOf(s).error, 'boom')
  })
  assert.ok(out.errors.some((e) => e.startsWith('group 1 ') && e.endsWith('inspector returned nothing')))
  assert.ok(out.errors.some((e) => e.startsWith('group 2 ') && e.includes('inspector failed: boom')))
})

test('implement-v2-visual: a state the inspector leaves out is missing, and a wrong note path is kept with an error', async () => {
  const rt = runtime((prompt) => ({
    states: [{ state: 'wide', verdict: 'unsure', defects: [], note: 'elsewhere.md' }],
  }))
  const out = await rt.run(visualArgs())
  const [wide, narrow, planted] = out.states
  assert.equal(wide.status, 'ok')
  assert.equal(wide.verdict, 'unsure')
  assert.equal(wide.note, 'elsewhere.md')
  assert.match(wide.error, /expected docs\/flows\/demo\/notes\/run\/visual-B-1-1\.md/)
  assert.equal(narrow.status, 'missing')
  assert.equal(planted.status, 'missing')
  assert.equal(planted.control, true)
})

test('implement-v2-visual: bad args refuse the whole launch and start no agent', async () => {
  const cases = [
    [{ slug: 'Bad Slug' }, 'slug '],
    [{ milestone: '' }, 'milestone '],
    [{ round: 0 }, 'round '],
    [{ round: '1' }, 'round '],
    [{ capture_dir: 'two\nlines' }, 'capture_dir '],
    [{ design: undefined }, 'design '],
    [{ states: [] }, 'states '],
    [{ states: ['../up'] }, 'states: '],
    [{ controls: 'planted' }, 'controls '],
    [{ controls: ['wide'] }, 'state wide '],
    [{ looks: null }, 'looks '],
    [{ looks: [{ task: 1, accept: 0, behaviour: 'x', states: ['ghost'] }] }, 'looks[0].states '],
    [{ looks: [{ task: 1, accept: -1, behaviour: 'x', states: ['wide'] }] }, 'looks[0].accept '],
    [{ looks: [{ task: 1, accept: 0, behaviour: ' ', states: ['wide'] }] }, 'looks[0].behaviour '],
  ]
  for (const [extra, prefix] of cases) {
    const rt = runtime(() => { throw new Error('no agent should start') })
    const out = await rt.run(visualArgs(extra))
    const what = JSON.stringify(extra)
    assert.deepEqual(rt.calls, [], what)
    assert.deepEqual(out.states, [], what)
    assert.ok(out.errors.some((e) => e.startsWith(prefix)), `${what}: ${JSON.stringify(out.errors)}`)
    assert.equal(out.errors.at(-1), 'no agent launched', what)
  }
})

test('implement-v2-visual: absent args refuse the launch', async () => {
  let launched = 0
  const agent = async () => { launched += 1 }
  const out = await VISUAL(undefined, agent, null, null, () => {}, () => {}, null)
  assert.equal(launched, 0)
  assert.deepEqual(out.states, [])
  assert.equal(out.errors.at(-1), 'no agent launched')
})
