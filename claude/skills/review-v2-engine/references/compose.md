# Compose — stage 3, and the stage 5 decisions

Composition is the run's core judgement step and the one stage nothing below you can do. `tomlctl` proposes a deterministic coverage map from the signals and the catalogue; you decide what the change actually needs, edit the map with a reason for every change, and account for everything you leave out. Every matched concern and every scope file ends up either in a cell or under a waiver, and `review run finish` refuses until that holds.

Your inputs are the surveyor's digest, `profile.md`, the proposal and the digest. You never read lens, facet or method bodies: a concern's id, its owned tags and the proposal's reasons are enough to compose with, and the agents read the bodies from the paths their cell binds.

Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store in every command below.

## 1. Judged candidates

The surveyor proposed judged signals (`trust-boundary`, `hot-path`, `new-seam`, any `project:` judged signal), each with evidence paths. Judged signals activate lenses (`trust-boundary` activates `security`; `new-seam` activates `contrarian`), so a careless accept spends cells and a careless reject drops coverage. Accept a candidate when its evidence paths show the thing its name says, inside the scope:

- `trust-boundary` — the change parses, deserialises, authorises or executes input that crosses a process, network, file or user boundary;
- `hot-path` — the code runs per request, per row, per file or inside a loop whose size the workload sets;
- `new-seam` — the change introduces an interface, trait, extension point or module boundary other code will build on.

Reject a candidate whose evidence shows only a name match, test code, or code outside the scope. Record every decision; an unrecorded candidate looks undecided in the run record:

```bash
tomlctl review run update <run> --slug <slug> --set-json 'signals=[{"name":"trust-boundary","accepted":true},{"name":"hot-path","accepted":false}]'
```

Only `accepted` may change on an existing judged signal; a retry of the same call is a no-op.

## 2. The proposal

```bash
tomlctl review plan --run <run> --slug <slug> --propose --accept-signals trust-boundary --lens +contrarian --lens -doc-drift --out <run-dir>/proposal.ndjson
```

- `--accept-signals` names the candidates you accepted, comma-separated; omit it when you accepted none.
- `--lens` repeats once per user `--lens` flag, passed through unchanged; omit it when the user gave none.
- `--out` keeps the proposal in the run directory, where you edit it.

The proposal is deterministic: floor lenses first, then signal-tier lenses whose signals matched, then lenses whose `always_at` is the run's depth, then candidate lenses ordered by how many of their signals matched, until the cell ceiling (about one cell per 100 changed lines, floor 3, capped by the depth). Each lens is sliced by its own `slice` rule, same-mode concerns with identical slices are bundled, repeat cells (`pass = 2`) are added at `standard` and `deep`, and every row carries `reason = "proposed"`. It also carries an estimated agent count, and lists the matched concerns that missed the ceiling as unselected. In the plan family, `plan-executor` cells hold up to 4 tasks each and sit outside the ceiling.

Read it with the Read tool. The rows are the cells you will write; each holds `slice`, `concerns`, `agent`, `reason`, and where present `pass`, `question` and `facets`.

## 3. Editing the map

Edit `<run-dir>/proposal.ndjson` into the wave's cell file, `<run-dir>/payloads/cells-w1.ndjson`. Leave a proposed row unchanged unless you can say why; every row you change, add or drop gets a reason. A changed or added row's `reason` replaces `proposed` with that reason in a sentence a later reader of `review stats` can act on. A dropped concern is accounted for by a waiver (section 6).

### Re-slicing

The proposal slices by rule; you slice by risk.

- **Hotspots get their own cell.** A churn hotspot from the surveyor's `hotspots:` that the proposal grouped with quiet files moves to a cell of its own, with the same concerns.
- **Split a slice a single agent cannot hold.** A `per-subsystem` slice spanning many unrelated files, or a whole-scope slice over a large diff, splits along the subsystems `profile.md` names.
- **Merge slivers.** Two one-file cells with the same concerns over closely coupled files become one; the saved cell can go to a probe or a candidate lens.
- **Keep slices repo-relative.** A slice entry is a path, a glob, or for `plan-executor` cells a task ref; never a directory outside the scope.

