# Milestone schedule

The rule `plan emit schedule` computes and the implement-v2 engine follows. One module, `tomlctl/src/tasks/milestones.rs`, owns it; the leaf in `tomlctl/src/plan/schedule.rs` only reads the stores, supplies the open elaborate runs and prints.

## Upstream milestones

Milestone N is upstream of milestone M when either holds:

- a row of M reaches a row of N through `needs` or `coupling`, the edges `tasks closure --checkpoint <M>` walks for its `dependency_closure`, and N is not M;
- M's `after` list names N.

`checkpoint/invalid-cut` keeps every milestone a row reaches earlier in store order, and `after` may name only an earlier milestone, so every upstream milestone of M is stored before M. An elaboration therefore adds needs only on milestones stored before the one it elaborates.

## States

Each milestone in `[[checkpoints]]` order gets the first state below that fits. `rows` is the milestone's rows; a row is **started** when its `agent` is `plan-worker-lite` or `plan-worker-deep` and its `commit` is empty.

| State | When | `on` |
|---|---|---|
| `elaborating` | `elaboration` is `deferred` and `plan.toml` holds an open run of step `elaborate:<id>` | the upstream milestones not `done` or `empty`, in store order |
| `deferred` | `elaboration` is `deferred` | the upstream milestones not `done` or `empty`, in store order |
| `empty` | The milestone has no rows | — |
| `done` | Every row is `done` with a non-empty `commit` | — |
| `unstamped` | Every row is `done` and at least one has an empty `commit` | — |
| `started` | A row is started and not every row is `done` | — |
| `parked` | No row is started, and a row is `parked` or every row not `done` is `deferred` | — |
| `waiting` | An upstream milestone is not `done` or `empty` | the upstream milestones not `done` or `empty`, in store order |
| `runnable` | None of the above | — |

## The pick

1. When any milestone is `unstamped`, the pick is empty: Re-entry settles it before anything runs. Every `started` and `runnable` milestone is reported `held`, `on` naming the unstamped ones.
2. Otherwise, when any milestone is `started`, the pick is the first `started` one in store order. Every other `started` or `runnable` milestone is reported `held`, `on` naming the pick.
3. Otherwise the pick is the first `runnable` milestone in store order, or empty when none is.

`held` is reported in place of the state it overrides; `waiting`, `deferred`, `elaborating`, `parked`, `done`, `unstamped` and `empty` are never overridden. A `parked` milestone is never picked, and a milestone downstream of it stays `waiting`.

## Output

One JSON object:

```json
{"pick": "C", "milestones": [{"id": "A", "state": "done", "on": [], "left": 0}, {"id": "B", "state": "elaborating", "on": [], "left": 6}, {"id": "C", "state": "runnable", "on": [], "left": 12}, {"id": "D", "state": "waiting", "on": ["B", "C"], "left": 5}]}
```

`pick` is the milestone id, or `""` when nothing can run. The report is a single object, so a reader takes the pick with `--get pick` and the milestones with `--rows milestones`, which then accepts every row option. `milestones` is in `[[checkpoints]]` order. `left` counts the milestone's rows that are not `done`. A `deferred` milestone with an empty `on` is ready to elaborate.
