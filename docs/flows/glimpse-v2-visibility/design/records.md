# Records

The shapes tomlctl gains so glimpse can show a v2 flow. tomlctl owns every parse; glimpse reads JSON.

## Snapshot keys

`tasks snapshot` keeps every key it emits today, in today's order, and adds the keys below. A v1 store emits them empty or absent, so a v1 flow's document changes only by the new empty keys.

Each task object gains, after `commit`:

| Key | Value |
|---|---|
| `spec` | The row's spec, absent when the store holds none for its `ref` |
| `file_notes` | Object, path to note, for this row's `ref`; `{}` when none |
| `closes`, `refs` | The backlog ids of this row's backlog link; `[]` when none |
| `finding` | `{store, fixes, settles}` from this row's finding link, absent when none |

`spec` holds `outcome`, `constraints`, `means` (`text`, `binding`, `detail`), `executor`, `design`, `precedent`, `accept` and `premises`.

- An `accept` entry holds `behaviour`, `check`, `kind`, `positive`, `writes_build`, `run_by`, `covered_by`, `owner` (the value `Accept::owner` returns), `probeable` and, once recorded, `probe` (`at`, `neg_rc`, `pos_rc`, `verdict`).
- A `premises` entry holds `claim`, `check`, `at`, `probeable` and, once recorded, `probe` (`at`, `neg_rc`, `verdict`).
- `probeable` is `false` exactly when the probe emitter would skip the entry: `probes::skip_reason` for an accept entry, `probes::premise_skip_reason` for a premise. glimpse never restates those rules.

The document gains three top-level keys, after `agents`:

| Key | Value |
|---|---|
| `milestones` | One object per `[[milestones]]` row: `id`, `gates`, `elaboration`, `defer_reason`, `gate_probes` (`gate`, `probe`, `verdict`, `at`) |
| `changes` | The `[[changes]]` trail in stored order: `at`, `ref`, `field`, `old`, `new`, `actor`, `reason` |
| `plan` | The plan store as JSON, `null` when the flow has no `plan.toml` |

`plan` is the store's own shape: `convert::toml_to_json` over `plan::schema::to_toml`, so its keys are the keys `plan-store.md` documents (`binding`, `decision`, `contract`, `approval`, `run`, `handover`, and the three arrays of the next section). A plan store that fails to parse fails the snapshot, as it does today.

## Run records

Three arrays in `plan.toml`, written only by the `plan run` leaves named here. Workers never write `plan.toml`, so a worker's `tasks update` cannot drop them.

```toml
[[attempt]]            # plan run notes; upserted on (task, run_id)
task = 7
run_id = "wf_4e7f630b-d4a"
milestone = "B"
status = "done"        # the note's Status word
summary = "the one line after the dash"
block = "present"      # present | absent | malformed
files_touched = ["glimpse/src/nav.rs"]

[[attempt.deviations]]
what = "Kept the selector overlay rather than adding a links overlay"
why = "The selector already lists and dispatches rows"
means = "Add a links overlay"   # the suggested means departed from, "" when none

[[attempt.tangential]]
kind = "debt"
area = "glimpse/src/app.rs"
summary = "…"
why = "…"

[[attempt.checks]]
command = "cargo clippy --manifest-path glimpse/Cargo.toml"
result = "pass"        # pass | fail | not-run

[attempt.handover]     # omitted when the worker proposes none
reason = "restart"
action = "…"
when = "…"
done_when = "…"

[[gate_round]]         # plan run gate; keyed on (milestone, round, seq)
milestone = "B"
round = 0              # 0 is the first gate run, n the run after fix round n
seq = 1                # 1 for the first gate run of this round, one more for each later run of it
at = 2026-11-12T10:00:00Z
outcome = "fail"       # pass when every command is pass or flaky; else fail; timeout when a timeout is the only non-pass

[[gate_round.commands]]
command = "cargo test --manifest-path glimpse/Cargo.toml"
outcome = "fail"       # pass | flaky | fail | timeout
exit = 101             # omitted when the agent reported none
duration_s = 84
summary = "412 passed; 2 failed"
failed_ids = ["view::plan::tests::sections"]
log = ".claude/flows/<slug>/gates/B-r0-s1-c2.log"   # "" when no log was kept

[[report]]             # plan run report; appended, one per invocation
at = 2026-11-12T10:30:00Z
milestone = "B"
why = "gate failed after round 2"

[[report.entries]]
kind = "gate"          # parked | failed | handover | no-worker | withheld | gate | audit | out-of-lane | store-change | reconcile
task = 0               # 0 when the entry names no task
text = "milestone B failed after round 2: cargo test … — view::plan::tests::sections"
path = ""              # a repo-relative path the entry points at, "" when none
needs_person = true    # false for a line that only informs; absent reads as true
```

A gate call without `--rerun` records a new run of its round: `seq` is one more than the highest recorded for that (milestone, round). With `--rerun` it merges into the latest run of the round, replacing the commands it names. So a gate run again at a later invocation never overwrites an earlier run. A kept log is named `<milestone>-r<round>-s<seq>-c<n>.log`.

A binary that predates these arrays must refuse a store that holds any of them, never rewrite it without them: the store's schema stamp rises when, and only when, one of the three arrays is non-empty.

