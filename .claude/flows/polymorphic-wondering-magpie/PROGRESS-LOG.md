<!-- Generated from execution-record.toml. Do not edit by hand. -->

# glimpse — in-process tomlctl, file watching, off-thread transcript tail — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | split-tomlctl-into-a-library-and-a-thin-binary | 2026-09-30 | | 3 files |
| E7 | point-the-pre-commit-gates-at-the-library | 2026-09-30 | | 1 file |
| E8 | add-a-root-taking-flow-listing | 2026-09-30 | | 2 files |
| E9 | expose-the-in-process-facade | 2026-09-30 | | 3 files |
| E16 | add-the-tomlctl-path-dependency-to-glimpse | 2026-09-30 | | 2 files |
| E17 | add-the-notify-dependency-to-glimpse | 2026-09-30 | | 2 files |
| E18 | fetch-snapshots-and-flow-lists-in-process | 2026-09-30 | | 3 files |
| E20 | record-hook-events-in-process | 2026-09-30 | | 2 files |
| E21 | update-root-claudemd-for-the-library-split | 2026-09-30 | | 1 file |
| E22 | add-the-watch-module | 2026-09-30 | | 2 files |
| E23 | update-the-remaining-agentstoml-writer-docs | 2026-09-30 | | 2 files |
| E24 | update-the-tomlctl-skills-agentstoml-writer-rule | 2026-09-30 | | 2 files |
| E25 | remove-the-tomlctl-config-key | 2026-09-30 | | 3 files |
| E35 | drive-the-poller-from-the-watcher | 2026-09-30 | | 1 file |
| E39 | remove-the-watch-modules-dead-code-allowance | 2026-09-30 | | 1 file |
| E45 | move-the-transcript-tail-onto-the-poller-thread | 2026-09-30 | | 3 files |
| E48 | update-glimpses-readme-and-claudemd | 2026-09-30 | | 2 files |
| E49 | measure-mimalloc-against-the-system-allocator-for-tomlctl | 2026-09-30 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E10 | falsifier run by adding --verify-integrity to the CLI side of the flow_list comparison in tomlctl/tests/library_facade.rs | 2026-09-30 | | list_all takes no verify parameter; flipping it needs an edit to tomlctl/src/flow/list.rs, outside the task files. Verifying one side only shows the same divergence and it fired. | — |
| E19 | falsifier retargeted to the poller revision comparison in glimpse/src/source.rs | 2026-09-30 | | the test uses FakeFetcher and never reaches InProcessFetcher or serde, so blanking revision in the fetcher left it green (0 failures); disabling the last_revision comparison fails it (1 failure). InProcessFetcher is exercised only by task 8s once_renders_a_live_flow_in_process. | — |
| E26 | once_renders_a_live_flow_in_process in glimpse/tests/cli.rs uses slug live-flow-demo and runs from cwd/nested, asserting the task row too | 2026-09-30 | | from cwd the repo_root unwrap_or(cwd) fallback passes without the ancestor walk, so the test proved nothing about it; a one-letter slug is too weak to assert on stdout. Falsifier cutting the ancestor walk to take(1) now fails it. | — |
| E33 | tasks 9 and 10 (checkpoint D) committed in checkpoint C train; task 11 dispatch held until C committed | 2026-09-30 | | task 9 shares glimpse/Cargo.toml and Cargo.lock with task 5, and task 10 shares glimpse/src/main.rs with task 6, so their edits are not separable in the tree; overlapping groups merge per the commit-train rule. Task 11 (source.rs, owned by C member 6) was held so the poller rewrite stays out of C. | — |
| E36 | Mode::Watching { _watcher } struct variant in glimpse/src/source.rs | 2026-09-30 | | the watcher is held only so dropping it stops the watch; the unread tuple field was flagged as dead code | — |
| E37 | new step() -> Option<bool> in glimpse/src/source.rs with tick() kept as a test-only wrapper | 2026-09-30 | | tick() already returns a receiver-alive bool that existing tests assert on | — |
| E38 | minted task 18 to remove the module-level allow(dead_code) from glimpse/src/watch.rs | 2026-09-30 | | task 10 added the allowance only because nothing used watch yet; task 11 wired it in, so the attribute now only hides future dead code. Cheap, in a file this run touched, covered by clippy. | — |
| E46 | runtime resets screen.tail to TailState::default() when the tail target changes, and run() starts with TailState::default() (glimpse/src/runtime.rs) | 2026-09-30 | | without the reset the previous agent entries draw under a newly selected agent until the first Event::Tail; the runtime tail is display-only since the poller owns the confined TailState | — |
| E47 | added Deadlines::due(now) -> Due in glimpse/src/source.rs beside next_wait | 2026-09-30 | | the run loop and the new test share one decision function instead of the test re-implementing the loop | — |
| E50 | tomlctl mimalloc is now opt-in (default = []) and the [features] comment in tomlctl/Cargo.toml was reworded | 2026-09-30 | | the task decision rule fired: typical-call loss ~3 ms (>1 ms) and no demonstrated heavy-verb gain. The [features] comment told library consumers to turn mimalloc off, which is no longer needed. glimpse default-features = false is now redundant but harmless. | — |
| E74 | tomlctl::snapshot is now (root, slug) with slug validation; SNAPSHOT_INPUTS is public; record_agent shares record_value with the CLI | 2026-09-30 | | /review found the in-process path had dropped the CLI's slug validation (glimpse --slug ../../x read outside .claude/flows) and that glimpse hand-copied the snapshot's input-file set. The user confirmed the signature change; the facade validates via flow::validate_slug, owns SNAPSHOT_INPUTS, and record_agent now runs the same record_value the CLI tests cover (the CLI reads the payload before rejecting an unknown harness). | — |
| E75 | the flows watch no longer follows symlinks below the flows root | 2026-09-30 | | notify's default follows symlinks, so on inotify/kqueue a repo-content link under .claude/flows (e.g. to /) made the recursive watch walk the target tree, exhaust max_user_watches and retry every poll tick. The watcher now uses Config::default().with_follow_symlinks(false); canonical_root still resolves a symlinked root. | — |
| E76 | Event::Tail carries a read-only TailView rather than the poller's TailState, and a tail for a stale path is dropped | 2026-09-30 | | Sending the live reader let the UI thread call refresh(), leaving 'the UI thread does no file I/O' enforced only by comment; TailView (path, entries, tokens, rejected) makes it a type rule. The runtime also ignores a queued tail whose path no longer matches the selected agent, so the old transcript is never drawn under the new one. | — |
| E77 | Watcher safety rules tightened after /optimise: miss streak resets only on a wake, watch-start retries back off, task-store flows re-stat every scan | 2026-09-30 | | notify 8.2's Windows backend drops a failed watch without an Err or Rescan, so the missed-change counter is the only dead-watch detector there, and a clean safety tick resetting it meant sparse writes never tripped it (up to 10 s latency for the session). On Linux a MaxFilesWatch error made the per-tick retry rebuild a watcher (thread + tree walk) twice a second. On Windows the enumerated directory mtime can lag a non-rename file creation, so the dir-mtime cache could hide a change from a safety tick. Now: only a wake resets the streak and a fresh watch starts with a fresh one; failed starts back off from poll_ms to 60 s; task-store flows are re-stat'd every scan and the dir-mtime cache gates only store-less flows. | — |
| E78 | Facade gained snapshot_if_changed and flow_list_matching after /optimise | 2026-09-30 | | snapshot_if_changed returns None straight after the revision hash when it matches the caller's last revision, so an identical-bytes rewrite (e.g. a same-day set last_updated) no longer costs glimpse a full parse, graph and JSON build. flow_list_matching applies a slug predicate before any context.toml read, so glimpse's relist parses only the flows that have a task store (7 of 46 here) and takes tasks mtimes from the poller's scan. snapshot and flow_list keep their signatures and output; the CLI JSON contracts are unchanged. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-30 | 78 entries: status-transition × 2, task-completion × 18, verification × 38, checkpoint × 5, deviation × 15 | 11ea3bc, 33088df, 4b8ae69, 575cd6c, 5f866a6, 6c2b815, a483efa, a7103bc, bad0772, c0c978e, c1f8420, c8949f0, d7d9f38, fc13dff, fe76011 |
