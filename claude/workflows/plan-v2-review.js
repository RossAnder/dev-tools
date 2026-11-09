export const meta = {
  name: 'plan-v2-review',
  description: 'Review a v2 plan over its flow directory: survey and baselines, a lead-composed coverage map, review-v2-pipeline waves the lead settles, then the lead audit',
  whenToUse: 'Launched by the plan-v2-engine skill review procedure and by /review-plan-v2 in flow mode, full or recheck, with the slug, the started run and the run directory as args; not for direct use',
  phases: [
    { title: 'Survey', detail: 'full mode: one review-surveyor, then one verification agent per resolved baseline, in sequence' },
    { title: 'Compose', detail: 'plan-review-lead writes the coverage map, its cells and waivers' },
    { title: 'Waves', detail: 'review-v2-pipeline per wave, then plan-review-lead settles it and decides a follow-up' },
    { title: 'Audit', detail: 'plan-review-lead audits the digest sample' },
  ],
}

// Agents and the lead write the store themselves, so this returns only an index.
// A stage that fails stops the run without retry: the orchestrator mops up the
// returned `unverified` ids and falls back to the engine at the first stage the
// store shows incomplete. Every prompt is built from `args` and earlier returns,
// so a same-args relaunch replays the journal; a re-check is always a fresh launch.
const SCRIPT_VERSION = '1'
const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/
const RUN = /^U[1-9][0-9]*$/
const CELL = /^C[1-9][0-9]*$/
const FINDING = /^F[1-9][0-9]*$/
const BASELINE_KEY = /^[a-z0-9][a-z0-9-]*$/
const MODES = ['full', 'recheck']
const DEPTHS = ['quick', 'standard', 'deep']
// The first wave plus the follow-up allowance; `quick` adds its one escalation probe wave.
const MAX_WAVES = { quick: 2, standard: 2, deep: 3 }
const BASELINE_TIMEOUT = 540

const errors = []
const fail = (msg) => { errors.push(msg); log(msg) }

const a = args || {}
const signals = a.signals || {}
const unverified = new Set()
const waves = []
let audit = null
const index = () => ({
  slug: a.slug || null,
  run: a.run || null,
  mode: a.mode || null,
  waves,
  unverified: [...unverified],
  audit,
  errors,
  script_version: SCRIPT_VERSION,
})

// RegExp.test coerces its argument, which would pass ['some-slug'] or 123.
const matches = (re, value) => typeof value === 'string' && re.test(value)
const oneLine = (value) => typeof value === 'string' && value.trim() !== '' && !/[\r\n]/.test(value)
const lineList = (value) => Array.isArray(value) && value.every(oneLine)

const argProblems = () => {
  const problems = []
  if (!matches(SLUG, a.slug)) problems.push(`slug ${JSON.stringify(a.slug)} does not match ${SLUG.source}`)
  if (!matches(RUN, a.run)) problems.push(`run ${JSON.stringify(a.run)} does not match ${RUN.source}`)
  if (!MODES.includes(a.mode)) problems.push(`mode ${JSON.stringify(a.mode)} is not one of ${MODES.join(', ')}`)
  if (!DEPTHS.includes(a.depth)) problems.push(`depth ${JSON.stringify(a.depth)} is not one of ${DEPTHS.join(', ')}`)
  if (!oneLine(a.run_dir)) problems.push('run_dir must be one non-empty line')
  if (matches(SLUG, a.slug) && a.ledger_line !== `ledger: .claude/flows/${a.slug}/review-v2.toml`) {
    problems.push(`ledger_line must be exactly \`ledger: .claude/flows/${a.slug}/review-v2.toml\`, the line agent correlation reads`)
  }
  if (!a.schemas || !a.schemas.cell_return || !a.schemas.verdict_return) problems.push('schemas must carry cell_return and verdict_return')
  if (signals.steering !== undefined && !lineList(signals.steering)) {
    problems.push('signals.steering must be an array of one-line flags, because a later line could pose as a prompt key')
  }
  if (a.mode === 'full') {
    const detected = signals.survey
    if (!Array.isArray(detected)) problems.push('signals.survey must be the array of {name, roots} `review survey` printed')
    else detected.forEach((s, i) => {
      if (!s || !oneLine(s.name) || !lineList(s.roots || [])) problems.push(`signals.survey ${i}: needs a one-line name and an array of one-line roots`)
    })
    if (a.targets !== undefined && !(Array.isArray(a.targets) && !a.targets.length)) problems.push('targets is for recheck mode only')
  }
  if (a.mode === 'recheck') {
    if (!lineList(a.targets) || !a.targets.length) problems.push('targets must be a non-empty array of one-line targets in recheck mode')
    if (a.depth !== 'quick') problems.push('a recheck runs at depth quick')
  }
  return problems
}

