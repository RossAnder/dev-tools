Verdict: The spec and plan records already enter the snapshot. `plan.toml` and `tasks.toml` are fingerprinted, hashed into `revision` and parsed (tomlctl/src/tasks/snapshot.rs:57-111), so carrying them changes only what the snapshot emits (snapshot.rs:166-191, 234-253). `docs/flows/<slug>/` is on no part of the path: it is not a snapshot input, it is not watched and it is not statted. It needs a feed of its own.

## Answer

**How it works today.** The poller thread (`glimpse/src/source.rs`, `Poller`) stats the five files named in `tomlctl::SNAPSHOT_INPUTS` (`tasks.toml`, `execution-record.toml`, `agents.toml`, `context.toml`, `plan.toml`; tomlctl/src/lib.rs:68-74). That gives a fixed-size `Fingerprint` (glimpse/src/source/scan.rs:19, 30-33). The flow is fetched again only when that fingerprint moves or a retry falls due (source.rs:488-532). The fetch calls `tomlctl::snapshot_if_changed(root, slug, known)` (scan.rs:261, lib.rs:93-110). That function hashes the raw bytes of the five files and returns `None` when the hash equals the known revision. Otherwise it parses the files, builds the graph and emits one JSON envelope, which glimpse deserialises into `model::Snapshot` (scan.rs:266) and swaps in through `App::apply_snapshot` (app.rs:1141).

The notify watch only wakes the poller. It covers `.claude/flows` recursively and `.claude` non-recursively, plus the flow-less ledger directories (glimpse/src/watch.rs:243-250), and an allowlist maps each event to a `Wake` (watch.rs:165-212). The safety tick and the per-scope miss streaks catch what the watch misses (source/mode.rs). Ledgers and `inputs.toml` travel apart from the snapshot, as subscribed `Feed`s with their own stat, revision and event (source.rs:79-143, 357-414).

**What carrying the spec and plan records takes:**
- **tomlctl.** The snapshot already parses the whole store (`specs`, `milestones`, `changes`, `file_notes`, `finding_links`, `backlog_links`; tasks/schema.rs:107-126) and the `PlanStore` (decisions, contracts, approvals, runs, handovers, binding; plan/schema.rs:142-156). It then keeps only `running_milestone` and the v1 row fields. So the change is new envelope keys, or new per-task keys, plus:
  - the two key-order tests (snapshot.rs:323-344 and tests/tasks_snapshot.rs:237-258);
  - the envelope table (claude/skills/tomlctl/references/tasks-store.md:234-251).

  `revision`, `SNAPSHOT_INPUTS`, the glimpse fingerprint and the watch all stay as they are.
- **glimpse.** New `#[serde(default)]` structs in `model.rs` (Snapshot at model.rs:15-35), and keys added to `tests/fixtures/snapshot.json` so the round-trip test (model.rs:670) still guards misspellings. Older and newer tomlctl copies degrade rather than fail (model.rs:4-7). glimpse embeds its own tomlctl, so a rebuild of glimpse is needed.

**What carrying `docs/flows/<slug>/` takes:** a new feed. It needs four parts:
1. A tomlctl library read that takes `root` explicitly. It lists the brief, `design/*` and `notes/**` with a revision, and reads one file's content. It keeps the slug rule and the symlink containment of `store::locate_brief`.
2. A walk-and-stat fingerprint, re-run on every scan or evicted by a wake.
3. A watch on `<root>/docs/flows`, added the way flow-less ledger directories are added when they appear late.
4. A `WatchScope` variant so the new watch keeps its own miss streak.

Folding the docs into the snapshot instead would tie roughly 70 files (about 300 KB on `v2-apply`) to every `tasks.toml` status write, and every note write to a graph rebuild.

## Findings

