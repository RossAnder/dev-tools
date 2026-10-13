# Agent waves — the stage 4 fallback

Stage 4 runs on the saved Workflow `review-v2-pipeline` (`claude/workflows/review-v2.js`, deployed under `~/.claude/workflows/`). This reference is its fallback: the same cells, the same prompts, the same routing and the same persistence contract, driven by the orchestrator with the Agent tool. Because every agent persists its own output to the store and every store write is idempotent, a wave run here leaves the store exactly as the Workflow would.

It also owns the one dispatch the Workflow never makes, the stage 2 surveyor (below), because that dispatch goes through the Agent tool on both substrates.

## When the fallback fires

Use it for the wave when any of these holds:

- the Workflow tool is disabled or absent from this session;
- `Workflow({name: "review-v2-pipeline", …})` reports the name unknown;
- the launch reports `error`;
- the run notification arrives with no result, or with an index whose `errors` show that no cell launched (`args missing …`).

A Workflow that completed with some cells `no-return` or `error` is not a fallback case: the store already holds whatever those cells wrote, so go to stage 5 and let the digest's unverified ids and unsealed cells drive a mop-up wave, which this reference runs.

Once the fallback has fired, run the rest of this run's waves on it rather than retrying the Workflow per wave. `review run start` records the substrate the run starts on, and `review run update` does not take `substrate`, so when the run started as `workflow` and fell back, say so in the close summary (`substrate: workflow → agent-waves at wave <n>`).

## Dispatch rules

- **One-shot, never named.** Every dispatch is `Agent({subagent_type: "<agent>", description: "<short label>", prompt: "<the template below>", run_in_background: true})` with no `name:` and no `model:`. A named spawn becomes a teammate with no return channel and its index object is lost; the model and effort live in each agent's frontmatter.
- **At most 16 per message.** Put a wave's dispatches in one message, up to 16 calls.
- **At most 16 running at once.** Before each message, count what is still running: agents of this wave, agents of an earlier wave that have not yet notified, the surveyor, and any `verification` agent. Dispatch only up to 16 minus that count. The session refuses spawns past 20 concurrent subagents and does not retry them, so the cap keeps headroom for the harness.
- **Background, then end the turn.** After the message, end your turn. Each agent reports by a completion notification; resume on each one, record its return, and dispatch the next work its return unlocks while the running count allows.
- **The prompt is the assignment.** Never paste constraints, lens text, rows or prior findings into a prompt. Agents fetch their own assignment from the store, and the pack carries the rest.

### The wave barrier

A cell wave is complete when either every dispatched agent has notified, or `review digest` shows every cell of the wave sealed. A verification wave is complete when every dispatched verifier and checker has notified, or the digest lists no unverified id from this wave's cells. Check the digest rather than waiting when an agent is overdue:

```bash
tomlctl review digest --run <run> --slug <slug> --since-wave <wave>
```

Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store, here and in every command below. A cell that notified with no return, or never notified, but shows sealed in the digest is done: its rows are in the store. A cell that is still `assigned` after its agent notified gets one re-dispatch with the identical prompt; `review add` supersedes any unverified rows the first attempt wrote, and `review cover` is a no-op on a cell sealed with the same status and note. A second failure leaves the cell `assigned`, and stage 5 decides whether a follow-up wave covers it.

## The prompts

Every prompt opens with the preamble. When a flow is bound, its first line is the `ledger:` line at column 0, with no indentation and nothing before it: agent correlation reads that line to attribute the agent to the run's store. A flow-less run omits it.

```text
ledger: .claude/flows/<slug>/review-v2.toml
run: <run id>
wave: <wave>
store: --slug <slug>
pack: <run directory>/pack.md
run directory: <run directory>
```

The `store:` line carries the flag exactly as the agent passes it: `--slug <slug>`, or `--scope <scope>` for a flow-less store. `pack:` is the path `review run start` printed joined with `pack.md`; `run directory:` is that path. A `review-finder-cf` cell is context-free, so its prompt leaves the `pack:` line out, on every dispatch of that cell, the re-dispatch of an `assigned` cell included; every other prompt keeps it.

### Cell prompt

The same template the Workflow builds, word for word, after the preamble. `<agent>` is the cell's `agent` from the composed map: `review-finder`, `review-finder-cf` (the same Opus finder, run without CLAUDE.md, for the context-free correctness cell), `review-sweeper` or `plan-executor`; any other value is a composition error, so skip the cell and note it.

```text
cell: <cell id>

Fetch your assignment with `tomlctl review show <cell id> --for finder <store flag>`, then run the cell as your procedure says: write its full finding set with one `tomlctl review add --run <run id> --cell <cell id> <store flag>` call, seal it with `tomlctl review cover <cell id>`, and return the cell-return object `review add` printed.
```

### Verifier prompt

One finding, or a minor batch from one cell, or a panel seat. The finding line and the instruction are the Workflow's; the `batch:` line is added for a batch, so the verifier can name its payload `payloads/<batch>.ndjson`.

```text
findings: <id>[, <id>…]
batch: verify-<cell id>-minor
panel seat: <seat> of 3

Fetch each finding with `tomlctl review show <ids, comma-separated> --for verifier <store flag>`, verify it refute-first, and record each verdict with `tomlctl review verdict` using `--by verifier`. Return the verdict-return object.
```

