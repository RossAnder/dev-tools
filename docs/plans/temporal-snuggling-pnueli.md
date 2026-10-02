# Plan: glimpse item surfaces — review, optimise, plan-review and backlog triage

**Plan path**: `docs/plans/temporal-snuggling-pnueli.md`
**Created**: 2026-10-02

## Summary

glimpse gains five item surfaces next to Tasks, switched with the digit keys:
- **Review, Optimise, Plan-review and Backlog** — live, filterable, groupable lists of findings and backlog rows.
- **Inbox** — user input records and agent questions.

Interaction is **hybrid**:
- **Form controls write control fields directly.** These are status dispositions with their rationale, and review/optimise severity/effort/category. Each write goes through a new silent tomlctl library facade, on a serial writer thread, guarded by a compare-and-set.
- **Free text and anything needing judgement go to a unified, git-ignored input store, `.claude/inputs.toml`.** This covers new-item captures, change requests and notes. Agents acknowledge and handle those records, and they can post async questions that you answer in glimpse.

**Making it safe for a human to write beside running flows:**
- **Stale-write guard:** `tomlctl items apply` gains a per-op `expect` precondition and an `--on-stale skip` mode, and every carrier's late status write uses them.
- **Agent attribution:** it learns a `ledger:` dispatch line, so review, optimise and apply agents show up live against their flow and item ids.
- **Folded backlog items:** four are delivered. Two make the stop-time transcript read bounded; two are small glimpse debts.

**The plan is large.** It is 76 files in 57 tasks over three milestone checkpoints (A read-only surfaces + attribution, B control-edit writes, C inputs + questions + Inbox + the 0.13.0 release). That is well past the ~25-file split guideline; the user chose one plan with milestones.

**Scrutinise first** (Approach sub-headings):
- "Write model" and "Ledger contract": which fields glimpse may write, and the transition table.
- "Stale-write precondition": the old-binary hazard, where an older tomlctl silently ignores `expect`.
- "Input store": its schema and lifecycle are a new cross-carrier contract.
- "Watching and feeds": the dead-watch detector must survive several watch scopes.

The New Flow wizard and flow pipelines are a follow-on plan.

## Context

glimpse already tracks a flow's task DAG well during `/implement`. Review, optimise, plan-review and backlog items have no comparable view: findings are read as console output or raw TOML, and triage happens only inside the carrier that produced them. The user wants classification and interaction:
- sort, group, filter and mark items;
- change their status with a rationale;
- file new items and change requests without interrupting a running session;
- answer agent questions asynchronously;
- watch findings arrive and resolve during `/review`, `/optimise`, `/review-plan` and the apply flows.

Today glimpse is strictly read-only, links tomlctl only for snapshots, flow lists and agent recording, and watches only `.claude/flows`. Two existing gaps make this harder:
- **Lost updates.** Carriers read ledger state at run start and write transitions much later, with no precondition, so a human edit made mid-run would be silently overwritten.
- **Missing attribution.** Agent attribution recognises only `/implement` dispatches, so review and apply agents are invisible to glimpse.

The intended outcome:
- **Triage console.** glimpse becomes a safe triage console for every item ledger.
- **Agent-owned content.** Core item content stays agent-managed, through the input store.
- **Foundations for the follow-on.** The follow-on New Flow / pipeline plan can build on the form framework and the input store.

## Scope

- **In scope**:
  - **glimpse:** item surfaces for review, optimise, plan-review and backlog ledgers (flow-local and flow-less), and an Inbox surface. Multi-scope file watching with per-scope dead-watch detection. A reusable form framework. A serial writer thread with undo. Multi-select, group-by, sort, a text filter, and arrival badges.
  - **tomlctl:** a silent library facade for ledger reads and control edits; `items apply` `expect` with `--on-stale`; the `wontapply_rationale` fix; the `inputs` store and verb group; ledger-dispatch agent attribution with `item_ids`; a bounded stop-time transcript read.
  - **Carriers and skills:** the carrier prose for the stale guard, the `ledger:` dispatch line and the Step-0 input sweep; a new `flow-contract-user-inputs` skill; policy text naming the human writer.
  - **Folded backlog items:** `B-ad940fc4`, `B-f010d1ce`, `B-f4bd798a`, `B-5c699ce5`.
- **Out of scope**:
  - Handing selections to the Claude pane, multiplexer-agnostic across herdr and tmux, and the id-taking modes for `/review-plan` and backlog.
  - The New Flow wizard, flow pipelines and sequencing.
  - An apply-batch (`items clusters`) view.
  - Editing backlog kind, area or summary in place.
  - Attribution for flow-less ledger runs, which have no `agents.toml`.
  - `/backlog-clear` as a question producer.
  - `B-b58537a5`.
