---
name: flow-contract-task-store
description: Canonical contract for a flow's per-flow task DAG store at `.claude/flows/<slug>/tasks.toml` and the `tomlctl tasks` verb group that owns it — the store schema and every row field, the `ref` slug rule that makes a task heading the store's primary key and the import upsert key, the status vocabulary, the `needs` / `coupling` edge kinds and the graph products (`ready`, `batches`, `closure`, `overlap`, checkpoint membership, `Checkpoint after`) computed on read rather than stored, the semantics of `import-plan`, `add`, `add-many`, `update`, `remove`, `show`, `list`, `edges`, `ready`, `batches`, `closure`, `check` and `render`, the deliberately non-required `--slug` / `--file` target group and the `import-plan --plan` plan-mode form, the `check` finding classes with their severities and exit policy, the render contract's three owned sections and its containment guarantee, the auto-import-at-Step-0 rule and the `/tdd` sub-flow exemption, the ref-set diff gate every re-import runs and the renamed-heading trap it exists to catch, the orchestrator-only status-write rule, the fetch-by-id dispatch idiom, `/implement`'s degradation halt, and `tomlctl integrity refresh` as the recovery step after a failed write. Consult before any read or write of a flow's tasks.toml by /plan-new, /implement, /review-plan, or /plan-update.
---

## Task-store contract

`.claude/flows/<slug>/tasks.toml` is a flow's task DAG: one row per plan task, the two stored edge sets, the execution policy, and the checkpoint groups. From `/plan-new` Phase 9 onward it is canonical and the plan document's `## Execution Policy`, `## Tasks` and `## Dependency Graph` sections are *rendered* from it — the same derive-don't-author pattern `PROGRESS-LOG.md` already follows. Everything a carrier used to re-derive in prose (the frontier, file-claim intersections, checkpoint closures, Kahn batches) is a verb.

The flag tables for every verb live in [`claude/skills/tomlctl/references/tasks.md`](../tomlctl/references/tasks.md). This document is the contract: what the fields mean, which verb a carrier reaches for, and the rules that bind all four carriers.

### 1. Store schema

```toml
schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/<slug>.md"
last_import_refs = ["scaffold-the-tasks-module-tree", "…"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
origin = "plan"
note = ""
commit_granularity_note = "— tasks 5 and 6 land in one commit"

[[checkpoints]]
id = "A"
rationale = "store, schema and graph engine — buildable alone"

[[file_notes]]
ref = "check-side-arm-width-at-a-narrow-viewport"
file = "packages/shell-react/src/studioOverflow.browser.test.tsx"
note = "(new)"

[[import_overrides]]
ref = "check-side-arm-width-at-a-narrow-viewport"
files = ["packages/shell-react/src/studioOverflow.test.tsx"]

[[items]]
id = 12
ref = "check-side-arm-width-at-a-narrow-viewport"
title = "Check side-arm width at a narrow viewport"
effort = "S"
status = "pending"
checkpoint = "A"
phase = "Phase 2: overflow behaviour"
phase_depth = 3
heading_depth = 4
files = ["packages/shell-react/src/studioOverflow.browser.test.tsx"]
needs = [10]
coupling = []
deps_note = "10 is what this task needs"
action = "…"
detail = "…"
acceptance = "…"
agent = ""
commit = ""
```