### Bundling

A cell bundles up to three concerns of one mode (two at `deep`); `review plan` refuses a mixed-mode bundle and a `probe:` or `check:` concern bundled with anything.

- Bundle concerns that read the same code the same way (`correctness` with `type-design`; `idioms` with `duplication`), so one agent's reading serves both.
- Separate concerns that compete for attention on a risky slice: on a hotspot, `correctness` alone beats `correctness` plus two others.
- Never bundle across modes: `ripple` and `hygiene` are mechanical and go to `review-sweeper`; judgement lenses go to `review-finder`; `plan-executor` is its own mode.

### Probes

A probe is a question the change raises that no catalogue lens owns. Add a `probe:<slug>` cell wherever the profile, the hotspots or the diff show such a risk: a retry loop with no backoff cap, a migration with no rollback, a cache with no invalidation story. Do not write a probe for a risk a lens already owns; re-slice that lens instead.

- `concerns` is exactly `["probe:<slug>"]`, the slug lowercase letters, digits and `-`.
- `question` is one paragraph: what could go wrong, where, and what would show it. The finder runs the method skill's probe template on it.
- `agent` is `review-finder`.
- `reason` opens with the nearest concern tag, `nearest <tag>:`, then why no lens covers it. Probe promotion is judged from recurring tags across runs, so pick the closest tag from the vocabulary's concern tags.

**User probes.** Each `--probe "<question>"` the user gave becomes one probe cell: write the question as given, slice it to where it applies (the whole scope when it names nowhere), and give it the reason `user probe`. A user probe is never dropped or merged into a lens cell.

### Facet edits

`review plan` attaches facets where a cell's slice meets the facet's signal roots and the cell's lenses are in the facet's `attaches_to`. Edit with `facets` on the row: `"+<id>"` attaches one the roots missed (a TypeScript file vendored under a Rust root), `"-<id>"` removes one that does not apply (the `tokio` facet on a slice that only names the runtime in a comment). Give the reason in the row's `reason`.

### Plan family

- Keep the proposed `plan-executor` cells; edit only their task selection, within the depth envelope's plan-executor row (L and M tasks first, checkpoint-marker tasks first).
- Keep the cell-run checks (`two-control-probe`, `falsifiability`, `acceptance-contradiction`) exactly as the proposal composes them into the `plan-checks` cell on `review-finder`. Do not hand-compose them.

### The ceiling

The depth's cell ceiling counts every wave, repeat cells included; plan-executor cells sit outside it. Edits that add cells must stay within it: drop a candidate-tier cell, merge slivers, or cut a repeat pass first. Never drop a floor lens to make room; floor lenses run on every run of their family.

### Family deltas

The carrier adds its own cells here; `/review-v2`'s context-free correctness cell is one. Write them into the same cell file with the reason the carrier gives.

## 4. The agent-count statement

Recompute the estimate for the edited map with the proposal's formula: cells, plus 1 surveyor, plus one verifier batch per cell, plus one checker batch per 20 expected cleanup rows (counted as 5 per cell), plus the `verification` dispatches already made. State it in your message before launching: `expected agents: <n> (<cells> cells)`. When it exceeds 25, add one sentence saying why the change needs that breadth; when you cannot justify it, cut cells until you can.

## 5. Confirm at deep

At `deep` only, show the user the composed map before writing it: one line per cell (id-to-be, slice, concerns, agent), the expected agent count and its justification, and the waivers you intend. Ask with `AskUserQuestion` whether to launch as composed or to adjust. Apply any adjustment and show the map again.

An empty answer (a headless session) launches the map as composed; the user chose `deep` for this run, and the close summary records that the map was not confirmed. At `quick` and `standard`, state the map in one line per cell and launch without asking.

## 6. Writing the map and the waivers

Write the cells, then read what landed:

```bash
tomlctl review plan --run <run> --slug <slug> --ndjson <run-dir>/payloads/cells-w1.ndjson
```

