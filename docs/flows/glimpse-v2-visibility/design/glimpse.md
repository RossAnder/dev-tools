# Glimpse

How glimpse shows a v2 flow. Every view here is read-only: nothing in this design writes a store.

## Modules

New files, each declared once by the module-declaration task so that later tasks add no `mod` line:

| File | Holds |
|---|---|
| `glimpse/src/nav.rs` | `Target`, `Location`, the back stack, `App::open` |
| `glimpse/src/authored.rs` | The authored-files listing as glimpse holds it |
| `glimpse/src/plan_state.rs` | `PlanState`, `PlanRow`, the sections and how rows are built |
| `glimpse/src/rollup.rs` | Time and token totals |
| `glimpse/src/model/plan.rs` | The v2 snapshot types and `PlanIndex` |
| `glimpse/src/source/doc.rs` | The poller's on-demand document reads |
| `glimpse/src/view/doc.rs` | `Doc` and `Link`, what a details builder returns |
| `glimpse/src/view/task_spec.rs` | A v2 task's spec sections |
| `glimpse/src/view/plan.rs` | The Plan surface's list |
| `glimpse/src/view/plan_details.rs` | The details of a Plan row |
| `glimpse/src/view/links.rs` | The links list |
| `glimpse/src/view/doc_view.rs` | A fetched document in the details panel |

## Target

One type names anything glimpse can show:

```rust
pub(crate) enum Target {
    Task(u32),
    Milestone(String),
    Decision(String),
    Contract(String),
    Handover(String),
    Run(usize),              // index into plan.run
    Authored(String),        // a listed path, such as notes/surface-nav.md
    Source { path: String, line: Option<u32>, read_at: String, dirty: bool },
    Url(String),
    Commit(String),
    Finding { store: String, id: String },
    Backlog(String),
    Input(String),
}
```

`App::open(target)` is the only way to follow a link:

- A target with a row on some surface (`Task`, `Milestone`, `Decision`, `Contract`, `Handover`, `Run`, `Authored`, `Backlog`, `Input`) switches to that surface and puts its cursor on the row. A task goes through `App::select`, so the traversal trail rule holds. A row the surface's filter or closed toggle hides is revealed first.
- A target with no row (`Source`, `Commit`, `Finding`) asks the poller for the document and shows it in the details panel (section "Document lane").
- `Url` copies the address to the clipboard through the existing copy path and says so in the footer; glimpse opens no browser.
- A target that resolves to nothing (a task id the snapshot lacks, a path the listing lacks) leaves everything as it was and says so in the footer.

Before it moves, `open` pushes the present `Location` (surface, that surface's cursor, and the shown document if any) onto `App::back`. `Backspace` pops it and restores that location. The stack is cleared when the flow changes. `Esc` keeps its present chain and never pops the stack.

## Links

A details builder returns `Doc { text, links }` in place of bare text. `links` is every record the shown one relates to, each a `Link { label, target }`, in the order the text mentions them.

`b` opens the links list for the record whose details are showing: a modal list of `links`, moved with the cursor keys, opened with `Enter` or a click on a row, closed with `Esc`. With no links it opens nothing and the footer says so. Details text is not clickable.

What links to what:

| From | Links |
|---|---|
| Task | needs, dependents and coupled tasks; its milestone; decisions and contracts that bind it; each `design` and `precedent` entry; its run note; its commit; its finding or backlog rows; its attempts' handover |
| Decision | bound tasks and paths; each note its `evidence` names as `notes/<id>.md` |
| Contract | its tasks |
| Handover | its source task |
| Milestone | its tasks; its gate rounds' kept logs |
| Run | the tasks it dispatched |
| Authored file | each of its sources; for a run note, its task; for any other note, each decision whose `evidence` names it as `notes/<id>.md` |
| Commit document | the tasks stamped with that commit |
| Finding document | the task that fixes or settles it |
| Change, departure | the task it names |

A `design/<name>.md#<anchor>` entry is an `Authored` target for the file. A `precedent` entry, and a bound path, is a `Source` target with no line and no `read_at`.

## Plan surface

`Surface::Plan` is the seventh surface, on key `7`, with the state name `plan` and the label `Plan`. The compact header labels a tab by `Surface::short`, a new two-letter label per surface, because `Plan` and `Plan-review` share an initial.

Its state is `PlanState { tracking: Tracking<PlanRow>, .. }`, one list in fixed sections, drawn like the Inbox's sections. `Tab` and `Shift-Tab` move the cursor to the next and previous section's first row on this surface, and keep their present meaning elsewhere. A row's id is `<section>:<key>`, so a refresh keeps the cursor.

| Section | One row per | Row shows |
|---|---|---|
| Brief | the brief, then each design file | title |
| Decisions | decision | id, door, provenance, the decision's first line |
| Contracts | contract | id, kind, claim |
| Handovers | handover | id, reason, done or open, action |
| Milestones | milestone | id, goal, tasks done of total, last gate outcome, fix rounds |
| Timeline | run row of the plan store | step, outcome, start, duration |
| Notes | note, challenge, cold read, backlog note, settle proposal, then run notes | kind, id, headline |
| Changes | `[[changes]]` row at or after the latest approval | time, ref, field, reason |
| Departures | deviation of an attempt | task, what |
| Cost | milestone, then executor, then the unattributed remainder | agents, time, tokens |
| Health | accept entry or premise that is unprobed while probeable, or whose verdict is not `healthy` or `holds`; gate probe with no verdict or one that is not `holds` | task or milestone, kind, verdict or `unprobed` |

A section with no rows is drawn as its heading and `none`. A flow with no plan store shows the surface with one line saying it is not a v2 flow. The tab's count is the number of Health rows plus open handovers; it is never drawn as an arrival flash.

The surface offers no write: the item menus, marks, classify and request actions do nothing on it and the footer lists none of them.

## Plan row details

The details panel for a Plan row shows the whole record:

- **Decision:** decision, why, provenance, door, surfaced and vetoed, evidence, if false, revisit if, check, each rejected option with its reason, and the bound tasks by id and title.
- **Contract:** kind, claim, check, provides, consumes, tasks.
- **Handover:** reason, action, when, done when, done or open, source task.
- **Milestone:** goal, elaboration and defer reason, each gate with its probe and verdict, then every run of every gate round in order: its outcome, and per command the outcome, duration, summary, failed ids and kept log path. Fix rounds are the rounds above 0.
- **Run:** step, workflow, run id, session, started, finished, outcome, skip reason or abandon reason, dispatched tasks.
- **Authored file:** the file's text, rendered as markdown, fetched through the document lane.
- **Change:** time, ref, field, old, new, actor, reason.
- **Departure:** task, run, what, why, the means departed from.
- **Cost row:** the agents counted, with each one's type, time and tokens.
- **Health row:** the behaviour or claim, the check, the probe's verdict, exit codes and the commit it was recorded at.

## Task details

A task's details keep everything they show today. A task with a `spec` also shows, after the action and detail: Outcome, Constraints, Means (each marked required or suggested, with its detail), Executor, Design, Precedent, Accept (behaviour, check, kind, who runs it, and the probe verdict or `unprobed` or `not probed`), Premises (claim, check, when, and the verdict), file notes beside the files they annotate, Changes (this ref's rows of the trail), Attempts (per attempt: run, status, summary, deviations, checks) and Links to findings and backlog rows. A task with no `spec` is drawn exactly as today.