- **Affected areas**: `glimpse/`, `tomlctl/src/`, `tomlctl/tests/`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/README.md`, `claude/commands/`, `claude/skills/`, `CLAUDE.md`, `.gitignore`

## User Decisions

Pre-plan discussion (2026-10-02, before `/plan-new`):
- **Sources** — backlog, review, optimise and plan-review ledgers, flow-local and flow-less.
- **Navigation** — a surface switcher inside one glimpse (Tasks | Review | Optimise | Plan-review | Backlog).
- **Use cases** — post-review triage, live view during a run, backlog grooming, apply monitoring.
- **Concurrency guard** — a precondition (`expect` compare-and-set) in tomlctl `items apply`, plus carrier prose; no run lock-out. _Prompted by: lost-update finding (Research Notes → Lost update)._
- **Optimise rationale field** — `wontapply_rationale` wins; fix `Item::validate` and the ledger-schema skill. _Prompted by: `items.rs:1840` vs `optimise-apply.md:38-39`._
- **Handoff to the Claude pane** — deferred; when picked up it must be multiplexer-agnostic (herdr and tmux). The id-taking modes for `/review-plan` and backlog travel with it.
- **Packaging** — one plan, `checkpoints: milestones`.

Phase 4 directed questions:
1. **CAS failure semantics** → *Skip + report*: a new `items apply --on-stale skip` drops stale ops, lands the rest, and lists `skipped_stale` ids in the envelope; without the flag a stale op aborts the batch. Carriers pass it and surface the skipped ids. _Prompted by: `items.rs:1307` (`compute_apply_mutation` all-or-nothing); `verification.md:109-124` (one-batch writes)._
2. **Agent attribution** → *Include*: match a `.claude/flows/<slug>/<ledger>.toml` path in dispatch prompts, record item ids in a new `item_ids` segment field, add the path to the review-plan and apply preambles. _Prompted by: `correlate.rs:190`; zero research-agent rows in any `agents.toml`._
3. **Arrivals on an unviewed surface** → *Badge only*: the surface tab shows `Review +12` and never switches by itself. _Prompted by: `review.md:74,80` (≤2 write bursts); `App.pending_changes`._
4. **Backlog fold-in** → `B-ad940fc4`, `B-f010d1ce`, `B-f4bd798a`, `B-5c699ce5` (all four offered; `B-b58537a5` not folded). _Prompted by: Exploration Notes → Backlog._
5. **Backlog edits / user input** → the user wants **user input records** that agents acknowledge and handle, making the actual edits themselves; agents can also pose multi-select or general questions for user input. Core data stays agent-managed. _Prompted by: backlog kind/area/summary feed the content-derived id (Research Notes → Field editability)._
6. **Write model (clarified)** → *Hybrid*: glimpse form controls (dropdowns, checklists) write **control fields** directly — status dispositions with their companion text captured in the same form, and review/optimise severity/effort/category — CAS-guarded. Free text the user supplies, and anything needing agent judgement (reclassify a backlog kind, edit a summary, a note), becomes an input record an agent acts on.
7. **Input store** → *One unified store*: repo-level, git-ignored `.claude/inputs.toml` with kinds `capture` (new backlog item — replaces the separate inbox), `request`, `note`, `answer`, plus agent-authored `question`; each row optionally targets a ledger + item; lifecycle new → acknowledged → handled with the agent's note.
8. **Input consumers** → *Owning carriers*: each carrier's Step 0 lists pending inputs targeting its ledger and handles them — `/review` + `/review-apply` (R), `/optimise` + `/optimise-apply` (O), `/review-plan` (P), `/backlog` (B items + captures).
9. **Agent questions** → *Include, async only*: carriers write `question` records (prompt, single/multi-select options or free text, optional target items); glimpse renders them as a form; the user's `answer` record is read by the next carrier run. No agent blocks waiting.
10. **Inbox in git** → *Git-ignore* (now `.claude/inputs.toml`). _Prompted by: public repo (Research Notes → Staging inbox)._
11. **Interaction scope** → all four: multi-select + bulk, undo last write (CAS-guarded), group-by + sort, text filter. _Prompted by: interaction prior-art research._
12. **Flow-less ledgers** → *In the flow selector*: `s` lists flows (now including ledger-only flows), then a separator and `review: <scope>` / `optimise: <scope>` / `plan-review: <scope>` entries. _Prompted by: `flows::list` requires `tasks.toml`._
13. **New Flow wizard + flow pipelines** (raised by the user: a glimpse-managed templated New Flow setup — add backlog items, deferred findings, initial prompt context — and pipelines that schedule review/optimise rounds, backlog triage and fix, and sequence flows) → *Follow-on plan*. This plan builds the foundations they reuse: item surfaces, a reusable form framework (text, dropdown, multi-select), the unified input store, agent questions. Where pipelines live (tomlctl + glimpse vs lumina) is decided in the follow-on.

## Approach

### Write model
glimpse writes two kinds of thing and nothing else:
1. **Control fields, through forms.** These are:
   - status dispositions with their companion text, captured in the same form;
   - review/optimise `severity`, `effort` and `category`.

   Every write is a call into the tomlctl library facade (below), made on glimpse's writer thread and guarded by a compare-and-set on the values glimpse displayed.
2. **Input records**, appended to `.claude/inputs.toml` (see "Input store"). This covers free text the user supplies (a new-item capture, a change request, a note) and answers to agent questions.

Never written by glimpse: item content (`summary`, `description`, `file`, `symbol`, `instances`, `evidence`), ids, `rounds`, `dedup_id`, `first_flagged`, `depends_on`, and every backlog field other than its triage status. glimpse never bumps a review, optimise or plan-review ledger's `last_updated`, because the apply freshness gate compares it against commit dates; backlog triage keeps the stamp `triage::apply_transition` already writes, which no freshness gate reads. glimpse never verifies integrity sidecars, though its writes go through `mutate_doc`, which keeps them current.

### Ledger contract
The new module `tomlctl/src/ledgers.rs` is exported from `tomlctl/src/lib.rs`. None of its functions print. Each takes `root: &Path`, and each write first refuses with `root mismatch` when `io::repo_or_cwd_root()` canonicalised differs from `root` canonicalised: the lock directory derives from that process root, so two writers must agree on it. Integration tests of the write facade therefore set `TOMLCTL_ROOT` in-process, each holding one shared file-static `Mutex` (in `tomlctl/tests/common/mod.rs`) across its `unsafe` `std::env::set_var` / `remove_var` pair, because `test_support::env_lock` is crate-internal and `cargo test` runs a binary's tests on parallel threads.

- `enum LedgerKind { Review, Optimise, PlanReview }` and `enum LedgerRef { Flow { slug, kind }, Scope { kind, scope }, Backlog, File(PathBuf) }`.
  - `Flow` validates the slug with `flow::validate_slug`.
  - `Scope` validates the scope name with the same regex.
  - The paths are `.claude/flows/<slug>/{review-ledger,optimise-findings,plan-review-findings}.toml`, `.claude/{reviews,optimise-findings,plan-review-findings}/<scope>.toml` and `.claude/backlog.toml`.
  - `File` is read-only, used by `--once`, and infers its kind from the basename and array.
  - Both enums derive `Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord`: glimpse carries them in its `Debug`-deriving `Control` / `Event` and keys per-feed fingerprints on them.
- `ledger_read(root, &LedgerRef) -> Result<Value>` returns `{"path", "kind", "revision", "items": [...]}`.
  - `kind` is one of `review|optimise|plan-review|backlog`.
  - `revision` is the hex sha256 of the file bytes.
  - `items` is the `items` array (or the `backlog` array) converted with the crate's existing TOML→JSON conversion, which renders dates as `YYYY-MM-DD` strings.
  - A missing file reads as `{"items": [], "revision": null}`.
- `ledger_scopes(root) -> Result<Value>` returns `{"flows": [{"slug", "has_tasks", "ledgers": ["review", …]}], "scopes": [{"kind", "scope"}]}`. It covers every `.claude/flows/<slug>/` holding any artifact, plus every flow-less ledger file.
- `ledger_transition(root, &LedgerRef, ids, to: &str, fields: Map, expect_status: &str) -> Result<Value>` returns `{"applied": [ids], "skipped_stale": [{id, field, expected, found}]}`. Each id is written only if its current status equals `expect_status`. A call has one from-status, so the App splits a mixed selection (a `dismissed` + `resolved` backlog reopen) into one request per displayed status; the same applies to `backlog_triage`. The write sets the new status and its companions, removes the previous status's companions, and checks the result against `Item::validate`.
- `ledger_classify(root, &LedgerRef, ids, fields, expect: Map)` sets only `severity`, `effort` and `category`, on review/optimise only. The existing dedup recompute runs.
- `ledger_restore(root, &LedgerRef, id, set: Map, unset: Vec<String>, expect: Map)` is undo's primitive. It restores exactly the fields a glimpse write changed, if they still hold what glimpse wrote.
- `backlog_triage(root, ids, Dismiss { reason } | Reopen { rationale } | Resolve { resolution }, expect_status)` goes through the existing transition logic in `backlog/triage.rs`, refactored into a value-returning function that the CLI `dispatch` also calls. The CLI gains the same check as `backlog triage --expect-status <status>`, so `/backlog` guards its own late triage writes.

Transitions glimpse offers (the action menu lists only these):

| Ledger | From → to | Form fields |
|---|---|---|
| review | open → deferred | `defer_reason`, `defer_trigger` |
| review | open → wontfix | `wontfix_rationale` |
| review | open → verified-clean | `verified_note` |
| review, optimise | deferred → open | `reopen_rationale` (keeps `defer_reason`, drops `defer_trigger`, matching the disposition sweep) |
| optimise | open → deferred | `defer_reason`, `defer_trigger` |
| optimise | open → wontapply | `wontapply_rationale` |
| plan-review | open → discarded | `discard_reason` (new optional field) |
| backlog | open → dismissed | `dismiss_reason` |
| backlog | open → resolved | `resolution` |
| backlog | dismissed/resolved → open | `reopen_rationale` |

`fixed`, `applied` and `merged` stay with the apply flows and the plan merge. A transition away from `critical` severity asks for confirmation first.

### Stale-write precondition
- **Per-op precondition.** An `items apply` op may carry `"expect": {"<field>": <json value>, …}`, where `null` means the field must be absent. Both executors, `apply_op_indexed` and `apply_single_op` in `tomlctl/src/items.rs`, check it against the current row before an update or remove.
- **What happens on a mismatch.** This is governed by a new `StalePolicy`:
  - `Abort` (the default): the batch fails, naming every stale id, field, expected and found value.
  - `Skip`: the stale op is dropped, the rest apply, and the envelope gains `skipped_stale`.
- **The flag.** The CLI flag is `--on-stale <abort|skip>`. `compute_apply_mutation` keeps its signature and delegates to a new `compute_apply_mutation_with(…, StalePolicy)`, so `items_sweep.rs` is untouched.
- **Old-binary hazard.** An older binary ignores unknown op keys, so `expect` alone would be silently dropped. Carriers therefore always pass `--on-stale skip` together with `expect`, and an older binary fails loudly on the unknown flag. That failure lands at the run's last write, after the work is done, so the new surface is also advertised through `tomlctl capabilities` (features `items_apply_expect`, `backlog_triage_expect`, `inputs`) and tomlctl is released as 0.13.0; the orchestrator reinstalls it at checkpoints B and C (see Risks).
- **Carrier usage.** Every carrier status write carries `expect: {"status": <the status it read>}`. Each one reports any `skipped_stale` ids under a "changed during the run" line and never retries them:
  - `/review` Step 4 dispositions;
  - `/optimise`'s Interim-checkpoint `items apply` (reopened items) and its Step 3 updates to existing items — `/optimise` has no disposition step;
  - the apply pipeline's two-call write and its Interim-checkpoint `items apply` (the ≤3-item `items update` loop fallback is withdrawn for status transitions, since `items update` carries no `expect`);
  - `/backlog`'s per-cluster triage (`backlog triage --expect-status`);
  - rollback reversals (`expect` = this run's transition status);
  - the disposition-sweep reopen (`expect` = `deferred`);
  - the `/review-plan` merged/discarded writes.

### Input store
- **File.** `.claude/inputs.toml`, git-ignored with its sidecar (`.claude/inputs.toml*`). It holds `schema_version`, `last_updated` and an array named `inputs`, never `items`, since `items` writes stamp `dedup_id`. Logic lives in the new `tomlctl/src/inputs.rs`.
- **Row fields:**
  - `id`: `I{n}`, assigned under the lock as max+1.
  - `kind`: one of `capture | request | note | question | answer`.
  - `author`: `user`, or the command name for an agent-authored question.
  - `status`: one of `new | acknowledged | handled | withdrawn`.
  - `created`: a datetime.
  - **Target, optional:** `ledger` (`review|optimise|plan-review|backlog`), `flow`, `scope`, `items` (an id array).
  - **Body:** `text`.
  - **Captures:** `capture_kind` (a backlog kind hint), `area`.
  - **Questions:** `prompt`, `choice` (`single|multi|text`), `options`.
  - **Answers:** `answers` (the question id) and `picked` (an option array).
  - **Lifecycle:** `acknowledged`, `handled` (datetimes), `handled_by`, `handled_note`.
- **Lifecycle rules:**
  - Recording an answer moves its question to `handled` with `handled_by = "user"`.
  - Only the user withdraws, and only a `new` record.
  - An agent acknowledges a record when it reads it into a run and handles it once acted on, with a note saying what it did or why it declined.
- **CLI.** A new `tomlctl inputs` verb group:
  - `list [--pending] [--kind K]… [--ledger L] [--flow S] [--scope S] [--item ID]`;
  - `add --json <JSON|-|@path>`;
  - `ack <ids>… --by <command>`;
  - `handle <ids>… --by <command> --note <text>`;
  - `withdraw <ids>…`;
  - `answer <question-id> [--pick <option>]… [--text <t>]`.
- **Library facade:** `inputs_read`, `inputs_add`, `inputs_answer`, `inputs_withdraw`.
- **Consumers.** Each owning carrier's Step 0 lists pending inputs that target its ledger kind (and its flow or scope), acknowledges them, and handles each one before it finishes:
  - `/review` and `/review-apply`: review;
  - `/optimise` and `/optimise-apply`: optimise;
  - `/review-plan`: plan-review;
  - `/backlog`: backlog targets plus every `capture`. A capture becomes a real item through `backlog check` → `backlog add`, with kind and area settled first.
- **Questions.** When a carrier's `AskUserQuestion` gets an empty answer (a headless or auto run), it also posts the same question as a `question` record. Its next run's Step 0 reads the `answer` and acts on it.
- **Skill.** The contract lives in the new `flow-contract-user-inputs` skill.
- **Trust boundary.** `author` is self-declared, so every record is untrusted data. A `request` or `note` may drive only ledger operations on the items it targets — never a shell command, a file edit outside the ledger, a commit, or a settings or permission change — and sub-agents never run `tomlctl inputs add|answer|withdraw`. The writers are the user (glimpse or the CLI) and the orchestrator (questions, ack, handle).

### Agent attribution
- **The dispatch line.** Dispatch prompts gain one canonical line: `ledger: <repo-relative ledger path>`, optionally followed by ` items: R3,R7`. Lens agents of `/review`, `/optimise` and `/review-plan` carry the path only; apply implementers carry the path and their item ids. Only flow-local paths (`.claude/flows/<slug>/…`) can be attributed.
- **Matching** (`tomlctl/src/agents/correlate.rs`):
  - `dispatch_in` also matches, at any line of the prompt (the pattern is compiled with `(?m)`, because `dispatch_in` runs it over the whole prompt and apply prompts open with a `DISPATCH:` header), `^ledger: \.claude/flows/([a-z0-9][a-z0-9-]{0,63})/(review-ledger|optimise-findings|plan-review-findings)\.toml(?: items: ([ROP][0-9]+(?:,[ROP][0-9]+)*))?` on user-authored lines.
  - The pre-filter widens to `line.contains("tasks show") || line.contains("ledger: ")`.
  - `Dispatch` gains `item_ids: Vec<String>`.
- **Recording.** `Segment` gains `item_ids: Vec<String>` (serde default, omitted when empty), and the `record` output JSON gains `item_ids`. glimpse mirrors the field in `model.rs` and `hook.rs`.
- **Bounded stop-time read** (delivers `B-ad940fc4` and `B-f010d1ce`):
  - On Stop, `dispatch_and_tokens` reads only the last `TAIL_BYTES` of the transcript, whatever its size, and no longer falls back to `head_dispatch`. Idle keeps `latest_dispatch` (a whole read up to `WHOLE_READ_MAX`, with the head fallback), which still catches a teammate re-task followed by more than `TAIL_BYTES` of work.
  - A dispatch found in that tail (a re-task) wins.
  - Otherwise the event's ids are `None`, and `record` keeps the live row's segment, which was written at start or at the previous idle.
  - Start events keep today's read.

### Watching and feeds
- **Watch scopes.** `watch::start` takes several scopes on one `RecommendedWatcher`:
  - `.claude/flows`, recursive, as today;
  - `.claude`, non-recursive;
  - each existing `.claude/{reviews,optimise-findings,plan-review-findings}`, non-recursive.
- **`classify` uses an allowlist.**
  - `.claude/backlog.toml` and `.claude/inputs.toml` give `Wake::Repo(file)`.
  - A file in a flow-less ledger dir gives `Wake::Scope(kind)`.
  - Flow paths give `Wake::Flow(slug)` as today.
  - The creation of a flow-less ledger dir gives a new `Wake::NewScope(LedgerScopeDir)`: the poller's `Mode` adds one NonRecursive watch for it on the watcher it holds (through a `watch::add_scope` helper) and rescans that scope once. `Wake::Rewatch` keeps today's meaning (the watch is suspect: evict everything and re-create the watcher).
  - Ignored: `*.sha256`, `.tmp*`, the `.locks` and `worktrees` entries, and dir-entry modifies on `.claude/flows`.
- **Never `Wake::All`.** Repo-level paths never produce `Wake::All`, which would clear every cached flow stat and the task fingerprint. Each path is watched exactly once, because a duplicate `watch` on Windows leaks a handle and doubles events.
- **Per-scope miss counting.** `MissedChanges` keeps a streak per scope (flows, repo, each flow-less dir). A wake resets only its own scope. A safety-tick change found in a scope counts toward that scope. When any scope reaches `MISS_LIMIT`, glimpse switches to polling, as today.
- **Feeds.** The poller holds feeds: `Feed::Ledger(LedgerRef)` and `Feed::Inputs`. The runtime subscribes to:
  - the current flow's (or the selected flow-less scope's) ledgers;
  - the backlog;
  - inputs.

  Each feed keeps an `(mtime, len)` fingerprint and is re-read through `Fetcher::fetch_ledger` / `fetch_inputs` only when that changes. It then posts `Event::Ledger { feed, ledger }` / `Event::InputRecords(value)` (distinct from the existing key-input `Event::Input`). Every detector is reachable from `Poller::evict` or from the per-scan re-stat, as glimpse's rules require.
- **Flow listing.** `flows_fingerprint` stats every artifact, not just `tasks.toml`, so a ledger appearing triggers a relist. With each listing the poller also posts `Event::Scopes(value)`, the `tomlctl::ledger_scopes` output, from which the App takes the ledger-only flows and the flow-less scopes for the selector. `FlowEntry` and `flows::list` stay task-store-only, so startup, `--once` and auto-flow never pick a flow without `tasks.toml`; selecting one fetches no snapshot (the poller skips the fetch for a flow with no `tasks.toml` and reports "no task store" once, without the retry loop).

### Surfaces and interaction
- **The surfaces.** `Surface { Tasks, Review, Optimise, PlanReview, Backlog, Inbox }` live in `glimpse/src/surface.rs`. Each item surface owns an `ItemsState`:
  - `rows`;
  - `cursor: Option<String>`;
  - `marks: BTreeSet<String>`;
  - `filter`, `group`, `sort`;
  - `show_closed`;
  - `flashes`;
  - `new_since_view`;
  - `saving`.

  These are pure functions over the ledger rows. The u32 task selection model is left untouched.
- **Status classes** drive the glyphs and the closed filter:
  - live: `open`;
  - parked: `deferred`, `promoted`;
  - done: `fixed`, `applied`, `merged`, `resolved`, `verified-clean`;
  - declined: `wontfix`, `wontapply`, `discarded`, `dismissed`.
- **Keys** (context-free in `keys::map`; the App ignores those irrelevant to the current surface):
  - **Surfaces and movement:** `1`–`6` switch surface. `j`/`k` move. `Enter` opens details.
  - **Marks:** `Space` marks the item and advances. `V` marks every visible item. `Esc` clears marks before closing any panel.
  - **Viewing:** `/` opens the filter (live substring over id, summary, file and area). `g` cycles group-by: none, severity, category, effort, file, status, plus kind and area on Backlog. `S` cycles sort: id, severity, effort, newest. `c` shows or hides closed items.
  - **Actions:** `m` opens the action menu, which acts on the marks, or on the cursor row when nothing is marked. `e` opens the classify form (review and optimise only). `r` makes a request or note on the marked or cursor items. `n` captures a new item. `u` undoes glimpse's own last write.
  - **Inbox:** `Enter` on a question opens its answer form, and `w` withdraws one of your `new` records.
- **Arrivals.** Rows that appear or change status flash, and the cursor never moves. The header's surface tabs show live counts and a `+N` badge for changes since the surface was last viewed. glimpse never switches surface on its own.

### Forms
`glimpse/src/form.rs` is hand-rolled, about 300 lines, with no form crate. No ratatui-0.30 crate covers a radio list, checklist, dropdown and text field together without taking over the event loop.

```rust
pub(crate) enum Field {
    Text { label, input: tui_input::Input },
    Select { label, options, cursor, open, free: Option<tui_input::Input> },
    Multi { label, options, checked, cursor },
}
pub(crate) struct Form { title, prompt, fields, focus, error }
pub(crate) enum FormOutcome { Pending, Submit(Vec<FieldValue>), Cancel }
pub(crate) enum FieldValue { Text(String), One(String), Many(Vec<String>) }
```

**Key routing:**
- `Form::handle_key(KeyEvent) -> FormOutcome`.
- `Tab` / `BackTab` move focus.
- `Esc` closes an open dropdown, otherwise cancels.
- `Enter` opens or confirms a dropdown, otherwise submits once required fields are non-empty.
- `j`/`k`/arrows move inside a Select or Multi. `Space` toggles in a Multi.
- Every other key in a Text field goes to `tui_input::backend::crossterm::EventHandler::handle_event`.

`Select.free` is the "other…" free-form value, used for category.

**Rendering.** `glimpse/src/view/form.rs` renders a centred `Clear` + `Block` modal, as `view/selector.rs` does. The text cursor is placed with `Frame::set_cursor_position`.

**Routing in the runtime.** While a form or prompt is open, `runtime::handle` sends every key to it before `keys::map`, so `j` or `q` typed into a field never moves or quits.

**Dependency.** `tui-input = "0.15.5"` with default features; no new crossterm.

### Writer thread
- **The thread.** `glimpse/src/writer.rs` runs one serial thread. It owns the facade calls, because a write may wait up to 30 s for the lock and must never run on the UI thread or the poller.
- **Requests.** `WriteRequest` is one of `Transition`, `Classify`, `Restore`, `BacklogTriage`, `InputAdd`, `InputAnswer`, `InputWithdraw`, each carrying a request id. Results arrive as `Event::Written(WriteOutcome { request, applied, skipped_stale, error })` on the existing event channel.
- **No optimistic update.** The App marks affected rows `saving` and clears the mark when the feed's next read reflects the change, or when the outcome reports an error or a stale skip; those show as a footer notice.
- **Undo.** Each submitted control edit pushes one undo entry holding, per id it applied, the fields to restore and the fields glimpse wrote (used as `expect`). `u` pops it into one `Restore` per applied id and reports any that come back stale. An input-record write is undone by withdrawing the record.

### Carrier changes
- **Stale guard.** Carriers adopt `expect` + `--on-stale skip` at every late status write (see "Stale-write precondition").
- **Dispatch line.** Carriers emit the `ledger:` dispatch line (see "Agent attribution").
- **Input sweep.** Carriers run the Step-0 input sweep and post a question record on an empty answer (see "Input store").
- **Policy text.** Policy text that names a sole writer is amended: the orchestrator remains the only *agent* writer, and the human writes control fields through glimpse and input records through the store.
- **Shared block.** The shared block `backlog-candidates` in `claude/agents/implement-*.md` is about sub-agents and stays byte-identical, untouched.

## Success Criteria
- forward: the optimise `wontapply` companion is `wontapply_rationale` in the validator — `grep -c '"wontfix" | "wontapply" => &\["wontfix_rationale"\]' tomlctl/src/items.rs` prints `0` (today: `1`).
- forward: every late carrier status write uses the stale guard — `grep -l -- '--on-stale skip' claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/skills/flow-contract-apply-pipeline/references/verification.md claude/skills/flow-contract-apply-rollback-protocol/SKILL.md claude/skills/flow-contract-ledger-disposition-sweep/SKILL.md | wc -l` prints `6` (today: `0`).
- forward: all six owning carriers invoke the input contract — `grep -l 'flow-contract-user-inputs' claude/commands/review.md claude/commands/review-apply.md claude/commands/optimise.md claude/commands/optimise-apply.md claude/commands/review-plan.md claude/commands/backlog.md | wc -l` prints `6` (today: `0`).
- forward: the input store is never committed — `git check-ignore -q .claude/inputs.toml` exits 0 (today: exits 1).
- forward: glimpse renders a review ledger headlessly — `cargo test --manifest-path glimpse/Cargo.toml --test cli once_renders_a_review_ledger` passes. **predicted, unverified** (needs a build).
- forward: a review lens dispatch is attributed to its flow — `cargo test --manifest-path tomlctl/Cargo.toml --test agents_record a_ledger_dispatch_records_its_flow_and_items` passes. **predicted, unverified**.
- forward: a stale carrier write is skipped, not applied — `cargo test --manifest-path tomlctl/Cargo.toml --test integration items_apply_on_stale_skip_reports_skipped_ids` passes. **predicted, unverified**. falsifier: ignoring `expect` under `--on-stale skip` makes it fail.
- forward: glimpse's triage write is compare-and-set guarded — `cargo test --manifest-path tomlctl/Cargo.toml --test library_facade ledger_transition_skips_a_changed_status` passes. **predicted, unverified**. falsifier: writing without comparing the current status makes it fail.
- forward: an answered agent question closes — `cargo test --manifest-path tomlctl/Cargo.toml --test inputs inputs_answer_marks_the_question_handled` passes. **predicted, unverified**.
- forward: arrivals flash without moving the cursor — `cargo test --manifest-path glimpse/Cargo.toml a_new_row_flashes_and_keeps_the_cursor` passes. **predicted, unverified**.
- guard: `bash scripts/verify-shared-blocks.sh` passes (no shared block drifts).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml && cargo build --manifest-path glimpse/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast && cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast
test.timeout: 1500
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets && cargo clippy --manifest-path glimpse/Cargo.toml --all-targets && cargo fmt --manifest-path tomlctl/Cargo.toml -- --check && cargo fmt --manifest-path glimpse/Cargo.toml -- --check
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: test "$(grep -c '"wontfix" | "wontapply" => &\["wontfix_rationale"\]' tomlctl/src/items.rs)" -eq 0
success: test "$(grep -l -- '--on-stale skip' claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/skills/flow-contract-apply-pipeline/references/verification.md claude/skills/flow-contract-apply-rollback-protocol/SKILL.md claude/skills/flow-contract-ledger-disposition-sweep/SKILL.md | wc -l)" -eq 6
success: test "$(grep -l 'flow-contract-user-inputs' claude/commands/review.md claude/commands/review-apply.md claude/commands/optimise.md claude/commands/optimise-apply.md claude/commands/review-plan.md claude/commands/backlog.md | wc -l)" -eq 6
success: git check-ignore -q .claude/inputs.toml
success: bash scripts/verify-shared-blocks.sh
success: cargo test --manifest-path glimpse/Cargo.toml --test cli once_renders_a_review_ledger
success: cargo test --manifest-path tomlctl/Cargo.toml --test agents_record a_ledger_dispatch_records_its_flow_and_items
success: cargo test --manifest-path tomlctl/Cargo.toml --test integration items_apply_on_stale_skip_reports_skipped_ids
success: cargo test --manifest-path tomlctl/Cargo.toml --test library_facade ledger_transition_skips_a_changed_status
success: cargo test --manifest-path tomlctl/Cargo.toml --test inputs inputs_answer_marks_the_question_handled
success: cargo test --manifest-path glimpse/Cargo.toml a_new_row_flashes_and_keeps_the_cursor
```

