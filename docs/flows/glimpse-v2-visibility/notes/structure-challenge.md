Verdict: restructure — add a typed cross-entity `Target` with one `open`/back path, detail builders that return their link targets, a single read-only `Plan` surface over the existing `Tracking<R>`, and one list-cursor accessor in `App::apply` in place of the Inbox-vs-items guard pairs; the data path stays an extension (v2 sections added to the existing `tasks snapshot`, plus one authored-files feed for `docs/flows/<slug>/`).

# What the change does to the code as it stands

## Data in: extension, not restructure

- **The snapshot already reads and hashes `plan.toml` but throws it away.** `tomlctl/src/tasks/snapshot.rs:47-71` reads all five `SNAPSHOT_INPUTS`, `plan.toml` included, into one revision. `:104-110` parses the plan only to get `running_milestone`. `task_json` (`:232-251`) emits only the v1 row fields. `Store` already holds every v2 task fact the future state names (`tomlctl/src/tasks/schema.rs:85-127`: `specs`, `milestones`, `changes`, `file_notes`, `backlog_links`, `finding_links`; `TaskSpec` at `:263-273` with `Accept`/`Premise`/`Probe`/`Means`), and `PlanStore` (`tomlctl/src/plan/schema.rs:142-156`) holds decisions with `rejected`, contracts, approvals, runs and handovers. What is forced: `snapshot` emits a per-task `spec` (with that ref's file notes, changes and links) plus top-level `plan`, `milestones` and `changes`. On the glimpse side, `model.rs` gets matching `#[serde(default)]` types. The contract-order test in `snapshot.rs` changes with them.
  - Ruled out: a separate `plan_snapshot` library call or feed. It would read and hash `plan.toml` and `tasks.toml` a second time, and two events could show a task's spec out of step with its row, while the single revision already covers both files.
  - Refuted if: some other consumer of `tasks snapshot` cannot tolerate new keys. I found none. glimpse's own model ignores unknown keys (`glimpse/src/model.rs:1-7`).
- **A plan with no task store yet produces no snapshot and is not listed.** `snapshot.rs:94-96` raises the missing-store error. `glimpse/src/source/scan.rs:59-61` (`has_artifact`) counts only `tasks.toml` or a ledger. If the Plan surface must show a plan before `tasks.toml` exists, two edits are forced, both in place: a plan-only document instead of the refusal, and `plan.toml` added to `has_artifact`. The future state does not settle this, so the planner should.
- **The notes, brief and design sit outside every watch scope.** `glimpse/src/watch.rs:1-5` watches `.claude/flows` recursively and `.claude` plus the flow-less ledger dirs non-recursively. `WatchScope` (`glimpse/src/source/mode.rs:87-94`) has no scope for `docs/flows/<slug>/`, where `tomlctl/src/plan/store.rs:47-53` puts `brief.md`, `design/` and `notes/`. What is forced: a `WatchScope::Authored` with its own miss streak, a `Wake` for it, and a `Feed` (`glimpse/src/source.rs:81-85`) that lists the notes with their kind (from the directory) and the `Verdict:` first line. This follows the existing `Wake::NewScope` pattern.
- **Reading a note in full, a cited source "as it was", or a commit is on-demand I/O.** The UI thread does no file I/O. The transcript already uses an on-demand lane (`Control::Tail`, `source.rs:158-168`, with `Host::set_tail`, `runtime.rs:398-404`). What is forced: one more request/response pair of that shape (e.g. `Control::Open(DocRequest)` and `Event::Doc`), served on the poller thread, using git for historical content. glimpse already shells out to git (`glimpse/src/flows.rs:182-193`). This is an extension. Recording which commit a note "was written at" is a data question for the planner. Notes cite `file:line` with no commit (`claude/agents/plan-investigator.md:28`).

## Interaction: this is where the present structure gives way

1. **No notion of "a thing glimpse can show" spans surfaces.**
   - Selection is three unrelated cursors: `App::selected: Option<u32>` for tasks (`glimpse/src/app.rs:302`), a `String` cursor per ledger `ItemsState` (`glimpse/src/surface.rs:419-438`, `:558-568`), and `InboxState` (`surface.rs:789-800`). The only move between surfaces is `SwitchSurface` (`app.rs:902-911`), which leaves the destination cursor wherever it was.
   - The future state needs links between decisions, notes, tasks, commits and findings, and from a source reference to the source. Without a shared target type, each (from, to) pair needs its own handler that knows the destination's cursor type. With roughly ten entity kinds, that grows pairwise.
   - Drafted: a new `nav.rs` with `enum Target { Task(u32), Milestone(String), Decision(String), Contract(String), Handover(String), Run(String), Note(String), Source { path, line, at }, Commit(String), Finding { store, id }, Backlog(String), Input(String) }`. Add one `App::open(Target)` that resolves a target to (surface, cursor) or to a doc request, and pushes the current location onto `App::back`. `Back` (`app.rs:1114-1134`) pops `back` before it starts closing panels. A task target goes through `App::select`, so the trail rule in `glimpse/CLAUDE.md` still holds.
   - Refuted if: the planner keeps every link within one surface. The future state's "from a decision, note, task, commit or finding the user can reach the others" rules that out.
2. **Detail builders return text only, so they cannot offer links.**
   - `details::content` (`glimpse/src/view/details.rs:116`) and `item_content` (`:323`) return `Text<'static>`. `render_text` (`:74-112`) returns only a height. `wrap`/`pieces` (`:686-759`) flatten spans to `(String, Style)`, so any identity attached to a span is lost at wrap time.
   - Forced: the builders return `(Text, Vec<Target>)` (or a `Doc` type that holds both), so a selected task, decision or note can list what it links to.
   - Drafted, lighter variant: a links overlay (keyboard: open it, move, Enter → `App::open`) listing that `Vec<Target>`. It reuses the existing overlay pattern and leaves `wrap` alone.
   - Heavier variant: carry link ranges through `wrap` into a new `Regions::links` (`app.rs:107-127`) so links are clickable inline. The planner picks one. The forced edit is the same either way: the builder signature changes.
   - Refuted if: references are shown as plain text only. That contradicts "each source reference can be selected".
3. **Plan records need a read-only list surface, and neither existing list state fits.**
   - `ItemsState` carries marks, a write target and the ledger feed (`surface.rs:558-568`, `app.rs:392-395`). `OpenMenu`, `OpenClassify` and `OpenRequest` build write `Selection`s from it (`app.rs:1028-1082`). Putting plan rows there would offer status moves on decisions, which glimpse must never write.
   - One surface per record kind does not fit either. Digits map only `'1'..='6'` (`glimpse/src/keys.rs:76`), and `Surface::ALL` is in digit order (`surface.rs:28-35`). Brief, decisions, contracts, handovers, milestones/gates, timeline, notes, changes, departures, waiting, cost and pre-launch health would need far more digits than exist.
   - Drafted: one `Surface::Plan` on key 7. Its state is `PlanState { tracking: Tracking<PlanRow>, section }`. `PlanRow` is keyed by its `Target` and grouped in fixed sections, as the Inbox groups its sections (`surface.rs:815`). It reuses `Tracking<R>` (`surface.rs:419`, `Tracked` at `:960`) for cursor, flashes and scroll at no new cost.
   - The sections: Brief, Decisions (with rejected options, evidence, bound tasks), Contracts, Handovers, Milestones (gates, gate probes, gate history from `verification` record entries with their fix rounds counted from `gate <id> round` change reasons), Timeline (runs), Notes (by kind, with verdict), Plan changes (the `changes` trail after approval), Departures (record `deviation` entries and run-note deviations), Waiting (parked rows, open handovers, needs-a-person items, linking Inbox questions through `Target::Input`), Cost (tokens and time by milestone and by executor, from agent segments joined to `spec.executor`), and Health (unprobed or unhealthy premises and accept checks).
   - Refuted if: the planner shows plan records only inside the task details panel. That cannot hold a brief, a timeline or a flow-wide departures list.
4. **`App::apply` dispatches list actions as Inbox-vs-items guard pairs, and a third list kind fails silently.**
   - `Page` (`app.rs:784-792`), `ScrollView` (`:793-802`), `SwitchSurface` viewed (`:907-909`), `ItemMove` (`:918-922`), `SelectItem` (`:925-940`), `FlipSortDir` (`:980-989`), `ToggleClosed` (`:1003-1027`), `has_cursor_row` (`:1219-1226`) and `copy_target` (`:1230-1238`) each test `surface == Inbox` and otherwise fall back to `self.items`. The same holds for `details::render` (`details.rs:35-66`, which falls to `current_items()` and shows "No item selected."), `view::draw_surface` (`glimpse/src/view/mod.rs:400-422`), `keys::scroll_view` (`keys.rs:234`), and the header and footer (`glimpse/src/view/header.rs:110,173`, `glimpse/src/view/footer.rs:254-348`).
   - A Plan surface not added to an arm falls into the items branch, where `items.get_mut(&Plan)` is `None`, so the key does nothing and nothing fails.
   - `ItemsState` and `InboxState` already have identically named but untied `visible`, `move_cursor`, `page_cursor`, `flip_dir`, `cycle_header`, `header_sort` and `cursor_row` (`surface.rs:614-759` and `:877-929`).
   - Drafted: a `CursorList` trait over those methods plus `scroll_by`, `viewed` and `cursor_id`, implemented by all three states, and one `App::list_mut()`. Each arm above then reads `if let Some(list) = self.list_mut()` once. Item-only actions (marks, classify, queue, menu) keep their `current_items()` guard.
   - Refuted if: no new list surface is added (see 3).

## What the change then looks like

The v2 task detail stays in `details::content`. It gains Outcome, Constraints, Means, Executor, Design/Precedent (links), Accept and Premises with probe verdicts, file notes beside Files, post-approval changes, the linked finding or backlog row (links), and the run note (a `Target::Note` link). The Plan surface, the notes reader, the source and commit viewer, and every cross-link go through `Target` and `App::open`. New data arrives through the extended snapshot, the authored feed and the doc lane. `model.rs` gains the v2 types. A `model/plan.rs` submodule with a string-keyed index (decisions binding a task, contracts by task, handovers by `source_task`, runs by milestone, changes by ref) is advisable, because `Index` (`model.rs:321-332`) is all `u32`-keyed task maps. That split is not forced.

# Coverage

Read in full: `glimpse/src/model.rs`.

Read in part:
- `glimpse/src/app.rs` 1-460 and 740-1300
- `glimpse/src/surface.rs` 1-200, plus outlines of the `ItemsState`, `InboxState` and `Tracking` sections
- `glimpse/src/view/details.rs` 1-340 and 686-776
- `glimpse/src/view/mod.rs` 300-440
- `glimpse/src/source.rs` 1-90 and 158-188
- `glimpse/src/source/scan.rs` 25-80
- `glimpse/src/watch.rs` 1-55
- `glimpse/src/flows.rs` 170-193
- `glimpse/src/keys.rs` 228-240
- `tomlctl/src/tasks/snapshot.rs` 1-250
- `tomlctl/src/tasks/schema.rs` 80-410
- `tomlctl/src/plan/store.rs` 1-90
- `tomlctl/src/plan/view.rs` 1-80
- `tomlctl/src/plan/mod.rs`
- `tomlctl/src/plan/schema.rs` and `tomlctl/src/flow/record_schema.rs` 20-140, by struct outline
- `claude/skills/implement-v2-engine/references/drain.md`, by grep

Alternatives ruled out:
- A separate plan fetch or feed (double read; out-of-step events).
- Plan rows in `ItemsState` (write affordances on read-only records).
- One surface per record kind (digit row; a guard set per surface).
- Building on `plan view --for digest`. It is a CLI printer (`plan/view.rs:61`), a summary for an orchestrator re-entering rather than the full records, and private to tomlctl. I did not read the digest body, so whether it could be exposed as a library call is unverified.
- A wholesale split of `app.rs`. No edit this change forces needs more than the list accessor.

Not settled here: where needs-a-person items and gate logs persist. `drain.md:424-450` describes the `needs_human` report as invocation output, and I found no store field for it or for gate log paths. That decides the Waiting and gate-history data sources, not the structure.