## Document lane

The UI thread reads no file and runs no command. The runtime sends the poller one request at a time, as it does for the transcript tail, and the poller answers with an event:

```rust
enum DocRequest {
    Authored { slug: String, path: String },
    Source { path: String, line: Option<u32>, read_at: String, dirty: bool },
    Commit { sha: String },
    Finding { store: String, id: String },
}
```

- `Authored` reads through `tomlctl::authored_read`.
- `Source` shows the file as the note's author saw it where that can be known: `git show <read_at>:<path>` when `read_at` is set and `dirty` is false. Otherwise it reads the working tree and the document says why: `read uncommitted` when `dirty`, `commit not recorded` when `read_at` is empty. When the working tree's file differs from the shown text, the document says `changed since`. A file larger than 2 MiB, or one that is not UTF-8, is refused with a one-line reason.
- `Commit` runs `git show --stat --format=fuller <sha>`.
- `Finding` reads through `tomlctl::review_finding`.

A `sha` or `read_at` is passed to git only when it is 7 to 64 hex digits, and a path only when it is repo-relative with no `..`, root, drive or UNC component; a working-tree read also canonicalises it and refuses one that leaves the repo root, while a `git show` read leaves resolution to git, so a file deleted or renamed since `read_at` still shows as it was, marked `changed since`. A failure is a document holding one line with the reason, never a panic or a stuck panel. A new request replaces the old one, and an answer for a request that is no longer current is dropped.

The details panel shows a fetched document in place of the row's details while one is open: its title names the target, a `Source` document starts scrolled to its line with that line marked, and markdown is rendered only for an authored file. `Backspace` leaves it.

## Authored feed

`Feed::Authored { slug }` carries the listing. Its change detector re-stats every file under `<root>/docs/flows/<slug>/` on each scan, because an in-place save moves no directory time, and reads the listing through `tomlctl::authored_list` only when a stat moved or a wake named the scope. A new `WatchScope::Authored` covers `<root>/docs/flows`, recursively, with its own miss streak; it is added when the directory exists; while it is missing, each scan probes for it, as a missing flows root is probed, and adds the watch when it appears, since no watched directory reports its arrival. A change the re-stat finds while this scope has no watch counts no miss. A path outside `docs/flows`, and a file that is not `.md`, wakes nothing.

## Cursor lists

`ItemsState`, `InboxState` and `PlanState` implement one trait, `CursorList`, over the list operations `App::apply` needs: move, page, scroll, viewed, cursor id and whether a cursor row exists. `App::list_mut()` returns the current surface's list, `None` on Tasks. Each arm of `App::apply` that today tests `surface == Surface::Inbox` and falls back to `self.items` calls `list_mut()` once instead. Actions that only an item surface has (marks, menus, classify, request, queue) keep their own guard.

## Needs you

The Inbox gains a first section, `needs you`, for the viewed flow: each parked task, each open handover, and each entry of the plan store's latest `[[report]]` whose kind is not `parked` or `handover` and whose `needs_person` is not false. A row shows its kind and one line. `Enter` on a row opens its target: the task, the handover, or the entry's path as a `Source`. They are held in a list of their own beside the input records and share only the visible order and the cursor. These rows take no write: the answer, acknowledge, withdraw and request actions skip them. The Inbox tab's count includes them.

## Rollups

`rollup.rs` totals time and tokens from the snapshot alone:

- A segment's time is its end less its start; an open segment runs to now.
- A segment naming several tasks splits its time and its agent's tokens evenly among them.
- Only agents whose type starts `plan-worker-` count toward a task. Every other agent of the flow goes to one remainder row, `other`, by agent type.
- A task's totals roll up to its milestone (its `checkpoint`) and to its spec's `executor`.
- Tokens are the agent's `usage` fields. An agent with no `usage` counts as not recorded, never as zero, and a total that includes one is marked partial.
- A milestone's row adds its gate rounds' command durations as gate time.

## Markdown

The markdown renderer handles tables: a header row, a rule, and body rows, with columns sized to their content and wrapped to the panel width.