- **Library/API or file**: tomlctl/src/tasks/snapshot.rs:57-111 `snapshot` (inputs hashed and parsed), :166-191 (emitted keys), :234-253 `task_json`
- **Source**: file read. The `[store_raw, …, plan_raw]` hash covers `plan.toml`. `schema::from_toml` parses `specs`/`milestones`/`changes` (tasks/schema.rs:722-797). The plan store is parsed only to compute `running_milestone` (snapshot.rs:105-111).
- **Evidence-grade**: high
- **Finding**: The spec, milestone, change, file-note, finding-link and plan records are already snapshot inputs that get parsed and then dropped. Carrying them is a change to what the snapshot emits, with no change to how it is fetched.
- **Details**: `Store` holds `specs`, `milestones`, `changes`, `file_notes`, `backlog_links` and `finding_links` (tasks/schema.rs:107-126). `PlanStore` holds `decisions`, `contracts`, `approvals`, `runs`, `handovers` and `binding` (plan/schema.rs:142-156). Neither has `Serialize`, so the JSON is built in either of two ways: by hand, as `tasks show` does, or via `crate::convert::toml_to_json` over `schema::to_toml` (tasks/schema.rs:782, plan/schema.rs:403), which is the CRLF-normalised form. A cyclic or dangling DAG refuses the whole snapshot (snapshot.rs:114), and the plan records would vanish with it.
- **Counter**: A design that needs records from a file outside `SNAPSHOT_INPUTS` (for example `review-v2.toml` for a linked finding's text) would also need a new fingerprint slot or a feed. `review-v2.toml` is 487 KB on `v2-apply` and is not an input today.
- **Impact on plan / report**: Touches tomlctl/src/tasks/snapshot.rs, tomlctl/tests/tasks_snapshot.rs, claude/skills/tomlctl/references/tasks-store.md, glimpse/src/model.rs and glimpse/tests/fixtures/snapshot.json. Not touched: lib.rs `SNAPSHOT_INPUTS`, source.rs, scan.rs, watch.rs.

- **Library/API or file**: tomlctl/src/tasks/show.rs:176-181 `premises_json`
- **Source**: file read. It emits `{claim, check, at}`, while `Premise` has `probe: Option<Probe>` (tasks/schema.rs:344-351). `accept_json` (show.rs:143-174) does emit `probe`.
- **Evidence-grade**: high
- **Finding**: Reusing `tasks show`'s `spec_json` for the snapshot would silently drop the premises' probe verdicts, which success criteria 1 and 10 need. The snapshot's spec emission must add `probe` to each premise.
- **Details**: `v2-apply` holds 64 `[specs.premises.probe]` tables. The obvious reuse of `spec_json` (show.rs:129-141, private) would lose every one of them. The fix is to emit `probe` in `premises_json`, which also changes `tasks show --part spec` output, or to give the snapshot its own premise emitter.
- **Counter**: If a test or caller asserts that `tasks show` premises carry exactly three keys, widening `premises_json` breaks it. Grep `tomlctl/tests` for `"premises"` before choosing which path to widen.
- **Impact on plan / report**: Decides whether the plan widens a shared helper (tomlctl/src/tasks/show.rs) or writes a second emitter.

- **Library/API or file**: glimpse/src/watch.rs:243-250 `start`; glimpse/src/source/scan.rs:19 `Fingerprint`; tomlctl/src/lib.rs:68-74 `SNAPSHOT_INPUTS`
- **Source**: file read. The watch roots are `claude_dir/flows` (recursive) and `claude_dir` (non-recursive). The fingerprint is `[FileStat; SNAPSHOT_INPUTS.len()]` over names joined onto `.claude/flows/<slug>/`.
- **Evidence-grade**: high
- **Finding**: `docs/flows/<slug>/` (brief, `design/`, `notes/`, `notes/run/`) needs its own feed. The pieces are a tomlctl read function, a walk-and-stat change detector, a `docs/flows` watch scope with its own `WatchScope` and miss streak, a new `Feed` and `Event`, and handling in `runtime.rs`. needs-plan
- **Details**: A fixed array of flow-directory file names cannot describe a nested tree with a variable file count. `place` in watch.rs:165-212 drops every path outside `.claude`. glimpse/CLAUDE.md says that "a new change detector has to be reachable from an eviction or the every-scan re-stat". A directory's mtime does not move on an in-place edit, so the detector re-stats each file on every scan (about 70 stats on `v2-apply`). `docs/flows` may not exist, so the watch must be added late, like `Mode::add_scope` (mode.rs:173-189) does, and the allowlist must ignore the Write tool's temporary files. On Windows the watch pins the directories it covers (glimpse/CLAUDE.md), which now includes `docs/`.
- **Counter**: If the plan decides glimpse reads notes on demand only when a note is opened, the change detector and the watch scope can be dropped for that view. A note list with verdict lines still needs a listing that refreshes.
- **Impact on plan / report**: Files: glimpse/src/source.rs, glimpse/src/source/scan.rs, glimpse/src/source/mode.rs, glimpse/src/watch.rs, glimpse/src/runtime.rs and tomlctl/src/lib.rs (new pub fn). The pieces have to move together, so this is one plan item with ordered steps (library read, then feed, then watch scope).

- **Library/API or file**: tomlctl/src/plan/store.rs:51-54 `authored_dir`, :83-95 `locate_brief`
- **Source**: file read. `authored_dir` joins onto `repo_or_cwd_root()` (the process root). The library facades take `root` explicitly and build their paths from it (lib.rs:100-104).
- **Evidence-grade**: high
- **Finding**: A library read of the docs directory cannot call `authored_dir`. It must build `<root>/docs/flows/<slug>` from its `root` argument and keep `validate_slug` and the `locate_brief` symlink containment, so glimpse cannot be led to read outside the flow's directory.
- **Details**: `locate_brief` canonicalises the brief and refuses any path that leaves `authored_anchor`, and `authored_anchor` also resolves through the process root. A root-parameterised variant, or the root passed through, is needed for every file the feed lists, not only the brief. Writes are not involved, so the facade's `root mismatch` guard does not apply.
- **Counter**: If glimpse calls `tomlctl::ensure_process_root(root)` before the read, the process-root functions become usable as they are. That couples the read to process state, and the poller thread does not call it today.
- **Impact on plan / report**: The new pub fn's contract in tomlctl/src/lib.rs, and a helper in tomlctl/src/plan/store.rs.

Outside this lens, for the planner: the success criterion about previewing "the source it cites, as it was when the note was written" needs a git read at a commit, and nothing on this path reads git. That belongs to the research-note lens.

Coverage: read in full glimpse/src/source.rs, glimpse/src/source/scan.rs, tomlctl/src/tasks/snapshot.rs, glimpse/src/watch.rs:1-252 and docs/flows/glimpse-v2-visibility/brief.md. Read in part tomlctl/src/lib.rs:50-140, model.rs:1-60, mode.rs:140-199, app.rs:1139-1168, tasks/show.rs:40-209, plan/store.rs:1-110, tasks/schema.rs (`Store`, `TaskSpec`, `Premise`), plan/schema.rs:141-156, tasks-store.md:219-297 and docs/ideas/v2-pipeline.md §"Flow state and glimpse". Checks run: table counts and sizes of the v2-apply stores and docs directory; a sweep of `SNAPSHOT_INPUTS`/`snapshot_if_changed`/`running_milestone` consumers. Ruled out: a revision or fingerprint change for the plan records (already covered); a frozen envelope contract (tasks-store.md's "Frozen contracts" does not list the snapshot keys); the docs files breaking the rule that glimpse parses no TOML of its own (they are markdown, and glimpse already reads transcripts directly).