The pre-commit hook also runs the `cli::dispatch::tests` gates when `claude/**/*.md` or `tomlctl/src/**` is staged, and runs glimpse `clippy --locked` when `tomlctl/src/lib.rs` is staged. A `tomlctl` change that alters the lockfile needs `glimpse/Cargo.lock` refreshed in the same commit. Manual smoke after Milestone C: run `glimpse` beside a `/review` and confirm findings flash in on the Review surface; then defer one from the action menu and see it written with `defer_reason` / `defer_trigger`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 6, 7, 24, 34, 35, 36, 37, 38, 39, 46, 47, 53, 57, 58, 59, 60
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### Milestone A — read-only surfaces and attribution

### 1. Scaffold the new glimpse modules [L]
- **Files**: `glimpse/src/main.rs`, `glimpse/src/view/mod.rs`, `glimpse/src/ledger.rs` (new), `glimpse/src/surface.rs` (new), `glimpse/src/writer.rs` (new), `glimpse/src/form.rs` (new), `glimpse/src/actions.rs` (new), `glimpse/src/view/items.rs` (new), `glimpse/src/view/form.rs` (new), `glimpse/src/view/inbox.rs` (new)
- **Depends on**: —
- **Action**: Create each new module file holding only a one-line module doc comment. Declare the top-level ones in `glimpse/src/main.rs` (`mod actions; mod form; mod ledger; mod surface; mod writer;`, in alphabetical order among the existing declarations) and the view ones in `glimpse/src/view/mod.rs` (`pub(crate) mod form; pub(crate) mod inbox; pub(crate) mod items;`).
- **Detail**: Edits are inseparable: a `mod` line without its file does not compile. Each doc comment states the module's purpose from Approach, "Surfaces and interaction", "Forms" and "Writer thread", in one line. This scaffold exists so that later tasks own disjoint files.
- **Acceptance**:
  - forward: `grep -cE '^mod (actions|form|ledger|surface|writer);' glimpse/src/main.rs` prints `5` (today: `0`).
  - guard: `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets` reports no warning in a new file. **predicted, unverified**.

### 2. Split glimpse's source module into scan and mode submodules [M]
- **Files**: `glimpse/src/source.rs`, `glimpse/src/source/scan.rs` (new), `glimpse/src/source/mode.rs` (new)
- **Depends on**: —
- **Action**: Move, without behaviour change, two groups out of `glimpse/src/source.rs`, keeping `source.rs` as the module root:
  - **Into `source/scan.rs`:** the flow-file scanning and fetching code (`Fingerprint`, `flow_dir`, `fingerprint`, `FlowStat`, `modified`, `flows_fingerprint`, `FlowsChange`, `task_store_mtimes`, the `Fetcher` trait, `InProcessFetcher`, `with_reinstall_hint`).
  - **Into `source/mode.rs`:** the watch-mode machinery (`Wakes`, `Mode`, `Backoff`, `WatchState`, `Phase`, `ModeChange`, `mode_change`, `safety_period`, `Deadlines`, `Due`, `next_wait`, `TickCause`, `MissedChanges`).

  `Event`, `Control`, `Poller` and `Source` stay in `source.rs`.
- **Detail**: Each unit test moves with the item it tests. `FakeFetcher` and the temp-root helpers go to `scan.rs` tests and are re-exported to `source.rs` tests as `pub(super)` test helpers. Visibility rises only as far as `pub(super)`. Every existing test name survives.
- **Acceptance**:
  - forward: `test -f glimpse/src/source/scan.rs && test -f glimpse/src/source/mode.rs && echo ok` prints `ok` (today: nothing).
  - guard: `cargo test --manifest-path glimpse/Cargo.toml source::` passes, and `cargo test --manifest-path glimpse/Cargo.toml source:: -- --list | grep -c ': test$'` prints the same count before and after the move. **predicted, unverified**.
  - falsifier: dropping a moved test (rather than moving it) lowers that count.

### 3. Add the tomlctl ledger read facade [M]
- **Files**: `tomlctl/src/ledgers.rs` (new), `tomlctl/src/lib.rs`, `tomlctl/tests/library_facade.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/ledgers.rs` with `LedgerKind`, `LedgerRef`, path resolution, `read(root, &LedgerRef)` and `scopes(root)`. Declare the module and export `LedgerKind`, `LedgerRef`, `ledger_read` and `ledger_scopes` from `tomlctl/src/lib.rs`, following the existing wrappers (`io::silence_advisories()` first, explicit `root`).
- **Detail**: Shapes and paths are in Approach, "Ledger contract".
  - `LedgerKind` and `LedgerRef` derive `Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord`; glimpse tasks cannot add derives to tomlctl.
  - Use the crate's existing TOML→JSON conversion so dates come out as `YYYY-MM-DD` strings.
  - `revision` is the sha256 of the bytes read.
  - `LedgerRef::File(path)` reads any path, and is used by glimpse `--once --ledger`.
  - The backlog's array is `backlog`; every other ledger's is `items`.
  - `scopes` lists `.claude/flows/*/` that hold any of `tasks.toml`, `review-ledger.toml`, `optimise-findings.toml` or `plan-review-findings.toml`, plus each `*.toml` (not `*.sha256`) under the three flow-less dirs.
- **Acceptance**:
  - forward: `library_facade.rs` gains:
    - `ledger_read_returns_rows_and_revision` (a fixture review ledger round-trips its ids and a stable revision);
    - `ledger_read_of_a_missing_file_is_empty`;
    - `ledger_scopes_lists_ledger_only_flows_and_flowless_scopes`.

    All three pass. **predicted, unverified**.
  - falsifier: returning `items` from the `backlog` array makes the backlog case of `ledger_read_returns_rows_and_revision` fail.

### 4. Attribute ledger dispatches and bound the stop-time transcript read [M]
- **Files**: `tomlctl/src/agents/correlate.rs`, `tomlctl/src/agents/record.rs` — only the test-only `Dispatch { .. }` literal, which the new field breaks
- **Depends on**: —
- **Backlog**: B-ad940fc4, B-f010d1ce
- **Action**:
  - Extend `Dispatch` with `item_ids: Vec<String>` and teach `dispatch_in` the `ledger:` line from Approach, "Agent attribution".
  - Widen the cheap pre-filter.
  - Make `dispatch_and_tokens` (and its Codex twin) read only the last `TAIL_BYTES` of the transcript, whatever the file's size, with no `head_dispatch` fallback.
- **Detail**:
  - A `tasks show` dispatch and a `ledger:` dispatch on the same line must name the same slug, or the line gives `None`, as for disagreeing slugs today.
  - Item ids are deduplicated and kept in first-seen order.
  - Compile the `ledger:` pattern with `(?m)`: `dispatch_in` runs it over the whole prompt, and an apply prompt opens with its `DISPATCH:` header, so the line is never at offset 0.
  - Add `item_ids: Vec::new()` to the test-only `Dispatch` literal in `tomlctl/src/agents/record.rs` so the crate's tests still compile; task 5 threads the field.
  - Add a bounded `read_last(path, TAIL_BYTES)` that seeks to `len - TAIL_BYTES` when larger and drops the partial first line. Leave `latest_dispatch` (the start path) on `read_tail` with its head fallback.
- **Acceptance**:
  - forward: new unit tests pass. **predicted, unverified**:
    - `a_ledger_line_names_its_flow`;
    - `a_ledger_line_with_items_collects_the_item_ids`;
    - `a_flowless_ledger_line_is_not_a_dispatch`;
    - `a_ledger_line_below_other_prompt_text_is_still_a_dispatch`;
    - `a_stop_reads_only_the_tail_of_a_large_transcript` (a transcript larger than `WHOLE_READ_MAX` with its dispatch only on line 1 gives `None` on Stop);
  - guard: `a_teammate_retask_supersedes_the_earlier_dispatch` and `a_spawn_prompt_names_its_flow_and_task` still pass.
  - falsifier: restoring the `head_dispatch` fallback in `dispatch_and_tokens_with` makes `a_stop_reads_only_the_tail_of_a_large_transcript` fail.
  - falsifier: dropping `(?m)` from the `ledger:` pattern makes `a_ledger_line_below_other_prompt_text_is_still_a_dispatch` fail.

### 5. Record item ids on agent segments [M]
- **Files**: `tomlctl/src/agents/schema.rs`, `tomlctl/src/agents/record.rs`, `tomlctl/tests/agents_record.rs`
- **Depends on**: 4
- **Backlog**: B-ad940fc4, B-f010d1ce
- **Action**:
  - Add `item_ids: Vec<String>` to `Segment` in `tomlctl/src/agents/schema.rs`: TOML read and write, omitted when empty, and `segment_from_toml` accepting a string array.
  - Thread `Dispatch.item_ids` through `Event::Start`, `Stop` and `Idle` and `apply` in `tomlctl/src/agents/record.rs`, as `task_ids` is. A changed id set closes and opens a segment exactly as a changed task set does.
  - Add `item_ids` to the `record` output JSON.
- **Detail**:
  - `choose_flow` already prefers the dispatch slug.
  - The `task_ids`-drop rule (dispatch slug ≠ chosen flow) applies to `item_ids` too.
  - A Stop whose bounded read found no dispatch passes `None` and keeps the live segment (Approach, "Agent attribution").
- **Acceptance**:
  - forward: `tomlctl/tests/agents_record.rs` gains two tests that pass. **predicted, unverified**:
    - `a_ledger_dispatch_records_its_flow_and_items`: a SubagentStart whose transcript's first user line is `ledger: .claude/flows/<slug>/review-ledger.toml items: R3,R7` writes a running row with segment `item_ids = ["R3","R7"]` in that flow's `agents.toml`;
    - `a_stop_without_a_tail_dispatch_keeps_the_live_segment`.
  - guard: `a_start_records_a_running_row_on_the_dispatched_task` still passes.
  - falsifier: omitting `item_ids` from `Segment`'s TOML write makes `a_ledger_dispatch_records_its_flow_and_items` fail.

### 6. Emit the ledger line in review and optimise lens dispatches [S]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`
- **Depends on**: 4
- **Action**: In each carrier's per-lens dispatch preamble (`claude/commands/review.md` near the "the ledger path" preamble text; `claude/commands/optimise.md` where it supplies "the ledger path"), require the canonical line `ledger: <ledger path>` on a line of its own at the top of every lens prompt. For a flow-less ledger, the line is still emitted; it simply goes unattributed.
- **Detail**: The format is fixed by Approach, "Agent attribution". Do not add item ids for lens agents. Keep each existing `Invoke the \`…\` skill` phrase intact, because `carrier_invokes_required_skills` gates them.
- **Acceptance**: forward: `grep -c 'ledger: <' claude/commands/review.md claude/commands/optimise.md` prints a count of 1 or more after each path (`claude/commands/review.md:N`, `claude/commands/optimise.md:N`; today: `0` each).

### 7. Emit the ledger line in review-plan and apply dispatches, and document attribution [M]
- **Files**: `claude/commands/review-plan.md`, `claude/skills/flow-contract-apply-pipeline/references/agent-prompt-contract.md`, `claude/skills/tomlctl/references/agents.md`
- **Depends on**: 4, 5
- **Action**:
  - Add the `ledger: <findings path>` line to `/review-plan`'s lens prompts.
  - Add `ledger: <ledger path> items: <comma-separated ids>` to the apply agent prompt contract's required preamble.
  - Document in `claude/skills/tomlctl/references/agents.md`:
    - the ledger-line attribution key;
    - the `item_ids` segment field and output key;
    - the bounded stop-time read;
    - the rule that flow-less ledgers go unattributed.
- **Detail**: Follow Approach, "Agent attribution". The agents.md reference must stay under 600 lines. Any bash-fenced `tomlctl` line must parse under `command_lint`.
- **Acceptance**:
  - forward: `grep -c 'ledger: ' claude/skills/flow-contract-apply-pipeline/references/agent-prompt-contract.md` prints `1` or more (today: `0`).
  - forward: `grep -c 'item_ids' claude/skills/tomlctl/references/agents.md` prints `1` or more (today: `0`).

### 8. Watch repo-level ledger scopes [M]
- **Files**: `glimpse/src/watch.rs`, `glimpse/src/source/mode.rs`
- **Depends on**: 2
- **Action**:
  - Change `watch::start` to register the scopes and the allowlist `classify` from Approach, "Watching and feeds".
  - Add `Wake::Repo(RepoFile)` (with `RepoFile::{Backlog, Inputs}`), `Wake::Scope(LedgerScopeDir)` and `Wake::NewScope(LedgerScopeDir)`, plus a `watch::add_scope(&mut RecommendedWatcher, &Path)` helper.
  - In `glimpse/src/source/mode.rs`, move the consumers onto the new watch: `Wakes` gains repo and scope fields with `Wakes::add` / `is_empty` arms for the new variants (the match is exhaustive), `Mode::start` calls the multi-scope `watch::start`, and `Mode::Watching` keeps its watcher usable so `Wake::NewScope` adds one watch to it.
- **Detail**:
  - Keep the callback non-blocking: it only posts a wake.
  - Match on the final file name. Ignore `*.sha256`, `.tmp*`, the `.locks` and `worktrees` entries, and a bare `Modify` on the `.claude/flows` directory entry.
  - Never call `watch` twice on one path.
  - A missing flow-less dir is skipped and is not an error.
  - The Windows backend reports a missing path as a generic error rather than `PathNotFound`, so check existence first.
