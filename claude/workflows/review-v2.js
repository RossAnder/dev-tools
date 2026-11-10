export const meta = {
  name: 'review-v2-pipeline',
  description: 'Run one review-v2 wave: one agent per cell, then route each finding to its verifier or pooled checker',
  whenToUse: 'Launched by the review-v2-engine skill at stage 4 with the run, wave, cells and schemas as args, or before the gate of an apply milestone with mode reproduce and the flow, milestone, round and units as args; not for direct use',
  phases: [
    { title: 'Cells', detail: 'one review-finder, review-finder-cf, review-sweeper or plan-executor per cell; each writes and seals its cell' },
    { title: 'Verify', detail: 'review-verifier per routed finding (panel: three seats), per-cell minor-defect and cleanup batches, pooled review-checker batches' },
    { title: 'Reproduce', detail: 'reproduce mode only: one review-verifier per apply task, recording whether each finding it fixed still reproduces' },
  ],
}

// Agents persist their own findings and verdicts in the store, so this returns
// only an index: per-cell counts, the number of agents launched, and each
// escalation's text, last so a truncated result loses those texts first. Only cell
// prompts are built from `args` alone: verifier and checker prompts embed the
// finding ids agents return, and checker batches fill in completion order, so a
// same-args relaunch can miss the journal from the first reshuffled batch. Store
// writes are idempotent either way. The review-v2-engine skill's
// references/agent-waves.md states what recovery costs.
const SCRIPT_VERSION = '3'
const CELL_AGENTS = ['review-finder', 'review-finder-cf', 'review-sweeper', 'plan-executor']
// The context-free correctness cell runs without the pack, so its prompt omits the `pack:` line.
const CONTEXT_FREE_AGENT = 'review-finder-cf'
const CHECKER_BATCH = 20
const PANEL_SEATS = 3
const ESCALATION_CHARS = 300

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }
const escalations = []
// Incremented beside every dispatch, so the count matches the launches the run paid for.
let agents = 0

const a = args || {}

