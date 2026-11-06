# /plan-v2 probes: runbook and record

The live checks that `docs/plans/we-ve-just-implement-the-shimmying-kettle.md` needs before milestone B is written. Subagents cannot launch Workflows, agents load only after a restart, and a resume test has to kill a session, so a person runs this file. Each probe answers a question the design turns on, and `## Results` at the end is what milestone B reads.

Every step says where it runs:

- **PowerShell** means one window, opened at the repo root and kept open throughout, because later steps use the variables and function from step 0. Only the links in step 1 need a separate, elevated one.
- **Claude session** means an interactive `claude`, and the step says which directory and whether it is new. Paste the quoted prompt.

Run the steps in order. Do them in one sitting if you can, since a resume test needs the session it started.

If a Workflow launch is refused or returns an error at any step, go to [If Workflows do not run](#if-workflows-do-not-run).

## Step 0: variables and a clean start

**PowerShell, repo root.** Milestone A should already be committed, so `git status --short` should print nothing. If it lists anything, note it, because the teardown compares against this.

```powershell
$repo = (Get-Location).Path
$cfg = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { "$HOME\.claude" }
$root = "$HOME\plan-v2-replay"
$repoProject = "$cfg\projects\" + ($repo -replace '[^A-Za-z0-9-]', '-')
$rootProject = "$cfg\projects\" + ($root -replace '[^A-Za-z0-9-]', '-')
$probeNotes = "$repo\docs\flows\plan-v2-probe\notes"
$target = "$repo\docs\flows\plan-v2-probe\t2-target.md"
function Get-AgentRows($dir, $since) {
  Get-ChildItem $dir -Recurse -Filter 'agent-*.jsonl' | Where-Object { $_.LastWriteTime -gt $since } | ForEach-Object {
    $lens = Select-String -LiteralPath $_.FullName -Pattern 'lens: (probe-[a-z]+)' -List
    [pscustomobject]@{
      File         = $_.Name
      Lens         = if ($lens) { $lens.Matches[0].Groups[1].Value } else { '' }
      Instructions = @(Select-String -LiteralPath $_.FullName -SimpleMatch '"attachment":{"type":"instructions"').Count
      NestedMemory = @(Select-String -LiteralPath $_.FullName -SimpleMatch '"type":"nested_memory"').Count
    }
  }
}
git status --short
```

## Step 1: links

**A second PowerShell window, elevated (Run as administrator), for this step only.** `New-Item -ItemType SymbolicLink` refuses here without elevation, with "Administrator privilege required", because Developer Mode is off. That window starts elsewhere and has no variables, so the block sets its own. Correct `$repo` if your clone is elsewhere.

Nothing deploys `claude/`, so these four entries are linked by hand. `workflows/` does not exist yet and is created first.

```powershell
$repo = 'C:\Users\rossa\dev\dev-tools'
$cfg = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { "$HOME\.claude" }
New-Item -ItemType Directory -Force "$cfg\workflows" | Out-Null
foreach ($name in 'plan-investigator', 'plan-challenger', 'plan-cold-reader') {
  New-Item -ItemType SymbolicLink -Path "$cfg\agents\$name.md" -Target "$repo\claude\agents\$name.md" | Out-Null
}
New-Item -ItemType SymbolicLink -Path "$cfg\workflows\plan-v2-investigate.js" -Target "$repo\claude\workflows\plan-v2-investigate.js" | Out-Null
Get-Content -TotalCount 1 "$cfg\agents\plan-challenger.md", "$cfg\workflows\plan-v2-investigate.js"
```

The last line prints `---` and `export const meta = {`, which shows both links resolve. Close the elevated window.

Close every open Claude session on this machine. Agents load at session start, so a session already running will not see these.

## Step 2: lookup

Does `Workflow({name:"plan-v2-investigate"})` find a symlinked script? The empty args make the script return its argument errors and launch no agent, so this costs nothing.

**Claude session, new, repo root:**

> Call `Workflow({name:"plan-v2-investigate", args:{}})`. When its notification arrives, show me `<result>` in full and stop.

- The result lists `slug` and `lenses` errors and `no agent launched`: the script was found. Write `Lookup: symlink`.
- The name is not found: copy instead, as below, start a new session and repeat this step. Write `Lookup: copy`, and milestone B's handover says to copy.

**PowerShell**, only if it was not found:

```powershell
Remove-Item "$cfg\workflows\plan-v2-investigate.js"
Copy-Item "$repo\claude\workflows\plan-v2-investigate.js" "$cfg\workflows\plan-v2-investigate.js"
```

## Step 3: replay

Would the structure challenge have changed what a human asked for? Six goals, each replayed against its base commit: the first three rows are goals where a human chose a restructure, and the last three are controls where none did.

### 3a. Review the goals table

Read the table below and rewrite any cell that names a solution, since the challenger reads these cells and nothing else about the goal. The cells were distilled by hand from each plan's Context only. No technique, module, verb or crate named in that plan's Scope or Tasks should appear. Keep each cell on one line, with no `|`. The later steps parse this table, so your edits reach the agents.

Then write `Intents: reviewed` under `## Results` and save the file before launching.

| Slug | Base | Intent | Future state |
|---|---|---|---|
| polymorphic-wondering-magpie | 5b58a7e | The live terminal view of a flow feels heavy on this machine. Every change to what it shows, every refresh of its flow list and every agent event costs one or two fresh process starts, each slow here, and it keeps waking every half second when nothing has happened. It also misses edits made in place to a file, and its drawing waits on reading a transcript. | A flow file changing reaches the screen with no process started. An idle view does next to no periodic work. Edits made in place are seen. Drawing never waits on a file read. All of this holds on Windows, Linux and macOS. |
| toasty-wobbling-stonebraker | 5dc5b8d | The v2 review system's findings store gives a finding's status no single meaning: separate commands each keep their own idea of which statuses count for what, and these already disagree. Reopening an exported finding is silently undone at the next run start. Writes break when repeated or given large payloads, and one signal gets settled from a look at git history that can read garbage. Later v2 stores are about to copy this one. | A finding's status means the same thing to every command. A reopened finding stays reopened across runs. Repeating a write is safe, large payloads get through, and the git-derived signal no longer settles on garbage. A new store can copy this one without inheriting these faults. |
| twinkling-hopping-otter | d7485a9 | Agents driving the repo's flow-store command-line tool keep wrapping it. Transcripts from this Windows machine show thousands of python and node snippets, plus helper scripts, written to narrow rows, reach into nested fields, slice long text, escape free text, type today's date and look up ids. Multi-line heredocs fail and the guidance contradicts itself on them, the commit sequence is prose each agent reimplements, one small settings file takes four calls to update, and agents guess commands that do not exist. | Each workaround class in the evidence has a direct form in the tool. The guidance shows only that form and cannot quietly drift back. Agents running the flows need no python, node, heredoc or shell loop around the tool, on Linux or Windows. |
| whimsical-hugging-puppy | 0d1bf49 | A plan's task section is the largest single thing an orchestrator loads, 50 to 86 KB on big plans, of which about 2 KB is the graph of ids, files and edges. The rest is prose only the agent doing that task needs. The frontier, file overlaps and checkpoint closures are worked out in prose every run and checked again by hand, planners bend dependency edges just to satisfy a checkpoint rule, and in one executed plan 36% of tasks were committed only by the final commit. | An orchestrator learns a plan's graph without loading its prose, and each implementing agent gets its own task's text without anyone pasting it. Frontiers, overlaps and closures come out the same on every run with no hand check. Planners no longer distort edges to make a checkpoint rule hold. |
| unified-singing-gem | 6fb8659 | An item promoted out of the backlog is forgotten. Nothing reads it again, no task points back at it, its promotion target is free text that often names nothing real, resolving it erases where it went, and later housekeeping reports it as settled. Of 47 promoted rows here, only 3 appear in any plan. | A promoted item stays live as a claim on a flow until the work claiming it is done, then closes with a record of what delivered it. Claims that lead nowhere surface and go back to triage. Promoting toward work not yet planned does not lose the item. |
| eventual-churning-sunrise | 411cde5 | Half of agents' calls to the repo's flow-store command-line tool pipe the output into another program: to pull one field, strip quotes, keep some fields, build strings, cut pretty JSON into invalid fragments, or loop per id, and a write takes a second call to stamp a date. The same commands fail outright on Windows, which has no stock jq and has tr and head only under Git Bash. | Any output an agent needs comes from the tool alone, the same way on Linux and Windows. Every skill and command shows those forms. The sibling terminal view that builds against the tool behaves as before. |

### 3b. Export the bases

**PowerShell.** Each base goes into a fresh directory outside the repo, with no `.git`, so the agents' working directory, git refs and `git status` hold no later commit. The challengers run from `$root`, so their reads and their notes stay inside the working directory. The `.tar` file stands in for a pipe, which PowerShell 5.1 corrupts on binary output, and the system `tar.exe` is named because Git's would read `C:` as a host. `.github` is left out because it holds only symlinks that cannot extract here.

```powershell
$goals = @(Get-Content -Encoding UTF8 "$repo\docs\ideas\plan-v2-probes.md" | Where-Object { $_ -match '^\| [a-z-]+ \| [0-9a-f]{7} \|' } | ForEach-Object {
  $cell = @($_.Trim().Trim('|').Split('|') | ForEach-Object { $_.Trim() })
  [pscustomobject]@{ slug = $cell[0]; base = $cell[1]; intent = $cell[2]; future_state = $cell[3] }
})
if ($goals.Count -ne 6) { throw "expected 6 goals, found $($goals.Count)" }
New-Item -ItemType Directory "$root", "$root\notes" | Out-Null
foreach ($g in $goals) {
  $code = "$root\code-$($g.base)"
  $tar = "$root\$($g.base).tar"
  New-Item -ItemType Directory $code | Out-Null
  git archive --format=tar -o $tar $g.base
  & "$env:SystemRoot\System32\tar.exe" -x -f $tar --exclude=.github -C $code
  Remove-Item $tar
}
[ordered]@{ root = ($root -replace '\\', '/'); goals = @($goals | Select-Object base, intent, future_state) } | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 "$root\goals.json"
$tReplay = Get-Date
```

`New-Item` refuses a `$root` that already exists, which is the point. Delete an old one by hand.

### 3c. Run the challengers

**Claude session, new, started in the replay directory.** Run `Set-Location $HOME\plan-v2-replay; claude` in a second PowerShell window, then paste:

> Read `goals.json` in this directory. Then call the Workflow tool once, with the fenced JavaScript under "The replay script" in `C:\Users\rossa\dev\dev-tools\docs\ideas\plan-v2-probes.md`, copied verbatim, as `script`, and the parsed contents of `goals.json`, unchanged, as `args`. Read nothing under any `code-*` directory. Report the `runId` and `transcriptDir` from the result and end your turn. When the notification arrives, show me `<result>` and `<failures>` in full and stop.

If your clone is elsewhere, correct that path.

#### The replay script

```js
export const meta = {
  name: 'plan-v2-replay',
  description: 'Replay planning goals blind: one plan-challenger per goal against its exported base, each writing one note',
  phases: [
    { title: 'Replay', detail: 'one plan-challenger per goal; each writes notes/<base>.md in the working directory' },
  ],
}

const RETURN_SCHEMA = {
  type: 'object',
  properties: {
    verdict: { type: 'string', pattern: '^[^\\r\\n]+$' },
    note: { type: 'string' },
  },
  required: ['verdict', 'note'],
  additionalProperties: false,
}

const oneLine = (value) => typeof value === 'string' && value.trim() !== '' && !/[\r\n]/.test(value)

const a = args || {}
const goals = Array.isArray(a.goals) ? a.goals : []
const complete = (g) => g && /^[0-9a-f]{7}$/.test(String(g.base)) && oneLine(g.intent) && oneLine(g.future_state)

if (!oneLine(a.root) || !goals.length || !goals.every(complete)) {
  log('no agent launched: args need root and goals with base, intent and future_state')
  return { notes: [], errors: ['bad args; no agent launched'] }
}

const prompt = (g) => `intent: ${g.intent}
future state: ${g.future_state}
code root: ${a.root}/code-${g.base}
note: notes/${g.base}.md

Draft the structure this work would want, as your procedure says, and write your note at that path. If a file is already at that path, write nothing, return \`note\` as an empty string and say so in \`verdict\`. Return only \`{verdict, note}\`.`

phase('Replay')
const returns = await parallel(goals.map((g) => () => agent(prompt(g), {
  agentType: 'plan-challenger',
  schema: RETURN_SCHEMA,
  label: `goal ${g.base}`,
  phase: 'Replay',
})))

const notes = goals.map((g, i) => {
  const ret = returns[i]
  if (!ret) return { base: g.base, status: 'no-return', verdict: null, note: null }
  if (!ret.note) return { base: g.base, status: 'no-note', verdict: ret.verdict, note: null }
  return { base: g.base, status: 'ok', verdict: ret.verdict, note: ret.note }
})
return { notes, errors: notes.filter((n) => n.status !== 'ok').map((n) => `goal ${n.base}: ${n.status}`) }
```

### 3d. Build the sheet

**PowerShell**, after the notification. A note that does not open `Verdict: ` counts as never written, so this refuses until all six do. To rerun one, delete its note, then ask the replay session to relaunch the same script with `goals` cut to that base.

```powershell
$bad = @($goals | Where-Object { $n = "$root\notes\$($_.base).md"; -not (Test-Path $n) -or (Get-Content -Encoding UTF8 -TotalCount 1 $n) -notmatch '^Verdict: ' })
if ($bad) { throw "no usable note for: $($bad.base -join ', ')" }
New-Item -ItemType Directory "$root\sheet" | Out-Null
$letters = 'A', 'B', 'C', 'D', 'E', 'F'
$shuffled = @($goals | Get-Random -Count 6)
$out = @(
  '# Rating sheet'
  ''
  'Question, the same for every entry: would this output have changed what was asked for?'
  ''
  'Answer yes or no for each letter, before opening key.txt. A "no restructure" verdict counts as yes only if reading it would have changed the ask.'
  ''
  'Rule, fixed before rating: keep the structure challenge at 2 of 6 or more. Later knowledge can leak and the rater knows the goals, so the count is an upper bound. Below 2 removes the step. 2 or more keeps it provisionally because it is cheap, not because it is shown to work.'
  ''
  'After rating, open key.txt and tally restructure goals and control goals separately.'
)
$key = @()
for ($i = 0; $i -lt 6; $i++) {
  $g = $shuffled[$i]
  $kind = if ([array]::IndexOf($goals, $g) -lt 3) { 'restructure' } else { 'control' }
  $key += "$($letters[$i])  $($g.slug)  $($g.base)  $kind"
  $text = (Get-Content -Raw -Encoding UTF8 "$root\notes\$($g.base).md") -replace $g.base, 'base'
  $out += @('', "## $($letters[$i])", '', "Rating: ", '', "Asked for: $($g.intent)", '', "Wanted afterwards: $($g.future_state)", '', $text)
}
$out | Set-Content -Encoding UTF8 "$root\sheet\sheet.md"
$key | Set-Content -Encoding UTF8 "$root\sheet\key.txt"
"$root\sheet\sheet.md"
```

### 3e. Rate

Read `sheet.md` and answer every letter before opening `key.txt`. The sheet carries no slugs and its order is shuffled, and the base hash is masked in each note. Then open `key.txt`, fill the ratings table under `## Results`, and tally restructure and control goals separately.

Write the count of yes answers as the verdict: `Verdict: keep 3 of 6` for 3, `Verdict: remove 1 of 6` for 1. Below 2 is `remove` and 2 or more is `keep`, and milestone B acts on it.

### 3f. Count the challengers' instruction rows

The replay is also the `omitClaudeMd` test. **PowerShell:**

```powershell
Get-AgentRows $rootProject $tReplay | Format-Table File, Instructions, NestedMemory
```

Expect six rows. If there are none, the project directory name was not what step 0 worked out: use `Get-AgentRows '<transcriptDir>' $tReplay` with the directory the launch reported.

`Instructions` counts `"attachment":{"type":"instructions"` rows, and zero in every row means it held. A marker that an agent read from a file arrives inside a tool result with its quotes escaped, so it does not match this literal. `NestedMemory` counts `"type":"nested_memory"` rows, where each base's own `CLAUDE.md` is expected to load: write what you see on the `Nested memory:` line. This is expected, not a surprise.

The reading waits for the positive control in step 4b.

## Step 4: resume

Can a re-run find what is missing from `notes/` alone, and what does a relaunch from the journal actually return? These run the real `plan-v2-investigate` Workflow on the throwaway slug `plan-v2-probe`, against the current tree. These three lenses are the args for every launch below.

### Lens args

```json
{"slug":"plan-v2-probe","lenses":[{"id":"probe-marker","question":"What word follows marker: in docs/flows/plan-v2-probe/t2-target.md? Read that file and quote its line."},{"id":"probe-stage","question":"Which stages can the plan stage verb of tomlctl report, and where under tomlctl/src does the code decide each one?"},{"id":"probe-writes","question":"Which plan verbs of tomlctl write more than one store in one call, and what guards each of them against a partial write?"}]}
```

`probe-marker` should finish in about a minute and the other two take longer, which gives T1 its window.

### 4a. T1: a killed run re-enters from `notes/`

**PowerShell**, once:

```powershell
Set-Location $repo
tomlctl plan open --slug plan-v2-probe
Set-Content -Encoding Ascii $target 'marker: alpha'
```

**Claude session, new, repo root:**

> Call the Workflow tool with `name: "plan-v2-investigate"` and, as `args`, the JSON under "Lens args" in `docs/ideas/plan-v2-probes.md`, verbatim. In the same turn run `tomlctl plan run start --slug plan-v2-probe --step investigate --workflow plan-v2-investigate --run-id <the runId> --session "$CLAUDE_CODE_SESSION_ID"`. Report the `runId` and `transcriptDir` and end your turn.

**PowerShell.** This waits for the first note and prints what has landed:

```powershell
while (@(Get-ChildItem $probeNotes -Filter *.md -ErrorAction SilentlyContinue).Count -lt 1) { Start-Sleep 5 }
Get-ChildItem $probeNotes -Filter *.md | Select-Object -ExpandProperty Name
```

As soon as it prints, end the Claude session: `/exit` and choose **Exit and stop tasks**, or kill the terminal outright. Never choose **Move to background**, which leaves the run alive and tests nothing.

**PowerShell:**

```powershell
Start-Sleep 60
Get-ChildItem $probeNotes -Filter *.md | Select-Object -ExpandProperty Name
$tT1 = Get-Date
```

- One or two notes: carry on. A note that appeared during that wait means an agent outlived its session. Write that under `Surprises:`.
- Three notes: the run finished first and this attempt tests nothing. Run `Remove-Item "$probeNotes\*.md"` and `tomlctl plan run abandon --slug plan-v2-probe --step investigate --reason 'T1 retry'`, then launch again.

**Claude session, new with plain `claude`, not `--resume`, repo root:**

> List `docs/flows/plan-v2-probe/notes/`. A lens from the JSON under "Lens args" in `docs/ideas/plan-v2-probes.md` is done only when `<id>.md` exists there and its first line opens `Verdict:`. Delete any note that exists and does not open that way, because the Workflow refuses to overwrite one. Run `tomlctl plan run abandon --slug plan-v2-probe --step investigate --reason 'T1: session killed'`. Then call the Workflow tool with `name: "plan-v2-investigate"` and those args cut to only the lenses not done, and run `tomlctl plan run start --slug plan-v2-probe --step investigate --workflow plan-v2-investigate --run-id <the runId> --session "$CLAUDE_CODE_SESSION_ID"`. Tell me which lenses you passed, and end your turn. When the notification arrives, run `tomlctl plan run finish --slug plan-v2-probe --step investigate --outcome ok`, or `--outcome errors` if `<failures>` is non-empty, show me `<result>` and stop.

**PowerShell:**

```powershell
Get-AgentRows $repoProject $tT1 | Where-Object Lens | Format-Table Lens, Instructions, NestedMemory
Get-ChildItem $probeNotes -Filter *.md | ForEach-Object { "$($_.Name): $(Get-Content -TotalCount 1 $_.FullName)" }
```

`T1: true` when the rows name exactly the lenses that had no usable note, once each, and all three notes now open `Verdict:`. Anything else is `T1: false`, which stops milestone B and goes to `/plan-update`, since re-entry rests on this.

### 4b. Read the `omitClaudeMd` result

The `Instructions` column above is the positive control: an investigator keeps CLAUDE.md, so it must hold a row.

| Challengers (step 3f) | Investigators (step 4a) | Write |
|---|---|---|
| All zero | At least one | `omitClaudeMd: held` |
| Any above zero | At least one | `omitClaudeMd: leaked` |
| Anything | All zero | `omitClaudeMd: inconclusive`, because the test cannot discriminate |

Milestone B stops on anything but `held`.

### 4c. Baseline run for T2 and T3

T2 and T3 relaunch from a run that finished all three lenses, so T1's partial notes are cleared first.

**PowerShell:**

```powershell
Remove-Item "$probeNotes\*.md"
```

**Claude session, new, repo root:**

> Call the Workflow tool with `name: "plan-v2-investigate"` and, as `args`, the JSON under "Lens args" in `docs/ideas/plan-v2-probes.md`, verbatim. Run `tomlctl plan run start --slug plan-v2-probe --step investigate --workflow plan-v2-investigate --run-id <the runId> --session "$CLAUDE_CODE_SESSION_ID"`, and tell me the `runId`, the `transcriptDir` and the value of `CLAUDE_CODE_SESSION_ID`. End your turn. When the notification arrives, run `tomlctl plan run finish --slug plan-v2-probe --step investigate --outcome ok`, or `--outcome errors` if `<failures>` is non-empty, show me `<result>` in full and stop.

All three lenses must come back `ok`. If not, clear the notes and repeat this step. Keep the `runId` and the session id, then end the session with `/exit`.

### 4d. T2: does a relaunch notice a changed file?

The prompt points at `t2-target.md` without holding its contents, so editing it leaves every prompt unchanged.

**Clear nothing for this test.** `plan-v2-investigate` refuses to overwrite a note, so leaving the notes in place is what makes the reading clean. A cached result comes back untouched, and a rerun comes back `no-note` saying a file is there.

**PowerShell:**

```powershell
Set-Content -Encoding Ascii $target 'marker: beta'
$tT2 = Get-Date
```

**Claude session, resumed.** Run `claude --resume <session id>` at the repo root, then:

> Call the Workflow tool with `name: "plan-v2-investigate"`, `resumeFromRunId: "<the baseline runId>"` and, as `args`, the JSON under "Lens args" in `docs/ideas/plan-v2-probes.md`, verbatim. Do not run any `plan run` verb. When the notification arrives, show me `<result>` and `<failures>` in full and end your turn.

**PowerShell:**

```powershell
Get-AgentRows $repoProject $tT2 | Where-Object Lens | Format-Table Lens
Get-Content "$probeNotes\probe-marker.md" -TotalCount 1
```

| `probe-marker` in the result | Agent rows | Write |
|---|---|---|
| `ok`, verdict says `alpha`, the same as the baseline | None | `T2: stale` |
| `no-note` saying a file exists, or a verdict saying `beta` | A row for `probe-marker` | `T2: fresh` |

`no-note` here means the agent reran. It is a reading of the test, not a failure, and belongs nowhere under `Surprises:`. A refused relaunch is `T2: not run`, with the message under `Surprises:`.

### 4e. T3: does a changed prompt miss?

These args change only the first lens's `question`, so the first prompt differs and the other two do not.

#### T3 args

```json
{"slug":"plan-v2-probe","lenses":[{"id":"probe-marker","question":"What word follows marker: in docs/flows/plan-v2-probe/t2-target.md? Read that file and quote its line, then say how many lines it has."},{"id":"probe-stage","question":"Which stages can the plan stage verb of tomlctl report, and where under tomlctl/src does the code decide each one?"},{"id":"probe-writes","question":"Which plan verbs of tomlctl write more than one store in one call, and what guards each of them against a partial write?"}]}
```

**Clear exactly one note first.** A rerun refuses a note that exists, so `probe-marker.md` has to go or a miss could not write an answer. Leave `probe-stage.md` and `probe-writes.md` in place: their refusals are how you see that the run went on missing past the changed prompt.

**PowerShell:**

```powershell
Remove-Item "$probeNotes\probe-marker.md"
$tT3 = Get-Date
```

**Claude session, the same resumed one:**

> Call the Workflow tool with `name: "plan-v2-investigate"`, `resumeFromRunId: "<the baseline runId>"` and, as `args`, the JSON under "T3 args" in `docs/ideas/plan-v2-probes.md`, verbatim. Do not run any `plan run` verb. When the notification arrives, show me `<result>` and `<failures>` in full and end your turn.

The relaunch uses the baseline run id rather than T2's, so both tests read the same journal. If it is refused, retry with the run id T2's launch returned, and say so under `Surprises:`.

**PowerShell:**

```powershell
Get-AgentRows $repoProject $tT3 | Where-Object Lens | Format-Table Lens
Test-Path "$probeNotes\probe-marker.md"
Get-Content "$probeNotes\probe-marker.md" -TotalCount 1
```

| `probe-marker` | Write |
|---|---|
| An agent row for it, a new note on disk, verdict says `beta` | `T3: miss` |
| No agent row, verdict says `alpha`, and no note on disk | `T3: hit` |

The expected shape of a miss has `probe-stage` and `probe-writes` returning `no-note`, each with an agent row, and `errors` non-empty. That shows they reran and refused, and it is neither a failure nor a surprise. If those two return `ok` with no agent rows, the journal is matching per prompt and not by prefix: still write the `T3:` line from `probe-marker`, and add that under `Surprises:`.

### How to read the three together

- T1 stands alone. It ran before the baseline, on its own notes, so T2 and T3 cannot change its reading.
- In T2 and T3 a `no-note` is evidence that an agent reran, because of what was deliberately left on disk. It never means the lens could not be answered, and those two tests say nothing about whether any note's answer is right.
- A real relaunch that misses would meet the same refusals, so an engine that relaunches has to clear the notes of everything from the changed prompt onward. Add that to `Surprises:` only if the `T3:` reading makes it matter in a way the table above did not predict.
- After T3 the notes are a mix of runs and prove nothing. The teardown deletes them.

## What the lines decide

| Line | Consequence |
|---|---|
| `Verdict:` | `remove` deletes `plan-challenger` and its step in milestone B. `keep` is provisional, per the rule on the sheet. Milestone B stops if this line matches neither form. |
| `Lookup:` | `copy` means milestone B's handover says to copy rather than link. |
| `omitClaudeMd:` | Anything but `held` stops milestone B. |
| `T1:` | `false` stops milestone B and goes to `/plan-update`, because re-entry rests on it. |
| `T2:`, `T3:` | The open-run rule permits a relaunch only when `T3:` reads `miss` and, if `T2:` reads `stale`, only when nothing a prompt points at has changed since launch. Any other result makes the rule always `plan run abandon`. `T3:` other than `miss` also stops the prove Workflow, since prompts built from `args` alone would no longer guard the journal. |
| `Surprises:` | Anything but `none` stops milestone B until you settle it, so the expected facts have their own lines. Keep this line for what nothing here predicted. |

## If Workflows do not run

If a launch is refused or returns an error, and it is not just a run still live in this session, Workflows do not run here. The rating still works:

1. **PowerShell:** run step 3b as written.
2. **Claude session, new, started in the replay directory:**

   > Read `goals.json` in this directory. For each goal, dispatch `Agent({subagent_type:"plan-challenger", prompt})` with no `name:`, all in one message, where `prompt` is the template string `prompt` in the fenced JavaScript under "The replay script" in `C:\Users\rossa\dev\dev-tools\docs\ideas\plan-v2-probes.md`, filled in from that goal and `root`. Read nothing under any `code-*` directory. Show me each return and stop.

3. Do steps 3d and 3e as written.
4. Write `not run` on `Lookup:`, `omitClaudeMd:`, `T1:`, `T2:` and `T3:`, and put the refusal under `Surprises:`. Skip step 4.

Milestone B then stops and the plan goes to `/plan-update`, because the engine has no Agent fallback. Milestone A stays committed, since its agents and scripts do nothing unlinked. Removing the links under the Claude config dir is the rollback.

## Teardown

Run this after the results are filled in, because the notes and transcripts are the evidence for them.

**PowerShell.** The first command prints the steps still open. Abandon each one it lists. An empty list means skip the second command, which is refused with nothing open.

```powershell
Set-Location $repo
tomlctl plan view --slug plan-v2-probe --for digest --select open_items.open_runs
tomlctl plan run abandon --slug plan-v2-probe --step investigate --reason 'probes finished'
```

Then delete the flow and the replay directories. T2's edited file lives in `docs/flows/plan-v2-probe/`, so deleting it restores the tree and no tracked file needs restoring. `$root` holds every `code-*` directory, the notes and the sheet.

```powershell
Remove-Item -Recurse -Force "$repo\docs\flows\plan-v2-probe", "$repo\.claude\flows\plan-v2-probe"
if (-not (Get-ChildItem -Force "$repo\docs\flows")) { Remove-Item "$repo\docs\flows" }
Remove-Item -Recurse -Force $root
git status --short
```

`git status --short` should show only `docs/ideas/plan-v2-probes.md`. Anything else that was not listed at step 0 came from these probes, so find out why before committing.

On `Verdict: remove` only, delete the link:

```powershell
Remove-Item "$cfg\agents\plan-challenger.md"
```

Commit this file before starting milestone B, which reads it. If the first `/implement` left the gate task `failed`, its Approach, "Sequence" names the `tomlctl tasks update` that clears it.

## Results

Replace each `<...>`. Write `not run` on any line whose probe could not run. The `Surprises:` line is `none` or the text.

Verdict: <keep|remove> <n> of 6
Intents: <reviewed>
Lookup: <symlink|copy>
omitClaudeMd: <held|leaked|inconclusive>
T1: <true|false>
T2: <stale|fresh>
T3: <miss|hit>
Surprises: <none, or the text>
Nested memory: <what step 3f showed>
Auto-memory: <the replay ran from a directory with none, and real runs keep theirs; add anything the agents' transcripts show>
Tally: <r> of 3 restructure goals, <c> of 3 control goals

| Sheet letter | Goal | Kind | Would it have changed what was asked for? |
|---|---|---|---|
| A | <slug> | <restructure or control> | <yes or no> |
| B | <slug> | <restructure or control> | <yes or no> |
| C | <slug> | <restructure or control> | <yes or no> |
| D | <slug> | <restructure or control> | <yes or no> |
| E | <slug> | <restructure or control> | <yes or no> |
| F | <slug> | <restructure or control> | <yes or no> |