const problems = argProblems()
if (problems.length) {
  problems.forEach(fail)
  fail('no agent launched')
  return index()
}

const recheck = a.mode === 'recheck'
const storeFlag = `--slug ${a.slug}`
const runDir = a.run_dir.replace(/[\\/]+$/, '')
const steering = signals.steering && signals.steering.length ? signals.steering : ['none']

// The schema forbids extra keys on every return here, so a real return never carries `thrown`.
const caught = async (run) => {
  try {
    return await run()
  } catch (e) {
    return { thrown: String((e && e.message) || e) }
  }
}
const failed = (ret) => !ret || ret.thrown !== undefined
const why = (ret) => (!ret ? 'returned nothing' : `failed: ${ret.thrown}`)

const STR = { type: 'string' }
const strings = (...keys) => Object.fromEntries(keys.map((k) => [k, STR]))
const CELL_LIST = {
  type: 'array',
  items: { type: 'object', properties: strings('id', 'agentType'), required: ['id', 'agentType'], additionalProperties: false },
}
const WAVE = { type: 'object', properties: { wave: { type: 'integer' }, cells: CELL_LIST }, required: ['wave', 'cells'], additionalProperties: false }
const IDS = { type: 'array', items: STR }

const LEAD_SCHEMAS = {
  compose: WAVE,
  settle: {
    type: 'object',
    properties: { next: { anyOf: [{ type: 'null' }, WAVE] }, unverified: IDS },
    required: ['next', 'unverified'],
    additionalProperties: false,
  },
  audit: {
    type: 'object',
    properties: {
      sampled: { type: 'integer' },
      overturned: IDS,
      contested: {
        type: 'array',
        items: {
          type: 'object',
          properties: { id: STR, verdict: { type: 'string', enum: ['confirmed', 'plausible', 'refuted'] } },
          required: ['id', 'verdict'],
          additionalProperties: false,
        },
      },
    },
    required: ['sampled', 'overturned', 'contested'],
    additionalProperties: false,
  },
}

// `command` is the digest's `verification:` line after its key: a command, or `none — <reason>`.
const SURVEY_SCHEMA = {
  type: 'object',
  properties: {
    digest: STR,
    verification: {
      type: 'array',
      items: { type: 'object', properties: strings('key', 'command'), required: ['key', 'command'], additionalProperties: false },
    },
  },
  required: ['digest', 'verification'],
  additionalProperties: false,
}

const BASELINE_SCHEMA = {
  type: 'object',
  properties: { outcome: { type: 'string', enum: ['pass', 'fail', 'timeout', 'flaky'] }, log: STR },
  required: ['outcome', 'log'],
  additionalProperties: false,
}

const block = (key, lines) => [`${key}:`, ...lines.map((l) => `  ${l}`)]