// Reproduce mode runs no cells, takes no pack and no schemas, and returns its own
// index; the store's reproductions, not this return, are what the engine acts on.
if (a.mode === 'reproduce') {
  const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
  const TASK_REF = /^[a-z0-9_-]+$/
  const MILESTONE = /^[A-Za-z0-9][A-Za-z0-9_-]*$/
  const FINDING = /^F[1-9][0-9]*$/
  const REPRODUCE_VERDICTS = ['gone', 'reproduces', 'unsettled']
  // RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
  const matches = (re, value) => typeof value === 'string' && re.test(value)
  const oneLine = (value) => typeof value === 'string' && value.trim() !== '' && !/[\r\n]/.test(value)

  const rStore = a.store || {}
  const units = Array.isArray(a.units) ? a.units : []
  const reproduceIndex = (unitRows) => ({ mode: 'reproduce', units: unitRows, errors, script_version: SCRIPT_VERSION })

  const problems = []
  if (!oneLine(a.run_dir)) problems.push('run_dir must be one non-empty line')
  const hasSlug = rStore.slug !== undefined
  const hasScope = rStore.scope !== undefined
  if (hasSlug === hasScope) problems.push('store must hold exactly one of slug or scope')
  else if (!matches(SLUG, hasSlug ? rStore.slug : rStore.scope)) problems.push(`store.${hasSlug ? 'slug' : 'scope'} does not match ${SLUG.source}`)
  if (!matches(SLUG, a.flow)) problems.push(`flow ${JSON.stringify(a.flow)} does not match ${SLUG.source}`)
  if (!matches(MILESTONE, a.milestone)) problems.push(`milestone ${JSON.stringify(a.milestone)} does not match ${MILESTONE.source}`)
  if (!Number.isInteger(a.round) || a.round < 1) problems.push(`round ${JSON.stringify(a.round)} is not a positive integer`)
  if (!Array.isArray(a.units) || !units.length) problems.push('units must be a non-empty array')
  const seenTasks = new Set()
  units.forEach((u, i) => {
    if (!u || typeof u !== 'object') { problems.push(`units[${i}] is not an object`); return }
    if (!matches(TASK_REF, u.task)) problems.push(`units[${i}].task ${JSON.stringify(u.task)} does not match ${TASK_REF.source}`)
    else if (seenTasks.has(u.task)) problems.push(`units[${i}].task ${u.task} appears twice`)
    else seenTasks.add(u.task)
    if (!Array.isArray(u.files) || !u.files.every(oneLine)) problems.push(`units[${i}].files must be an array of one-line paths`)
    if (!Array.isArray(u.findings) || !u.findings.length || !u.findings.every((f) => matches(FINDING, f))) {
      problems.push(`units[${i}].findings must be a non-empty array of ids matching ${FINDING.source}`)
    }
  })
  if (problems.length) {
    problems.forEach(fail)
    fail('no agent launched')
    return reproduceIndex([])
  }

  const rStoreFlag = hasSlug ? `--slug ${rStore.slug}` : `--scope ${rStore.scope}`
  const rRunDir = a.run_dir.replace(/[\\/]+$/, '')
  const unitSchema = {
    type: 'object',
    properties: {
      task: { type: 'string' },
      reproductions: {
        type: 'array',
        items: {
          type: 'object',
          properties: { finding: { type: 'string' }, verdict: { type: 'string', enum: REPRODUCE_VERDICTS } },
          required: ['finding', 'verdict'],
          additionalProperties: false,
        },
      },
    },
    required: ['task', 'reproductions'],
    additionalProperties: false,
  }

  // A flow-bound store carries the `ledger:` line, which agent correlation reads at column 0.
  const reproducePrompt = (u) => {
    const payload = `${rRunDir}/payloads/reproduce-${u.task}.ndjson`
    return `${hasSlug ? `ledger: .claude/flows/${rStore.slug}/review-v2.toml\n` : ''}mode: reproduce
store: ${rStoreFlag}
flow: ${a.flow}
milestone: ${a.milestone}
round: ${a.round}
task: ${u.task}
files: ${u.files.length ? u.files.join(', ') : 'none'}
findings: ${u.findings.join(', ')}
run directory: ${rRunDir}

The apply task \`${u.task}\` has landed a fix for each finding above. Read them with \`tomlctl review show ${u.findings.join(',')} ${rStoreFlag} --for implementer\`, then judge against the working tree, as it stands now, whether each still reproduces: \`gone\` when the cited failure or cleanup can no longer occur, \`reproduces\` when it still can, \`unsettled\` when the decisive check is out of reach. Write one JSON line per finding to \`${payload}\` with the keys finding, flow (\`${a.flow}\`), milestone (\`${a.milestone}\`), round (${a.round}), task (\`${u.task}\`), verdict, by (\`verifier\`) and citation (a file:line or a command and its decisive output), and record them with \`tomlctl review reproduce ${rStoreFlag} --ndjson ${payload}\`. Change no other file: no source edit, no other store write. Return \`{task, reproductions: [{finding, verdict}]}\` for this task.`
  }

  const runUnit = async (u) => {
    let ret
    try {
      ret = await agent(reproducePrompt(u), { agentType: 'review-verifier', schema: unitSchema, label: `reproduce ${u.task}`, phase: 'Reproduce' })
    } catch (e) {
      const error = String((e && e.message) || e)
      fail(`reproduce ${u.task}: ${error}`)
      return { task: u.task, status: 'error', reproductions: [], error }
    }
    if (!ret) {
      fail(`reproduce ${u.task}: agent returned nothing; ${u.findings.join(', ')} unchecked`)
      return { task: u.task, status: 'no-return', reproductions: [], error: null }
    }
    const list = Array.isArray(ret.reproductions) ? ret.reproductions.filter((r) => r && u.findings.includes(r.finding)) : []
    const lost = u.findings.filter((id) => !list.some((r) => r.finding === id))
    const problemsOf = [
      ret.task !== u.task ? `returned task ${JSON.stringify(ret.task)}` : null,
      lost.length ? `no reproduction returned for ${lost.join(', ')}` : null,
    ].filter(Boolean)
    if (problemsOf.length) {
      const error = problemsOf.join('; ')
      fail(`reproduce ${u.task}: ${error}`)
      return { task: u.task, status: 'error', reproductions: list, error }
    }
    return { task: u.task, status: 'ok', reproductions: list, error: null }
  }

  phase('Reproduce')
  const rows = await parallel(units.map((u) => () => runUnit(u)))
  return reproduceIndex(rows.map((r, i) => r || { task: units[i].task, status: 'error', reproductions: [], error: 'unit did not settle' }))
}

const index = (cells, verified) => ({ run: a.run || null, wave: a.wave ?? null, cells, verified, agents, errors, script_version: SCRIPT_VERSION, escalations })

const store = a.store || {}
const storeFlag = store.slug ? `--slug ${store.slug}` : store.scope ? `--scope ${store.scope}` : null
const schemas = a.schemas || {}
const mechanicalLenses = Array.isArray(a.mechanical_lenses) ? a.mechanical_lenses.filter((l) => typeof l === 'string' && l) : []
const missing = ['run', 'wave', 'depth', 'pack_path'].filter((k) => a[k] === undefined || a[k] === null || a[k] === '')
if (!Array.isArray(a.cells)) missing.push('cells')
if (!storeFlag) missing.push('store.slug|store.scope')
if (!schemas.cell_return) missing.push('schemas.cell_return')
if (!schemas.verdict_return) missing.push('schemas.verdict_return')
if (missing.length) {
  fail(`args missing ${missing.join(', ')}; no agent launched`)
  return index([], 0)
}

