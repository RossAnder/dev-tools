Verdict: a worker could carry out milestone B mostly as written. Three points would make a worker guess (the fixture's needs-you content for task 32, how needs-you rows sit in the Inbox's typed list for task 31, and where a milestone's "goal" comes from for tasks 21 and 26), one premise is false (task 20's time parsing), and four worker checks pass on the present tree, so they cannot tell whether the change was made.

# Cold read: milestone B (tasks 15 to 33)

## Unactionable

1. **Task 32 (and task 15): the fixture may never produce a needs-you row.** Task 32 says "`--surface inbox` prints `needs you` when the fixture holds a parked task", and its worker check requires the string `needs you` in `glimpse/tests/cli.rs`. Task 15 owns `glimpse/tests/fixtures/snapshot.json`. Its list of fixture contents ("two milestones, a decision with a rejected option, a contract, a handover, runs, ... a report, ...") has no parked task. It does not say whether the handover is open, and it does not say whether the report holds an entry whose kind is not `parked` or `handover`. Task 31's tests ask for "a fixture with a parked task, an open and a done handover and a report", but task 31 cannot edit the fixture (its files are surface.rs, view/inbox.rs and app.rs). Task 32 may only edit cli.rs and may add no production code. If the fixture yields no needs-you rows, task 31's rule ("omitted when it has no rows") leaves task 32 with no way to meet its check. What is missing: task 15's fixture list should name a parked task, an open handover, and a report entry of another kind.
2. **Task 31: how needs-you rows join the Inbox list is never decided.** The task makes them the first section of the Inbox, gives them ids `need:<kind>:<id>`, and says the cursor reaches them. But `InboxState` tracks `Tracking<InputRow>` (glimpse/src/surface.rs:789, and the premise grep `pub(crate) tracking: Tracking<InputRow>,`), and `inbox_section` groups `InputRow`s by status (surface.rs:944). The rows "are not input records". The plan never says which of two designs to use. One is to build synthetic `InputRow`s, which risks breaking the constraint that input-record behaviour is unchanged and that no row reaches the input store. The other is to change the row type or keep a second list with a joined cursor, and then to decide how `Tracking`, the flashes and the marks span both lists. That is a design decision a deep worker would have to make alone, and three later behaviours depend on it: the tab count, "no arrival flash", and "a marked set never includes one".
3. **Tasks 21 and 26: the plan does not say where a milestone's "goal" comes from.** The Plan surface's Milestones row shows "id, goal, ..." and the milestone details start with "goal". But the snapshot's `milestones` keys, as design/records.md lists them, are `id, gates, elaboration, defer_reason, gate_probes`, with no goal. The goal is stored as the checkpoint's `rationale` (tomlctl/src/plan/milestone.rs:3: "a `[[checkpoints]]` row (the id and its goal, as `rationale`)"). glimpse already holds it (glimpse/src/model.rs:201 and :244 `pub(crate) rationale: String`). A worker who finds this is fine, but nothing in either task points there.

## False premises

1. **Task 20, Detail:** "Parse the agents' RFC 3339 times with what model.rs already uses for agent durations." model.rs parses no times. It only compares `started_at` strings (glimpse/src/model.rs:382). The parser is `hook::parse_utc` (glimpse/src/hook.rs:126), which view/activity.rs:50 uses for elapsed time.

## Check commands

I ran every base premise probe at the present tree. All hold: 15, 16, 17, 19 (both), 21, 22 (both), 23 (compact_tabs and `1-6`), 24, 26, 27, 28, 30, 31, 32 and 33 printed OK. Premises marked `after:N` depend on milestone A tasks (3, 4, 7, 11, 12, 14), which are all `pending`. For example, glimpse/src/model/, nav.rs, authored.rs, rollup.rs, plan_state.rs and the new view/ and source/ files do not exist yet; task 14 creates them.

The worker-owned forward checks fail on the present tree, as they should, for tasks 15, 16, 18, 19, 20, 22 (`'1'..='7'`, `fn short`), 23 (`1-7`), 24, 25, 30, 31, 32 and 33. These worker checks do **not** discriminate, because they already pass:

- **Task 17:** `grep -q "fn open" glimpse/src/nav.rs glimpse/src/app.rs` passes today on `fn open_input` (glimpse/src/app.rs:1241). Suggested check: `fn open(`.
- **Task 22:** `grep -q "Self::Plan" glimpse/src/surface.rs` passes today on `Self::PlanReview` (surface.rs:32). Task 23's premise "Plan is a surface (at after:22)" is the same grep, so it holds trivially. Suggested check: `Self::Plan\b` or `Self::Plan =>`.
- **Task 23:** `grep -q "Surface::Plan" glimpse/src/view/mod.rs` passes today on `Surface::PlanReview` (view/mod.rs:411).
- **Task 27:** `grep -q "KeyCode::Char('b')" glimpse/src/keys.rs` passes today on the existing test assertions `assert_eq!(map(press(KeyCode::Char('b'))), None);` (keys.rs:280, :334).

I did not run the `cargo test --manifest-path glimpse/Cargo.toml <filter>` checks. Each is `Writes build: yes` and run by the gate, and before milestone A lands they could only fail for the missing change.

## Observations (not counted)

- **Task 32 depends on `App::new`.** `--once` builds its app with `App::new(snapshot, ...)` and applies no snapshot later (glimpse/src/runtime.rs:264). Task 22 says the `PlanState` is "rebuilt when the snapshot ... change[s]", and task 31 says needs-you rows are "built from the snapshot whenever it changes". If either is built only on a snapshot update and not also in `App::new`, task 32's headless render shows empty sections and no needs-you rows.
- **Stale `--surface` help text.** `--surface plan` parses through `Surface::from_key` (glimpse/src/cli.rs:251), so it works once task 22 adds the key. But the help text listing six surfaces (cli.rs:28) is in no task's files, so it stays stale.
- **`Wake::NewScope` takes a `LedgerKind`.** It is `Wake::NewScope(LedgerKind)` (glimpse/src/watch.rs:35), so task 19's "add the watch the way `Wake::NewScope` adds a flow-less ledger directory" needs a new wake variant or a widened payload. Both watch.rs and mode.rs are in the task's files, so this is doable.
- **Task 16 versus the design.** Task 16 implements `CursorList` for `ItemsState` and `InboxState` only, while design/glimpse.md, Cursor lists, also names `PlanState`. Task 21 adds that one, so this is consistent.

## Coverage

I read the worker views and file lists for tasks 15 to 33, plus the ids, files, status and checkpoint of tasks 3, 4, 7, 11, 12 and 14. I checked these code files: glimpse/src/app.rs, keys.rs, surface.rs, state.rs, cli.rs, main.rs, model.rs, runtime.rs, hook.rs, flows.rs, watch.rs, view/details.rs, view/header.rs, view/footer.rs, view/legend.rs, view/items.rs, view/mod.rs, view/markdown.rs, glimpse/Cargo.toml (pulldown-cmark 0.13), glimpse/README.md and glimpse/tests/cli.rs (premise only). In tomlctl I checked src/plan/milestone.rs and src/tasks/snapshot.rs (rationale).

I could not check anything that depends on milestone A's output: the snapshot's new keys, `authored_list` and `authored_read`, `review_finding`, the plan store's run-record arrays, agent `usage`, or the stub modules task 14 creates.