const leadPrompt = (phaseName, wave, extra) => [
  a.ledger_line,
  `phase: ${phaseName}`,
  `run: ${a.run}`,
  `store: ${storeFlag}`,
  `run directory: ${runDir}`,
  `mode: ${a.mode}`,
  `depth: ${a.depth}`,
  wave ? `wave: ${wave}` : null,
  ...(extra || []),
  '',
  `Run phase ${phaseName} as your procedure says and return only its object.`,
].filter((l) => l !== null).join('\n')

const lead = (phaseName, wave, extra) => caught(() => agent(leadPrompt(phaseName, wave, extra), {
  agentType: 'plan-review-lead',
  schema: LEAD_SCHEMAS[phaseName],
  label: wave ? `lead ${phaseName} wave ${wave}` : `lead ${phaseName}`,
  phase: phaseName === 'compose' ? 'Compose' : phaseName === 'audit' ? 'Audit' : 'Waves',
}))

const validWave = (w) => w && Number.isInteger(w.wave) && w.wave >= 1 && Array.isArray(w.cells) && w.cells.length > 0
  && w.cells.every((c) => matches(CELL, c.id))

// Full mode: the surveyor's digest and the recorded baseline lines the lead's compose phase receives.
let survey = null
const baselineLines = []

if (!recheck) {
  phase('Survey')
  const detected = signals.survey.map((s) => `- ${s.name} — roots: ${(s.roots || []).length ? s.roots.join(', ') : 'none'}`)
  const surveyorPrompt = [
    a.ledger_line,
    `run: ${a.run}`,
    'wave: 0',
    `store: ${storeFlag}`,
    `pack: ${runDir}/pack.md`,
    `run directory: ${runDir}`,
    'family: plan',
    `scope: docs/flows/${a.slug}/brief.md, docs/flows/${a.slug}/design/*.md`,
    `flow: ${a.slug}`,
    ...(detected.length ? ['signals:', ...detected] : ['signals: none']),
    '',
    'Run the survey for this run as your procedure says and return your digest under `digest`, exactly as your procedure shapes it. Return each of its `verification:` lines again under `verification`, as `{key, command}`: `command` holds the resolved command, or `none — <reason>` when the check resolved none.',
  ].join('\n')
  const ret = await caught(() => agent(surveyorPrompt, { agentType: 'review-surveyor', schema: SURVEY_SCHEMA, label: 'surveyor', phase: 'Survey' }))
  if (failed(ret)) {
    fail(`surveyor ${why(ret)}; no cell composed`)
    return index()
  }
  if (/^\s*missing:/.test(ret.digest)) {
    fail(`surveyor stopped with \`${ret.digest.trim().split(/\r?\n/)[0]}\`; no cell composed`)
    return index()
  }
  survey = ret.digest

  const seen = new Set()
  for (const v of ret.verification) {
    if (!matches(BASELINE_KEY, v.key) || seen.has(v.key) || !oneLine(v.command)) {
      fail(`surveyor verification line ${JSON.stringify(v)} is malformed or repeats its key; not dispatched, the lead records nothing for it`)
      continue
    }
    seen.add(v.key)
    if (/^none\b/.test(v.command.trim())) {
      baselineLines.push(`${v.key}: ${v.command.trim()}`)
      continue
    }
    // One at a time: the commands share the build's target directory and its lock.
    const prompt = `${a.ledger_line}
commands:
- ${v.command}
  timeout: ${BASELINE_TIMEOUT}

Run the command as your procedure says, then return its \`outcome\` and its \`log\` path.`
    const b = await caught(() => agent(prompt, { agentType: 'verification', schema: BASELINE_SCHEMA, label: `baseline ${v.key}`, phase: 'Survey' }))
    if (failed(b) || !oneLine(b.log)) {
      fail(`baseline ${v.key}: verification ${failed(b) ? why(b) : 'returned no log'}`)
      baselineLines.push(`${v.key}: none — the verification agent returned no log`)
      continue
    }
    if (b.outcome === 'timeout') log(`baseline ${v.key}: timeout, its log is partial`)
    baselineLines.push(`${v.key}: ${b.log}`)
  }
}