const runDir = a.pack_path.replace(/[\\/][^\\/]*$/, '')
// A flow-bound run carries the `ledger:` line, which agent correlation reads at column 0.
const preambleLines = (withPack) => [
  a.ledger_line ? a.ledger_line.replace(/^\s+/, '') : null,
  `run: ${a.run}`,
  `wave: ${a.wave}`,
  `store: ${storeFlag}`,
  withPack ? `pack: ${a.pack_path}` : null,
  `run directory: ${runDir}`,
].filter(Boolean).join('\n')
const preamble = preambleLines(true)

const batchSchema = {
  type: 'object',
  properties: { verdicts: { type: 'array', items: schemas.verdict_return } },
  required: ['verdicts'],
}

const cellPrompt = (cell) => `${preambleLines(cell.agentType !== CONTEXT_FREE_AGENT)}
cell: ${cell.id}

Fetch your assignment with \`tomlctl review show ${cell.id} --for finder ${storeFlag}\`, then run the cell as your procedure says: write its full finding set with one \`tomlctl review add --run ${a.run} --cell ${cell.id} ${storeFlag}\` call, seal it with \`tomlctl review cover ${cell.id}\`, and return the cell-return object \`review add\` printed.`

// A batch name doubles as the agent's payload file name. It holds only a cell id,
// or the wave and a launch count, so a relaunch can reuse a name for different rows.
const verifierPrompt = (ids, seat, batch) => `${preamble}
findings: ${ids.join(', ')}
${batch ? `batch: ${batch}\n` : ''}${seat ? `panel seat: ${seat} of ${PANEL_SEATS}\n` : ''}
Fetch each finding with \`tomlctl review show ${ids.join(',')} --for verifier ${storeFlag}\`, verify it refute-first, and record each verdict with \`tomlctl review verdict\` using ${seat ? `\`--by panel --seat ${seat}\`` : '`--by verifier`'}. Return ${!batch ? 'the verdict-return object' : 'one verdict-return object per finding under `verdicts`'}.`

// The checker may refute only a sweeper cell's row or a cleanup row in a mechanical
// lens, and its projection carries neither the cell nor the lens's mode.
const checkerPrompt = (ids, batch, sweeperIds) => `${preamble}
findings: ${ids.join(', ')}
batch: ${batch}
sweeper rows: ${sweeperIds.length ? sweeperIds.join(', ') : 'none'}
mechanical lenses: ${mechanicalLenses.length ? mechanicalLenses.join(', ') : 'none'}

Fetch the rows with \`tomlctl review show ${ids.join(',')} --for checker ${storeFlag}\`, check each by your predicates, and record each verdict with \`tomlctl review verdict --by checker\`. Return one verdict-return object per finding under \`verdicts\`.`

const verifiedIds = new Set()
const record = (expected, ret, label) => {
  if (!ret) { fail(`${label}: agent returned nothing; ${expected.join(', ')} left unverified`); return [] }
  const list = Array.isArray(ret.verdicts) ? ret.verdicts : [ret]
  const seen = new Set()
  for (const v of list) {
    if (!v || !v.finding) continue
    seen.add(v.finding)
    if (v.verdict && v.verdict !== 'unsettled') verifiedIds.add(v.finding)
  }
  const lost = expected.filter((id) => !seen.has(id))
  if (lost.length) fail(`${label}: no verdict returned for ${lost.join(', ')}`)
  return list
}

const verifyOne = async (id) => {
  agents++
  const ret = await agent(verifierPrompt([id]), { agentType: 'review-verifier', schema: schemas.verdict_return, label: `verify ${id}`, phase: 'Verify' })
  record([id], ret, `verify ${id}`)
}

const verifyPanel = (id) => parallel(Array.from({ length: PANEL_SEATS }, (_, i) => async () => {
  agents++
  const ret = await agent(verifierPrompt([id], i + 1), { agentType: 'review-verifier', schema: schemas.verdict_return, label: `panel ${id} seat ${i + 1}`, phase: 'Verify' })
  record([id], ret, `panel ${id} seat ${i + 1}`)
}))

const verifyBatch = async (cellId, ids, kind) => {
  agents++
  const ret = await agent(verifierPrompt(ids, null, `verify-${cellId}-${kind}`), { agentType: 'review-verifier', schema: batchSchema, label: `verify ${cellId} ${kind} x${ids.length}`, phase: 'Verify' })
  record(ids, ret, `verify ${cellId} ${kind} batch`)
}