| Field | Owner | Meaning |
|---|---|---|
| `schema_version` | the tool | `1`. Seeded by `flow init` and by the auto-create write path alike. |
| `last_updated` | the tool | Bare TOML date, restamped on every mutation. Nothing else writes it. |
| `plan_path` | `import-plan` | Repo-relative plan the store was imported from, and `render`'s target under `--file`. |
| `last_import_refs` | `import-plan` | Ref set the last import produced. Drives `plan/orphan-row`; empty means "never imported". |
| `policy.checkpoints` | the plan | `single` \| `milestones` \| `per-batch`. |
| `policy.max_parallel` | the plan | 1–8. Outside that range is an error-class finding. |
| `policy.commit_granularity` | the plan | `per-task` \| `per-checkpoint` \| `single-commit`. |
| `policy.origin` | `import-plan` | `plan` \| `default`. `default` means the plan authored no `## Execution Policy` and all three fields above are house defaults; it is the only legacy-mode discriminator, because the import always materialises a `[policy]` table. |
| `policy.note` | the plan | Prose under the `## Execution Policy` bullets that belongs to no bullet, re-rendered as trailing prose. Authored text only — never a tool diagnostic, and never a mode test. |
| `policy.checkpoints_note` / `.max_parallel_note` / `.commit_granularity_note` | the plan | The clause trailing that bullet's value, kept against the bullet that carried it and re-rendered on that bullet's own line. Each is omitted from the file while empty. |
| `[[checkpoints]].id` / `.rationale` | the plan's markers | Group id and its marker prose. Group *membership* is not here — it is on each row. |
| `[[file_notes]].ref` / `.file` / `.note` | the plan | The annotation a `Files` entry carried — `(new)` and the like — against the row that claims the path. A row's `files` holds bare paths because every file-claim comparison reads them, so this is where the annotation lives. Rebuilt from the plan at each import for the rows the plan names, kept only for paths the merged row still claims, and kept whole for a row the plan no longer names. |
| `[[import_overrides]].ref` / `.files` / `.needs` | `update --unlock-import-fields` | The plan value a hand patch replaced, keyed by `ref` (§5a). Absent from a store nothing has hand-patched. |
| `id` | the plan / `add` | The plan's task number. `add` mints `max(id) + 1`. |
| `ref` | derived (§2) | Primary key. |
| `title`, `effort`, `files`, `needs`, `deps_note`, `action`, `detail`, `acceptance` | the plan | Overwritten wholesale by every import. `files` and `needs` are the one exception, and only while a stamp holds (§5a). |
| `status`, `agent`, `commit`, `coupling` | the execution | Preserved across every re-import. |
| `checkpoint` | derived at import | Group id, or `""` for a row no group holds. `add` and `update` refuse a group id no `[[checkpoints]]` entry declares, naming the ones it does; `""` is the "no group" spelling and stays legal. A retained row's id is blanked when the plan stops declaring the group. |
| `phase`, `phase_depth`, `heading_depth` | the plan | The phase label standing over the row in `## Tasks`, that label's `#` run, and the row's own. Overwritten by every import like the plan's other fields, and reachable through neither `add-many` nor `--set`. Absent, they read `""`, `0` and `3`. |

`[[items]]` is the array name on purpose: the whole `items list` predicate, projection and aggregation surface applies to task rows unchanged. The store sits under `.claude/`, so the write-path containment guard, the exclusive lock, the `.sha256` sidecar and the auto-create-from-seed all apply without special-casing.

Fields not in the table above are dropped on write. The store is tool-owned; a hand-added key does not survive the next mutation.

`origin` was added without a `schema_version` bump, so the reader carries the back-compat: an absent `origin` reads as `plan`, **except** that a `[policy]` whose `note` is exactly `policy absent in source plan` reads as `origin = "default"` with the note cleared. That sentence is what pre-`origin` imports stamped into `note` to mark the same condition, and clearing it is what stops a tool diagnostic reaching the plan at the next render. Nothing else may test `note`.

`[[file_notes]]` and the three `*_note` keys were likewise added without a `schema_version` bump. Both are absent from every store written before them and read as empty; both are omitted on write while empty, so a store nothing has annotated round-trips byte-identical.

### 2. The `ref` rule

A row's `ref` is derived from its plan heading's title — the text after the `N. ` number and before any trailing ` [S|M|L]` tag. The rule is the GitHub heading-anchor rule with two deliberate divergences: runs of whitespace and hyphens collapse to a **single** hyphen (`Setup & Config` → `setup-config`, not `setup--config`), and underscores are kept rather than stripped, because execution records already on disk carry `task_ref`s containing them. Everything else outside `[a-z0-9_]` is dropped after lowercasing.

Repeats are suffixed in document order: the first heading keeps the bare ref, the second becomes `<ref>-2`, the third `<ref>-3`. A title carrying no slug characters at all is refused rather than keyed to an empty string.

`ref` is the store's primary key and the import's upsert key, and it is the join column the execution record's `task_ref` and `last_import_refs` both use. That is why it is immutable through `--set` and reachable only through an explicit `tasks update <id> --ref <slug>`: rewriting it orphans the row in both directions at once. A `--set title=` deriving a different ref is refused for the same reason unless `--ref` rides along, so a retitle moves the key deliberately or not at all.

### 3. Status vocabulary

`pending` · `in-progress` · `done` · `failed` · `deferred`. Case-sensitive — `Done` is not `done`.

A new row arrives `pending`. `import-plan` preserves whatever an existing row already carries, and under `--reconcile-record` promotes a row to `done` when the flow's `execution-record.toml` holds a `type = "task-completion"` entry with `status = "done"` whose `task_ref` matches. A record entry that is `failed` or `skipped` is ignored — adopting one would mark an unfinished row done.

### 4. Edge kinds, and what is derived

Two edge sets are **stored**:

- `needs` — the plan's `Depends on` edges. What the task waits on.
- `coupling` — acceptance-reachability edges: ids whose work this row's acceptance command exercises. Counted in the in-degree exactly like `needs`, so a coupling edge pushes its dependent a layer back.

Everything else about the graph is **computed on every read and never persisted**: the ready frontier, Kahn batches, `closure_up` / `closure_down`, `overlap` (two rows claiming a file with no directed path either way), each checkpoint group's maximal elements, and the plan's `Checkpoint after` bullet. The one derived value that *is* stored is a row's `checkpoint`, computed at import as the dependency closure of its marker's `after` list minus everything an earlier marker already claimed.

**Lower set and antichain** are the terms for the checkpoint cut rule, and they are what make the derivation sound. A group is committable exactly when the union of it with every earlier group is a **lower set** (downward-closed) under the dependency order — nothing in the prefix depends on anything outside it. The `Checkpoint after` list a plan author writes is the **antichain of maximal elements** of that lower set: the tasks with no dependents inside the group. Naming the antichain is enough, because the closure of those elements *is* the group. That is why `## Dependency Graph` carries markers only and never mirrors per-task edges.

The `## Tasks` markdown has no syntax for `coupling`. `render` folds `needs ∪ coupling` into one `Depends on` line, and `import-plan` subtracts a row's existing coupling from the parsed edge list — which is what keeps `import(render(store)) == store` a round-trip rather than a slow migration of coupling into needs.

### 5. Verb semantics

Read verbs take `--verify-integrity`; write verbs take the write-integrity bundle. Every verb emits JSON on stdout.

**`import-plan`** — parse a plan's three sections and upsert the store keyed on `ref`. Existing rows keep `status`, `agent`, `commit`, `coupling` and any record-adopted `ref`; new rows arrive `pending`; **nothing is ever deleted**. A named row's file annotations are re-derived from the plan on every import, so a `files` patch the import honours keeps only the annotations for paths the merged row still claims. A row the plan no longer produces stays and is reported in `removed_refs`. A real (non-`--dry-run`) import refuses before writing if any finding is error-class.

A retained row keeps every field but one: a `checkpoint` naming a group the plan no longer declares is blanked, and the row is named in `cleared_checkpoint_refs`. Membership is recomputed for every row the plan does produce, so retention was the only route by which an undeclared group id entered the store with no write at fault.

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

**`add`** — append one row, minting its id and `ref`. Dangling dependency targets and cycles are refused before anything is written. Returns `{ok, id, ref, batch}`, where `batch` is the zero-based Kahn round the row lands in.

```bash
tomlctl tasks add --slug <slug> --title "Extract the token reader" --effort S --files src/auth/token.rs --needs 3,4 --checkpoint B --action "Move read_token out of session.rs" --acceptance-file <path>
```

**`add-many`** — the same validation over NDJSON, one row object per line, all-or-nothing: a malformed line, a dangling target or a cycle aborts before the file is touched. Two rows in one batch may reference each other. An **unknown input key is a hard error** naming the row's 1-based index and listing the accepted key set (`title`, `effort`, `files`, `needs`, `coupling`, `deps_note`, `checkpoint`, `action`, `detail`, `acceptance`) — a mistyped `neds` would otherwise silently discard an edge. Returns `{ok, added, rows}` with per-row `id`, `ref` and `batch`.

```bash
printf '%s\n' '{"title":"Add the retry guard","effort":"S","files":["src/retry.rs"],"needs":[3]}' | tomlctl tasks add-many --slug <slug> --ndjson -
```

**`update`** — patch one row's mutable fields. `changed[]` reports what actually moved, not what was passed, so a re-issued `--status done` reports no change. `--set` reaches `title`, `effort`, `status`, `checkpoint`, `agent`, `commit`, `deps_note`, `action`, `detail`, `acceptance` and `coupling`; `files` and `needs` are owned by `import-plan` and refused unless `--unlock-import-fields` rides along (§5a).

```bash
tomlctl tasks update <id> --slug <slug> --status in-progress --agent implement-deep
```

#### 5a. `--unlock-import-fields`, and the stamp it leaves

A plan-review merge that has agreed a task's `Files` or `Depends on` line otherwise has to go back to the markdown, edit it by anchor, re-import and re-render. `--unlock-import-fields` opens the two fields to `--set` and records the plan values the patch replaced as a `[[import_overrides]]` entry keyed on the row's `ref`:

```bash
tomlctl tasks update 12 --slug <slug> --unlock-import-fields --set files=src/a.rs,src/b.rs --set needs=3,4
```

The entry holds the **base** — what the plan stated — not the patch. The import compares each field's parsed plan value against that base by membership, not order:

- **The plan still states the base.** The row's hand-patched value stands, and the import reports `plan/override-held`. This is the window the stamp exists for, and `tasks render --check` reports the same divergence as `render/drift` throughout it.
- **The plan states anything else.** The plan wins outright: its value replaces the patch, the entry is dropped, and the import reports `plan/override-released`.

So the stamp cannot pin a row past the document. The intended exit is `tasks render`, which writes the patch into the plan — the next import then reads it as a change, releases the stamp, and the values agree with no override left. An author who wants the plan's value back instead runs `tasks update <id> --relock-import-fields`, which drops the entry and leaves the row's values alone until the next import restores them; patching a field back to its base drops it the same way.

`needs` is validated against the whole store like `coupling` — a dangling target or an edge closing a cycle is refused before the row moves. `--ref` carries the stamp with the row, `tasks remove` drops it with the row, and an entry keying on a `ref` no row holds is dropped at the next import.

`[[import_overrides]]` and `[[file_notes]]` are the store's two side tables, both keyed by `ref` rather than by id: the first holds what a hand patch replaced and is written by `--unlock-import-fields`, the second holds the annotations a plan's `Files` entries carried and is rebuilt by the import. `tasks show` reports the first on the row it was asked for.

**`remove`** — hard-delete one row, the only path that takes a row out of the store. `import-plan` keeps every row it stops producing, so a task deleted from the plan is retired here or not at all. Two removals are refused unless `--force`: a row past `pending`, named with its `ref` (the execution record's `task_ref` and the commit train's SHA both join on it), and a row other rows depend on, named with its dependents. Under `--force` the removed id is pruned from every dependent's `needs` and `coupling` **and the removed row's own `needs` are spliced into each dependent's** — a dependent left one edge short would dispatch ahead of work it still waits on. `rewired[]` names the rows whose edges moved. There is no `--dry-run`, and a refusal writes nothing.

A successful removal also drops the row's `[[import_overrides]]` entry, so the store never holds a stamp keyed on a `ref` no row carries. The prune is on the success path only — a refused removal still writes nothing — and `pruned_override_fields[]` names the stamped fields the entry was holding, a subset of `files`, `needs` in that order. It is always present, and empty when the row carried no stamp.

```bash
tomlctl tasks remove <id> --slug <slug> --force
```

**`show`** — one row. Without `--with` the output is the summary shape (`id`, `ref`, `title`, `effort`, `status`, `checkpoint`, `files`, `needs`, `coupling`). `id` is always emitted whatever `--with` selects, so a fetched row can be matched back to the id that was asked for. `deps` is the row's own direct targets; `dependents` is the transitive successor set.

A stamped row (§5a) carries one more key, `import_override`, last and ungated by `--with`: `{"files": [...], "needs": [...]}` holding the plan **base** each patched field replaced, not the patched value the row reports above. An unstamped field is omitted and an unstamped row carries no key at all, so a store nothing has hand-patched shows exactly what it showed before. The `ref` is not repeated inside it, and the summaries nested under `deps` / `dependents` never carry it — the stamp belongs to the row that was asked for.

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

**`list`** — task rows through the shared query surface: `--where` / `--where-in` / `--where-prefix` and the rest, `--select` / `--pluck`, `--sort-by` / `--limit`, `--count` / `--count-by` / `--group-by`, `--ndjson` / `--raw` / `--lines`.

```bash
tomlctl tasks list --slug <slug> --where status=pending --select id,ref,files --ndjson
```

**`edges`** — the edge list as `{kind, from, to}`, `from` being the prerequisite. `--kind needs|coupling|overlap` narrows it; `--dot` emits Graphviz source instead.

```bash
tomlctl tasks edges --slug <slug> --kind overlap
```

**`ready`** — the dispatchable frontier: `{ready[], held[{id, blocked_on_file, holder}], next[], blocked[{id, blocker, blocker_status}]}`. `ready` excludes what `held` names, so it is directly dispatchable. `held` is the file-claim exclusion against the ids passed to `--in-flight`, `next` is what becomes ready once the current round lands, and `blocked` names the rows no later wave can reach — each with the **nearest ancestor** stranding it and that ancestor's status. The walk climbs through intervening `pending` rows, so the id named is the stall itself and never a pending row between, which is what makes `blocker` directly the row to resume. An ancestor stalls when it is neither `done`, nor `pending`, nor declared in flight, so a task genuinely being worked keeps its dependents in `next` while one a crashed run left behind puts every pending row behind it in `blocked`, and an empty frontier still separates a finished graph from a stalled one.