phase('Compose')
const composeExtra = [
  ...block('steering', steering),
  ...(recheck ? block('targets', a.targets) : []),
  ...(!recheck ? block('baselines', baselineLines.length ? baselineLines : ['none']) : []),
  ...(!recheck ? block('survey', survey.split(/\r?\n/)) : []),
]
const composed = await lead('compose', 1, composeExtra)
if (failed(composed)) {
  fail(`lead compose ${why(composed)}; fall back to engine stage 3`)
  return index()
}
if (!validWave(composed)) {
  fail(`lead compose wrote no wave (wave ${composed.wave}, ${composed.cells.length} cells); fall back to engine stage 3`)
  return index()
}

const maxWaves = MAX_WAVES[a.depth]
// A recheck runs at quick, so its only follow-up is the one escalation probe cell.
const escalationProbe = (w) => Array.isArray(w.cells) && w.cells.length === 1 && w.cells[0].agentType === 'review-finder'
let current = { wave: composed.wave, cells: composed.cells }
let halted = false

while (current) {
  phase('Waves')
  const ran = await caught(() => workflow('review-v2-pipeline', {
    run: a.run,
    wave: current.wave,
    depth: a.depth,
    pack_path: `${runDir}/pack.md`,
    cells: current.cells,
    store: { slug: a.slug },
    schemas: { cell_return: a.schemas.cell_return, verdict_return: a.schemas.verdict_return },
    ledger_line: a.ledger_line,
  }))
  if (failed(ran)) {
    fail(`wave ${current.wave}: review-v2-pipeline ${why(ran)}; the store shows which cells sealed`)
    waves.push({ wave: current.wave, cells: [], verified: 0, settled: false })
    halted = true
    break
  }
  const childErrors = Array.isArray(ran.errors) ? ran.errors : []
  childErrors.forEach((e) => fail(`wave ${current.wave}: ${e}`))
  const cells = Array.isArray(ran.cells) ? ran.cells : []
  const entry = { wave: current.wave, cells, verified: Number.isInteger(ran.verified) ? ran.verified : 0, settled: false }
  waves.push(entry)
  if (!cells.length) {
    fail(`wave ${current.wave}: review-v2-pipeline ran no cell`)
    halted = true
    break
  }

  const settled = await lead('settle', current.wave)
  if (failed(settled)) {
    fail(`lead settle wave ${current.wave} ${why(settled)}; read the digest for unverified ids`)
    halted = true
    break
  }
  entry.settled = true
  settled.unverified.forEach((id) => {
    if (matches(FINDING, id)) unverified.add(id)
    else fail(`lead settle wave ${current.wave} returned unverified id ${JSON.stringify(id)}`)
  })

  const next = settled.next
  if (!next) { current = null; continue }
  if (recheck && !escalationProbe(next)) {
    fail(`lead settle wave ${current.wave} wrote follow-up wave ${next.wave} in recheck mode, which allows only one review-finder escalation probe cell; not launched`)
    current = null
  } else if (!validWave(next) || next.wave <= current.wave) {
    fail(`lead settle wave ${current.wave} returned an unusable follow-up ${JSON.stringify(next)}; not launched`)
    current = null
  } else if (waves.length >= maxWaves) {
    fail(`lead settle wave ${current.wave} wrote wave ${next.wave} past the ${maxWaves}-wave allowance at ${a.depth}; cells ${next.cells.map((c) => c.id).join(', ')} not launched`)
    current = null
  } else {
    log(`wave ${next.wave}: ${next.cells.length} follow-up cells`)
    current = { wave: next.wave, cells: next.cells }
  }
}

if (halted) {
  fail('no audit ran')
  return index()
}

phase('Audit')
const audited = await lead('audit', null)
if (failed(audited)) fail(`lead audit ${why(audited)}`)
else audit = { sampled: audited.sampled, overturned: audited.overturned, contested: audited.contested }

return index()
