export const meta = {
  name: 'plan-v2-review',
  description: 'Review a v2 plan over its flow directory: survey and baselines, a lead-composed coverage map, review-v2-pipeline waves the lead settles, then the lead audit',
  whenToUse: 'Launched by the plan-v2-engine skill review procedure and by /review-plan-v2 in flow mode, full or recheck, with the slug, the started run and the run directory as args; not for direct use',
  phases: [
    { title: 'Survey', detail: 'full mode: one review-surveyor, then one verification agent for every resolved baseline' },
    { title: 'Compose', detail: 'plan-review-lead writes the coverage map, its cells and waivers' },
    { title: 'Waves', detail: 'review-v2-pipeline per wave, then plan-review-lead settles it and decides a follow-up, and review-verifier re-verifies what it names' },
    { title: 'Audit', detail: 'plan-review-lead audits the digest sample' },
  ],
}

// Agents and the lead write the store themselves, so this returns only an index.
// A stage that fails stops the run without retry: the orchestrator mops up the
// returned `unverified` ids and falls back to the engine at the first stage the
// store shows incomplete. Every prompt is built from `args` and earlier returns,
// so a same-args relaunch replays the journal; a re-check is always a fresh launch.
const SCRIPT_VERSION = '3'
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
// This script's own dispatches, incremented at each call site; each wave adds the pipeline's count.
let agents = 0
const index = () => ({
  slug: a.slug || null,
  run: a.run || null,
  mode: a.mode || null,
  waves,
  unverified: [...unverified],
  audit,
  agents,
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
  if (a.mechanical_lenses !== undefined && !lineList(a.mechanical_lenses)) {
    problems.push('mechanical_lenses must be an array of one-line lens ids, the `mode = "mechanical"` lenses `review catalogue` lists')
  }
  if (a.mode === 'full' && a.targets !== undefined && !(Array.isArray(a.targets) && !a.targets.length)) problems.push('targets is for recheck mode only')
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
    properties: { next: { anyOf: [{ type: 'null' }, WAVE] }, unverified: IDS, reverify: IDS, reverify_single: IDS },
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
  properties: {
    baselines: {
      type: 'array',
      items: {
        type: 'object',
        properties: { key: STR, outcome: { type: 'string', enum: ['pass', 'fail', 'timeout', 'flaky'] }, log: STR },
        required: ['key', 'outcome', 'log'],
        additionalProperties: false,
      },
    },
  },
  required: ['baselines'],
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

const lead = (phaseName, wave, extra) => caught(() => {
  agents++
  return agent(leadPrompt(phaseName, wave, extra), {
    agentType: 'plan-review-lead',
    schema: LEAD_SCHEMAS[phaseName],
    label: wave ? `lead ${phaseName} wave ${wave}` : `lead ${phaseName}`,
    phase: phaseName === 'compose' ? 'Compose' : phaseName === 'audit' ? 'Audit' : 'Waves',
  })
})

// A batch name doubles as the verifier's payload file name, and one reverify batch runs per wave.
const reverifyPrompt = (ids, wave, batch) => [
  a.ledger_line,
  `run: ${a.run}`,
  `wave: ${wave}`,
  `store: ${storeFlag}`,
  `pack: ${runDir}/pack.md`,
  `run directory: ${runDir}`,
  `findings: ${ids.join(', ')}`,
  ...(batch ? [`batch: ${batch}`] : []),
  '',
  `Fetch ${batch ? 'each finding' : 'the finding'} with \`tomlctl review show ${ids.join(',')} --for verifier ${storeFlag}\`, verify it refute-first, weighing each rule-out its row carries, and record ${batch ? 'each verdict' : 'the verdict'} with \`tomlctl review verdict\` using \`--by verifier\`. Return ${batch ? 'one verdict-return object per finding under `verdicts`' : 'the verdict-return object'}.`,
].join('\n')

const reverify = (id, wave) => caught(() => {
  agents++
  return agent(reverifyPrompt([id], wave), { agentType: 'review-verifier', schema: a.schemas.verdict_return, label: `reverify ${id}`, phase: 'Waves' })
})

const reverifyBatch = (ids, wave) => caught(() => {
  agents++
  const schema = { type: 'object', properties: { verdicts: { type: 'array', items: a.schemas.verdict_return } }, required: ['verdicts'] }
  return agent(reverifyPrompt(ids, wave, `reverify-${wave}`), { agentType: 'review-verifier', schema, label: `reverify x${ids.length}`, phase: 'Waves' })
})

const validWave = (w) => w && Number.isInteger(w.wave) && w.wave >= 1 && Array.isArray(w.cells) && w.cells.length > 0
  && w.cells.every((c) => matches(CELL, c.id))

// Full mode: the surveyor's digest and the recorded baseline lines the lead's compose phase receives.
let survey = null
const baselineLines = []

if (!recheck) {
  phase('Survey')
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
    '',
    `Run the survey for this run as your procedure says, reading its signals with \`tomlctl review run show ${a.run} ${storeFlag}\`, and return your digest under \`digest\`, exactly as your procedure shapes it. Return each of its \`verification:\` lines again under \`verification\`, as \`{key, command}\`: \`command\` holds the resolved command, or \`none — <reason>\` when the check resolved none.`,
  ].join('\n')
  const ret = await caught(() => {
    agents++
    return agent(surveyorPrompt, { agentType: 'review-surveyor', schema: SURVEY_SCHEMA, label: 'surveyor', phase: 'Survey' })
  })
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
  const toRun = []
  for (const v of ret.verification) {
    if (!matches(BASELINE_KEY, v.key) || seen.has(v.key) || !oneLine(v.command)) {
      fail(`surveyor verification line ${JSON.stringify(v)} is malformed or repeats its key; not dispatched, the lead records nothing for it`)
      continue
    }
    seen.add(v.key)
    if (/^none\b/.test(v.command.trim())) baselineLines.push(`${v.key}: ${v.command.trim()}`)
    else toRun.push(v)
  }

  if (toRun.length) {
    // The verification agent runs its list in order, so commands sharing a build lock never overlap.
    const prompt = [
      a.ledger_line,
      'commands:',
      ...toRun.flatMap((v) => [`- ${v.command}`, `  timeout: ${BASELINE_TIMEOUT}`]),
      'keep_going: true',
      '',
      'Run the commands as your procedure says. Return one entry per command under `baselines`, as `{key, outcome, log}`: `key` is the baseline key below for that command, `log` its log path.',
      ...block('keys', toRun.map((v) => `${v.key}: ${v.command}`)),
    ].join('\n')
    const b = await caught(() => {
      agents++
      return agent(prompt, { agentType: 'verification', schema: BASELINE_SCHEMA, label: `baselines x${toRun.length}`, phase: 'Survey' })
    })
    if (failed(b)) fail(`baselines: verification ${why(b)}`)
    const returned = new Map()
    for (const r of failed(b) ? [] : b.baselines) {
      if (toRun.some((v) => v.key === r.key) && !returned.has(r.key)) returned.set(r.key, r)
      else fail(`baselines: verification returned an unknown or repeated key ${JSON.stringify(r.key)}`)
    }
    for (const v of toRun) {
      const r = returned.get(v.key)
      if (!r || !oneLine(r.log)) {
        if (!failed(b)) fail(`baseline ${v.key}: verification returned no log`)
        baselineLines.push(`${v.key}: none — the verification agent returned no log`)
        continue
      }
      if (r.outcome === 'timeout') log(`baseline ${v.key}: timeout, its log is partial`)
      baselineLines.push(`${v.key}: ${r.log}`)
    }
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
  // A recheck runs no survey, so the orchestrator copies the first run's pack.md and
  // profile.md here before launching; the pipeline takes its run directory from this
  // path's parent, so pointing it at the first run's pack would redirect every payload.
  const ran = await caught(() => workflow('review-v2-pipeline', {
    run: a.run,
    wave: current.wave,
    depth: a.depth,
    pack_path: `${runDir}/pack.md`,
    cells: current.cells,
    store: { slug: a.slug },
    schemas: { cell_return: a.schemas.cell_return, verdict_return: a.schemas.verdict_return },
    ledger_line: a.ledger_line,
    batch_defects: true,
    ...(a.mechanical_lenses !== undefined ? { mechanical_lenses: a.mechanical_lenses } : {}),
  }))
  if (failed(ran)) {
    fail(`wave ${current.wave}: review-v2-pipeline ${why(ran)}; the store shows which cells sealed`)
    waves.push({ wave: current.wave, cells: [], verified: 0, agents: 0, settled: false })
    halted = true
    break
  }
  const childErrors = Array.isArray(ran.errors) ? ran.errors : []
  childErrors.forEach((e) => fail(`wave ${current.wave}: ${e}`))
  const cells = Array.isArray(ran.cells) ? ran.cells : []
  const waveAgents = Number.isInteger(ran.agents) ? ran.agents : 0
  agents += waveAgents
  const entry = { wave: current.wave, cells, verified: Number.isInteger(ran.verified) ? ran.verified : 0, agents: waveAgents, settled: false }
  waves.push(entry)
  if (!cells.length) {
    fail(`wave ${current.wave}: review-v2-pipeline ran no cell`)
    halted = true
    break
  }

  const escalated = (Array.isArray(ran.escalations) ? ran.escalations : [])
    .filter((e) => e && typeof e.text === 'string')
    .map((e) => `${typeof e.cell === 'string' ? e.cell : '?'}: ${e.text.replace(/\s*[\r\n]+\s*/g, ' ').trim()}`)
  const settled = await lead('settle', current.wave, block('escalations', escalated.length ? escalated : ['none']))
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

  // An id the orchestrator mops up as unverified is verified once, there.
  const again = [...new Set(Array.isArray(settled.reverify) ? settled.reverify : [])].filter((id) => {
    if (!matches(FINDING, id)) { fail(`lead settle wave ${current.wave} returned reverify id ${JSON.stringify(id)}`); return false }
    return !unverified.has(id)
  })
  if (again.length) {
    // An id carrying `same_as` goes alone, since only a single verdict can strike the link.
    const singleIds = new Set(Array.isArray(settled.reverify_single) ? settled.reverify_single : [])
    singleIds.forEach((id) => {
      if (!again.includes(id)) fail(`lead settle wave ${current.wave} returned reverify_single id ${JSON.stringify(id)} outside its reverify list`)
    })
    const singles = again.filter((id) => singleIds.has(id))
    const batched = again.filter((id) => !singleIds.has(id))
    log(`wave ${current.wave}: re-verifying ${again.join(', ')}`)
    const wave = current.wave
    const rets = await parallel([
      ...singles.map((id) => () => reverify(id, wave)),
      ...(batched.length ? [() => reverifyBatch(batched, wave)] : []),
    ])
    // A reverify id is a finding of this wave that already holds a verdict, so the
    // pipeline's `verified` counts it; a second verdict adds no distinct id.
    singles.forEach((id, i) => {
      const r = rets[i]
      if (failed(r)) {
        fail(`reverify ${id}: review-verifier ${why(r)}; left for the orchestrator's mop-up`)
        unverified.add(id)
      }
    })
    if (batched.length) {
      const r = rets[singles.length]
      if (failed(r)) {
        fail(`reverify batch ${batched.join(', ')}: review-verifier ${why(r)}; left for the orchestrator's mop-up`)
        batched.forEach((id) => unverified.add(id))
      } else {
        const returned = new Set((Array.isArray(r.verdicts) ? r.verdicts : []).map((v) => v && v.finding))
        const lost = batched.filter((id) => !returned.has(id))
        if (lost.length) {
          fail(`reverify batch: no verdict returned for ${lost.join(', ')}; left for the orchestrator's mop-up`)
          lost.forEach((id) => unverified.add(id))
        }
      }
    }
  }

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