The four buckets do not partition the pending set. `next` is one wave deep by design, so a healthy row two or more waves out is enumerated by none of them; only `blocked` reaches arbitrarily far back, because a stall strands everything behind it.

```bash
tomlctl tasks ready --slug <slug> --in-flight 3,4
```

**`batches`** — Kahn layers over the whole graph, each ascending.

```bash
tomlctl tasks batches --slug <slug>
```

**`closure`** — a checkpoint group as `{checkpoint, members[], maximal[], dependency_closure[], valid_cut}`, or one task's walk as `{task, direction, ids[]}`. `valid_cut` covers the union of the group with every earlier one, so it reads as "committable here", not "self-contained". `members` is the group and `dependency_closure` everything it reaches upward — the set the rendered marker prints under that same name. Both are reported because they coincide exactly when every dependency already sits in an earlier group, which is the case a reader cannot use to tell them apart.

```bash
tomlctl tasks closure --slug <slug> --checkpoint A
tomlctl tasks closure --slug <slug> --task <id> --up
```

**`check`** — the invariant checks (§7).

```bash
tomlctl tasks check --slug <slug> --plan
```

**`render`** — rewrite the plan's three owned sections from the store (§8).

```bash
tomlctl tasks render --slug <slug> --check
```

`ready`, `batches` and `closure` **refuse a cyclic store** rather than answering. Kahn strands a cycle's members, and a stranded task reads in a frontier exactly like one that is merely waiting — a partial answer would be indistinguishable from a clean bill. Run `tasks check` to get the cycle's members named.

`closure --checkpoint <id> --up` (or `--down`) is likewise refused with `kind=validation`: a direction walks one task, and silently ignoring it on a checkpoint would answer a question nobody asked.

### 6. The target group, and plan mode

Every `tasks` verb takes a mutually-exclusive `--slug <slug>` **or** `--file <path>` target. `--slug` resolves `<root>/.claude/flows/<slug>/tasks.toml`; `--file` passes a path through verbatim, still under the write guard unless `--allow-outside`.

The group is deliberately **not** `required`. `import-plan` may run with `--plan <path>` and no store target at all — plan mode, the pre-flow validation `/plan-new` Phase 7 runs before `ExitPlanMode` — and every other verb refuses an empty target in post-parse validation instead, so the refusal arrives as a tagged `kind=validation` error — a machine-readable envelope under `--error-format json` — rather than clap usage prose on stderr.

```bash
tomlctl tasks import-plan --plan docs/plans/<slug>.md --dry-run
```

Plan mode requires `--dry-run`: without a store there is nothing to write into, and the combination is refused rather than silently reporting a no-op.

`import-plan` reads its plan from `--plan` when given, otherwise from the flow's `context.toml` `plan_path`. That recorded value is file-controlled input and `atomic_write` runs no guard, so a `plan_path` that is absolute, `..`-bearing, resolves outside the repo root, or does not name a `.md` document is refused with `kind=validation` rather than read. An explicit `--plan` is a caller argument on the way in — resolved against the repo root when it does not resolve against the working directory, so the verb works from a subdirectory — but the path the import **records** is put through that same check, so a plan the read side would refuse back is refused at the import instead of wedging every render after it.

**`flow ensure-artifact --kind tasks` reports the store; it does not seed one.** `--bootstrap --kind tasks` is a marker-carrying no-op that writes nothing. Seeding is `flow init`'s job, and the auto-create write path covers the rest. No carrier may treat `ensure-artifact` as a seeding route.

```bash
tomlctl flow ensure-artifact --slug <slug> --kind tasks
```

### 7. `check` classes and exit policy