It validates every row, binds each concern's content hash, path, methods and `v1_category`, attaches facets, applies your facet edits, and appends the cells as `C<n>` with status `assigned`. A refused row is reported with its reason: fix it and resubmit only the refused rows, because a call appends. Keep each minted cell id with its `agent`; that list is the wave's `cells` for stage 4.

Then read the digest's accounting:

```bash
tomlctl review digest --run <run> --slug <slug>
```

It lists the **unselected** matched concerns and the **unsliced** scope files without a waiver. A concern a project disabled shows as `disabled` and needs no waiver. Write one waiver per listed item, as a JSON array with the Write tool to `<run-dir>/payloads/waivers.json`:

```json
[
  {"concern": "doc-consolidation", "reason": "doc-growth matched on CHANGELOG.md only; no prose in scope duplicates another doc"},
  {"file": "tomlctl/README.md", "reason": "prose only; doc-drift covers the code it describes"}
]
```

```bash
tomlctl review run update <run> --slug <slug> --set-json "waivers=$(cat <run-dir>/payloads/waivers.json)"
```

Each waiver names exactly one `concern` or one `file`, and a repeated key replaces its reason. A reason says why the concern cannot matter here, or what else covers it. A waiver given only for budget says so plainly, `ceiling: 16 cells at standard; correctness and security took priority`, so `review stats` can tell a judgement from a squeeze. Re-read the digest until it lists nothing unwaived.

## 7. Follow-up and mop-up — the stage 5 decisions

After each wave, `review resolve` and `review digest --since-wave <wave>` give you the wave's outcome. Decide in this order.

### Mop-up first

An **unverified id** in the digest is a finding whose verification did not land: a lost return, a restarted agent, a checker batch that never came back, an unknown route. A mop-up is not a new wave of cells; it dispatches verifiers and checkers from the store, per `agent-waves.md` (Mop-up). Run it whenever the digest lists an unverified id, at every depth, and before deciding a follow-up. An id that fails a second mop-up stays unverified and is named in `run finish --incomplete-file`.

A cell still `assigned` after its agent notified gets one re-dispatch with the identical prompt (`agent-waves.md`, The wave barrier). A cell that fails twice is a follow-up candidate, not a mop-up.

### Follow-up

A follow-up is a new wave of cells, written through `review plan` as the next wave and launched through stage 4. It is allowed only within the depth envelope: `quick` 0, `standard` 1, `deep` 2, and the cell ceiling counts its cells. Launch one only for these reasons, each with a reason on the new cell:

| Trigger | New cell |
|---|---|
| a cell sealed `uncovered` | the same concerns over a narrower slice, so one agent can cover it; the note says what it could not reach |
| a cell sealed `thin` | the same concerns over the files the note names, with a `question` naming what was thin |
| a cell that failed twice | the same row, re-sliced smaller |
| an escalated sweeper candidate | a `probe:<slug>` cell on `review-finder` whose `question` carries the candidate and whose slice is its file |
| a gap the findings revealed | a probe or a lens cell on the place the gap points: a defect pattern that likely recurs in sibling files, or a finding in a file no cell sliced |
| an `after_wave` lens the run enabled, such as an opted-in `synthesis` | the lens over the whole scope, after the wave it names |

**Escalations at `quick`.** `quick` allows no follow-up wave, but a sweeper's escalated candidate needs judgement it could not give. Collect every escalation of the run into one `review-finder` probe cell whose `question` lists them, write it as the next wave, and launch it: one Opus batch, not a wave of cells. It counts against the ceiling.

Do not launch a follow-up for a cell sealed `covered` with no findings; a clean result is a result. Do not re-run a lens on a slice it already covered to "double-check" a finding; verification owns that. When the allowance or the ceiling is spent and a trigger remains, the cell stays as sealed, and its status shows in the report's coverage map.

### Then stage 6

When no unverified id remains and no follow-up is warranted or allowed, go to the audit in `triage-and-close.md`.