// Checker rows are pooled across cells: a batch launches as soon as the pool
// holds CHECKER_BATCH rows, and the remainder flushes once every cell is done.
const pool = []
const checkerRuns = []
let checkerBatches = 0
const launchChecker = (rows) => {
  const n = ++checkerBatches
  const ids = rows.map((r) => r.id)
  const sweeperIds = rows.filter((r) => r.agentType === 'review-sweeper').map((r) => r.id)
  checkerRuns.push((async () => {
    agents++
    const ret = await agent(checkerPrompt(ids, `check-${a.wave}-${n}`, sweeperIds), { agentType: 'review-checker', schema: batchSchema, label: `check batch ${n} x${ids.length}`, phase: 'Verify' })
    const list = record(ids, ret, `check batch ${n}`)
    const reroute = new Set()
    for (const v of list) {
      if (!v || !v.finding) continue
      if (Array.isArray(v.reroute)) v.reroute.forEach((id) => reroute.add(id))
      else if (v.reroute || v.verdict === 'unsettled') reroute.add(v.finding)
    }
    if (reroute.size) {
      log(`check batch ${n}: ${reroute.size} unsettled → review-verifier (${[...reroute].join(', ')})`)
      await parallel([...reroute].map((id) => () => verifyOne(id)))
    }
  })())
}
const poolChecker = (ids, agentType) => {
  pool.push(...ids.map((id) => ({ id, agentType })))
  while (pool.length >= CHECKER_BATCH) launchChecker(pool.splice(0, CHECKER_BATCH))
}

const runCell = async (cell) => {
  if (!cell || !cell.id || !CELL_AGENTS.includes(cell.agentType)) {
    fail(`skipped cell ${cell && cell.id ? cell.id : '(no id)'}: agentType ${cell && cell.agentType ? cell.agentType : '(none)'} is not one of ${CELL_AGENTS.join(', ')}`)
    return { skipped: true }
  }
  agents++
  const ret = await agent(cellPrompt(cell), { agentType: cell.agentType, schema: schemas.cell_return, label: `cell ${cell.id} ${cell.agentType}`, phase: 'Cells' })
  if (!ret) fail(`cell ${cell.id}: agent returned nothing; the digest shows whether it sealed`)
  return ret
}

const routeCell = async (ret, cell) => {
  const id = cell && cell.id ? cell.id : null
  if (ret && ret.skipped) return { id, status: 'skipped', findings: 0 }
  if (!ret) return { id, status: 'no-return', findings: 0 }
  const findings = Array.isArray(ret.findings) ? ret.findings : []
  const escalate = Array.isArray(ret.escalate) ? ret.escalate : ret.escalate ? [ret.escalate] : []
  for (const e of escalate) {
    const text = typeof e === 'string' ? e : JSON.stringify(e)
    log(`cell ${cell.id} escalated: ${text}`)
    escalations.push({ cell: cell.id, text: text.length > ESCALATION_CHARS ? text.slice(0, ESCALATION_CHARS) : text })
  }
  if (ret.status && ret.status !== 'covered') log(`cell ${cell.id} sealed ${ret.status}`)

  const singles = []
  const panels = []
  const minors = []
  const cleanups = []
  const checks = []
  for (const f of findings) {
    if (!f || !f.id) continue
    if (f.route === 'panel') panels.push(f.id)
    else if (f.route === 'checker') checks.push(f.id)
    else if (f.route === 'verifier') {
      if (f.kind === 'cleanup') cleanups.push(f.id)
      else if (f.kind === 'defect' && f.severity === 'minor' && a.depth !== 'deep') minors.push(f.id)
      else singles.push(f.id)
    } else if (f.route && f.route !== 'none') fail(`cell ${cell.id}: ${f.id} has unknown route ${f.route}; left unverified`)
  }
  if (checks.length) poolChecker(checks, cell.agentType)
  if (cleanups.length === 1) singles.push(cleanups.pop())

  const work = [
    ...singles.map((id) => () => verifyOne(id)),
    ...panels.map((id) => () => verifyPanel(id)),
    ...(minors.length ? [() => verifyBatch(cell.id, minors, 'minor')] : []),
    ...(cleanups.length ? [() => verifyBatch(cell.id, cleanups, 'cleanup')] : []),
  ]
  if (work.length) await parallel(work)
  return { id: cell.id, status: ret.status || null, findings: findings.length, escalated: escalate.length }
}

phase('Cells')
const cells = await pipeline(a.cells, runCell, routeCell)

phase('Verify')
if (pool.length) launchChecker(pool.splice(0))
await Promise.all(checkerRuns)

return index(cells.map((c, i) => c || { id: a.cells[i] && a.cells[i].id ? a.cells[i].id : null, status: 'error', findings: 0 }), verifiedIds.size)