| Class | Severity | Meaning |
|---|---|---|
| `dag/cycle` | error | The named tasks form a dependency cycle. Suppresses every class that needs reachability. |
| `dag/dangling-ref` | error | A row's `needs` or `coupling` names a task the store does not hold. |
| `dag/duplicate-number` | error | Two rows claim one task number. |
| `dag/unbuildable` | error | The graph engine refuses the store for a reason the row scans did not name — past the 256-node cap is the live case. Suppressed when `dag/duplicate-number` or `dag/dangling-ref` already accounts for the refusal. |
| `dag/unreachable-claim` | warning | Two rows claim a shared file with no directed path either way — a file-claim collision under parallel dispatch. |
| `dag/stalled-dependency` | warning | A row that is `in-progress`, `failed` or `deferred` while pending rows still sit behind it — the frontier's `blocked` bucket. One finding per blocker, its `ids` naming the blocker and its `detail` the dependents. |
| `dag/symbol-without-edge` | warning | A symbol one row's `Action` introduces — `Add`, `Create` or `export` within three words of a backticked identifier — that another row's body names, with no directed path either way. One finding per pair, its `ids` naming both. This is the edge `coupling` exists to carry and nothing populates. Two suppressions: a row whose `files` are all markdown introduces nothing (it is documenting the symbol), and a name more than one row introduces raises nothing against a row that already reaches one of them. |
| `checkpoint/orphan-task` | warning | A row no checkpoint group holds: `checkpoint = ""`, or a group id no `[[checkpoints]]` entry declares. Each case is its own finding, and either way only the final commit train commits the task. |
| `checkpoint/invalid-cut` | error | A group's prefix union is not downward-closed; the ids named are the dependencies that must move earlier. |
| `checkpoint/marker-mismatch` | warning | The authored `Checkpoint after` bullet disagrees with the markers' derived maximal elements. Raised only by `import-plan` — the bullet is never stored. |
| `files/closure` | info | Backticked paths under `packages/`, `apps/`, `docs/` or `scripts/` that a row's `Action` or `Detail` names and its `files` does not claim, one finding per row. Lines carrying `read-only`, `model of`, `pattern` or `cite` are skipped. The only `info`-severity class, and the severity is the contract: it is a list a lens reads, never a gate. |
| `policy/max-parallel-range` | error | `policy.max_parallel` outside 1–8. |
| `policy/checkpoints-value` | error | `policy.checkpoints` outside the vocabulary §1 lists. |
| `policy/commit-granularity-value` | error | `policy.commit_granularity` outside the vocabulary §1 lists. |
| `policy/origin-value` | error | `policy.origin` is neither `plan` nor `default`. Only a hand-edited store reaches it — the import writes one of the two. |
| `plan/policy-absent` | warning | The plan carries no `## Execution Policy`, so `origin` is `default` and every policy field is a house default. Raised only by `import-plan`, at the import that makes the substitution. |
| `plan/effort-untagged` | warning | **One** finding per import, its `ids` naming every heading carrying no `[S|M|L]` tag. Raised only by `import-plan` — the tag is an input the store never holds. The `M` default applies only where neither the heading nor an existing row supplies one, so a re-import of an untagged heading keeps the row's current effort and still warns; the next render writes whichever value the row holds back into the plan. |
| `plan/no-tasks` | error | The plan's `## Tasks` section holds no numbered task heading. Raised only by `import-plan`, which refuses rather than replacing the checkpoint table and the policy with the empty and default values a taskless plan yields — both are assigned whole on every import, so nothing else would preserve them. A section holding only links to sibling documents that do carry task headings gets the multi-file diagnostic instead, the same one a plan with no `## Tasks` section at all raises: the import reads exactly one document, so a multi-file plan keeps its task headings in the outline. |
| `plan/orphan-row` | warning / **error** | A stored row whose `ref` the last import did not produce — renamed or deleted in the plan. `warning` while its status is `pending`; **`error`** once it is not, because `render` would resurrect a settled task into `## Tasks`. |
| `plan/override-held` | warning | A row whose stamped `files` or `needs` the plan has not contradicted, so the store's hand-patched value stands (§5a). Raised only by `import-plan`. |
| `plan/override-released` | warning | The plan restated a stamped row's `files` or `needs`, so the plan's value replaced the patch and the stamp is gone (§5a). Raised only by `import-plan`. |
| `render/drift` | warning | The plan markdown differs from the store's render after line-ending normalisation. Raised by `check --plan` and by `render --check`, which do not agree on the exit code — see the exit policy below. |

**The two prose heuristics.** `files/closure` and `dag/symbol-without-edge` read a row's `Action` and `Detail` rather than its fields, and both are scoped by what an earlier attempt measured: `files/closure` as a *gate* flagged 80 of 117 path tokens across 28 tasks — a 68% false-flag rate — because the plan format requires a read-only reference to stay off the `Files` line (`docs/ideas/plan-flow-mechanical-verification.md`, "Deferred: `tomlctl plan lint`"). What ships is that check narrowed to four path roots, skipping the lines whose prose marks a reference as read-only, and raised at `info` so no carrier can gate on it — a list a lens reads, not a verdict. `/review-plan` keeps its Files-line closure sub-check as prose regardless, because separating an edit target from a reference is the judgement the heuristic cannot make. `dag/symbol-without-edge` is warning-class rather than info because it names a specific pair and a specific symbol, and it reads only spans an introducing verb covers for the same reason the closure check was narrowed: an untargeted token scan is what measured unusable.

`plan/orphan-row` fires **only when `last_import_refs` is non-empty**. An empty ref set means the store has never been imported, not that every row is orphaned.

