export const meta = {
  name: 'review-v2-pipeline',
  description: 'Run one review-v2 wave: one agent per cell, then route each finding to its verifier or pooled checker',
  whenToUse: 'Launched by the review-v2-engine skill at stage 4 with the run, wave, cells and schemas as args; not for direct use',
  phases: [
    { title: 'Cells', detail: 'one review-finder, review-sweeper or plan-executor per cell; each writes and seals its cell' },
    { title: 'Verify', detail: 'review-verifier per routed finding (panel: three seats), minor-defect batches, pooled review-checker batches' },
  ],
}

// Agents persist their own findings and verdicts in the store, so this returns
// only an index. Every prompt is built from `args` alone: a relaunch with the
// same args replays the same agent calls, and the store writes are idempotent.
const SCRIPT_VERSION = '1'
const CELL_AGENTS = ['review-finder', 'review-sweeper', 'plan-executor']
const CHECKER_BATCH = 20
const PANEL_SEATS = 3

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const index = (cells, verified) => ({ run: a.run || null, wave: a.wave ?? null, cells, verified, errors, script_version: SCRIPT_VERSION })

const store = a.store || {}
const storeFlag = store.slug ? `--slug ${store.slug}` : store.scope ? `--scope ${store.scope}` : null
const schemas = a.schemas || {}
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
const preamble = [
  a.ledger_line ? a.ledger_line.replace(/^\s+/, '') : null,
  `run: ${a.run}`,
  `wave: ${a.wave}`,
  `store: ${storeFlag}`,
  `pack: ${a.pack_path}`,
  `run directory: ${runDir}`,
].filter(Boolean).join('\n')

const batchSchema = {
  type: 'object',
  properties: { verdicts: { type: 'array', items: schemas.verdict_return } },
  required: ['verdicts'],
}

const cellPrompt = (cell) => `${preamble}
cell: ${cell.id}

Fetch your assignment with \`tomlctl review show ${cell.id} --for finder ${storeFlag}\`, then run the cell as your procedure says: write its full finding set with one \`tomlctl review add --run ${a.run} --cell ${cell.id} ${storeFlag}\` call, seal it with \`tomlctl review cover ${cell.id}\`, and return the cell-return object \`review add\` printed.`

// A batch name doubles as the agent's payload file name, so it is derived from
// args alone and a relaunch reuses it.
const verifierPrompt = (ids, seat, batch) => `${preamble}
findings: ${ids.join(', ')}
${batch ? `batch: ${batch}\n` : ''}${seat ? `panel seat: ${seat} of ${PANEL_SEATS}\n` : ''}
Fetch each finding with \`tomlctl review show ${ids.join(',')} --for verifier ${storeFlag}\`, verify it refute-first, and record each verdict with \`tomlctl review verdict\` using ${seat ? `\`--by panel --seat ${seat}\`` : '`--by verifier`'}. Return ${ids.length === 1 ? 'the verdict-return object' : 'one verdict-return object per finding under `verdicts`'}.`

// The checker may refute only a sweeper cell's row, and its projection does not carry the cell.
const checkerPrompt = (ids, batch, sweeperIds) => `${preamble}
findings: ${ids.join(', ')}
batch: ${batch}
sweeper rows: ${sweeperIds.length ? sweeperIds.join(', ') : 'none'}

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
  const ret = await agent(verifierPrompt([id]), { agentType: 'review-verifier', schema: schemas.verdict_return, label: `verify ${id}`, phase: 'Verify' })
  record([id], ret, `verify ${id}`)
}

const verifyPanel = (id) => parallel(Array.from({ length: PANEL_SEATS }, (_, i) => async () => {
  const ret = await agent(verifierPrompt([id], i + 1), { agentType: 'review-verifier', schema: schemas.verdict_return, label: `panel ${id} seat ${i + 1}`, phase: 'Verify' })
  record([id], ret, `panel ${id} seat ${i + 1}`)
}))

const verifyBatch = async (cellId, ids) => {
  const ret = await agent(verifierPrompt(ids, null, `verify-${cellId}-minor`), { agentType: 'review-verifier', schema: batchSchema, label: `verify ${cellId} minor x${ids.length}`, phase: 'Verify' })
  record(ids, ret, `verify ${cellId} minor batch`)
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
  for (const e of escalate) log(`cell ${cell.id} escalated: ${typeof e === 'string' ? e : JSON.stringify(e)}`)
  if (ret.status && ret.status !== 'covered') log(`cell ${cell.id} sealed ${ret.status}`)

  const singles = []
  const panels = []
  const minors = []
  const checks = []
  for (const f of findings) {
    if (!f || !f.id) continue
    if (f.route === 'panel') panels.push(f.id)
    else if (f.route === 'checker') checks.push(f.id)
    else if (f.route === 'verifier') {
      if (f.kind === 'defect' && f.severity === 'minor' && a.depth !== 'deep') minors.push(f.id)
      else singles.push(f.id)
    } else if (f.route && f.route !== 'none') fail(`cell ${cell.id}: ${f.id} has unknown route ${f.route}; left unverified`)
  }
  if (checks.length) poolChecker(checks, cell.agentType)

  const work = [
    ...singles.map((id) => () => verifyOne(id)),
    ...panels.map((id) => () => verifyPanel(id)),
    ...(minors.length ? [() => verifyBatch(cell.id, minors)] : []),
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