- **Acceptance**:
  - forward: new `watch.rs` unit tests pass. **predicted, unverified**:
    - `a_backlog_write_wakes_the_repo_scope_not_all`;
    - `a_lock_file_event_is_ignored`;
    - `a_sidecar_or_temp_name_is_ignored`;
    - `creating_a_flowless_dir_asks_for_a_new_scope`.
  - guard: the existing `classify` tests for flow paths still pass.
  - falsifier: mapping a non-flow path to `Wake::All` (today's `classify`) makes `a_backlog_write_wakes_the_repo_scope_not_all` fail.

### 9. Count missed changes per watch scope [M]
- **Files**: `glimpse/src/source/mode.rs`
- **Depends on**: 2, 8
- **Action**: Make `MissedChanges` keep one streak per scope (flows, repo, each flow-less dir). A wake resets only its own scope's streak, and a safety-tick change counts against the scope it was found in. `on_tick` reports abandonment when any scope reaches `MISS_LIMIT`.
- **Detail**: Approach, "Watching and feeds". Keep `TickCause` semantics. The poller (task 11) supplies which scopes woke and which scopes a safety tick found changed.
- **Acceptance**:
  - forward: unit test `a_busy_scope_does_not_mask_a_dead_one` passes: repo wakes keep arriving while the flows scope misses twice, and that abandons. **predicted, unverified**.
  - falsifier: resetting every scope on any wake (today's behaviour) makes that test fail.

### 10. Add the glimpse ledger model [M]
- **Files**: `glimpse/src/ledger.rs`, `glimpse/tests/fixtures/review-ledger.toml` (new), `glimpse/tests/fixtures/backlog.toml` (new)
- **Depends on**: 1, 3
- **Action**: Implement lenient `#[serde(default)]` row types for review, optimise, plan-review and backlog rows, parsed from the `ledger_read` JSON. Add a unified `ItemRow` view model with:
  - id, summary, description, status and status class;
  - severity, or kind for backlog; category; effort;
  - anchor: `file:line:symbol`, `plan_section` or `area`;
  - dates, rationale and companion fields;
  - tags, evidence, instances;
  - the raw JSON, kept for details.

  Add a `Ledger { kind, revision, rows }` loader over a `serde_json::Value`.
- **Detail**:
  - Unknown statuses keep their text with the status class `live`.
  - Rows without an `id` (plan-review) get a synthetic `#<index>` id, flagged read-only.
  - Fixtures are real-shaped TOML: about 6 review rows covering every status, and about 4 backlog rows. Tests load them through `tomlctl::ledger_read(root, &LedgerRef::File(..))`.
- **Acceptance**: forward: unit tests pass. **predicted, unverified**:
  - `review_fixture_maps_every_status_to_its_class`;
  - `a_plan_review_row_without_an_id_is_read_only`;
  - `backlog_rows_carry_kind_and_area`.

### 11. Feed ledgers through the poller [M]
- **Files**: `glimpse/src/source.rs`, `glimpse/src/source/scan.rs`, `glimpse/src/runtime.rs` — placeholder `Event::Ledger` / `Event::Scopes` arms only; task 21 replaces them, `glimpse/src/source/mode.rs`
- **Depends on**: 2, 3, 9, 10
- **Action**:
  - Add `Feed`, `Control::Subscribe(Vec<Feed>)` with a `pub(crate) fn subscribe(&self, feeds: Vec<Feed>)` on `Source` beside `set_slug` / `set_tail` (`Control` stays private), `Event::Ledger { feed, ledger }` and `Fetcher::fetch_ledger` (`InProcessFetcher` calls `tomlctl::ledger_read`).
  - Track per-feed `(mtime, len)` fingerprints, re-read on change, and post an event only when the revision differs.
  - Route `Wake::Repo` and `Wake::Scope` to their feeds, and feed the per-scope miss counter.
  - Make `flows_fingerprint` stat any artifact, and with each flow listing post `Event::Scopes(value)` from a new `Fetcher::list_scopes` (`tomlctl::ledger_scopes`).
  - Skip the snapshot fetch for a flow with no `tasks.toml`: post one `Event::SourceError("no task store")` and do not enter the `RETRY_AFTER` loop.
  - Add `Event::Ledger { .. } | Event::Scopes(_) => Step::Nothing` to `runtime::handle` in `glimpse/src/runtime.rs` so the crate compiles; task 21 routes them.
- **Detail**:
  - Approach, "Watching and feeds".
  - `Wake::Repo` and `Wake::Scope` never evict the flow caches.
  - Every new detector is reachable from `Poller::evict` or the per-scan re-stat.
  - Extend `FakeFetcher` with scripted ledger results.
  - `Feed::Inputs` is declared but unfetched until task 49.
- **Acceptance**:
  - forward: unit tests pass. **predicted, unverified**:
    - `a_ledger_change_posts_one_event`;
    - `an_unchanged_ledger_is_not_refetched`;
    - `a_backlog_wake_keeps_the_task_fingerprint`;
    - `a_ledger_only_flow_posts_its_scopes`;
    - `a_flow_without_a_task_store_is_not_snapshotted`.
  - guard: `fingerprint_changes_when_a_flow_file_changes` and `an_unchanged_fingerprint_does_not_refetch` still pass.
  - falsifier: re-reading every feed on every tick makes `an_unchanged_ledger_is_not_refetched` fail.

### 12. List ledger-only flows and flow-less scopes in the selector [M]
- **Files**: `glimpse/src/flows.rs`, `glimpse/src/view/selector.rs`, `glimpse/src/app.rs`
- **Depends on**: 3, 11, 15
- **Action**:
  - Add `flows::Scopes::from_value(&Value)`, parsing the `tomlctl::ledger_scopes` output into ledger-only flow slugs and flow-less `ScopeEntry { kind, scope }` rows. `flows::list`, `FlowEntry`, `rank` and `freshest` stay task-store-only.
  - Render the selector as flows with a task store, then ledger-only flows, then a separator, then `review: <scope>` / `optimise: <scope>` / `plan-review: <scope>` rows.
  - In `glimpse/src/app.rs`, hold the `Scopes` beside `flows` with an `App::apply_scopes`. Enter on a ledger-only flow returns `SwitchFlow` as today; Enter on a scope entry returns a new `Action::SwitchScope(kind, scope)` from `App::apply`, as `SwitchFlow` is returned today, and sets `App.scope`.
- **Detail**: User Decision 12; Approach, "Watching and feeds" (Flow listing). A ledger-only flow shows a muted "no tasks" marker and is never chosen as the freshest flow, so startup, `--once` and auto-flow keep ranking only flows with a task store. Cursor movement skips the separator. The runtime routes `Event::Scopes` and `SwitchScope` in task 21.
- **Acceptance**:
  - forward: unit tests `flows_without_a_task_store_are_listed` and `the_selector_lists_flowless_scopes_after_a_separator` pass. **predicted, unverified**.
  - guard: `freshest` still ignores a flow without `tasks.toml` (existing `flows.rs` ranking tests pass).

### 13. Add item, input and form theme tokens [S]
- **Files**: `glimpse/src/theme.rs`
- **Depends on**: —
- **Action**: Add element tokens to `TOKENS`, each defaulting to a palette token, plus `Theme::severity(&str)` and `Theme::item_status(class)` helpers:
  - `severity_critical`→`danger`, `severity_warning`→`warning`, `severity_suggestion`→`info`;
  - `item_live`→`fg`, `item_parked`→`idle`, `item_done`→`success`, `item_declined`→`muted`;
  - `item_mark`→`accent`, `item_saving`→`warning`;
  - `badge`→`accent`, `surface_tab`→`secondary`, `surface_tab_active`→`accent`;
  - `facet`→`muted`, `group_header`→`section`;
  - `input_new`→`info`, `input_acknowledged`→`warning`, `input_handled`→`success`;
  - `question`→`violet`;
  - `form_label`→`secondary`, `form_focus`→`accent`, `form_error`→`danger`.
- **Detail**: glimpse's rule is that colours come only from `TOKENS`. The README token rows are added by tasks 24 and 53.
- **Acceptance**:
  - forward: `grep -c '"severity_critical"' glimpse/src/theme.rs` prints `1` (today: `0`).
  - guard: theme unit tests pass. **predicted, unverified**.

### 14. Add the surface and item-list state machine [M]
- **Files**: `glimpse/src/surface.rs`
- **Depends on**: 1, 10
- **Action**: Implement `Surface` (with `ALL`, digit mapping and labels), `ItemsState`, `Group`, `Sort`, `VisibleRow::{Header{label,count}, Item(id)}`, plus:
  - `apply_ledger(rows, revision, viewing)`: computes flashes for new or changed-status ids, bumps `new_since_view` when not viewing, keeps the cursor id, and clears `saving` ids whose rows changed;
  - `visible()`: filter, closed toggle, group, sort;
  - `move_cursor`, `toggle_mark` (marks and advances), `mark_visible`, `clear_marks`, `targets()` (marks, or the cursor row);
  - `cycle_group` and `cycle_sort` (per-surface option lists).
- **Detail**: Approach, "Surfaces and interaction". Pure code: no IO and no ratatui. `Surface::Inbox` exists from the start with an empty state.
- **Acceptance**: forward: unit tests pass. **predicted, unverified**:
  - `a_new_row_flashes_and_keeps_the_cursor`;
  - `marks_survive_a_ledger_refresh_but_drop_vanished_ids`;
  - `grouping_by_severity_orders_critical_first`;
  - `closed_rows_are_hidden_until_toggled`;
  - `targets_fall_back_to_the_cursor_row`.
  - falsifier: moving the cursor to the first new row in `apply_ledger` makes `a_new_row_flashes_and_keeps_the_cursor` fail.

### 15. Integrate surfaces into the App [M]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/state.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 14
- **Action**:
  - Add `surface`, a per-surface `ItemsState` map, and `scope: Option<(LedgerKind, String)>` to `App`.
  - Add the actions `SwitchSurface(Surface)`, `ItemMove(Dir)`, `SelectItem(String)`, `ToggleMark`, `MarkVisible`, `CycleGroup`, `CycleSort`, `ToggleClosed`.
  - Add `items: Vec<(Rect, String)>` to `Regions`, so task 18 can record item-row hit regions and task 16 can map a click on one to `SelectItem`.
  - Add `App::apply_ledger(kind, ledger)`.
  - Route `Move` and `Details` by surface.
  - Make `Back` clear marks after closing any form or prompt and before the existing overlay chain.
  - Persist `surface`, plus `group` and `sort` per surface, in `glimpse/src/state.rs`.
- **Detail**:
  - The task selection model and `App::new(Snapshot, &Config)` stay unchanged.
  - The surfaces start with empty states.
  - Details on an item surface shows the cursor row.
  - `state.rs` field lists are hand-maintained in parse/render/capture/apply/save, so update all five.
- **Acceptance**: forward: unit tests pass. **predicted, unverified**:
  - `switching_surface_keeps_the_task_selection`;
  - `back_clears_marks_before_closing_details`;
  - `state_round_trips_surface_group_and_sort`.

### 16. Map the surface keys [S]
- **Files**: `glimpse/src/keys.rs`
- **Depends on**: 15
- **Action**: Map the following in `keys::map`, keeping it context-free and dropping `Release` events as today:
  - `1`–`6` → `SwitchSurface`;
  - `Space` → `ToggleMark`;
  - `V` → `MarkVisible`;
  - `g` → `CycleGroup`;
  - `S` → `CycleSort`;
  - `c` → `ToggleClosed`.

  In `keys::mouse`, map a left click inside a `Regions.items` rect to `SelectItem(id)`.
- **Detail**: No existing binding collides. `s` stays the flow selector and `a` stays auto-flow.
- **Acceptance**:
  - forward: unit tests `digits_switch_surfaces_and_space_marks` and `a_click_on_an_item_row_selects_it` pass. **predicted, unverified**.
  - guard: existing key tests pass.

### 17. Mirror item ids in glimpse's agent model [S]
- **Files**: `glimpse/src/model.rs`, `glimpse/src/hook.rs`
- **Depends on**: 5
- **Action**:
  - Add `item_ids: Vec<String>` (serde default) to `Segment` in `glimpse/src/model.rs`, plus an `Index` lookup returning the running agents whose newest segment names a given item id.
  - Add `item_ids: Vec<String>` to `RecordResult` in `glimpse/src/hook.rs`.
- **Detail**: Approach, "Agent attribution".
- **Acceptance**: forward: unit test `running_agents_are_found_by_item_id` passes. **predicted, unverified**.

### 18. Render the item list view [M]
- **Files**: `glimpse/src/view/items.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 1, 13, 15, 17
- **Action**:
  - Implement `view::items::render`:
    - a facet row (`group <g> · sort <s> · closed shown|hidden · /<filter>`);
    - group header rows `▾ warning (5)`;
    - item rows: mark, status glyph in the status-class token, id, severity chip (or kind), category, effort, summary, dimmed `file:line` right-aligned;
    - a running-agent mark from the item-id lookup;
    - a saving mark;
    - row flashes.
  - Windowing follows `layers.rs`'s row helpers.
  - In `glimpse/src/view/mod.rs`, dispatch the body by surface (Tasks unchanged) and add per-surface footer hints.
  - Fill `app.regions.items` (added by task 15) with each drawn item row's rect and id; task 16 maps the click.
- **Detail**: Approach, "Surfaces and interaction". The Inbox surface draws a placeholder line until task 50. Keep frame tests in the substring style.
- **Acceptance**: forward: view tests `the_review_surface_lists_rows_grouped_by_severity` and `a_marked_row_shows_its_mark` pass. **predicted, unverified**.

### 19. Show surface tabs and arrival badges in the header [S]
- **Files**: `glimpse/src/view/header.rs`
- **Depends on**: 13, 15
- **Backlog**: B-5c699ce5
- **Action**:
  - Render surface tabs (`Tasks  Review 12 +3  Optimise 4  Plan-review —  Backlog 6  Inbox 2?`): live counts, a `+N` badge from `new_since_view`, the active tab in `surface_tab_active`, and Inbox's unanswered-question count with `?`.
  - Replace the stale `"tomlctl failed"` fixture text in the header tests with a representative in-process read/parse error message.
- **Detail**: In compact density, tabs collapse to `R12+3 O4 P— B6 I2?`.
- **Acceptance**:
  - forward: `grep -c 'tomlctl failed' glimpse/src/view/header.rs` prints `0` (today: `2`).
  - forward: header test `surface_tabs_show_counts_and_badges` passes. **predicted, unverified**.

### 20. Render item details [M]
- **Files**: `glimpse/src/view/details.rs`
- **Depends on**: 10, 15, 17
- **Action**:
  - Split `render` so the scroll and wrap frame takes a `Text`.
  - Keep `content(app, now)` for tasks, and add `item_content(row, agents)`:
    - a summary heading;
    - the status and its companion fields;
    - classification;
    - the anchor;
    - the description through `markdown::to_text`;
    - evidence and instances lists;
    - dates and rounds;
    - running agents.
  - Choose by surface.
- **Detail**: The pending-input list joins in task 52.
- **Acceptance**:
  - forward: test `item_details_show_the_wontfix_rationale` passes. **predicted, unverified**.
  - guard: existing details tests pass.

### 21. Wire surfaces through the runtime [M]
- **Files**: `glimpse/src/runtime.rs`, `glimpse/src/main.rs`
- **Depends on**: 11, 12, 15, 18, 19, 20
- **Action**:
  - On flow switch, scope switch and start, call `Source::subscribe` with the current flow's (or scope's) three ledgers, plus the backlog.
  - Route `Event::Ledger` to `App::apply_ledger`, redrawing only when the event's surface is the one showing or a badge changed, replacing task 11's placeholder arm.
  - Route `Event::Scopes` to `App::apply_scopes`, redrawing only while the selector is open, and route `SwitchScope`.
  - Extend `render_once` to take an optional `(Surface, Ledger)`, and update its caller in `glimpse/src/main.rs` to pass `None`.
- **Detail**: Follow the 9ed739b redraw-gating pattern. Update `App::tick_interval` if flashes need ticks while not following.
- **Acceptance**:
  - forward: runtime tests `a_ledger_event_for_a_hidden_surface_does_not_redraw` and `switching_flow_resubscribes_its_ledgers` pass. **predicted, unverified**.
  - guard: existing runtime tests pass.

### 22. Add the --surface and --ledger flags for headless rendering [L]
- **Files**: `glimpse/src/cli.rs`, `glimpse/src/main.rs`, `glimpse/tests/cli.rs`, `glimpse/src/runtime.rs` — the `RunOpts.surface` field and the initial surface in `runtime::run` only
- **Depends on**: 1, 10, 21
- **Action**:
  - Add `--surface <tasks|review|optimise|plan-review|backlog|inbox>` and `--ledger FILE` (only with `--once`) to `glimpse/src/cli.rs` (`ViewArgs`, `HELP`, parse arms, the test literal).
  - Wire them through `main.rs::run_view` to `render_once`; the ledger is loaded via `LedgerRef::File`.
  - Add `RunOpts.surface` in `glimpse/src/runtime.rs` and start the live App on it.
  - Add sandboxed `glimpse/tests/cli.rs` cases.
- **Detail**: `--ledger` without `--once` is a usage error (exit 2); `--once --ledger` with neither `--snapshot` nor `--slug` renders over `Snapshot::default()` instead of resolving the freshest flow, so the sandboxed test needs no flow on disk. `--surface` without `--once` opens the live view on that surface. The edits are inseparable: the flag, its parse arm and its two consumers land together.
- **Acceptance**:
  - forward: `once_renders_a_review_ledger` passes. It renders `glimpse/tests/fixtures/review-ledger.toml` and finds an id and a group header. **predicted, unverified**.
  - forward: `ledger_without_once_is_a_usage_error` passes. **predicted, unverified**.
  - falsifier: resolving the freshest flow when `--ledger` is given makes `once_renders_a_review_ledger` fail in its empty sandbox.

### 23. Bound the poll_ms setting [S]
- **Files**: `glimpse/src/config.rs`
- **Depends on**: —
- **Backlog**: B-f4bd798a
- **Action**: Reject `poll_ms` outside 50–10000. The out-of-range value names the key and the range, and the whole file falls back to defaults, as for any invalid value.
- **Detail**: Follow the existing range validation style used for `panel_percent` and `column_max`.
- **Acceptance**: forward: config test `poll_ms_out_of_range_is_rejected` passes. **predicted, unverified**.

### 24. Document Milestone A in the legend and README [M]
- **Files**: `glimpse/src/view/legend.rs`, `glimpse/README.md`
- **Depends on**: 16, 18, 19, 22, 23
- **Action**:
  - Add to `legend::KEYS` the keys `1`–`6`, `Space`, `V`, `g`, `S` and `c`.
  - Add to the legend the item status glyphs, the severity chips, the mark, the saving and running marks, and the badge.
  - In `glimpse/README.md`, add:
    - a "Surfaces" section (the item surfaces, the selector's flow-less scopes, arrivals and badges);
    - the key table rows;
    - the new token table rows (task 13);
    - the `--surface` / `--ledger` usage;
    - the `poll_ms` range.
- **Detail**: glimpse is still read-only in this milestone, so keep README line 5. No test enforces sync, so cross-check the legend `KEYS`, the README table and the footer hints by hand.
- **Acceptance**: forward: `grep -c 'severity_critical' glimpse/README.md` prints `1` or more (today: `0`).

### Milestone B — control-edit writes

### 25. Add the expect precondition to items apply [M]
- **Files**: `tomlctl/src/items.rs`
- **Depends on**: —
- **Action**:
  - Parse an optional `expect` object per op in both `apply_op_indexed` and `apply_single_op`, and check it against the current row before an update or remove. JSON equality applies, and `null` means the field is absent.
  - Add `StalePolicy { Abort, Skip }` and `compute_apply_mutation_with(…, StalePolicy)`. It returns the plan plus `skipped_stale: Vec<StaleOp { id, field, expected, found }>`.
  - Keep `compute_apply_mutation` as a delegating wrapper with `Abort`.
- **Detail**: Approach, "Stale-write precondition".
  - `Abort` errors with every stale op listed, not just the first.
  - An `expect` on an `add` op is an error.
  - Dates compare as their TOML string form.
- **Acceptance**:
  - forward: unit tests pass. **predicted, unverified**:
    - `an_expect_mismatch_aborts_the_batch`;
    - `an_expect_mismatch_is_skipped_under_skip`;
    - `a_null_expect_requires_the_field_absent`;
    - `the_indexed_path_honours_expect` (more than 2 updates).
  - guard: `items_apply_runs_batch_atomically` still passes.
  - falsifier: checking `expect` only in `apply_single_op` makes `the_indexed_path_honours_expect` fail.

### 26. Expose --on-stale on the items apply CLI [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/tests/integration.rs`
- **Depends on**: 25
- **Action**: Add `--on-stale <abort|skip>` (default `abort`) to `ItemsOp::Apply`. Pass it to `compute_apply_mutation_with`, and add `skipped_stale` to the output envelope, `--dry-run` included.
- **Detail**: The flag must appear in `items apply --help`. Integration tests run the binary against a temp ledger.
- **Acceptance**:
  - forward: integration tests `items_apply_on_stale_skip_reports_skipped_ids` and `items_apply_stale_expect_fails_by_default` pass. **predicted, unverified**.
  - guard: `items_apply_reads_ops_from_stdin_dash` still passes.
  - falsifier: defaulting `--on-stale` to `skip` makes `items_apply_stale_expect_fails_by_default` fail.

### 27. Name the optimise wontapply companion wontapply_rationale [S]
- **Files**: `tomlctl/src/items.rs`, `claude/skills/flow-contract-ledger-schema/SKILL.md`
- **Depends on**: 25
- **Action**:
  - Make `Item::validate` require `wontapply_rationale` for `wontapply` and keep `wontfix_rationale` for `wontfix`. Update its doc comment and the test `item_validate_flags_wontapply_missing_rationale`.
  - In `claude/skills/flow-contract-ledger-schema/SKILL.md`, split the `wontfix` / `wontapply` companion bullet accordingly.
- **Detail**: User decision (pre-plan). All five real `wontapply` rows already use `wontapply_rationale`.
- **Acceptance**:
  - forward: `grep -c '"wontfix" | "wontapply" => &\["wontfix_rationale"\]' tomlctl/src/items.rs` prints `0` (today: `1`).
  - forward: `grep -c 'wontapply_rationale' claude/skills/flow-contract-ledger-schema/SKILL.md` prints `1` or more (today: `0`).

### 28. Add the ledger control-edit facade [L]
- **Files**: `tomlctl/src/ledgers.rs`, `tomlctl/src/lib.rs`, `tomlctl/tests/library_facade.rs`, `tomlctl/tests/common/mod.rs` — an env-locked `TOMLCTL_ROOT` guard every facade write test holds
- **Depends on**: 3, 25, 27
- **Action**:
  - Implement `transition`, `classify` and `restore` in `tomlctl/src/ledgers.rs` per Approach, "Ledger contract". Each writes under `io::mutate_doc_conditional` with `OnMissing::Error`, and none bumps `last_updated`.
  - Export `ledger_transition`, `ledger_classify` and `ledger_restore` from `tomlctl/src/lib.rs`, each with the root-equality check.
- **Detail**:
  - Enforce the transition table from Approach, "Ledger contract"; any other transition is an error naming it.
  - Remove the previous status's companions and validate with `Item::validate`.
  - Stale ids are skipped and reported, and the rest are written.
  - `classify` refuses plan-review and backlog.
  - Add to `tomlctl/tests/common/mod.rs` a guard holding one file-static `Mutex` across an `unsafe` `std::env::set_var("TOMLCTL_ROOT", sandbox)` / `remove_var` pair (Approach, "Ledger contract"); every facade write test holds it, and tasks 29 and 42 reuse it.
- **Acceptance**:
  - forward: facade tests pass. **predicted, unverified**:
    - `ledger_transition_defers_an_open_finding`;
    - `ledger_transition_skips_a_changed_status`;
    - `ledger_transition_refuses_fixed`;
    - `ledger_restore_undoes_a_transition`;
    - `ledger_writes_refuse_a_mismatched_root`;
    - `ledger_classify_recomputes_dedup_id`;
    - `ledger_classify_refuses_plan_review_and_backlog`;
    - `ledger_transition_discards_a_plan_review_finding_with_its_reason`;
    - `ledger_transition_wontapply_requires_wontapply_rationale`.
  - falsifier: leaving the previous status's companions in place makes `ledger_restore_undoes_a_transition` fail.

### 29. Add the backlog triage facade [M]
- **Files**: `tomlctl/src/backlog/triage.rs`, `tomlctl/src/lib.rs`, `tomlctl/tests/library_facade.rs`, `tomlctl/src/backlog/mod.rs`
- **Depends on**: 28
- **Action**:
  - Factor `triage::dispatch` into a value-returning `triage_value(root, …)` that the CLI path prints.
  - Add an `expect_status` check.
  - Export `backlog_triage(root, ids, BacklogTriage::{Dismiss{reason}, Reopen{rationale}, Resolve{resolution}}, expect_status)` from `tomlctl/src/lib.rs`.
- **Detail**:
  - It reuses the existing private `apply_transition` and `schema::clear_for_transition`, so the validator's companion rules hold. `apply_transition` is all-or-nothing, so filter the ids on `expect_status` before calling it, and keep the `last_updated` stamp it writes.
  - It takes the backlog path from `root`, not from `repo_or_cwd_root()`, after the root-equality check.
  - Facade tests hold task 28's `TOMLCTL_ROOT` guard. The CLI flag `--expect-status` is task 55's.
- **Acceptance**:
  - forward: facade tests `backlog_triage_dismisses_with_a_reason` and `backlog_triage_skips_a_changed_status` pass. **predicted, unverified**.
  - falsifier: passing every id to `apply_transition` regardless of `expect_status` makes `backlog_triage_skips_a_changed_status` fail.
  - guard: `tomlctl/tests/backlog_write.rs` still passes.

### 30. Build the form framework [M]
- **Files**: `glimpse/src/form.rs`, `glimpse/Cargo.toml`, `glimpse/Cargo.lock`
- **Depends on**: 1
- **Action**: Add `tui-input = "0.15.5"` (default features) to `glimpse/Cargo.toml`, and refresh `glimpse/Cargo.lock` with `cargo tree --manifest-path glimpse/Cargo.toml`. Implement `Field`, `Form`, `FormOutcome`, `FieldValue` and `Form::handle_key` per Approach, "Forms".
- **Detail**:
  - Required fields are marked per field.
  - Submitting with an empty required field sets `error` and stays `Pending`.
  - `Select.free` handles the "other…" option.
  - Release events are ignored.
  - The only lockfile addition is `tui-input`. A second `crossterm` version is a stop-and-report.
- **Acceptance**:
  - forward: unit tests pass. **predicted, unverified**:
    - `tab_moves_focus_and_enter_submits`;
    - `j_typed_into_a_text_field_is_text`;
    - `space_toggles_a_multi_option`;
    - `an_empty_required_field_blocks_submit`;
    - `esc_closes_an_open_dropdown_before_cancelling`.
  - forward: `grep -c '^name = "tui-input"' glimpse/Cargo.lock` prints `1` (today: `0`).
  - guard: `grep -c '^name = "crossterm"' glimpse/Cargo.lock` prints `1`, so there is no second crossterm version.

### 31. Render forms as a modal [S]
- **Files**: `glimpse/src/view/form.rs`
- **Depends on**: 13, 30
- **Action**: Render a `Form` as a centred `Clear` + `Block` modal:
  - labels in `form_label`, and the focused field in `form_focus`;
  - an open dropdown drawn with `List` / `ListState`;
  - checkboxes `[x]`;
  - the error line in `form_error`;
  - the text cursor via `Frame::set_cursor_position`.
- **Detail**: Follow `glimpse/src/view/selector.rs`'s overlay pattern.
- **Acceptance**: forward: view test `a_form_shows_its_fields_and_error` passes. **predicted, unverified**.

### 32. Run writes on a serial writer thread [M]
- **Files**: `glimpse/src/writer.rs`, `glimpse/src/source.rs`, `glimpse/src/runtime.rs`
- **Depends on**: 11, 22, 28, 29
- **Action**:
  - Implement `Writer::spawn(root, events)` with `submit(WriteRequest)` per Approach, "Writer thread", calling the tomlctl facade. Define only `Transition`, `Classify`, `Restore` and `BacklogTriage` here; task 51 adds the `Input*` variants once the inputs facade (task 42) exists.
  - Add `Event::Written(WriteOutcome)` to `glimpse/src/source.rs`, with a placeholder `Event::Written(_) => Step::Nothing` arm in `runtime::handle` (task 34 routes it).
  - Spawn the writer beside `Source::start` in `runtime::run` (`glimpse/src/runtime.rs`), which owns the event channel, and hold it in `TerminalHost`; `--once` never reaches `runtime::run`, so it never spawns one.
  - Add a `FakeWriter` for tests.
- **Detail**: Requests execute in submission order. A facade error becomes `WriteOutcome.error`, and the thread never panics on one.
- **Acceptance**:
  - forward: unit tests `writes_run_in_submission_order` and `a_facade_error_is_reported_not_panicked` pass. **predicted, unverified**.
  - falsifier: running each request on its own spawned thread makes `writes_run_in_submission_order` fail.

### 33. Add the action menu, control-edit forms, bulk edits and undo [M]
- **Files**: `glimpse/src/actions.rs`, `glimpse/src/app.rs`
- **Depends on**: 15, 30, 32
- **Action**:
  - **Menu and forms.** Build the action menu for the current surface from the transition table (Approach, "Ledger contract"), and the matching forms: disposition, with its companion fields, and classify, with dropdowns for severity and effort and a free-form category.
  - **Submitting.** On submit, produce one `WriteRequest` covering every target (marks, else the cursor row) with `expect_status` set to each row's displayed status. Mark the targets `saving`.
  - **Undo.** Keep the undo stack, one entry per submitted write; `u` turns the top entry into one `Restore` per id that write applied, reporting any that come back stale (Approach, "Writer thread").
  - **Confirmation.** Ask before moving a `critical` item to `wontfix`, `wontapply`, `discarded` or `dismissed`.
  - **App wiring.** In `glimpse/src/app.rs`, add `overlay: Option<Overlay::{Menu, Form, Prompt}>`, the actions `OpenMenu`, `OpenClassify`, `Undo` and `OpenFilter`, and handling for `Event::Written` outcomes (clear `saving` on error or stale skip, footer notice).
- **Detail**: A bulk selection that mixes statuses offers only the transitions valid for every target, and splits into one request per displayed from-status (Approach, "Ledger contract"). The filter prompt is an `Overlay::Prompt` holding a `tui_input::Input`. `Back` closes the overlay first.
- **Acceptance**: forward: unit tests pass. **predicted, unverified**:
  - `deferring_two_marked_findings_submits_one_transition`;
  - `a_mixed_status_selection_offers_only_common_transitions`;
  - `undo_submits_a_restore_with_the_written_values`;
  - `a_stale_outcome_clears_saving_and_notices`.
  - falsifier: submitting one `WriteRequest` per marked row makes `deferring_two_marked_findings_submits_one_transition` fail.

### 34. Route keys to open forms and prompts [M]
- **Files**: `glimpse/src/keys.rs`, `glimpse/src/runtime.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 16, 21, 31, 32, 33
- **Action**:
  - In `runtime::handle`, send raw key events to the open overlay before calling `keys::map`.
  - Map `m`, `e`, `u` and `/` in `keys::map`.
  - Submit `WriteRequest`s to the writer through a new `Host::write` on `TerminalHost`, and route `Event::Written`, replacing task 32's placeholder arm.
  - Draw the overlay in `view::render` (`glimpse/src/view/mod.rs`) beside the selector and legend overlays: a `Form` or `Overlay::Menu` through `view::form::render` (the menu renders as a single-`Select` form).
- **Detail**: While an overlay is open, the mouse is blocked, as the selector blocks it today. `Ctrl+C` still quits.
- **Acceptance**:
  - forward: runtime tests `typing_q_in_a_form_does_not_quit` and `submitting_a_form_reaches_the_writer` pass. **predicted, unverified**.
  - falsifier: calling `keys::map` before the overlay makes `typing_q_in_a_form_does_not_quit` fail.

### 35. Render the inline filter prompt and closed toggle [S]
- **Files**: `glimpse/src/view/items.rs`
- **Depends on**: 18, 33
- **Action**: While `Overlay::Prompt` is open, draw the filter input in the facet row with the cursor, using `visual_scroll`. Show the `closed shown|hidden` state.
- **Detail**: Approach, "Surfaces and interaction"; the filter applies as you type.
- **Acceptance**: forward: view test `the_filter_prompt_renders_in_the_facet_row` passes. **predicted, unverified**.

### 36. Guard the apply pipeline's late writes with expect [M]
- **Files**: `claude/skills/flow-contract-apply-pipeline/references/verification.md`, `claude/skills/flow-contract-apply-pipeline/SKILL.md`, `claude/skills/flow-contract-apply-rollback-protocol/SKILL.md`
- **Depends on**: 26
- **Action**:
  - In the two-call write pattern, each status-transition op carries `"expect":{"status":"<status read at Step 1>"}`, and the call passes `--on-stale skip`.
  - The SKILL.md "Interim checkpoint" `items apply` gets the same `expect` and flag (one line), and `verification.md`'s "≤ 3 items → loop of single-item `tomlctl items update`" fallback is withdrawn for status transitions, since `items update` carries no `expect`.
  - Add a "changed during the run" line in the final summary for `skipped_stale` ids, which are never retried.
  - Note in the pipeline SKILL.md's ledger-mutation section that a human may edit through glimpse mid-run.
  - Rollback reversals carry `expect` equal to this run's transition status.
- **Detail**: The pipeline SKILL.md is at 479/500 lines, so keep the addition to a few lines and put the detail in `verification.md`. The rollback skill's description is at 1016/1024 characters, so do not touch it. Bash-fenced `tomlctl` lines must parse.
- **Acceptance**: forward: `grep -l -- '--on-stale skip' claude/skills/flow-contract-apply-pipeline/references/verification.md claude/skills/flow-contract-apply-rollback-protocol/SKILL.md | wc -l` prints `2` (today: `0`).

### 37. Guard review, optimise and disposition-sweep writes with expect [M]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/skills/flow-contract-ledger-disposition-sweep/SKILL.md`
- **Depends on**: 6, 26
- **Action**: Add `expect` and `--on-stale skip` to:
  - `/review` Step 4's disposition batch (`expect` status `open`);
  - `/optimise`'s Interim-checkpoint `items apply` (reopened items carry `expect` status `deferred`) and its Step 3 updates to existing items (`expect` status `open`) — `/optimise` has no disposition step;
  - the disposition sweep's reopen batch (`expect` status `deferred`).

  Each reports the `skipped_stale` ids.
- **Detail**: The Step 3 `rounds` patch needs no `expect`. Keep every `Invoke the \`…\` skill` phrase.
- **Acceptance**: forward: `grep -l -- '--on-stale skip' claude/commands/review.md claude/commands/optimise.md claude/skills/flow-contract-ledger-disposition-sweep/SKILL.md | wc -l` prints `3` (today: `0`).

### 38. Guard review-plan's merge writes and record discard reasons [S]
- **Files**: `claude/commands/review-plan.md`
- **Depends on**: 7, 26
- **Action**:
  - Add `expect` `{"status":"open"}` and `--on-stale skip` to the `merged` / `discarded` transitions.
  - Replace "`/review-plan` is the sole writer" with: the only agent writer, while the human may discard through glimpse.
  - Add the optional `discard_reason` field to the plan-review schema line.
- **Detail**: Approach, "Ledger contract" and "Stale-write precondition".
- **Acceptance**:
  - forward: `grep -c -- '--on-stale skip' claude/commands/review-plan.md` prints `1` or more (today: `0`).
  - forward: `grep -c 'is the sole writer' claude/commands/review-plan.md` prints `0` (today: `1`).

### 39. Document expect and the human writer in the items references [M]
- **Files**: `claude/skills/tomlctl/references/write.md`, `claude/skills/flow-contract-ledger-schema/SKILL.md`
- **Depends on**: 26, 27
- **Action**:
  - In the `items apply` section of `claude/skills/tomlctl/references/write.md`, document:
    - the per-op `expect` object;
    - `--on-stale` (a flag-table row);
    - the `skipped_stale` envelope;
    - the old-binary hazard.
  - In the ledger-schema skill:
    - add `expect` to the op shape;
    - qualify "concurrent invocations are safe" as per-call, not per-run;
    - name glimpse as a control-field writer;
    - list `reopen_rationale` and `discard_reason`.
- **Detail**: `flag_table_lint` checks that each flag row names a real flag, which is why this task depends on 26.
- **Acceptance**: forward: `grep -c -- '--on-stale' claude/skills/tomlctl/references/write.md` prints `1` or more (today: `0`).

### Milestone C — user input records, agent questions and the Inbox

### 40. Add the inputs store module [M]
- **Files**: `tomlctl/src/inputs.rs` (new), `tomlctl/src/lib.rs`, `.gitignore`
- **Depends on**: 29
- **Action**: Implement the store per Approach, "Input store", with value-returning functions that never print:
  - the schema and its validation;
  - `list` (filters: pending, kind, ledger, flow, scope, item);
  - `add` (assigns `I{n}` and `created`, status `new`);
  - `ack`, `handle` (`--by`, `--note`), `withdraw` (only `new`), and `answer` (writes the answer record and moves the question to `handled`).

  Declare `mod inputs;` in `tomlctl/src/lib.rs`. Add `.claude/inputs.toml*` to `.gitignore` with a one-line comment.
- **Detail**:
  - The array is `inputs`.
  - Writes use `mutate_doc` with `OnMissing::Create(seed)`, where the seed carries `schema_version` and `last_updated`.
  - Validation enforces the per-kind required fields: capture needs `text`; question needs `prompt`, `choice`, and `options` unless `choice = "text"`; answer needs `answers` plus `picked` or `text`.
  - An answer to a question that is not `new` is an error.
- **Acceptance**:
  - forward: unit tests pass. **predicted, unverified**:
    - `add_assigns_sequential_ids`;
    - `answer_handles_its_question`;
    - `withdraw_refuses_an_acknowledged_record`;
    - `a_question_without_options_is_invalid`.
  - falsifier: letting `withdraw` move any status makes `withdraw_refuses_an_acknowledged_record` fail.
  - forward: `git check-ignore -q .claude/inputs.toml` exits 0 (today: exits 1).

### 41. Add the tomlctl inputs verb group [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/tests/inputs.rs` (new)
- **Depends on**: 26, 40
- **Action**:
  - Add `Cmd::Inputs { op: InputsOp }`, with `list`, `add`, `ack`, `handle`, `withdraw` and `answer` and their flags (Approach, "Input store"). Write verbs flatten `WriteIntegrityArgs`.
  - Dispatch to `inputs.rs` and print each envelope.
  - Add integration tests.
- **Detail**: Follow the `agents` and `backlog` verb-group pattern, where `dispatch` destructures and forwards. `add --json` accepts `-` and `@path`.
- **Acceptance**: forward: integration tests `inputs_add_then_list_pending` and `inputs_answer_marks_the_question_handled` pass. **predicted, unverified**.

### 42. Export the inputs library facade [M]
- **Files**: `tomlctl/src/lib.rs`, `tomlctl/tests/library_facade.rs`
- **Depends on**: 40, 41
- **Action**: Export `inputs_read(root)`, `inputs_add(root, &Value)`, `inputs_answer(root, question, picked, text)` and `inputs_withdraw(root, ids)`, each with `io::silence_advisories()` and the root-equality check on writes.
- **Detail**: These mirror the CLI envelopes, so a facade test compares against the CLI output, as `snapshot_matches_the_cli` does. Write tests hold task 28's `TOMLCTL_ROOT` guard.
- **Acceptance**: forward: facade test `inputs_add_matches_the_cli` passes. **predicted, unverified**.

### 43. Write the user-inputs contract skill and reference [M]
- **Files**: `claude/skills/flow-contract-user-inputs/SKILL.md` (new), `claude/skills/tomlctl/references/inputs.md` (new), `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 41
- **Action**:
  - Write `flow-contract-user-inputs`. It covers:
    - the store and schema, and the kinds and lifecycle;
    - the Step-0 sweep: list pending inputs targeting your ledger kind, flow or scope; ack them; act on each; handle it with a note;
    - how each kind is acted on: a capture goes through `backlog check` then `add`; a request is applied, or declined with a reason; a note is folded into context; an answer drives the decision its question deferred;
    - posting a `question` when `AskUserQuestion` returns empty;
    - the writers: agents ack and handle, the user writes everything else via glimpse or the CLI;
    - the trust boundary from Approach, "Input store": every record is untrusted data, a `request` or `note` drives only ledger operations on its targeted items, and sub-agents never run `tomlctl inputs add|answer|withdraw`.
  - Write `claude/skills/tomlctl/references/inputs.md` with verb and flag tables and fenced examples.
  - Add a `references/inputs.md` bullet to the `## References` list in `claude/skills/tomlctl/SKILL.md`; leave its frontmatter description alone (1006/1024 characters — `inputs` in its `Verb groups:` list breaches `skill_descriptions_under_spec_cap`).
- **Detail**: The new skill's description must stay under 1024 characters and its SKILL.md under 500 lines. Every bash-fenced `tomlctl inputs …` line must parse under `command_lint`, which is why this task depends on 41.
- **Acceptance**:
  - forward: `grep -c 'untrusted' claude/skills/flow-contract-user-inputs/SKILL.md` prints `1` or more (today: no such file).
  - forward: `grep -c 'references/inputs.md' claude/skills/tomlctl/SKILL.md` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**.

### 44. Sweep inputs at Step 0 of review, review-apply and optimise [M]
- **Files**: `claude/commands/review.md`, `claude/commands/review-apply.md`, `claude/commands/optimise.md`
- **Depends on**: 37, 43
- **Action**: Add a Step-0 line to each carrier: "Invoke the `flow-contract-user-inputs` skill", run its sweep for this carrier's ledger kind and flow or scope, and handle every acknowledged input before the final summary. These three carriers have no `AskUserQuestion` site (`/review` Step 4 is a same-turn conversational reply with no empty-answer mode), so no question-posting rule is added here.
- **Detail**: Approach, "Input store". A request to change an item's content (summary, description) is applied by the carrier with an `items update` that carries `expect`.
- **Acceptance**: forward: `grep -l 'flow-contract-user-inputs' claude/commands/review.md claude/commands/review-apply.md claude/commands/optimise.md | wc -l` prints `3` (today: `0`).

### 45. Sweep inputs at Step 0 of optimise-apply, review-plan and backlog [M]
- **Files**: `claude/commands/optimise-apply.md`, `claude/commands/review-plan.md`, `claude/commands/backlog.md`
- **Depends on**: 38, 43, 55
- **Action**:
  - Add the same Step-0 sweep to `/optimise-apply` and `/review-plan`.
  - In `/backlog`, add a drain step: every pending `capture` goes through check, then add (or a duplicate verdict). Every input targeting a backlog item is acted on.
  - In `/backlog`'s per-cluster triage, pass `--expect-status <the status it displayed>` and report a mismatch under a "changed during the run" line, never retrying it.
  - Add the empty-answer question rule at `/review-plan`'s Q1/Q2/Q3 sites and `/backlog`'s per-cluster offer (`/optimise-apply` has no `AskUserQuestion` site).
- **Detail**: `/backlog` must keep its `backlog-capture` invocation phrase. Kind and area are settled before `check`, and the same values are reused for `add`.
- **Acceptance**: forward: `grep -l 'flow-contract-user-inputs' claude/commands/optimise-apply.md claude/commands/review-plan.md claude/commands/backlog.md | wc -l` prints `3` (today: `0`).

### 46. Gate the six carriers on the inputs skill [S]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`
- **Depends on**: 44, 45
- **Action**: Add `flow-contract-user-inputs` to the required-skills table of `carrier_invokes_required_skills` for `review.md`, `review-apply.md`, `optimise.md`, `optimise-apply.md`, `review-plan.md` and `backlog.md`.
- **Detail**: Follow the table's existing entry shape.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::skills::carrier_invokes_required_skills` passes. **predicted, unverified**.
  - falsifier: removing the invocation phrase from `claude/commands/backlog.md` makes it fail.

### 47. Name the human writer and the input store in policy docs [M]
- **Files**: `CLAUDE.md`, `claude/skills/backlog-capture/SKILL.md`, `glimpse/CLAUDE.md`
- **Depends on**: 43
- **Action**:
  - `CLAUDE.md` "Backlog capture": the orchestrator is the only *agent* writer. The human triages via glimpse (dismiss, reopen, resolve). Captures arrive through `.claude/inputs.toml`, which `/backlog` drains.
  - `claude/skills/backlog-capture/SKILL.md`: the same amendment at the "orchestrator is the only writer" and "status is moved only by triage and reconcile" passages, noting that glimpse calls the same triage logic.
  - `glimpse/CLAUDE.md`: update the Structure paragraph (the new modules and the `source/` split; `watch.rs` now watches `.claude`, `.claude/flows` and the flow-less ledger dirs), the watch gotchas (per-scope miss streaks; `Wake::NewScope`) and the `agents.toml` second-writer line (glimpse also writes ledgers, `backlog.toml` and `inputs.toml`), plus new rules:
    - glimpse writes only through the tomlctl facade on the writer thread;
    - no optimistic model updates;
    - control fields only;
    - free text goes to the input store.
- **Detail**: The backlog-capture description is at 844/1024 characters, so leave it alone. The implement-* `backlog-candidates` shared block stays untouched.
- **Acceptance**: forward: `grep -c 'inputs.toml' CLAUDE.md claude/skills/backlog-capture/SKILL.md glimpse/CLAUDE.md` prints `1` or more per file (today: `0` each).

### 48. Model input records and the Inbox state in glimpse [M]
- **Files**: `glimpse/src/ledger.rs`, `glimpse/src/surface.rs`
- **Depends on**: 14, 42
- **Action**:
  - Add `InputRow` (all store fields) and an `Inputs` loader to `glimpse/src/ledger.rs`.
  - Add `InboxState` to `glimpse/src/surface.rs`: unanswered questions first, then your records grouped by status, with flashes on lifecycle changes, plus a `pending_for(ledger, item)` lookup used for item rows and details.
- **Detail**: Approach, "Input store".
- **Acceptance**: forward: unit tests `unanswered_questions_sort_first` and `pending_inputs_are_found_by_item` pass. **predicted, unverified**.

### 49. Feed input records through the poller [M]
- **Files**: `glimpse/src/source.rs`, `glimpse/src/source/scan.rs`, `glimpse/src/runtime.rs`, `glimpse/src/ledger.rs`
- **Depends on**: 32, 34, 42, 48, 54
- **Action**:
  - Fetch `Feed::Inputs` via `tomlctl::inputs_read`, posting `Event::InputRecords` on a revision change.
  - Always subscribe to it.
  - Route `Event::InputRecords` to `App::apply_inputs` (task 54), which updates the Inbox badge and the pending marks.
- **Detail**: Driven by `Wake::Repo(Inputs)`, with the same fingerprint discipline as task 11. `Event::InputRecords` is distinct from the existing key-input `Event::Input`.
- **Acceptance**:
  - forward: test `an_inputs_change_posts_one_event` passes. **predicted, unverified**.
  - falsifier: posting on every safety tick rather than on a revision change makes it fail.

### 50. Render the Inbox surface [M]
- **Files**: `glimpse/src/view/inbox.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 18, 31, 34, 48, 54
- **Action**:
  - Render the Inbox: questions (prompt, author, target, options) in the `question` token, then records with kind, status in the `input_*` tokens, target, a text excerpt, and the handled note.
  - Replace task 18's placeholder in `view/mod.rs`, and add Inbox footer hints (`Enter` answer, `w` withdraw, `n` capture).
- **Detail**: Approach, "Surfaces and interaction".
- **Acceptance**: forward: view test `the_inbox_lists_questions_before_records` passes. **predicted, unverified**.

### 51. Add capture, request, note and answer forms [M]
- **Files**: `glimpse/src/actions.rs`, `glimpse/src/writer.rs`
- **Depends on**: 33, 42, 48
- **Action**:
  - **Forms** (`glimpse/src/actions.rs`):
    - `n` capture: summary, a kind dropdown over the backlog kinds, area, note;
    - `r` request or note on the targets: a kind select over request and note, plus text;
    - the answer form, built from a question's `choice` and `options`;
    - `w` withdraw.
  - **Writer** (`glimpse/src/writer.rs`): add the `InputAdd`, `InputAnswer` and `InputWithdraw` request variants and execute them through the inputs facade.
- **Detail**: The capture's `capture_kind` and `area` are hints that `/backlog` settles. Each input write is undone by withdrawing it. The keys and App actions that open these forms are task 54's.
- **Acceptance**: forward: unit tests `a_capture_form_submits_an_input_add` and `answering_a_multi_question_sends_the_picked_options` pass. **predicted, unverified**.

### 52. Mark item rows and details with pending inputs [S]
- **Files**: `glimpse/src/view/items.rs`, `glimpse/src/view/details.rs`
- **Depends on**: 20, 35, 48, 54
- **Action**: Draw a pending-input mark on item rows that `InboxState::pending_for` (held as `App.inbox`, task 54) matches. List those records (kind, status, text) in item details.
- **Detail**: Use the `input_new` and `input_acknowledged` tokens.
- **Acceptance**: forward: view test `a_row_with_a_pending_request_shows_the_mark` passes. **predicted, unverified**.

### 53. Document Milestones B and C in the legend and README [M]
- **Files**: `glimpse/src/view/legend.rs`, `glimpse/README.md`
- **Depends on**: 24, 34, 35, 49, 50, 51, 52, 54
- **Action**:
  - Add `m`, `e`, `r`, `n`, `u`, `/`, `w` and Inbox `Enter` to `legend::KEYS`, plus the input marks and question glyph.
  - In `glimpse/README.md`:
    - replace "It is read-only" with the write model (control fields via forms, input records via the store);
    - document forms, undo, the Inbox and the input store;
    - add the input/form token rows;
    - update "How it works" with the writer thread and the extra watch scopes.
- **Detail**: Hand-sync the legend `KEYS`, the README key table and the footer hints.
- **Acceptance**: forward: `grep -c 'It is read-only' glimpse/README.md` prints `0` (today: `1`).

### 54. Wire the Inbox and input forms through the App and keys [M]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/actions.rs`, `glimpse/src/view/header.rs`
- **Depends on**: 34, 48, 51
- **Action**:
  - In `glimpse/src/app.rs`, add `inbox: InboxState` to `App` with `App::apply_inputs(value)`, and the actions `OpenCapture`, `OpenRequest`, `Withdraw` and `AnswerQuestion` that open task 51's forms in `Overlay::Form` (or submit the withdraw). `Details` on the Inbox surface opens the answer form for a question.
  - In `glimpse/src/keys.rs`, map `n` → `OpenCapture`, `r` → `OpenRequest` and `w` → `Withdraw`, keeping `keys::map` context-free; the App ignores them where they do not apply.
- **Detail**: Approach, "Surfaces and interaction" and "Input store". `r` acts on the marks, or on the cursor row when nothing is marked. `w` withdraws only your own `new` record.
- **Acceptance**:
  - forward: unit tests pass. **predicted, unverified**:
    - `n_r_and_w_map_to_input_actions`;
    - `enter_on_a_question_opens_its_answer_form`;
    - `apply_inputs_updates_the_inbox_badge`.
  - falsifier: leaving `n` unmapped makes `n_r_and_w_map_to_input_actions` fail.

### 55. Expose the triage precondition on the backlog CLI [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/tests/backlog_write.rs`, `tomlctl/src/backlog/dispatch.rs`, `tomlctl/src/backlog/triage.rs`
- **Depends on**: 29, 41
- **Action**: Add `--expect-status <status>` to `backlog triage` and pass it to task 29's `triage_value`; an id whose status differs is reported under `skipped_stale` in the envelope and left untouched.
- **Detail**: Approach, "Ledger contract". Depends on 41 only because both edit `cli/types.rs` and `cli/dispatch.rs`. Without the flag, behaviour is unchanged.
- **Acceptance**:
  - forward: integration test `backlog_triage_expect_status_skips_a_changed_row` passes. **predicted, unverified**.
  - guard: the existing `tomlctl/tests/backlog_write.rs` tests pass.
  - falsifier: ignoring `--expect-status` makes `backlog_triage_expect_status_skips_a_changed_row` fail.

### 56. Advertise the new verbs and flags in tomlctl capabilities [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 26, 41, 55
- **Action**:
  - Add `"inputs"` to `SUBCOMMANDS`, and `items_apply_expect`, `backlog_triage_expect` and `inputs` to `FEATURES`, in `tomlctl/src/cli/types.rs`.
  - Mirror them in `tomlctl/tests/capabilities.rs` (the expected-features list and the read/write `--help` integrity lists).
  - Add the `inputs` verb lines and the `--on-stale` / `--expect-status` flags to `tomlctl/README.md`.
- **Detail**: Approach, "Stale-write precondition". Follow the existing entry shapes.
- **Acceptance**:
  - forward: `grep -c '"items_apply_expect"' tomlctl/src/cli/types.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes. **predicted, unverified**.

### 57. Release tomlctl 0.13.0 [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 30, 56
- **Action**: Bump `tomlctl/Cargo.toml` to `0.13.0`, then refresh `tomlctl/Cargo.lock` and `glimpse/Cargo.lock` (`cargo tree --manifest-path glimpse/Cargo.toml`), staged in one commit.
- **Detail**: The pre-commit hook runs glimpse `clippy --locked`, so the glimpse lockfile must land with the bump.
- **Acceptance**:
  - forward: `grep -c '^version = "0.13.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - guard: `cargo clippy --manifest-path glimpse/Cargo.toml --locked` passes. **predicted, unverified**.

### 58. Fix the debug-build stack overflow in tomlctl capabilities [M]
- **Files**: `tomlctl/src/main.rs`, `tomlctl/src/cli/dispatch.rs`
- **Depends on**: —
- **Action**: A debug build of 'tomlctl capabilities' overflows the 1 MB Windows main-thread stack (release passes), failing 9 tests in tomlctl/tests/capabilities.rs. Diagnose whether it reproduces at HEAD (git archive HEAD into the scratchpad, separate target dir) or was introduced by this flow's CLI growth (inputs verb group, items apply --on-stale, backlog triage --expect-status), then fix the root cause, for example by running dispatch on a spawned thread with a larger stack in main.rs or by boxing or shrinking the large dispatch stack frame.
- **Acceptance**: cargo test --manifest-path tomlctl/Cargo.toml --test capabilities passes in a debug build (all tests, none failing).

### 59. Wire the filter-prompt cursor and drop stale dead-code allows [S]
- **Files**: `glimpse/src/view/mod.rs`, `glimpse/src/view/items.rs`, `glimpse/src/view/form.rs`, `glimpse/src/app.rs`, `glimpse/src/actions.rs`, `glimpse/src/writer.rs`, `glimpse/src/source.rs`
- **Depends on**: 34, 35, 51
- **Action**: In view/mod.rs, where the item surface calls items::render(frame.buffer_mut(), area, app), add: if let Some(p) = items::prompt_cursor(area, app) { frame.set_cursor_position(p); } and drop prompt_cursor's dead_code allow. Remove every #[allow(dead_code, reason = ...)] whose item is now used: app.rs OpenMenu/OpenClassify/Undo/OpenFilter, overlay_key, take_writes, apply_written; actions.rs Overlay::form; view/form.rs module-level allow; writer.rs WriteRequest/WriteOutcome/Writer::submit/FakeWriter::submit; source.rs Event::Written payload if read. Keep an allow only where clippy still reports dead code, and say which. Fix the two clippy some_filter warnings in view/form.rs with focused.then(|| draw_input(..)).
- **Acceptance**: cargo clippy --manifest-path glimpse/Cargo.toml --all-targets reports no warnings; the filter prompt test still passes.

### 60. Toggle closed Inbox records with c [S]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 50, 54
- **Action**: In app.rs, make Action::ToggleClosed on the Inbox surface flip app.inbox.show_closed (moving the inbox cursor to the first visible row if its record was just hidden, and showing the same footer notice the item surfaces show); add an app test that c on the Inbox shows handled and withdrawn records. In view/mod.rs, add 'c closed' to the Inbox footer hints.
- **Acceptance**: An app unit test shows ToggleClosed on the Inbox flips inbox.show_closed and the Inbox footer hints include c closed; cargo clippy --manifest-path glimpse/Cargo.toml --all-targets stays warning-free.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 6, 7, 24, 58 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 58. Read-only item surfaces, flow-less scopes, multi-scope watching and ledger-dispatch attribution; glimpse still writes nothing (buildable increment)

— CHECKPOINT B after tasks 34, 35, 36, 37, 38, 39 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39. The stale-write precondition and its carrier adoption, the ledger and backlog control-edit facade, forms, writer thread, action menu and undo (buildable increment)

— CHECKPOINT C after tasks 46, 47, 53, 57, 59, 60 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 37, 38, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 59, 60. The input store, its verbs, facade and skill, carrier Step-0 sweeps, async agent questions, the Inbox surface, the backlog triage precondition, and the 0.13.0 release (buildable increment)

## Risks
- **Old tomlctl binary.** An older installed tomlctl silently ignores `expect` in ops, which would void the stale guard. Mitigation: carriers always pass `--on-stale`, which an old binary rejects loudly, and the surface is advertised through `tomlctl capabilities` (task 56). After Merge reinstalls tomlctl before any carrier runs.
- **Carrier prose goes live before the binary does.** `~/.claude/commands/*.md` are symlinks and `~/.claude/skills/*` junctions into this tree, so a carrier edit takes effect in every session the moment it is written, while the installed tomlctl (0.12.0) rejects `--on-stale`. Mitigation: the orchestrator runs `cargo install --path tomlctl` as soon as checkpoint B verifies, and after checkpoint C reinstalls again and junctions `flow-contract-user-inputs`; no `/review`, `/optimise`, `/review-plan`, `/backlog` or apply run starts in any session between task 36's dispatch and the checkpoint-B install, or between task 44's dispatch and the checkpoint-C install.
- **Dirty glimpse baseline.** Eleven glimpse files carry uncommitted mouse-scroll/effort-colour edits, and eight are task files (`app.rs`, `view/mod.rs`, `view/details.rs`, `keys.rs`, `model.rs`, `theme.rs`, `view/legend.rs`, `README.md`). Path-staged commits would absorb them and a task rollback would stash them. Mitigation: commit the baseline before `/implement` dispatches anything.
- **Input records as an injection path.** `author` is self-declared and sub-agents can run `tomlctl`. Mitigation: the trust boundary in Approach, "Input store", stated in the skill (task 43).
- **Weakened dead-watch detection.** The per-scope miss counter is the only dead-watch detector on Windows, and notify silently unwatches on a buffer overflow. Mitigation: per-scope streaks (task 9), with the test that a busy scope cannot mask a dead one; repo-level wakes never clear the flow caches.
- **Writer and carrier racing.** Both write the same ledger. Mitigation: every write is per-call atomic under the lock. glimpse writes carry `expect` and report stale skips; carrier writes carry `expect` with `--on-stale skip` and surface `skipped_stale`.
- **Root mismatch.** glimpse's root and tomlctl's process root could disagree, so locks would land in different directories. Mitigation: every facade write refuses on `root mismatch`.
- **Plan size.** 76 files is far past the ~25-file guideline. Mitigation: three milestone checkpoints, each a buildable and useful increment. The user chose one plan. `/review-plan` round 1 recommends gating Milestone C on a go/no-go after checkpoint B's manual smoke, or running it as its own plan: its closure needs nothing beyond checkpoint B, and it is the milestone that changes every owning carrier's Step 0.
- **Cascading gate failures from carrier prose edits.** The `cli::dispatch::tests` gates (`carrier_invokes_required_skills`, `command_lint`, `flag_table_lint`, size caps) can fail on a prose edit. Mitigation: prose tasks that show new CLI syntax depend on the CLI task, and the caps are stated in each task.
- **Key-collision creep.** New keys could collide with existing ones. Mitigation: open forms take every key first (task 34), and the new keys were checked against the current map.
- **Agent-question fan-out.** Questions posted on every empty answer could flood the Inbox in auto mode. Mitigation: the skill scopes posting to decisions that can wait for the next run, and one question per decision.

## After Merge
- `cargo install --path tomlctl`, then `cargo install --path glimpse`. glimpse links tomlctl's code, so it needs the rebuild, and the carriers shell out to the installed `tomlctl`, which must know `--on-stale` and `inputs`. The hooks run the installed `glimpse hook`, which until rebuilt strips `item_ids` from every `agents.toml` segment it rewrites — reinstall glimpse at checkpoint A too if attribution is wanted during Milestones B and C.
- Junction the new skill into the user skills directory (`claude/skills/flow-contract-user-inputs` → `~/.claude/skills/flow-contract-user-inputs`), the way the other repo skills are linked, and add the `.github/skills/flow-contract-user-inputs` mirror symlink (mode 120000; this checkout has `core.symlinks=false`, so stage it with `git update-index --add --cacheinfo`); then restart open Claude Code sessions so carriers load it.
- Restart any running glimpse panes.
- Run the manual smoke from Verification Commands: `/review` beside glimpse, then defer a finding from the action menu.
- Update the `MEMORY.md` note on glimpse with the item surfaces and the write model.

## Exploration Notes

### glimpse (working tree, with uncommitted mouse-scroll/effort-colour edits in 11 files)
- `glimpse/CLAUDE.md` rules: colours only via `TOKENS` (`theme.rs:19-70`) + README token row + legend row for marks; `keys::map` drops Release; unmapped mouse → `Step::Nothing`; selection changes via `App::select`; `watch::start` callback never blocks (posts `Control::Wake` only); every change detector reachable from `Poller::evict` or the per-scan re-stat; never verify sidecars; time-driven screen changes reflected in `App::tick_interval`; `tests/cli.rs` stays inside its sandbox. `.claude/rules/documentation.md`: no task refs / `file:NN` in source, ≤4 comment lines per declaration.
- Deps: edition 2024, rust 1.98; ratatui 0.30 (locked 0.30.2), crossterm 0.29.0 via `ratatui::crossterm` only, notify 8.2.0, pulldown-cmark 0.13, serde/serde_json(preserve_order), toml 1, `tomlctl = { path = "../tomlctl", default-features = false }` (tomlctl 0.12.0). Rustdoc deny lints on.
- Sizes >800 lines (serialisation hazards): `source.rs` 1520, `view/ego.rs` 1335, `app.rs` 1284, `view/layers.rs` 1176, `diagram/position.rs` 1036, `setup.rs` 977, `diagram/mod.rs` 828. Others: `view/mod.rs` 699, `runtime.rs` 689, `model.rs` 672, `view/details.rs` 669, `cli.rs` 459, `keys.rs` 339, `theme.rs` 372, `herdr.rs` 298, `watch.rs` 203, `legend.rs` 185, `flows.rs` 176, `state.rs` 172, `main.rs` 157, `selector.rs` 138.
- Selection is u32-task-keyed throughout: `App.selected: Option<u32>`, `Action::Select(u32)`, `Navigator::neighbor(u32, Dir)`, `Regions.tasks: Vec<(Rect,u32)>`. `App::new(Snapshot,&Config)` has ~38 call sites. `App::apply(Action) -> Option<Action>` returns only Quit/SwitchFlow. `Back` closes legend → selector → fullscreen details → details → activity → quit.
- Input: `keys::map(KeyEvent) -> Option<Action>` is context-free (`keys.rs:21`), Ctrl only `c`, Alt nothing; `runtime::handle(screen, event, host, freshest) -> Step` (`runtime.rs:327`) calls it statelessly. No text-entry mode exists.
- Watch: `watch::start(flows_root, on_wake)` one recursive watch on `.claude/flows`; `Wake = Flow(String) | All | Rewatch`; events outside flows → `Wake::All` (clears every cached stat + task fingerprint, `source.rs:589-598`).
- Poller (`source.rs:475`): fields root, slug, fetcher, events, last_fingerprint, last_revision, failed_at, last_flows, flow_stats, relist_pending, retry_after, tail. `Fingerprint` over `SNAPSHOT_INPUTS` (4 files). `TickCause = Wake|Safety|Other`; `MissedChanges::on_tick` (`:463`) any wake resets the miss streak; `MISS_LIMIT=2` → permanent `poll_ms` polling. `trait Fetcher { fetch, list_flows }`; `FakeFetcher` at `:895`.
- `source::Event` (`:32`): Snapshot, SourceError, Flows, FlowMtimes, Tail, Input. `Control` (`:47`): SetSlug, Tail, Wake, Stop.
- Host: `trait Host { draw; set_slug; set_tail }`, `FakeHost` at `runtime.rs:462`. `render_once(opts, snapshot, select: Option<u32>, w, h) -> String` (`runtime.rs:129`).
- `flows::list(root, task_stores)` lists only flows with `tasks.toml` (≥8 flows hold ledgers without one). `flows::repo_root(cwd)`.
- `state.rs` `State { view, orientation, split, panel_percent, show_implied }`, hand-listed fields in parse/render/capture/apply/save.
- Views: `view/mod.rs` `render` → header, body (`draw_view` over `ViewKind::{Layers,Ego,Diagram}`), 1-row footer, overlays (selector, legend). `footer(app, now)` (`:311`) per-mode hint vectors. `ViewKind::ALL` in `config.rs:194-231`. `view/details.rs::render(frame, area, app, scroll)` — split point `:46` (`wrap(content(app, now), width)`); `content` (`:78`) and `wrap` (`:326`) are `pub(crate)`. `selector.rs` hard-wired to `app.flows`. `Theme::status` (`theme.rs:260`) maps unknown → pending.
- CLI: `cli.rs::parse_inner` single match over flags; new flag = local + arm + `ViewArgs` field + `HELP` + test literal update; valueless flags join the `matches!` list.
- Tests: every module but `main.rs` has unit tests; `tests/cli.rs` 15 sandboxed cases; one fixture `tests/fixtures/snapshot.json` via `model::fixture()`; frame tests assert substrings (no golden files). Legend `KEYS`, README key table and footer hints are hand-synced — no test enforces agreement.
- Commands: `cargo test --manifest-path glimpse/Cargo.toml`; `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`; `cargo fmt --manifest-path glimpse/Cargo.toml -- --check` (hook-enforced).

### tomlctl
- `lib.rs` facade: `SNAPSHOT_INPUTS`, `validate_slug`, `snapshot(root,slug)`, `snapshot_if_changed`, `flow_list`, `flow_list_matching`, `record_agent(harness,payload)` (changes cwd + pins root process-wide). Each wrapper calls `io::silence_advisories()` (stderr only; stdout printing lives in dispatch layers). Tested by `tests/library_facade.rs` (wrapper JSON == CLI JSON via `TOMLCTL_ROOT`).
- `io.rs`: `mutate_doc(file, allow_outside, IntegrityOpts, OnMissing, f) -> Result<bool>` (:821), `mutate_doc_conditional` (:929, closure returns bool to skip write), `mutate_doc_plan` (:983, used by `items apply`), `with_exclusive_lock` (:1196), `lock_path_for` (:1150) → `repo_or_cwd_root()/.claude/.locks/<sha256>.lock`. `repo_or_cwd_root()` (:1602) = `TOMLCTL_ROOT` env (read per call) else `static OnceLock` (:1615). ~45 callers; no parameter-level root seam. `OnMissing { Error, Create(..) }` (:698), `on_missing_for` (:754). Sidecar-then-TOML atomic write (:882, :2019). `IntegrityOpts{write_sidecar, verify_on_read, strict}`.
- `items.rs`: ops are untyped JSON; executors `apply_op_indexed` (:698, arms :720-827) and `apply_single_op` (:918) read only `op`,`id`,`json`,`unset` — **unknown keys (a future `expect`) are silently ignored by older binaries**. `items_apply_parsed_to_opts` (:626), `compute_apply_mutation` (:1307; also called by `items_sweep.rs:556`). Update merge `items_update_value_to` (:524, :555-565) and twin `update_at_index` (:860, :898-910) — CAS check fits before `apply_dedup_id_on_update` (:114-161). `Item::validate` (:1827) is `#[allow(dead_code)]`, unused; :1840 maps wontapply → `wontfix_rationale`; test `item_validate_flags_wontapply_missing_rationale` (:4123) asserts it. CLI `ItemsOp::Apply` (`cli/types.rs:1758`), dispatch `cli/dispatch.rs:888-960`, `MAX_OPS_PER_APPLY`.
- Backlog: `triage.rs` private `apply_transition(doc, ids, &Transition, today)` (:189), private `enum Transition {Promote, Dismiss, Resolve, Reopen}`; `dispatch` uses `repo_or_cwd_root()` (:283), `schema::backlog_path()` (:294), `mutate_doc` (:298), prints (:311). `schema.rs`: `TERMINAL_CLUSTERS`, `MANAGED_FIELDS`, `CLAIM_FIELDS`, `clear_for_transition` (:224), `required_fields`. Array must be `backlog`, never `items` (items writes overwrite content-derived dedup_id). No verb edits `tags` on an existing row.
- `clusters.rs:28` `pub(crate) fn items_clusters(doc, root, ids)`.
- Agents: `correlate.rs` `struct Dispatch { slug, task_ids: Vec<u32> }` (:38), regex `tasks show ([0-9]+) --slug (...)` (:190) behind `line.contains("tasks show")` (:197). `record.rs::record` (:400), `choose_flow` (:327) dispatch slug → own row → session affinity (:345-347); task ids dropped if slug differs (:499); writes via `mutate_doc_conditional` (:547). `schema.rs` `AgentRecord` (:45), `Segment { task_ids: Vec<u32>, started_at, ended_at }` (:70), `segment_from_toml` (:448) rejects non-integers. glimpse mirrors: `model.rs:266` `Vec<u32>`, `hook.rs:36` `Vec<u64>`.
- Commands: `cargo test --manifest-path tomlctl/Cargo.toml` (lib gates: `--lib -- cli::dispatch::tests`), `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`, fmt check; editing `tomlctl/src/lib.rs` triggers the hook's glimpse `clippy --locked`.

### Harness prose (amendment sites)
- Early-read/late-write carriers: `review.md:37` (read), `:74` (checkpoint apply), `:80` (Step 3), `:86-88` (dispositions batch); `optimise.md:39`, `:87`, `:97` (no Step-4 disposition handler); `review-apply.md:38,61`; `optimise-apply.md:38-40,61`; `review-plan.md:120` ("sole writer"), `:186`, `:205`, `:212`, `:217`; apply-pipeline `SKILL.md:24,61,314-333,377-383` (479/500 lines); `references/verification.md:74-101,109-124` (op values `add|update|remove` at :116-118), `:135-138`; `references/pre-analysis.md:48-59`; rollback skill `:30,:60` (description 1016/1024 chars); `ledger-disposition-sweep/SKILL.md:46`.
- `flow-contract-ledger-schema/SKILL.md`: `:87-88` wontapply companion, `:106`, `:185` op shape, `:204`, `:206` "concurrent invocations are safe", `:213`.
- Backlog: `backlog-capture/SKILL.md:3` (description 844 chars), `:73-80`, `:82-87`, `:102-105` "orchestrator is the only writer"; `commands/backlog.md:16,18,36`; `CLAUDE.md:101-102`; `tomlctl/references/backlog.md` 595/600 lines (new inbox text needs its own reference file); `write.md:219-241` (`items apply`), `:61-68`, `:378`.
- Shared block `backlog-candidates` (`implement-deep.md:92-108`, `implement-lite.md:114-130`) contains "The orchestrator is the only writer" — edit byte-identically in both.
- Dispatch preambles: `review.md:55` includes the ledger path; `optimise.md:63` path below the divider; `review-plan.md:65-69` no path/ids; apply `agent-prompt-contract.md:16-18` ids but no path/slug.
- Gates: `carrier_invokes_required_skills` (`skills.rs:1193`), `command_lint` (`lint.rs:261`, bash-fenced `tomlctl` lines parsed by clap — JSON op keys are not), `flag_table_lint` (`lint.rs:665`), SKILL.md ≤500 lines, references ≤600, descriptions ≤1024, links resolve.
- `glimpse/README.md:5` "It is read-only".

### Backlog
Live rows in `glimpse/`, `tomlctl/`, `claude/` (all `open`, none promoted):
- `B-f4bd798a` (debt) — glimpse `poll_ms` has no upper bound.
- `B-5c699ce5` (debt) — glimpse header test fixture uses stale error text 'tomlctl failed'.
- `B-ad940fc4` (debt) — `agents record` reads the whole transcript (≤4 MiB) on every SubagentStop.
- `B-b58537a5` (debt) — flow active/doctor/init rewrite TOML unconditionally, bypassing mutate_doc's unchanged-bytes skip.
- `B-f010d1ce` (direction) — `agents record` re-derives the dispatch from the whole transcript on Stop instead of the live agents.toml row.

## Research Notes

### Pre-plan research (2026-10-02, four research-deep lenses, orchestrator spot-checked)
- **Sources and vocabularies** (real-file scan): review 25 files / 854 rows (`R{n}`; severity/effort/category, 50 distinct categories vs 8 in schema; statuses open, deferred, fixed, wontfix, verified-clean); optimise 6 / 87 (`O{n}`; statuses open, deferred, applied, wontapply); plan-review 37 / 730 (`P{n}`, 25 rows lack `id`; severity/category, no effort; statuses open, merged, discarded); backlog 210 (`[[backlog]]`, `B-<hash>`; kind incl. `flaky-test`/`other`, area, tags; statuses open, promoted, dismissed, resolved). Companions (`Item::validate`): fixed/applied → resolved+resolution; deferred → defer_reason+defer_trigger; wontfix → wontfix_rationale; verified-clean → verified_note. Backlog companions per `TERMINAL_CLUSTERS`; reopen takes `reopen_rationale`.
- **Lost update.** Carriers read at run start and write much later with no precondition; `items apply` update merges and never clears prior companions, so a glimpse wontfix mid-/review-apply ends as `fixed` with both companions. Rollback likewise reverts a human edit. Impact: CAS precondition on `items apply` + carrier prose.
- **Field editability.** review/optimise: severity/effort/category editable (recomputes `dedup_id`; /review's merge keys on file+symbol+summary, not dedup_id); file/symbol/summary read-only. plan-review: classification edits are reverted next round (`review-plan.md:217`) → read-only; only open→discarded. backlog: kind/area/summary feed the content-derived id → read-only; only tags (and context) safe. Never: id, rounds, first_flagged, instances, related, depends_on, flow, dedup_id. Do not bump `last_updated` on review/optimise (apply freshness gate compares it to commit dates).
- **Live-signal reality.** /review, /optimise, /review-plan write findings in ≤2 bursts after the vet pass (interim checkpoint, consolidation). vet_events timestamps are orchestrator-authored (order only). Apply writes fixed/applied in one atomic call after verification; rollback writes rollback_events + reverts to open; clusters are console-only. Per-agent progress needs agent attribution, which today only /implement dispatches get.
- **Handoff** (deferred by user decision): `herdr pane send-text` (no Enter), `herdr agent prompt` (refuses at dialogs); must be multiplexer-agnostic (herdr + tmux) when picked up.
- **glimpse architecture.** Don't generalise the u32 selection model — item surfaces get their own state (string ids, cursor, filter, marks). Non-recursive watches for `.claude/` and the flow-less ledger dirs; per-scope miss counting (a busy scope's wakes must not mask a dead flows watch). Writes on a dedicated serial writer thread (lock wait up to 30 s, `TOMLCTL_LOCK_TIMEOUT`); no optimistic model update — mark the row "saving" and let the poller's re-read confirm, otherwise the status-flash diff misfires.
- **Interaction prior art** (lazygit, k9s, gh-dash, aerc, taskwarrior-tui, Linear): Space mark+advance, `V` mark all filtered, Esc clears marks first; `/` filter, `g` group-by cycle, `S` sort cycle (keeps `s` = flow selector); `m` action menu with verbs from the store's schema; `e` edit classification; `n` quick capture; `u` undo own last write (CAS-guarded); inline footer prompt; two-field modal for deferred; facet row under the header; group headers `▾ warning (5)`; live arrivals flash + `+N new`, cursor never moves. ratatui has no multi-select list (index-only `ListState`) — keep id-keyed marks.
- **Staging inbox.** Own file `.claude/backlog-inbox.toml`, array `[[staged]]` (not `items`, avoids dedup stamping; outside backlog.toml's lock/validator). Row: summary (req), kind?, area?, note?, flow?, created, origin="glimpse". Consumer: a drain step in `/backlog` (check → add, kind+area settled before `check`).
- **Policy text** contradicted by a human writer: `backlog-capture/SKILL.md:102` + `CLAUDE.md:101` (only writer), `:78` (status only by triage/reconcile — fine if glimpse calls triage logic), `review-plan.md:120` (sole writer), `review.md:86` (dispositions on user reply), ledger-schema `:206` (concurrency is per-call, not per-run), `:88` (wontapply companion).
- **Tangential**: apply-pipeline `SKILL.md:253` claims the DISPATCH header "is captured in the execution record"; apply commands never write it.

### Text input crate — Agent-1 (research-lite, lens text-input-crate)
- **tui-input 0.15.5** (published 2026-09-26, newest on crates.io as of 2026-10-02). Add `tui-input = "0.15.5"` with default features: `ratatui-crossterm` enables `ratatui/crossterm` and imports through `ratatui::crossterm`; never enable the implicit `crossterm` feature. Normal deps `unicode-segmentation ^1.13` and `unicode-width ^0.2.2` are already locked (1.13.3, 0.2.2 — verified in `glimpse/Cargo.lock`), so only the `tui-input` entry is new. No `rust-version`. MIT, active (six 2026 releases), OSV: no advisories. Grade: high.
- API: `Input::new(String)` / `.with_value(s)` (cursor at end); `tui_input::backend::crossterm::EventHandler::handle_event(&Event) -> Option<StateChanged>`; `value()`, `visual_cursor()`, `visual_scroll(width)`; render via `Paragraph::new(value).scroll((0, scroll))` + `frame.set_cursor_position(..)`. Consumes chars, Backspace/Ctrl+H, Delete, Left/Right, Ctrl+Left/Right, Home/Ctrl+A, End/Ctrl+E, Ctrl+U/W/K/Y, Alt+Backspace. Returns `None` for Enter, Esc, Tab, BackTab, Up/Down — the caller routes those. Grade: high.
- Only Press/Repeat handled (Release → None). `(Char, CONTROL|ALT)` inserts as text (Windows AltGr); CONTROL|ALT|SHIFT is dropped. Grade: high (behaviour), medium (AltGr+Shift edge). Counter: if a `cargo tree -d` after adding shows two crossterm versions, the reuse claim is wrong.
- Impact: add the dependency; prompt mode routes Enter/Esc itself and gives every other key to `handle_event`.
- Searched: crates.io API (tui-input versions + deps, ratatui-textarea), static.crates.io tui-input 0.15.5 source, OSV, GitHub API — fetched 2026-10-02.

### File watching — Agent-2 (research-lite, lens file-watching)
- One `RecommendedWatcher` holds several `watch()` calls with mixed `RecursiveMode`; on Windows each gets its own handle keyed by path (notify-8.2.0 `windows.rs:238`, verified). A NonRecursive `.claude` watch reports only direct children: `.locks/` churn arrives as `Modify(Any)` on the `.claude/.locks` *directory entry* (one per lock op), flows writes arrive not at all except as a dir-entry modify on `.claude/flows`. Today's `classify` (`watch.rs:40-48`, verified) maps any non-flow path to `Wake::All` → needs an allowlist by final file name. Watching the same path twice on Windows leaks the old handle → duplicate events. Grade: high.
- Missing path: Windows returns `Error::generic("Input watch path is neither a file nor a directory.")` (`windows.rs:538-541`, verified), Linux `PathNotFound`. Pattern: the NonRecursive `.claude` watch reports `Create` when `reviews/` etc. appear → add that dir's watch then, and rescan once. `unwatch`+`watch` work at runtime. Grade: high.
- Temp + rename over existing (Windows, observed via .NET FileSystemWatcher, same API): `Create .tmpX` → `Modify .tmpX` → `Remove backlog.toml` → `Modify(Name(From)) .tmpX` / `Modify(Name(To)) backlog.toml`. tomlctl stages the temp in the target's own dir (`io.rs:2100-2108`, verified). Match on final names; ignore `.tmp*` and `*.sha256`. Grade: medium (Rust `fs::rename` sequence unconfirmed).
- No filtering API: `Config` is ignored by the Windows backend (`windows.rs:563`); single-file watches go stale on Linux after rename-over. Use NonRecursive directory watches filtered in the callback. Cost of `.locks` churn: one cheap callback per lock op. Grade: high.
- Windows caveats: 16 KB buffer per watch; an unknown ReadDirectoryChangesW error (incl. overflow) logs and silently unwatches (`windows.rs:355-367`, verified) — no `Rescan`; a deleted watched dir is silently unwatched; handles pin dirs against rename. The safety-tick miss counter remains the only dead-watch detector. Grade: high (code), medium (which error code overflow yields).
- Searched: installed notify-8.2.0 source only; experiment `scratchpad/fsw.ps1`.

_Vet (recorded here; plan mode forbids the `vet_events` ledger write — no flow exists until Phase 9):_ Agent-1 4 sampled / 0 dropped / 0 downgraded; Agent-2 4 sampled / 0 dropped / 0 downgraded. Spot-checks: lockfile versions, notify `windows.rs:238`, `:355-367`, `:538-541`, glimpse `watch.rs:40-48`, tomlctl `io.rs:2100-2108`.