`dag/stalled-dependency` is a warning because a row a live run is working on and one a crashed run abandoned read identically from the store. `check --in-flight <ids>` is how the caller that does know says so: those rows are treated as legitimately busy rather than stalled, exactly as `ready` treats the same set, so a carrier running `check` while agents hold rows passes the ids it last passed to `ready`. One gating *before* it dispatches anything — `/implement`'s Phase-1 gate — passes nothing, and every stall it then reports is a row a crashed run left behind. Undeclared, the class leaves the exit code at `0`; at error severity it would refuse a valid import and fail the gate on every healthy run.

`checkpoint/orphan-task` covers two cases and stays a warning for the first: `checkpoint = ""` is the legitimate spelling for a row only the final commit train commits. The second — a group id no `[[checkpoints]]` entry declares — no longer has a legitimate producer now that the import blanks it on a retained row, so only a hand edit reaches it. Raising that half to error class is a separate change; the class is one finding class with one severity today.

**Exit policy.** `tasks check` exits `1` if any finding is error-class and `0` otherwise — warnings and info alone exit `0`. `tasks render --check` exits `1` on any drift, even though `render/drift` is a warning class, because its whole job is to be a gate. The two therefore answer differently on one drifted plan: `check` derives its status from severity and `render --check` from the finding's presence. **A carrier gating on drift runs `tasks render --slug <slug> --check`**; `check --plan` reports the same finding and exits `0`, so a run gated on it passes a drifted plan silently. A refusal (`kind=validation`, cyclic graph, uncontained `plan_path`, empty target) exits `1` on the error path — stderr, no findings array, and no `ok` key at all. A real `import-plan` that hits an error-class finding refuses and writes nothing; the same import under `--dry-run` reports the finding in its envelope and exits `0`.

### 8. Render contract

`render` owns exactly three sections — `## Execution Policy`, `## Tasks`, `## Dependency Graph` — and **every other byte of the plan document is preserved**. A section runs to the next `^## `; a missing one is inserted in canonical order (Execution Policy before Tasks, Dependency Graph after Tasks), and one already present at the wrong place is moved into that order — the owned sections' slots are refilled by rewriting the heading lines in place, so every other section keeps its position. A permutation leaves every body byte-identical, so `--check` reports it as its own drift ("plan sections out of canonical order") rather than as a difference it cannot name. The source document's dominant line ending is detected and re-applied to the replaced sections, so a CRLF plan is not rewritten to mixed endings and `--check` does not report permanent drift.

What each section becomes:

- `## Execution Policy` — the four bullets, each carrying back the clause the store holds against it, with `Checkpoint after` derived as the union of every group's maximal elements, then `policy.note` as trailing prose.
- `## Tasks` — `N. Title [E]` at the row's own `heading_depth`, in **store order** (the plan's own task order, not id order), then `Files` — each path with the annotation the store holds against it (`` `path` (new) ``), whitespace-collapsed onto the one line — `Depends on` (`needs ∪ coupling` sorted, then `(deps_note)`), `Action`, `Detail`, `Acceptance`, bodies re-indented two spaces. An empty body emits no line. A non-empty `phase` is emitted as a heading at its `phase_depth` ahead of the row wherever the label or that depth differs from the previous row's, so a run of rows under one label yields the one heading the author wrote. Both depths are clamped to 3–6 on render: two hashes would open a `## ` section and split the one being written, and past six is no heading at all.
- `## Dependency Graph` — the preamble sentence and one marker per group: `— CHECKPOINT A after tasks <maximal> — dependency closure: <sorted ids>. <rationale>`, with `(INVALID CUT)` appended when the group's `valid_cut` is false. The label names the upward walk, not the member set `closure` reports under `members`.

`render` **refuses** — returns an error and writes nothing — on a cycle, a dangling edge, or a graph past the 256-node cap. A marker naming the wrong tasks is worse than a refusal, and closures are undefined through a cycle. `--check` and `--stdout` both branch before the write lands, so neither leaves a byte behind on a plan it disagrees with.

The output is a **derived write**, like `PROGRESS-LOG.md`: no `.sha256` sidecar. The target is resolved from the flow's `context.toml` under `--slug` and from the store's `plan_path` under `--file` — never from a free argument — and is refused if it is absolute, carries a `..` component, canonicalises outside the repo root, or does not name a `.md` document. Under `--slug` the two must also agree: a store whose recorded `plan_path` resolves to a different document than the context's is refused until the plan is re-imported, because the document a render rewrites has to be the one the store was imported from. An empty `plan_path` records no import from a file at all and so binds nothing.

