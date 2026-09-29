# tomlctl — backlog reconcile reference

The flag surface of `tomlctl backlog reconcile`, the buckets it sorts promoted rows into, its
`render_needed` duty, what `--apply` writes, and its output shape. The rest of the `backlog`
group and the store it writes are [backlog.md](backlog.md); when to reconcile is the
`backlog-capture` skill's job (`claude/skills/backlog-capture/SKILL.md`).

## `backlog reconcile`

Joins every `promoted` row to the tasks that close it in its target flow's
`.claude/flows/<slug>/tasks.toml`, and buckets the row by how far that work has got. It judges
by task-row status; the flow's own `status` only decides whether the flow is closed. With
neither `--adopt` nor `--apply` it writes nothing.

```bash
tomlctl backlog reconcile
tomlctl backlog reconcile --flow <slug> --apply
tomlctl backlog reconcile --adopt
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--flow` | slug | Only rows whose `promoted_to` resolves to this flow, by slug or by the plan it binds. An unknown slug matches nothing rather than erroring. | every promoted row |
| `--adopt` | — | Link each row no task links yet to every task whose prose names its id. Runs before bucketing. | off |
| `--apply` | — | Resolve every `ready` row, recording its flow, tasks and commits. | off |

The link is the store's `[[backlog_links]]` side table, rebuilt from each task's
`- **Backlog**:` plan bullet on import. A `closes` link gates resolution; a `refs` link is
recorded but never gates. `--to` values resolve exactly as
[`triage --promote`](backlog.md#backlog-triage) resolves them, so rows stored under either the
slug or a plan path join. A row whose target is not a flow is `dangling` or `external`; a row
whose target is a flow lands in the first of the other five buckets that fits, in table order:

| Bucket | When |
|---|---|
| `ready` | At least one task closes the row and every one is `done`. Tested first, so a ready row in a closed flow is `ready`, not `orphaned`. |
| `orphaned` | The flow is at `review` or `complete`. |
| `unlinked` | No task closes the row — including a flow with no `tasks.toml`, and a row only `refs` links name. |
| `stalled` | A closing task is `deferred` or `failed`. |
| `in-progress` | A closing task is still `pending` or `in-progress`. |
| `dangling` | `promoted_to` resolves to no flow: an unknown value, or a plan no flow binds. |
| `external` | `promoted_to` is an `external:` reference. |

A link naming a `ref` no task carries — left behind by `tasks remove`
until the next import — is dropped from the join and noted in `reason`.

`--adopt` considers each row whose target flow has a `tasks.toml` in which no link, `closes`
or `refs`, names it. It adds the id to the `closes` of every task whose title, action, detail
or acceptance names it as a whole token: not preceded by an ASCII letter or digit and not
followed by a hex digit, so `B-1a2b3c4d` does not match inside the widened `B-1a2b3c4d5e`. It
writes each flow's store once, under its lock, and never creates a missing store. Bucketing
then reads the stores as `--adopt` left them, so `--adopt --apply` resolves a row adopted in
the same run.

**`render_needed` is a duty, not a hint.** The plan owns the links: `tasks import-plan`
rebuilds `[[backlog_links]]` from the plan's bullets, so a link `--adopt` wrote into the store
is lost at the next import unless the flow is rendered first. Check for pre-existing drift
*before* adopting — an adopted link is itself drift, so `--check` after `--adopt` always exits
1 — and adopt one flow at a time, only into a flow that checks clean:

```bash
tomlctl tasks render --slug <slug> --check
tomlctl backlog reconcile --flow <slug> --adopt
tomlctl tasks render --slug <slug>
```

A flow that already drifts is reported and left alone; rendering it would overwrite the hand
edits the drift stands for.

`--apply` resolves the `ready` rows in one locked write to `.claude/backlog.toml`. Each row
becomes `status = "resolved"` dated today, keeps its claim, and gains:

- `resolution` — ``resolved by flow `<slug>` (tasks <closes>)``;
- `resolved_flow` — the slug;
- `resolved_tasks` — the `closes` task refs, then the `refs` ones;
- `resolved_commits` — the distinct non-empty `commit` values of the `closes` tasks only,
  since a `refs` task did not deliver the item; possibly `[]` when the last batch is not yet
  committed.

Each row is re-read and re-checked under the lock. One no longer in the backlog, no longer
`promoted`, whose `promoted_to` has changed since the survey, or that fails validation is
listed under `skipped` with a reason, and the rest still resolve. When nothing resolves the
store and its sidecar stay byte-identical. `--apply` never writes a `tasks.toml`.

```json
{"ok":true,"flow":"alpha",
 "buckets":{"ready":[{"id":"B-a1b2c3d4","summary":"…","target":"alpha","flow_status":"in-progress",
                      "closes":["add-the-guard"],"refs":["document-the-guard"],
                      "reason":"every closing task is done: add-the-guard"}],
            "in-progress":[{"id":"B-1a2b3c4d","summary":"…","target":"alpha","flow_status":"in-progress",
                            "closes":["fix-the-probe"],"refs":[],
                            "reason":"closing tasks not done: fix-the-probe (pending)"}],
            "stalled":[],"unlinked":[],"orphaned":[],"dangling":[],"external":[]},
 "adopted":[{"id":"B-1a2b3c4d","flow":"alpha","task_refs":["fix-the-probe"]}],
 "applied":["B-a1b2c3d4"],
 "skipped":[],
 "skipped_flows":[{"path":".claude/flows/broken/context.toml",
                   "reason":"TOML parse error at line 1: unclosed array, expected `]`"}],
 "render_needed":["alpha"]}
```

- `flow` echoes `--flow`, and is `null` without it.
- Every bucket key is present, and an empty bucket is `[]`.
- `target` is the resolved value, so a row stored under a bound plan path reads as the flow's
  slug. `flow_status` is `null` in `dangling` and `external`.
- `closes` and `refs` are task `ref` slugs, not ids.
- `adopted`, `applied` and `skipped` are `[]` without the flag that fills them, as is
  `render_needed` without `--adopt`. `skipped` names backlog ids.
- `skipped_flows` lists each `.claude/flows/*/context.toml` that could not be read or parsed,
  in the shape `flow list` reports under `skipped`, and is always present — `[]` when every
  flow read. A skipped flow is out of the index, so a row promoted to it lands in `dangling`,
  and `--flow` does not filter this list.

`reason` is prose for a reader, not an enum. Branch on the bucket, never on `reason`.