`log` names a file the gate verb wrote: the last 200 lines of the verification agent's log for that command, cut to 64 KiB, kept only for a `fail`, `timeout` or `flaky` command. The verb reads a source log only when its canonical path lies under the OS temp directory, where the verification agent's `mktemp -d` puts it; a log path outside it, or one that cannot be read, stores `log = ""` and is reported in the verb's output; it never fails the record.

## Run-note block

A worker's run note keeps `Status: <done|failed|parked> — <one line>` as its first line. A blank line follows, then one fenced block whose info string is `toml run-note`, then the prose as today.

````
Status: done — the links overlay lists a task's related records

```toml run-note
files_touched = ["glimpse/src/view/links.rs"]

[[deviations]]
what = "…"
why = "…"
means = "…"

[[tangential]]
kind = "debt"
area = "glimpse/src/app.rs"
summary = "…"
why = "…"

[[checks]]
command = "cargo clippy --manifest-path glimpse/Cargo.toml"
result = "pass"
```

## What changed
…
````

- Every key is optional; an empty block is valid and means no deviation, no tangential discovery and no handover.
- `[handover]`, when present, holds `reason`, `action`, `when` and `done_when`.
- `kind` in `[[tangential]]` takes the values a `TANGENTIAL:` line takes today.
- A note with no block reads as `block = "absent"`: the reader still takes its Status line and any `TANGENTIAL: <kind> | <area> | <summary> | <why>` lines in the prose, so notes written before this contract, and the orchestrator's own parked notes, still record.
- A block that is not valid TOML, holds an unknown key, or is separated from the Status line by anything but blank lines, reads as `block = "malformed"` with the Status line and nothing else, and the reader says which note and why.

## Sources block

A research note (investigator, challenger or cold reader) keeps `Verdict: <one line>` as its first line. A fenced block whose info string is `toml sources` follows it, after a blank line.

````
Verdict: …

```toml sources
read_at = "207a03a1c0ffee…"     # git rev-parse HEAD when the agent read the code
dirty = ["docs/ideas/v2-pipeline.md"]   # cited paths git status listed as modified or untracked

[[source]]
path = "glimpse/src/app.rs"
line = 902
claim = "SwitchSurface leaves the destination cursor where it was"

[[source]]
url = "https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html"
claim = "tables need ENABLE_TABLES"
```
````

- `path` is repo-relative with `/` separators; `line` is optional. A source has `path` or `url`, never both.
- A path in `dirty` was not at `read_at` when the agent read it, so its text at that commit is not what the agent saw.
- A note with no block still yields sources: each `path:line` or `path:line-line` in its prose whose path names a file that exists under the repo root, with no `read_at`.

## Authored files

Two library functions over `<root>/docs/flows/<slug>/`, both taking `root` explicitly and refusing a slug `validate_slug` refuses and any file whose canonical path leaves that directory.

`authored_list(root, slug)` returns:

```json
{"files": [{
  "path": "notes/surface-nav.md",
  "kind": "note",
  "id": "surface-nav",
  "headline": "A seventh surface is a new `Surface` variant …",
  "state": "",
  "task": 0,
  "sources": [{"path": "glimpse/src/surface.rs", "line": 16, "claim": "", "url": "", "read_at": "", "dirty": false, "origin": "inline"}]
}]}
```

| `kind` | File | `id` | `headline` | `state` |
|---|---|---|---|---|
| `brief` | `brief.md` | `brief` | the `# ` title | `""` |
| `design` | `design/<name>.md` | `<name>` | the `# ` title | `""` |
| `note` | `notes/<id>.md` not matched below | `<id>` | the Verdict line's text | `""` |
| `challenge` | `notes/structure-challenge.md` | `structure-challenge` | the Verdict line's text | `""` |
| `cold-read` | `notes/cold-read-<m>.md` | `<m>` | the Verdict line's text | `""` |
| `backlog` | `notes/backlog-<id>.md` | `<id>` | the first non-blank line | `""` |
| `run-note` | `notes/run/task-<n>.md` | `task-<n>` | the Status line's text after the dash | the Status word |
| `settle` | `notes/settle/<F>.md` | `<F>` | the first line | `""` |

`task` is `<n>` for a run note and `0` otherwise. `origin` is `block` or `inline`. Files are listed sorted by `path`. Any other file under the directory is left out.

`authored_read(root, slug, path)` returns `{"path", "text"}` for one listed path, the text LF-normalised, and refuses a path the listing would not carry.

## Agent usage

An agent record gains `usage`, written when the agent stops, beside `context_tokens`, which keeps its meaning:

```toml
[agents.usage]
input = 1820
cache_write = 40211
cache_read = 913004
output = 15233
turns = 41
```

Each field sums that count over the transcript's assistant messages, counting each message id once, since one message can span several transcript lines that repeat its usage. `turns` is the number of distinct messages. The table is omitted while every field is zero. A Codex agent records none.

## Finding read

`review_finding(root, store, id)` returns one finding of a v2 review store as the object `review show --for report` prints for it, or an error naming the store or id it could not find. `store` is the repo-relative path a finding link carries.