### 9. Auto-import at Step 0, and the `/tdd` exemption

Every carrier that consumes the store ensures it at Step 0: if the store is absent or holds no rows, run `import-plan --slug <slug> --reconcile-record` before anything reads it. `--reconcile-record` is what keeps a re-imported store from re-dispatching work the execution record already recorded as done.

**`/tdd` sub-flows are exempt.** A sub-flow whose slug matches `<parent>-tdd-<NNN>` keeps today's record-derived skip-list and **never touches a store** — it does not create one, import one, or read one. Its parent flow owns the plan and therefore the store; a sub-flow importing the parent's plan into its own directory would mint a second store with the same refs and no owner.

### 10. The ref-set diff gate

Every re-import runs as a two-step gate: **`--dry-run` first, inspect `added_refs` / `removed_refs`, then the real import.**

```bash
tomlctl tasks import-plan --slug <slug> --dry-run
```

A non-empty `removed_refs` means a heading was renamed or a task deleted. Abort on any removed ref whose row is not `pending` — that is a settled task about to be orphaned — and surface the added/removed pair either way.

**A renamed heading does not produce a clean add/remove on a real import.** This is the single most important operational rule here, because `/review-plan`'s merge and `/plan-update reformat` both rephrase headings. The old row is never deleted, so it keeps the task number it was imported under; the renamed heading arrives with a new `ref` and *the same* number. The import therefore raises `dag/duplicate-number` at error severity and refuses before writing anything. The recovery is to rename the row rather than the store:

```bash
tomlctl tasks update <id> --slug <slug> --ref <new-ref>
```

then re-import. The gate is specified as a `--dry-run` step precisely so the rename is caught while it is still a diff, not after a refusal.

`--reconcile-record` covers the other half: it adopts the execution record's spelling of a `ref` when the normalised forms match uniquely on both sides, and reports every record `task_ref` it could not place in `unmatched_refs`. Adoption demands uniqueness on both sides and a free target — a rename onto a sibling's key would silently merge two tasks.

### 11. Status writes are the orchestrator's

Only the orchestrating carrier writes to the store. `import-plan`, `add`, `add-many`, `update`, `remove` and `render` are orchestrator verbs; a sub-agent gets the read verbs — `show`, `list`, `edges`, `ready`, `batches`, `closure`, `check` — and nothing else.

An implementing agent reports its outcome in its return payload, exactly as it does today. The orchestrator moves the row (`--status in-progress` at dispatch, `--status done` or `--status failed` on return) and appends the execution-record entry. Two writers on one store means a row's status and the record's `task-completion` entries can disagree, and the store has no supersession mechanism to reconcile them.

### 12. Fetch by id at dispatch

A dispatch prompt carries the task's **id** and the command to fetch its body, not pasted prose:

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

That is the whole point of the store: `## Tasks` is the largest single thing an orchestrator loads, and only the agent executing a task needs that task's prose. The prompt still carries the byte-identical shared preamble every agent gets; the per-task text is fetched.

`--with deps` gives the agent its direct dependency summaries, which is what it needs to know what already exists. `--with dependents` is a planning read, not a dispatch read — it walks the transitive successor set and grows with the plan.

### 13. Degradation is a halt, never a fallback

`/implement` halts with a named diagnostic when any of these holds:

- `tasks check` reports an error-class finding;
- the store is absent after an import that claimed to succeed;
- `tomlctl tasks` is unrecognised — a stale binary on `PATH`.

The diagnostic names the two recoveries: `cargo install --path tomlctl` for the stale binary, and a `git revert` of the four carrier adoptions to fall back to the prose frontier. It **never proceeds on partial store output**. A frontier computed from a store that failed its own checks dispatches the wrong tasks in the wrong order, and it does so silently.

### 14. Recovery after a failed write

A write that fails mid-flight — the sidecar rename losing a race with a virus scanner is the live case on Windows — leaves the TOML intact and its `.sha256` stale. The next read with `--verify-integrity` then errors, and nothing auto-repairs it. Confirm the store parses, then refresh the sidecar from the on-disk bytes:

```bash
tomlctl validate .claude/flows/<slug>/tasks.toml
tomlctl integrity refresh .claude/flows/<slug>/tasks.toml
```

`integrity refresh` never parses TOML — it hashes whatever is on disk — so `validate` first is what stops a truncated file receiving a valid sidecar. If the store itself is damaged, delete it and re-run `import-plan --slug <slug> --reconcile-record`: every plan-owned field is re-derived from the plan and every execution-owned field from the record, which is exactly the pair the store keeps.