- Single finding: omit `batch:` and `panel seat:`; the instruction ends `Return the verdict-return object.`
- Minor batch: keep `batch:`, omit `panel seat:`; the instruction ends `Return one verdict-return object per finding under \`verdicts\`.`
- Panel seat: omit `batch:`; keep `panel seat:` and replace `--by verifier` with `--by panel --seat <seat>`.

### Checker prompt

A pooled batch of up to 20 cleanup rows. The checker's projection does not carry `cell` or `lens`, and the checker may return `refuted` only on a row a sweeper cell wrote, so the prompt names those rows. The `batch:` and `sweeper rows:` lines are additions to the Workflow's template; write `sweeper rows: none` when no row in the batch came from a `review-sweeper` cell.

```text
findings: <id>, <id>, …
batch: check-<wave>-<n>
sweeper rows: <ids from review-sweeper cells, or none>

Fetch the rows with `tomlctl review show <ids, comma-separated> --for checker <store flag>`, check each by your predicates, and record each verdict with `tomlctl review verdict --by checker`. Return one verdict-return object per finding under `verdicts`.
```

The orchestrator knows which rows are sweeper rows from the cell-returns: each return names its cell, and the composed map names each cell's agent. In a mop-up, `review list --for orchestrator` carries each row's `cell` (see Mop-up).

### Surveyor prompt

Stage 2 dispatches one `review-surveyor` after `review survey` has run. `review survey` prints the signals it recorded, and no read-only verb returns a run's signals afterwards, so the prompt must carry them; without them the surveyor stops and returns `missing: signals`.

```text
family: <code|perf|plan>
scope: <the run's scope globs, comma-separated>
plan: <plan path — plan family only>
signals:
- <name> — roots: <root>, <root>
- <name> — roots: <root>

Run the survey for this run as your procedure says and return your digest.
```

It follows the preamble, so it also carries the run, the store flag and the run directory. Copy each signal's `name` and `roots` from `review survey`'s output as printed. The surveyor counts toward the 16 running agents.

## Routing

Route each cell-return's `findings` by its `route`, which `review add` computed; never re-derive a route from the kind or the file.

| `route` | Dispatch |
|---|---|
| `verifier`, a `defect` of severity `minor`, depth `quick` or `standard` | one `review-verifier` minor batch per cell, holding all of that cell's minor defects |
| `verifier`, anything else | one `review-verifier` per finding |
| `panel` | three `review-verifier` dispatches, seats 1, 2 and 3 (only computed at `deep`) |
| `checker` | into the checker pool |
| `none` | nothing: advisory rows are judged at triage |
| anything else | nothing; log it as an error and leave the id to the mop-up |

**The checker pool.** Rows from every cell of the wave pool together. Launch a `review-checker` batch whenever the pool holds 20 rows, numbering batches `check-<wave>-1`, `check-<wave>-2`, …; flush the remainder as a final batch once every cell of the wave has returned or is sealed.

**Unsettled rows.** A checker return marks each row it could not settle with `verdict = "unsettled"` and `reroute`. Dispatch one `review-verifier` per such row with the single-finding verifier prompt. `review verdict` has already re-routed it to `verifier` in the store.

**Starting early.** Verification may start for a cell as soon as that cell returns, while other cells still run, provided the running count allows; that is what the Workflow's pipeline does. When the count is tight, finish dispatching the cell wave first.

**Returns.** A cell agent returns the cell-return object: `{cell, findings: [{id, kind, severity, route}], status, escalate?}`. A verifier returns one verdict-return object `{finding, verdict, reroute?}` for a single finding, and a batch — a checker batch included — returns them under `verdicts`. Collect every `escalate` entry from the cell-returns for stage 5, which decides whether an escalated sweeper candidate gets a `review-finder` cell.

### The wave index

Keep the same index the Workflow returns, so stage 5 reads one shape whichever substrate ran:

- `run`, `wave`;
- `cells`: one entry per dispatched cell, `{id, status, findings, escalated}`, where `status` is the sealed status from the return, or `skipped`, `no-return` or `error`, `findings` the count of returned ids and `escalated` the count of `escalate` entries;
- `verified`: the number of distinct finding ids that received a verdict other than `unsettled` in this wave;
- `errors`: one line per skipped cell, lost return, missing verdict or unknown route.

After the wave's last notification, record the running agent total for the run (every dispatch so far, surveyor and `verification` agents included):

```bash
tomlctl review run update <run> --slug <slug> --set-json agents=<total>
```

## Mop-up

Stage 5 runs `review resolve` and then `review digest --since-wave <wave>`. Any **unverified id** the digest lists — a lost return, a restart, a checker row whose batch never came back, an unknown route — gets a mop-up dispatch from the store, not from the returns:

1. Take the ids and their routes from the digest. For the `checker` rows, read their cells, which decide the sweeper marking:

   ```bash
   tomlctl review list --run <run> --slug <slug> --status unverified --route checker --for orchestrator
   ```

2. Dispatch by `route` as above, with two differences: every `verifier` row gets its own single-finding verifier (a mop-up set is small, so it skips minor batching), and every `panel` row gets all three seats again — a verdict is an upsert on (finding, `by`, `seat`), so a seat that already voted overwrites its own vote rather than adding a fourth.
3. Pool `checker` rows into batches of up to 20 as above, marking as sweeper rows those whose `cell` the composed map assigned to `review-sweeper`.
4. Number the mop-up batches after the wave's own (`check-<wave>-<n+1>`), and end the turn as for any wave.

An id the mop-up also fails to verify stays `unverified`, and `review run finish` refuses until it is verified or the run finishes with `--incomplete-file` naming it.
