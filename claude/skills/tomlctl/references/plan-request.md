# tomlctl — plan request reference

The four `plan request` leaves: a running worker's one request of its run, for files its task does
not hold and for corrections to its own spec, and the orchestrator's rulings on it. Each takes the
required `--slug` and the write bundle described in [plan.md](plan.md), which holds every other
`plan` verb; `list` takes the read bundle. The `[[request]]` record's keys are
[plan-store.md](plan-store.md)'s, and the `request` kind of a needs-human report entry is
[plan-run.md](plan-run.md)'s. Which step of a run posts, rules or applies a request is owned by the
[`implement-v2-engine`](../../implement-v2-engine/SKILL.md) skill.

Nothing here writes the task store: the drain writes the granted items to `tasks.toml` and then
marks the request `applied`.

## Contents

- [`plan request add`](#plan-request-add)
- [`plan request rule`](#plan-request-rule)
- [`plan request list`](#plan-request-list)
- [`plan request applied`](#plan-request-applied)

## `plan request add`

Posts the one request a running task makes in its run. The task must be listed by the `tasks` of an
open `implement:<milestone>` run; the newest such run owns the request, and the request records its
`run_id` and `milestone`. A task posts one request per run: a second is `kind=validation`, naming
the first. A request names at most 16 paths, each path once (compared by its folded key, below), and
needs at least one item in `paths` or `changes`.

```bash
tomlctl plan request add --slug <slug> --task <n> --json <src>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--task` | integer | The requesting task. | required |
| `--json` | source | The payload below; `-` reads stdin, `@<path>` a file (a `.toml` path is a [record file](write.md)). | required |

The payload is one JSON object; an unknown key at any level refuses the call, naming it:

```json
{"paths":[{"path":"src/a.rs","why":"The caller lives here."}],"changes":[{"field":"accept","what":"Check the new flag too.","evidence":"tests/a.rs:12"}]}
```

| Key | Meaning |
|---|---|
| `paths` | Files the task needs and does not hold; each `{path, why}`, both non-empty. A path is normalised to repo-relative with `/` separators. |
| `changes` | Corrections to the task's own spec; each `{field, what, evidence}`, all non-empty. `field` is `accept`, `means`, `constraints`, `detail` or `files`. |

Items are numbered in the order given: `p1`, `p2`, … for paths and `c1`, `c2`, … for changes. The
request id is `Q<n>`, minted under the lock one past the highest `Q` number the store has held.

`add` rules each path item it can settle mechanically, inside the `plan.toml` write lock, so two
concurrent posts cannot both be granted one path. The first rule that matches decides, in this
order:

1. **Forbidden**: denied. An absolute path, a path with a `..` segment, a path under `.claude/` or
   under the flow's own `docs/flows/<slug>/`.
2. **Already held**: denied. The task's own `files` already name it.
3. **Held by another row or an earlier grant**: denied. Another remaining row of the task's milestone
   could edit the path alongside it, by `files` or by a granted request item not yet applied; the
   reason names the task and, for a grant, the request.
4. **Declared generated output**: granted. The path is an output, declared in
   `.claude/generated.toml`, of a generator whose input the task holds.
5. **Otherwise open**, left for `plan request rule`.

Paths are compared by a folded key (slashes normalised, a leading `./` dropped, letter case folded),
so a case-only variant of a held file counts as the same file. A change item is never ruled by `add`:
it stays open. Mechanical rulings are written with `by = "tomlctl"`; the request is `ruled` at once
when no item is left open, and `open` otherwise.

Prints the request and one entry per item:

```json
{"ok":true,"id":"Q1","status":"open","items":[{"item":"p1","ruling":"open","by":"","reason":""},{"item":"c1","ruling":"open","by":"","reason":""}]}
```

| Key | Meaning |
|---|---|
| `id` | The request id. |
| `status` | `open` while any item awaits a ruling, otherwise `ruled`. |
| `items[].item` | `p<n>` or `c<n>`. |
| `items[].ruling` | `open`, `granted`, `denied` or `needs-user`. |
| `items[].by` | `tomlctl` for a mechanical ruling, `orchestrator` for one made by `rule`, `""` while open. |
| `items[].reason` | Why, in a line; `""` while open. |

## `plan request rule`

Rules the open items of an open request, all or nothing: one bad row refuses the batch before any
write, naming its line. A request already `ruled` is refused.

```bash
tomlctl plan request rule <request-id> --slug <slug> --ndjson <src>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `<request-id>` | id | The request, such as `Q1`. | required |
| `--ndjson` | source | One row per item, `-` for stdin or a file path (a leading `@` is accepted, and a `.toml` path is a [record file](write.md)). | required |

Each row is `{"item","ruling","reason"}`, all three required and no other key:

| Key | Meaning |
|---|---|
| `item` | `p<n>` or `c<n>`, naming an item the request has; an item is ruled once per call and only while it is `open`. |
| `ruling` | `granted`, `denied` or `needs-user`. `open` is not a ruling. |
| `reason` | A non-blank line saying why. |

Granting a path re-checks it against the holders in rule 3 above and refuses the batch when another
row or an earlier grant holds it. The items are written with `by = "orchestrator"`; the request
becomes `ruled` once no item is left open. A row need not name every open item, so a partial call
leaves the request `open`.

Prints the same object as `add`.

## `plan request list`

Prints every request, one row each, oldest first.

```bash
tomlctl plan request list --slug <slug>
```

Takes no flag beyond the target and the read bundle. The output is an array of rows, and `--lines`
prints one per line:

```json
[{"id":"Q1","run_id":"wf_1","milestone":"A","task":3,"at":"2026-11-18T10:00:00Z","status":"ruled","ruled_at":"2026-11-18T10:02:00Z","applied":false,"paths":[{"path":"src/a.rs","why":"The caller.","ruling":"granted","by":"orchestrator","reason":"Disjoint from every running task."}],"changes":[{"field":"accept","what":"Check the flag.","evidence":"tests/a.rs:12","ruling":"denied","by":"orchestrator","reason":"The accept check is the plan's to change."}]}]
```

| Key | Meaning |
|---|---|
| `id`, `run_id`, `milestone`, `task`, `at` | Who posted it, in which run, and when. |
| `status` | `open` or `ruled`. |
| `ruled_at` | When the last item was ruled; `""` while open. |
| `applied` | Whether the drain has written the granted items to the task store. |
| `paths` | Each `{path, why, ruling, by, reason}`. |
| `changes` | Each `{field, what, evidence, ruling, by, reason}`. |

## `plan request applied`

Marks ruled requests as applied once their granted items are in the task store.

```bash
tomlctl plan request applied <request-id>[,<request-id>...] --slug <slug>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `<request-id>` | ids | One or more request ids, comma-separated. A repeated id counts once. | required |

An unknown id is refused, and so is a request still `open`: every item is ruled first. Nothing is
written on a refusal. Prints `{"ok":true,"ids":["Q1","Q2"]}`.
