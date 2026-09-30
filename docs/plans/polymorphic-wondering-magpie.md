# Plan: glimpse — in-process tomlctl, file watching, off-thread transcript tail

**Plan path**: `docs/plans/polymorphic-wondering-magpie.md`
**Created**: 2026-09-30

## Summary

glimpse stops starting processes on its hot paths and stops waking up when nothing has changed.
tomlctl becomes a lib+bin crate. A small facade in `tomlctl/src/lib.rs` (`snapshot`, `flow_list`, `record_agent`) lets glimpse build snapshots, list flows and record hook events in-process. That removes the `tomlctl` spawns from the TUI and from `glimpse hook`, along with the capabilities probe, the `OLD_TOMLCTL` check and the `tomlctl` config key.

The 500 ms poller becomes a `notify` watcher that only wakes the poller. The existing fingerprint logic still decides what changed. A ~10 s safety tick stays, with an automatic fallback to `poll_ms` polling on any OS or filesystem where events don't arrive.

The transcript tail moves onto the poller thread and arrives as `Event::Tail`. mimalloc for glimpse was researched and dropped. tomlctl's own mimalloc becomes an optional default feature, so glimpse builds without it.

Scrutinise first:
- Approach, "Watcher as a wake-up signal": the fallback and missed-change rules are what answer the earlier plan's objection to notify (`docs/plans/lively-twirling-babbage.md`).
- Approach, "tomlctl as a library": the `--bin tomlctl` → `--lib` gate switch. Without it the pre-commit test gate silently runs 0 tests.
- Risks: glimpse now compiles in its own copy of tomlctl and must be reinstalled whenever tomlctl changes.

Before `/implement`: the glimpse tree must be clean (Risks, "Precondition: a clean glimpse tree").

## Context

On this machine Sophos makes every process start slow (~28 ms median for `tomlctl --version`). glimpse pays that on every snapshot change (`tomlctl tasks snapshot`), on every flow-list refresh (`tomlctl flow list`), and on every subagent hook event. For hook events the cost is doubled, since `glimpse hook` itself spawns `tomlctl agents record`. Each call also does a JSON round-trip and needs a version-drift probe (`tomlctl capabilities`).

Separately, the poller wakes every 500 ms even when idle. Its directory-mtime cache can't see in-place edits, which is a documented gotcha in `glimpse/CLAUDE.md`. The transcript tail also reads files on the UI thread before each redraw.

The intended outcome:
- no process start between a flow file changing and glimpse redrawing;
- no periodic work beyond a 10 s safety tick while a watch is healthy;
- in-place edits seen;
- no file I/O on the UI thread for the tail;
- all of it on Windows, Linux and macOS.

## Scope

- **In scope**:
  - tomlctl lib+bin split with an optional `mimalloc` feature.
  - A clap-free facade for snapshot, flow list and agent recording.
  - Root-taking flow listing.
  - Advisory silencing for in-process callers.
  - Pre-commit: switch `--bin` to `--lib`, and add a narrow glimpse clippy gate.
  - glimpse in-process fetch, flow list and hook recording.
  - Removal of the capabilities probe and the `tomlctl` config key.
  - A `notify` 8.2.0 watcher with safety tick and fallback.
  - The transcript tail on the poller thread.
  - Docs in `glimpse/README.md`, `glimpse/CLAUDE.md` and root `CLAUDE.md`.
- **Out of scope**:
  - mimalloc in glimpse (dropped; see User Decisions).
  - Watching transcript files.
  - `flows::repo_root`'s one-time `git rev-parse` at start-up.
  - `app.rs` `refresh_stale` transcript mtime stats (one `stat` every 30 s).
  - Upgrading to notify 9.0 (still at rc).
  - Any change to the `tasks snapshot` JSON contract or its docs.
- **Affected areas**: `tomlctl/Cargo.toml`, `tomlctl/src/main.rs`, `tomlctl/src/lib.rs`, `tomlctl/src/io.rs`, `tomlctl/src/flow/list.rs`, `tomlctl/tests/library_facade.rs`, `.githooks/pre-commit`, `glimpse/`, `CLAUDE.md`

## User Decisions

- **Scope** — one plan covering all three items (library dep, file watching, transcript tail off-thread). Prompted by the Phase 1 scope rule (items could ship independently).
- **Cross-platform** — every change must work on Windows, Linux and macOS, not just Windows. Any backend-specific behaviour goes behind `cfg` or a runtime fallback, never a Windows-only path. (User instruction mid-exploration.) Mechanism: use `cfg(target_os)` for OS differences, because it is automatic and cannot be mis-selected. Use Cargo features only for opt-in by a consumer. Research settled both open points:
  - tomlctl gets an optional default `mimalloc` feature, not a `cli` feature with `required-features`, which breaks `cargo install` and assert_cmd.
  - notify keeps its default features: turning them off breaks the macOS build. Its backend stays chosen by target.
- **mimalloc for glimpse** — raised by the user mid-exploration, then dropped after research (see table). `#[global_allocator]` stays only in `tomlctl/src/main.rs`. A lib-declared allocator is legal Rust and would silently become glimpse's allocator, so an acceptance check guards against it.
- **Prior decision to revisit** — `docs/plans/lively-twirling-babbage.md:72` chose polling over `notify` 8.2: the Windows backend has a 16 KiB buffer with no overflow rescan, and rename-replace needs care. The watcher design must answer both.
- **Transcript stutter observed?** — No. Item 3 is speculative and low priority. It stays in the plan as the last and smallest item.

Phase 4 directed questions:

| Question | Answer | Prompting finding |
|---|---|---|
| mimalloc for glimpse? | **Drop from scope.** tomlctl still gets an optional `mimalloc` feature so glimpse builds without it. | Research Notes → "Do not adopt mimalloc in glimpse" (+4–6 ms start-up, ~40 µs/snapshot gain) |
| Hook `tomlctl agents record` in-process? | **Yes, in-process.** One process start per hook event; `OLD_TOMLCTL` check goes; accepts glimpse as a second `agents.toml` writer that must be reinstalled when tomlctl changes. | Research Notes → "Hook start-up is dominated by the tomlctl child" (28 ms median) |
| Watcher safety policy? | **Watch + ~10 s safety tick + auto-fallback** to `poll_ms` polling when a watch cannot be established or two consecutive safety ticks catch unreported changes; retry the watch. No config switch. | Research Notes → "Silent watch death" |
| Guard against tomlctl changes breaking glimpse? | **Narrow pre-commit gate**: `cargo clippy --manifest-path glimpse/Cargo.toml` only when the facade or the modules behind it are staged. | Exploration Notes → "No pre-commit step builds glimpse when only `tomlctl/src/**` is staged" |
| glimpse `tomlctl` config key? | **Remove it outright** — a config still setting it fails with `unknown key` until the line is deleted. | `glimpse/src/config.rs:352` rejects unknown keys |
| Checkpoint cadence? | **milestones** — after the tomlctl lib + facade, after glimpse goes in-process, after watcher + tail. | ~20 files across two crates + hook |
| Transcript tail driver? | **Keep the 1 s tick**, moved to the poller thread; runtime sends the target path, poller sends `Event::Tail`. No transcript watch. | `glimpse/src/runtime.rs:274-278` |

### Phase 5 outcome
Skipped. Every answer is either covered by `## Research Notes` (mimalloc, agents-record facade, watcher fallback, pre-commit gate) or is a local code decision needing no library research (config key, cadence, tail driver).

## Approach

### tomlctl as a library

`tomlctl` becomes lib+bin, the same layout as `lumina/server`.

**`tomlctl/src/lib.rs`** (new) takes over everything below from `main.rs`:
- every `mod` declaration, all still private;
- `#[cfg(test)] mod test_support;`;
- `emit_error`;
- the `Cli::parse` / `cli::run` wiring, as `pub fn run() -> std::process::ExitCode`. This preserves exit code 1 on error; clap's own exit on a parse error is unchanged.

**`main.rs`** shrinks to the allocator static and `fn main() -> ExitCode { tomlctl::run() }`. The allocator static is now guarded by `#[cfg(feature = "mimalloc")]`.

**`tomlctl/Cargo.toml`**:
- adds `[lib] name = "tomlctl"`, `path = "src/lib.rs"`, `doctest = false` (all 11 fences in `tomlctl/src` are non-Rust);
- makes `mimalloc` `optional = true`, with `[features] default = ["mimalloc"]`;
- does **not** use `required-features` on the bin.

Private modules keep dead-code reporting intact. The unit tests keep their module paths (`cli::dispatch::tests` etc.) but move to the lib target. `.githooks/pre-commit` and root `CLAUDE.md` must therefore say `--lib`: `--bin tomlctl` would silently match 0 tests.

### Library facade

Three `pub fn` wrappers in `tomlctl/src/lib.rs`. They are wrappers, not `pub use`: re-exporting a `pub(crate)` fn is E0364. No clap type appears in any public signature. Their doc comments must not intra-doc-link private items, or `cargo doc` fails under `private_intra_doc_links = "deny"`.

- `pub fn snapshot(slug: &str, store_path: &Path) -> anyhow::Result<serde_json::Value>` calls `tasks::snapshot` with `ReadIntegrityArgs { verify_integrity: false, strict_read: false }`. That path is pure file reads: no `repo_or_cwd_root`, no locks.
- `pub fn flow_list(root: &Path) -> anyhow::Result<serde_json::Value>` calls the new `flow::list::list_all(root)`. It returns the `{ok, flows, skipped}` envelope unfiltered, with no integrity checks.
- `pub fn record_agent(harness: &str, payload: &serde_json::Value) -> anyhow::Result<serde_json::Value>` mirrors `agents::dispatch::dispatch_record`:
  - it parses the harness with `Harness::parse`, using the same unknown-harness error;
  - it calls `agents::record::record` with every `WriteIntegrityArgs` flag false, the CLI's no-flag defaults;
  - its doc states that it changes the process's working directory and fixes the repo root for the rest of the process, so it is called once per process.

Each wrapper first calls a new `io::silence_advisories()`. That function pins the `advisories_visible` cache to `false`, so an `advise!` can never write into glimpse's alternate screen. The static moves to module scope in `tomlctl/src/io.rs` so both functions can reach it.

glimpse depends with `tomlctl = { path = "../tomlctl", default-features = false }`. It builds the store path `<root>/.claude/flows/<slug>/tasks.toml` itself, and never calls `store::resolve_store_path` (slug regex plus root lookup).

### In-process glimpse

**`glimpse/src/source.rs`**:
- `TomlctlFetcher` becomes `InProcessFetcher`. It calls `tomlctl::snapshot` and then `serde_json::from_value::<Snapshot>`, which works as-is because every model struct is `#[serde(default)]`.
- The `Fetcher` trait stays, since the tests fake it.
- The whole probe path goes: `REQUIRED_FEATURE`, `REQUIRED_MESSAGE`, `check_capabilities`, `probe_tomlctl`, `Probe`, `halted`, and the `probe` parameter of `Source::spawn`. With the code compiled in there is no version to drift.

**`glimpse/src/flows.rs`**: `flows::list(root)` calls `tomlctl::flow_list`, and `parse_flows` takes a `&serde_json::Value`.

**`glimpse/src/hook.rs`**: `record` parses the payload once and calls `tomlctl::record_agent`. The spawn, the stdin writer thread, `record_args`, `failure_message` and `OLD_TOMLCTL` all go. The in-process call relies on `record` doing its own `set_current_dir`, which is safe in the short-lived hook process.

**`glimpse/src/config.rs`**: the `tomlctl` config key is removed.

Anyhow errors are rendered with `{e:#}` wherever glimpse turns them into `String`.

### Watcher as a wake-up signal

- **Dependency**: `notify = "8.2.0"` with its default features. Never set `default-features = false`: `mod fsevent` compiles on macOS whatever the features are, so dropping the defaults breaks the macOS build invisibly from Windows.

- **New module `glimpse/src/watch.rs`**, which never imports `source.rs`:
  - `pub(crate) enum Wake { Flow(String), All }`.
  - `pub(crate) fn classify(event: &notify::Result<notify::Event>, flows_root: &Path) -> Option<Wake>`:
    - `None` for `EventKind::Access(_)`. inotify's `OPEN` mask would otherwise wake glimpse on its own snapshot reads.
    - `Wake::Flow(slug)` when the first path component is found via `strip_prefix(flows_root)`.
    - `Wake::All` for an `Err`, a `need_rescan()` event, a `Remove` of `flows_root` itself, or a path outside `flows_root`. The last covers macOS reporting canonical `/private/...` paths.
  - `pub(crate) fn start(flows_root: &Path, on_wake: impl Fn(Wake) + Send + 'static) -> notify::Result<notify::RecommendedWatcher>`: one `RecursiveMode::Recursive` watch on `<root>/.claude/flows`. The callback only classifies and calls `on_wake`, which must never block. The Windows backend drops events silently when its callback stalls, and on macOS dropping the watcher joins the callback's thread.

- **Changes to `Poller` in `glimpse/src/source.rs`**:
  - A new `Control::Wake(Wake)` message.
  - The watcher is created and owned on the poller thread, never in `Source`, so a macOS drop-join stays off the exit path.
  - A new `Mode`:
    - `Watching(RecommendedWatcher)`: safety wait `SAFETY_TICK = 10 s`.
    - `Polling`: wait `poll_ms`. The watch is retried on each tick while its last start attempt *errored*, for example `.claude/flows` did not exist yet.
  - Each wait is `min(mode interval, time left until a pending retry_after)`. Today `retry_due` is only checked on a tick, which would stretch the 5 s retry to the safety interval.
  - **Coalescing**: after a wake, keep draining the control channel with `recv_timeout(50 ms)` until it goes quiet or 250 ms have passed, then run one `tick()`. A tomlctl write is ~18 raw events.
  - **Cache eviction**: `Wake::Flow(slug)` removes `flow_stats[slug]` before the tick, so an in-place edit is re-statted. `Wake::All` clears `flow_stats` and forces a snapshot fetch. A `Wake::All` caused by a watcher error drops and re-creates the watcher.
  - **Missed-change detector**: a *safety* tick (a timeout, not a wake) that finds a fingerprint or flow-list change with no wake since the previous tick counts one miss. Two consecutive misses drop the watcher and switch to `Polling` for the rest of the session: sticky, so a filesystem that accepts a watch but delivers nothing cannot flap. A wake resets the count.
  - **Shutdown**: `Control::Stop` stays the shutdown signal. The callback's cloned `Sender` keeps the channel alive, so `Disconnected` no longer fires.
  - `tick()` and the fingerprint logic stay the decider, so the existing synchronous tests keep working.

### Transcript tail on the poller thread

- **Poller side**:
  - A new `Control::Tail(Option<String>)` and `Event::Tail(Box<TailState>)`.
  - The poller owns an `Option<TailState>`. `TailState` is already `Clone`, and the poller builds it with `TailState::new`, which confines reads to `claude_dir()`.
  - On `Control::Tail(Some(path))` it retargets and refreshes at once and always sends; `None` clears the tail.
  - While a tail is targeted, the poller's wait is additionally capped at `app::TICK` (1 s). Each tail refresh that returns `true` sends a clone.
- **Runtime side**:
  - `Host` gains `set_tail(Option<String>)`.
  - After each handled batch the runtime computes the target: `view::activity::agent(&app)`'s `transcript_path` while `activity_open`, else `None`. It sends it only when it differs from the last one sent.
  - `Event::Tail` replaces `Screen.tail` and redraws.
  - `Screen::refresh_tail` and the activity-specific `TICK` override in `run_loop` go. App clocks keep using `App::tick_interval`.
  - `render_once` (`--once`) keeps a synchronous one-shot read.

## Success Criteria

- forward: glimpse's data path starts no tomlctl process. The only `Command::new` left in `glimpse/src/source.rs`, `glimpse/src/flows.rs` and `glimpse/src/hook.rs` is `flows::repo_root`'s `git` (count 1; today: 5).
- forward: the probe and version-drift machinery is gone. No `probe_tomlctl`, `TomlctlFetcher`, `REQUIRED_MESSAGE` or `OLD_TOMLCTL` is left in `glimpse/src` (today: present).
- forward: `glimpse/Cargo.lock` has `notify` and no `mimalloc` (today: no notify).
- forward: the pre-commit tomlctl test gate still runs tests. `.githooks/pre-commit` has no `--bin tomlctl` (today: 1). **predicted, unverified**: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` reports a non-zero `running N tests`.
- forward: the `tomlctl` config key is gone from `glimpse/src/config.rs` (today: 8 mentions).
- guard: `#[global_allocator]` appears only in `tomlctl/src/main.rs` (today: holds).
- **predicted, unverified**: `glimpse --once --slug <slug>` renders a live flow with no `tomlctl` on `PATH`.
- **predicted, unverified**: an idle glimpse wakes at most every 10 s while a watch is healthy. An in-place edit (not a rename) to another flow's `context.toml` refreshes its selector row within ~1 s.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml && cargo build --manifest-path glimpse/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast && cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast
test.timeout: 1200
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets && cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets --no-default-features && cargo clippy --manifest-path glimpse/Cargo.toml --all-targets && cargo fmt --manifest-path tomlctl/Cargo.toml -- --check && cargo fmt --manifest-path glimpse/Cargo.toml -- --check && cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: test "$(grep -c -- '--bin tomlctl' .githooks/pre-commit)" -eq 0
success: ! grep -rqE 'probe_tomlctl|TomlctlFetcher|REQUIRED_MESSAGE|OLD_TOMLCTL' glimpse/src
success: test "$(cat glimpse/src/source.rs glimpse/src/flows.rs glimpse/src/hook.rs | grep -c 'Command::new')" -eq 1
success: grep -q '^name = "notify"' glimpse/Cargo.lock && ! grep -q '^name = "mimalloc"' glimpse/Cargo.lock
success: ! grep -q 'tomlctl' glimpse/src/config.rs
success: test "$(grep -rl 'global_allocator' tomlctl/src)" = "tomlctl/src/main.rs"
```

Prefix full runs with `CARGO_INCREMENTAL=0` (sccache). Final manual smoke steps are listed in After Merge:
- `glimpse --once --slug <slug>` with `tomlctl` off `PATH`;
- a live glimpse left idle, then hand-edit a flow file in place.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 3, 4, 8, 13, 14, 15
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Split tomlctl into a library and a thin binary [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/src/main.rs`, `tomlctl/src/lib.rs` (new)
- **Depends on**: —
- **Action**: Move every module declaration, `#[cfg(test)] mod test_support;`, `emit_error` and the parse-run-report wiring out of `tomlctl/src/main.rs` into a new `tomlctl/src/lib.rs`. Expose them as `pub fn run() -> std::process::ExitCode`. Make mimalloc an optional default feature.
- **Detail**:
  - See Approach, "tomlctl as a library".
  - Modules stay private (`mod`, not `pub mod`). The `advise!` macro already uses `$crate`, so it works in the lib.
  - `main.rs` keeps only:
    - the allocator static, under `#[cfg(feature = "mimalloc")]` plus `#[global_allocator]`;
    - `fn main() -> std::process::ExitCode { tomlctl::run() }`.
  - `run` returns `ExitCode::FAILURE` where `main` used to `std::process::exit(1)`.
  - `tomlctl/Cargo.toml`:
    - add `[lib] name = "tomlctl"`, `path = "src/lib.rs"`, `doctest = false`;
    - change mimalloc to `{ version = "0.1", default-features = false, optional = true }`;
    - add `[features] default = ["mimalloc"]`;
    - leave `[[bin]]` without `required-features`.
  - Keep the existing module-order comments with their modules. Never put `#[global_allocator]` in `lib.rs`: a lib-declared allocator compiles and silently becomes every dependent's allocator.
- **Acceptance**:
  - forward: `grep -c '^mod ' tomlctl/src/main.rs` prints `0` (today: `27`).
  - forward: `grep -c 'cfg(feature = "mimalloc")' tomlctl/src/main.rs` prints `1` (today: `0`).
  - forward: `grep -c '^\[lib\]' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - guard: `grep -rl 'global_allocator' tomlctl/src` prints only `tomlctl/src/main.rs` (today: holds).
  - forward: **predicted, unverified**: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` reports a non-zero test count, and the same filter with `--bin tomlctl` reports `running 0 tests`.
  - forward: **predicted, unverified**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets --no-default-features` compiles with no new warnings.

### 2. Add a root-taking flow listing [S]
- **Files**: `tomlctl/src/flow/list.rs`
- **Depends on**: —
- **Action**: Add `pub(crate) fn list_all(root: &Path) -> Result<JsonValue>` to `tomlctl/src/flow/list.rs`. It returns the `{"ok": true, "flows": [...], "skipped": [...]}` envelope for `<root>/.claude/flows` with no filters and no integrity checks. Build `dispatch`'s output through the same envelope helper so the two cannot diverge.
- **Detail**:
  - `list_all` calls `enumerate_flows(root, &root.join(".claude").join("flows"), false, false)` and never calls `repo_or_cwd_root`.
  - Factor the `serde_json::json!({"ok": true, "flows": …, "skipped": …})` construction into a private helper used by both `dispatch` and `list_all`.
  - Add a unit test `list_all_reports_flows_and_skips` in `tomlctl/src/flow/list.rs`. It uses a temp dir with one valid and one malformed `context.toml`, and asserts one flow and one skip.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) fn list_all' tomlctl/src/flow/list.rs` prints `1` (today: `0`).
  - guard: **predicted, unverified**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_list` passes. Falsifier: changing the shared envelope helper's key from `skipped` to `skip` turns it red.

### 3. Point the pre-commit gates at the library [S]
- **Files**: `.githooks/pre-commit`
- **Depends on**: 1
- **Action**:
  - Change the tomlctl unit-test gate from `--bin tomlctl` to `--lib`.
  - Add a gate that runs `cargo clippy --manifest-path "$ROOT/glimpse/Cargo.toml"` when the staged set contains any of `tomlctl/Cargo.toml`, `tomlctl/src/lib.rs`, `tomlctl/src/io.rs`, `tomlctl/src/tasks/**`, `tomlctl/src/flow/list.rs` or `tomlctl/src/agents/**`.
- **Detail**:
  - The test gate becomes `gate cargo test --manifest-path "$ROOT/tomlctl/Cargo.toml" --lib -- cli::dispatch::tests`.
  - New block: `if grep -Eq '^tomlctl/(Cargo\.toml|src/(lib|io)\.rs|src/tasks/.*|src/flow/list\.rs|src/agents/.*)$' <<<"$STAGED"; then gate cargo clippy --manifest-path "$ROOT/glimpse/Cargo.toml"; fi`.
  - Put it after the existing glimpse fmt gate. Its one-line comment says glimpse compiles tomlctl in as a library, so a facade change can break glimpse's build.
  - Follow the file's existing `gate` / `$STAGED` idiom. The file is an unsandboxed hook: change nothing else.
- **Acceptance**:
  - forward: `grep -c -- '--bin tomlctl' .githooks/pre-commit` prints `0` (today: `1`).
  - forward: `grep -c 'cargo clippy --manifest-path "$ROOT/glimpse/Cargo.toml"' .githooks/pre-commit` prints `1` (today: `0`).
  - guard: `bash -n .githooks/pre-commit` exits 0.

### 4. Expose the in-process facade [M]
- **Files**: `tomlctl/src/lib.rs`, `tomlctl/src/io.rs`, `tomlctl/tests/library_facade.rs` (new)
- **Depends on**: 1, 2
- **Action**:
  - Add `pub fn snapshot`, `pub fn flow_list` and `pub fn record_agent` to `tomlctl/src/lib.rs`.
  - Add `pub(crate) fn silence_advisories()` to `tomlctl/src/io.rs`, and call it first in each facade fn.
  - Add an integration test that compares the facade with the CLI.
- **Detail**:
  - See Approach, "Library facade" for the signatures and semantics.
  - In `io.rs`, move `advisories_visible`'s `static VISIBLE: OnceLock<bool>` to module scope. `silence_advisories` does `let _ = VISIBLE.set(false);`.
  - `record_agent` builds `WriteIntegrityArgs { allow_outside: false, no_write_integrity: false, verify_integrity: false, strict_integrity: false }`.
  - Doc comments: at most four lines each, and no intra-doc links to private items.
  - `tomlctl/tests/library_facade.rs`:
    - set up a flow in a temp root with `assert_cmd` (`tomlctl flow init` plus `tasks add`, or copying a fixture from `tomlctl/tests/fixtures`);
    - assert `tomlctl::snapshot(slug, &store)` equals the parsed stdout of `tomlctl tasks snapshot --slug <slug>` run with `TOMLCTL_ROOT=<root>`;
    - assert `tomlctl::flow_list(&root)` equals the parsed stdout of `tomlctl flow list`;
    - assert `tomlctl::record_agent("manual", &json!({}))` is an `Err` naming the unimplemented adapter.
  - Drop any field that depends on the current time before comparing.
- **Acceptance**:
  - forward: `grep -cE '^pub fn (snapshot|flow_list|record_agent)' tomlctl/src/lib.rs` prints `3` (today: the file is absent).
  - forward: `grep -c 'fn silence_advisories' tomlctl/src/io.rs` prints `1` (today: `0`).
  - forward: **predicted, unverified**: `cargo test --manifest-path tomlctl/Cargo.toml --test library_facade` passes. Falsifier: making `flow_list` pass `verify = true` adds `skipped` entries for flows without sidecars and turns the equality red.
  - forward: **predicted, unverified**: `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items` succeeds.

### 5. Add the tomlctl path dependency to glimpse [S]
- **Files**: `glimpse/Cargo.toml`, `glimpse/Cargo.lock`
- **Depends on**: 4
- **Action**:
  - Add `tomlctl = { path = "../tomlctl", default-features = false }` to `glimpse/Cargo.toml` `[dependencies]`, with a one-line comment: in-process snapshot, flow list and hook recording, without tomlctl's allocator.
  - Update the package `description` to "Live terminal view of a flow's task graph, checkpoints and running agents, built on tomlctl's snapshot code".
  - Let cargo regenerate `glimpse/Cargo.lock`.
- **Acceptance**:
  - forward: `grep -c 'path = "../tomlctl"' glimpse/Cargo.toml` prints `1` (today: `0`).
  - guard: `grep -c '^name = "mimalloc"' glimpse/Cargo.lock` prints `0` after the lock regenerates (today: `0`).

### 6. Fetch snapshots and flow lists in-process [M]
- **Files**: `glimpse/src/source.rs`, `glimpse/src/flows.rs`, `glimpse/src/main.rs`
- **Depends on**: 5
- **Action**:
  - Replace `TomlctlFetcher` with `InProcessFetcher`. Delete the capabilities probe and the halted state.
  - Rewrite `flows::list(root)` to call `tomlctl::flow_list` and parse a `serde_json::Value`.
  - Update `main.rs`'s `--once` path to use them.
- **Detail**:
  - See Approach, "In-process glimpse".
  - `InProcessFetcher::fetch` calls `tomlctl::snapshot(slug, &root.join(".claude").join("flows").join(slug).join("tasks.toml"))`. Map the error with `format!("{e:#}")`, then `serde_json::from_value`.
  - Delete from `source.rs`: `REQUIRED_FEATURE`, `REQUIRED_MESSAGE`, `check_capabilities`, `probe_tomlctl`, the `Probe` type, `Poller.probe` and `Poller.halted` with the halted wait loop, and `Source::spawn`'s `probe` parameter.
  - `Source::start` keeps `&Config` for `poll_ms` only.
  - Delete the tests `capabilities_without_the_snapshot_feature_are_rejected` and `a_failed_probe_reports_and_never_polls`, and update `the_thread_fetches_on_set_slug_and_stops_on_request` to the new `spawn` signature.
  - In `flows.rs`:
    - `parse_flows(value: &serde_json::Value, mtime)` deserialises `Envelope` with `serde_json::from_value`;
    - `list(root: &Path)` drops the `tomlctl` parameter;
    - update the docs that say "Parses `tomlctl flow list` output" and the parse tests.
  - In `main.rs`:
    - `once_snapshot` / `fetch_once` drop the `tomlctl` binding and the probe fallback;
    - update the `use crate::source::…` import;
    - keep the doc comment accurate: a `--snapshot` file is read as-is, otherwise the flow's files are read.
  - Update `source.rs`'s module doc and the `Fetcher` doc ("shells out to tomlctl").
- **Acceptance**:
  - forward: `grep -rcE 'probe_tomlctl|TomlctlFetcher|REQUIRED_MESSAGE|check_capabilities' glimpse/src/source.rs glimpse/src/main.rs glimpse/src/flows.rs` prints `0` for each file (today: `17` / `3` / `0`).
  - forward: `grep -c 'Command::new' glimpse/src/source.rs glimpse/src/flows.rs` prints `0` and `1` (today: `2` and `2`).
  - guard: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml -- source:: flows::` passes. Falsifier: `an_unchanged_revision_is_not_sent_twice` goes red if the fetcher stops deserialising `revision`.

### 7. Record hook events in-process [M]
- **Files**: `glimpse/src/hook.rs`, `glimpse/tests/cli.rs`
- **Depends on**: 5
- **Action**:
  - Replace the `tomlctl agents record` spawn in `glimpse/src/hook.rs` with `tomlctl::record_agent`. Delete the spawn-only helpers.
  - Stop the CLI sandbox naming a tomlctl, and re-anchor the hook log tests on a payload that fails in-process.
- **Detail**:
  - See Approach, "In-process glimpse".
  - `record(harness, payload: &[u8]) -> Result<RecordResult, String>`:
    - parse `serde_json::from_slice::<Value>`; on failure return `Err("invalid hook payload: …")`;
    - call `tomlctl::record_agent(harness.as_str(), &value)`, mapping errors with `{e:#}`;
    - convert with `serde_json::from_value::<RecordResult>`.
  - `handle` no longer passes `config` to `record`.
  - Delete `record_args`, `failure_message`, `OLD_TOMLCTL`, the stdin-writer thread, the `current_dir` handling (`record_agent` sets the cwd itself) and their unit tests.
  - `parse_record_result` becomes a `from_value` conversion. Keep its tests, adapted.
  - Update the module doc and the `RecordResult` doc ("The one JSON line `tomlctl agents record` prints").
  - `glimpse/tests/cli.rs`:
    - stop writing `tomlctl = "glimpse-test-no-such-tomlctl"` into the sandbox config (write an empty file), and drop the module doc's "names a tomlctl" clause;
    - `hook_is_silent_and_logs_one_line` stays as is: an empty payload is invalid JSON, so one log line;
    - `a_codex_hook_is_silent_and_logs_one_line` sends `not json` instead of `{}`;
    - add `a_hook_for_an_unsupported_event_records_nothing`: `hook --harness codex` with `{}` exits 0 with empty stdout and stderr, and writes no `claude/glimpse/hook.log` and no `cwd/.claude`.
- **Acceptance**:
  - forward: `grep -cE 'Command::new|OLD_TOMLCTL|record_args' glimpse/src/hook.rs` prints `0` (today: `8`).
  - forward: `grep -c 'glimpse-test-no-such-tomlctl' glimpse/tests/cli.rs` prints `0` (today: `1`).
  - forward: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml --test cli` passes. Falsifier: with the codex case still sending `{}`, `a_codex_hook_is_silent_and_logs_one_line` fails with zero log lines.

### 8. Remove the `tomlctl` config key [S]
- **Files**: `glimpse/src/config.rs`, `glimpse/src/cli.rs`
- **Depends on**: 6, 7
- **Action**:
  - Delete the `tomlctl` field, its default, its parse arm and its assertions from `glimpse/src/config.rs`. Add a test that a config setting `tomlctl` fails with `unknown key`.
  - In `glimpse/src/cli.rs`, reword the `--snapshot` help line "instead of running tomlctl" to "instead of reading the flow".
- **Acceptance**:
  - forward: `grep -c 'tomlctl' glimpse/src/config.rs` prints `0` (today: `8`).
  - forward: `grep -c 'instead of running tomlctl' glimpse/src/cli.rs` prints `0` (today: `1`).
  - forward: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml -- config::` passes, including the new unknown-key test.

### 9. Add the notify dependency to glimpse [S]
- **Files**: `glimpse/Cargo.toml`, `glimpse/Cargo.lock`
- **Depends on**: 5
- **Action**: Add `notify = "8.2.0"` with default features to `glimpse/Cargo.toml`. The line gets a one-line comment: defaults kept because macOS's FSEvents backend needs `macos_fsevent`. Let cargo regenerate `glimpse/Cargo.lock`.
- **Detail**: Do not add `notify-debouncer-mini`/`-full`. They key events by path and drop the pathless rescan events.
- **Acceptance**:
  - forward: `grep -c '^notify' glimpse/Cargo.toml` prints `1` (today: `0`).
  - forward: `grep -c '^name = "notify"' glimpse/Cargo.lock` prints `1` (today: `0`).

### 10. Add the watch module [M]
- **Files**: `glimpse/src/watch.rs` (new), `glimpse/src/main.rs`
- **Depends on**: 6, 9
- **Action**: Create `glimpse/src/watch.rs` with `Wake`, `classify` and `start`, and declare `mod watch;` in `glimpse/src/main.rs`.
- **Detail**:
  - See Approach, "Watcher as a wake-up signal".
  - `classify` is pure. Unit-test it with synthetic `notify::Event`s built via `Event::new(kind).add_path(..)` and `.set_flag(Flag::Rescan)`:
    - an `Access(Open)` event yields `None`;
    - a `Modify(Name(To))` on `<flows>/a/tasks.toml` yields `Flow("a")`;
    - a path outside the root yields `All`;
    - a rescan flag yields `All`;
    - an `Err` yields `All`;
    - a `Remove` of the root itself yields `All`.
  - Add one live test that watches a temp `flows` dir. Write `a/tasks.toml` by temp file plus rename, and separately in place. Expect a `Wake::Flow("a")` within 5 s for each, received on an mpsc channel. The generous timeout absorbs FSEvents latency.
  - The module doc (≤15 lines) states the non-blocking callback rule and why `Access` is ignored.
  - `watch` stays unused until task 11. That dead-code window is internal to checkpoint C.
- **Acceptance**:
  - forward: `test -f glimpse/src/watch.rs; echo $?` prints `0` (today: `1`).
  - forward: `grep -c '^mod watch;' glimpse/src/main.rs` prints `1` (today: `0`).
  - forward: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml -- watch::` passes. Falsifier: dropping the `Access` arm makes the `Access(Open)` case return `Flow`.

### 11. Drive the poller from the watcher [M]
- **Files**: `glimpse/src/source.rs`
- **Depends on**: 6, 10
- **Action**: Rework `Poller::run` in `glimpse/src/source.rs` around a watcher-driven `Mode`. It adds `Control::Wake`, the 10 s safety tick, coalescing, per-flow cache eviction, the retry-aware wait, and the missed-change fallback to `poll_ms` polling.
- **Detail**:
  - Implement Approach, "Watcher as a wake-up signal", exactly.
  - Create the watcher inside the spawned thread with `watch::start(&flows_root, move |w| { let _ = tx.send(Control::Wake(w)); })`, where `tx` is a clone of the control `Sender`. The send is unbounded and never blocks.
  - Factor the wait computation into a pure `fn next_wait(...) -> Duration` and unit-test it:
    - `Watching` with no failure gives 10 s;
    - `Watching` with a retry due in 2 s gives 2 s;
    - `Polling` gives `poll_ms`.
  - Unit-test eviction deterministically without a real watcher:
    1. `tick()` once;
    2. write `.claude/flows/b/context.toml` in place, restoring the directory's mtime if the platform moved it;
    3. assert the next `tick()` sends no `Flows` event;
    4. apply `Wake::Flow("b")` and assert the following `tick()` relists.
  - Unit-test the missed-change detector: two safety ticks that each find a change with no wake switch the mode to `Polling`, and a wake in between resets the count.
  - Keep all existing tick-driven tests passing unchanged.
  - Update the `flows_fingerprint` doc comment: in-place writes are caught by the watcher's eviction, not by the directory mtime. Note that the cache gap exists on ext4 too.
- **Acceptance**:
  - forward: `grep -cE 'SAFETY_TICK|Control::Wake' glimpse/src/source.rs` prints ≥ `2` (today: `0`).
  - forward: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml -- source::` passes, including the new `next_wait`, eviction and missed-change tests. Falsifier: removing the `flow_stats.remove(slug)` on `Wake::Flow` turns the eviction test red.

### 12. Move the transcript tail onto the poller thread [M]
- **Files**: `glimpse/src/source.rs`, `glimpse/src/runtime.rs`, `glimpse/src/view/activity.rs`
- **Depends on**: 11
- **Action**:
  - Add `Control::Tail` / `Event::Tail` and the poller-owned `TailState`, refreshed each second while targeted.
  - In `glimpse/src/runtime.rs`, replace `Screen::refresh_tail` with target tracking plus `Host::set_tail`.
  - Update the `glimpse/src/view/activity.rs` module doc, which says the caller owns and refreshes the tail.
- **Detail**:
  - See Approach, "Transcript tail on the poller thread".
  - In `run_loop`, remove the `activity_open && agent.is_some()` `TICK` override. Remove both `screen.refresh_tail()` calls, and after each handled batch call `host.set_tail(target)` when the target changed.
  - `handle` gains an `Event::Tail(t) => { screen.tail = *t; Step::Redraw }` arm. It borrows `screen`, not `app`, so restructure the match accordingly.
  - The scripted test `Host` records `set_tail` calls. Rewrite the existing tail test (the one exercising `refresh_tail`) to assert:
    - opening the activity panel on a running agent sends `Some(path)`;
    - closing it sends `None`;
    - an `Event::Tail` updates `screen.tail`.
  - `render_once` keeps its one-shot synchronous `TailState` read.
  - Update the `runtime.rs` module doc (thread list) and the `run_loop` doc.
- **Acceptance**:
  - forward: `grep -c 'refresh_tail' glimpse/src/runtime.rs` prints `0` (today: `6`).
  - forward: `grep -c 'Tail(' glimpse/src/source.rs` prints ≥ `2` (today: `0`).
  - forward: **predicted, unverified**: `cargo test --manifest-path glimpse/Cargo.toml -- runtime:: source::` passes. Falsifier: skipping the `Event::Tail` arm leaves `screen.tail` empty and fails the rewritten tail test.

### 13. Update glimpse's README and CLAUDE.md [M]
- **Files**: `glimpse/README.md`, `glimpse/CLAUDE.md`
- **Depends on**: 8, 12
- **Action**: Rewrite every passage that describes spawning tomlctl, the capabilities probe, the 500 ms poller, the `tomlctl` config key, the in-place-edit limitation or the main-thread tail, so they describe the in-process, watcher-driven design.
- **Detail**:
  - `glimpse/README.md`:
    - the intro saying data comes from `tomlctl`, and the ≥0.12.0 / `tomlctl capabilities` requirement. Replace it with: glimpse compiles tomlctl in and must be reinstalled after tomlctl changes.
    - the `poll_ms` row: now the fallback polling interval, used when a watch can't be set up or has been detected as dead;
    - delete the `tomlctl` row;
    - the data-flow diagram and the "Live view" paragraph: watcher → poller → in-process snapshot; the 10 s safety tick; the fallback rules.
  - `glimpse/CLAUDE.md`:
    - the intro line and the structure line (`source.rs`: watcher-woken fingerprint poller; add `watch.rs`);
    - the sandbox line (no tomlctl named);
    - the torn-read gotcha: retried on the next wake or safety tick;
    - replace the "flow scan sees renames, not in-place edits" gotcha with the watcher's cache-eviction rule, plus the rule that the callback must never block;
    - the idle-loop gotcha: the tail tick now lives on the poller thread;
    - the `--once` "no tomlctl" command note;
    - add a gotcha: glimpse links tomlctl's code, so a tomlctl change needs `cargo install --path glimpse` too.
  - Find the stale passages with `grep -nE 'tomlctl|poll|in-place|capabilit|refresh_tail' glimpse/README.md glimpse/CLAUDE.md`.
- **Acceptance**:
  - forward: `grep -cE 'tomlctl capabilities|sees renames, not in-place' glimpse/README.md glimpse/CLAUDE.md` prints `0` for each file (today: `1` / `1`).
  - forward: `grep -c 'watch.rs' glimpse/CLAUDE.md` prints ≥ `1` (today: `0`).

### 14. Update root CLAUDE.md for the library split [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 1, 3, 6, 7
- **Action**: In root `CLAUDE.md`:
  - change the Developer-setup hook paragraph's `--bin tomlctl` to `--lib`, and describe the new glimpse clippy gate and the paths that trigger it;
  - add `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items` to Build & test as the only complete run of the rustdoc deny lints;
  - reword the Sibling crates `glimpse/` bullet to say it links tomlctl as a library (snapshot, flow list, agent recording) and needs a reinstall after tomlctl changes.
- **Acceptance**:
  - forward: `grep -c -- '--bin tomlctl' CLAUDE.md` prints `0` (today: `1`).
  - forward: `grep -c -- '--document-private-items' CLAUDE.md` prints `1` (today: `0`).

### 15. Measure mimalloc against the system allocator for tomlctl [M]
- **Files**: `tomlctl/Cargo.toml`
- **Depends on**: 3, 4
- **Action**: Time tomlctl built with and without its `mimalloc` feature on a typical call and on a heavy verb. Then keep or drop `mimalloc` from the default features, and record the measurement in the `tomlctl/Cargo.toml` comment above the dependency.
- **Detail**:
  - The research that dropped mimalloc from glimpse measured a stand-in binary on this machine:
    - start-up: +3.8 ms median with `+crt-static`, +5–6 ms with the dynamic CRT;
    - work: ~40 µs saved per 9.8 KB JSON parse + serialise.
    tomlctl is a one-shot CLI, so the start-up cost may dominate here too. This task measures tomlctl itself.
  - **Builds.** Build two release binaries, both into a scratch `--target-dir` outside the repo, so the shared `tomlctl/target` is not rebuilt:
    - with default features;
    - with `--no-default-features`.
    Run both builds from inside `tomlctl/` (`cd tomlctl && cargo build --release --target-dir <scratch>/…`). Cargo finds `tomlctl/.cargo/config.toml` (x86-64-v3, `+crt-static`) from the working directory, so a build started from the repo root measures the wrong binary.
  - **Timing.** Interleave the two binaries, ≥100 runs each per command, and report median and p90. No hyperfine is installed; use a PowerShell `Stopwatch` loop or a bash loop over `date +%s%N`. Run everything within one session: machine load swings up to 6×, so only compare numbers taken in the same run.
  - **Commands to time**, all read-only, against this repo:
    1. a typical call: `tomlctl tasks snapshot --slug polymorphic-wondering-magpie`;
    2. a small ledger read: `tomlctl items list .claude/backlog.toml --array backlog --count`;
    3. a heavy verb: `tomlctl sweep -e 'fn '` over the git-tracked files.
  - **Decision.** Keep `default = ["mimalloc"]` only if the heavy verb's median gain is larger than the typical call's median loss, and the typical-call loss is under 1 ms. Otherwise set `default = []`, keeping the optional dependency so `--features mimalloc` still works.
  - **Record the result.** Replace the Microsoft-benchmark / rust-analyzer rationale comment above the `mimalloc` dependency in `tomlctl/Cargo.toml` with the measured medians, the date and the timing command, per the documentation rule that a measurement carries value, date and producing command. Keep the comment to four lines or fewer.
  - **No other file changes.** `main.rs`'s `#[cfg(feature = "mimalloc")]` works either way.
  - **Report the numbers.** Put them in the task's return so the orchestrator can record them in the execution record.
- **Acceptance**:
  - forward: `grep -c 'Microsoft' tomlctl/Cargo.toml` prints `0` (today: `1`), because the benchmark-citation comment has been replaced.
  - forward: `grep -B4 '^mimalloc' tomlctl/Cargo.toml | grep -cE '20[0-9]{2}-[0-9]{2}-[0-9]{2}'` prints `1` (today: `0`), i.e. the comment carries the measurement date.
  - forward: `grep -cE '^default = \[("mimalloc")?\]' tomlctl/Cargo.toml` prints `1`, recording the decision either way (today: `0`, since task 1 adds the line).
  - guard: **predicted, unverified**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets --no-default-features` stays clean.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 3, 4 — dependency closure: 1, 2, 3, 4. Tomlctl is lib+bin with the facade, and the hook gate runs the lib tests. The tomlctl crate is green on its own, and nothing consumes the facade yet.

— CHECKPOINT B after tasks 8, 14, 15 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 14, 15. Glimpse fetches, lists and records in-process. The probe and the config key are gone, and root docs match. A buildable glimpse still polls every `poll_ms`.

— CHECKPOINT C after tasks 13 — dependency closure: 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13. The watcher drives the poller, the tail runs on the poller thread, and glimpse's docs are current.

## Risks

- **glimpse carries its own copy of tomlctl.** A tomlctl change to the snapshot, flow list or `agents.toml` schema reaches glimpse only on reinstall, and a stale glimpse hook becomes a second writer of `agents.toml`. Mitigations:
  - the narrow pre-commit clippy gate (task 3) catches build breaks;
  - a CLAUDE.md gotcha plus an After Merge step to reinstall both.
- **Advisories into the TUI.** tomlctl's `advise!` writes to stderr when stderr is a terminal, and glimpse's is. Mitigation: `silence_advisories()` runs first in every facade fn (task 4).
- **Process-global state from `record_agent`.** It calls `set_current_dir` and fixes the repo-root `OnceLock`. Mitigation: it is called only from the one-shot `glimpse hook` process and documented as once-per-process. The TUI never calls it.
- **notify 8.2.0 silent watch failures.** Windows overflow, a deleted watch root, read-start failures, and mounts that deliver no events. Mitigation: the 10 s safety tick, the sticky missed-change fallback, and re-creating the watcher on a `Wake::All` caused by an error. The design treats notify 9.0's rescan events as a later improvement, not a requirement.
- **Linux inotify drop panics.** `Drop` `.unwrap()`s, and glimpse builds with `panic = "abort"`. Mitigation: the watcher lives on the poller thread and `detach` never joins it. A panic there aborts only at exit, when inotify's own thread has already died. Accepted as low probability.
- **macOS and Linux event shapes are read from source, not observed.** Mitigation: `classify` treats any non-`Access` event as a wake, and any path it can't place as `All`. The live watch test in task 10 runs on whatever OS runs the suite.
- **The lib target narrows the rustdoc deny lints.** A same-named lib+bin documents only the lib's public items. Mitigation: the `--document-private-items` command in `lint:` and root `CLAUDE.md`.

## After Merge

- `cargo install --path tomlctl` then `cargo install --path glimpse`: glimpse now embeds tomlctl code, so reinstall both after any future tomlctl change as well.
- Delete any `tomlctl = …` line from your glimpse config (`GLIMPSE_CONFIG` / the default config path), or glimpse reports `unknown key`.
- Restart running glimpse panes so they pick up the new binary.
- Run the **predicted, unverified** success criteria by hand:
  - `glimpse --once --slug <slug>` in a shell whose `PATH` has no `tomlctl`;
  - leave a live glimpse idle, confirm it does no periodic work beyond the 10 s tick, then hand-edit another flow's `context.toml` in place and watch the selector row update.
- Update `glimpse/CLAUDE.md`'s transcript version anchor only if the tail's parsing changed. It shouldn't have.

## Exploration Notes

**tomlctl as a library** (`tomlctl/` is bin-only; no workspace, each crate has its own `Cargo.lock`/`target`; `lumina/server` is the in-repo lib+bin precedent with `#[global_allocator]` in `main.rs`).
- Snapshot entry: `tasks::snapshot(slug: &str, store_path: &Path, read_opts: &ReadIntegrityArgs) -> anyhow::Result<serde_json::Value>` (`tomlctl/src/tasks/snapshot.rs:35`). Key order `schema, revision, slug, plan_path, flow_status, policy, tasks, layers, frontier, edges, checkpoints, record, agents`; `schema` = `const SCHEMA: u32 = 1` (`:26`). With both integrity flags false it is pure `std::fs::read` + parse: no `repo_or_cwd_root`, no locks, no stderr. The verify path takes `with_shared_lock` → `repo_or_cwd_root()` (process `OnceLock`, `TOMLCTL_ROOT` re-read per call; `io.rs:1555-1580`). glimpse must build `<root>/.claude/flows/<slug>/tasks.toml` itself, not call `store::resolve_store_path` (slug regex + root lookup).
- Clap leaks: `ReadIntegrityArgs` (`#[derive(Args)]`, two bools) + `read_integrity_opts` from `crate::cli`; `edges::edge_list_with(.., Option<EdgeKind>)` where `EdgeKind: ValueEnum`; `convert.rs`, `dedup.rs` derive `ValueEnum`. Removing clap from the closure is not realistic; a clap-free facade signature is.
- `flow list`: only `flow/list.rs:60 dispatch(status, branch, active_only, integrity) -> Result<()>` which prints; worker `enumerate_flows(root, flows_dir, verify, strict_read) -> Result<(Vec<FlowRecord>, Vec<JsonValue>)>` is private, `FlowRecord` private. Needs a root-taking, value-returning function.
- **lib+bin trap**: `.githooks/pre-commit` runs `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests`; root `CLAUDE.md` documents it (Developer setup paragraph, Build & test bullet). Once modules live in `lib.rs`, `--bin tomlctl` matches 0 tests and exits 0 — a silent no-op. Must switch to `--lib`. Module path `cli::dispatch::tests` (`cli/dispatch.rs:1373-1374`; `tests/lint.rs:261` command_lint, `tests/skills.rs:1193`) is unchanged. `tomlctl/tests/*.rs` are all black-box `assert_cmd` (224 `cargo_bin`), unaffected. `test_support` moves as-is. `emit_error` + the `TaggedError`/`ErrorFormat`/`Cli` readers should move into one `pub fn` in the lib so nothing else needs widening; modules stay private (dead-code warnings preserved). `[lints.rustdoc] private_intra_doc_links = "deny"` bites any new `pub` item doc-linking a `pub(crate)` one.
- Capabilities `FEATURES` (`cli/types.rs:21`) has `tasks_snapshot`, `flow_list`.
- Dep weight: snapshot needs toml, serde_json, anyhow, sha2, clap (derives), regex (store slug regex); compiled-but-unneeded: mimalloc (C build), globset, memchr, jiff, tempfile; windows-sys/libc only on the verify lock path (already target-gated in tomlctl `Cargo.toml`). Feature unification turns on `toml/preserve_order` in glimpse (both lock toml 1.1.6). No pre-commit step builds glimpse when only `tomlctl/src/**` is staged — a tomlctl change can break glimpse unseen.

**glimpse data path** (working tree has uncommitted UI edits in 20 files; `source.rs`, `flows.rs`, `transcript.rs`, `hook.rs`, `tests/cli.rs` untouched).
- `source.rs`: `Event` (:21) {Snapshot, SourceError, Flows, FlowMtimes, Input}; `REQUIRED_FEATURE`/`REQUIRED_MESSAGE` (:41-42); `RETRY_AFTER` 5 s; `FLOW_FILES` = tasks, execution-record, agents, context (:48); `fingerprint` (:62); `flows_fingerprint` (:91, dir-mtime gated → the in-place-edit gap); `trait Fetcher: Send { fetch(root, slug) -> Result<Snapshot,String>; list_flows(root) -> Result<Vec<FlowEntry>,String> }` (:131); `TomlctlFetcher` (:137, spawns with `TOMLCTL_ROOT`, `serde_json::from_slice`); `check_capabilities`/`probe_tomlctl` (:171/:182); `Poller` (:196) with `retry_after`, `probe`, `halted`; `run` uses `recv_timeout(interval)` (:343); `Source::start(root, slug, &Config, events)` (:370) reads `poll_ms`; `Source::spawn(.., interval, fetcher, probe, events)` (:394).
- Callers: `main.rs:30` imports `Fetcher`/`TomlctlFetcher`; `main.rs:109-140` `--once` path (`once_snapshot` probes on failure, `fetch_once`); `runtime.rs:81` `Source::start`; `hook.rs:24` duplicates the message as `OLD_TOMLCTL`.
- Tests: tick-driven fingerprint tests :545, :567, :628, :664 (break under a watcher); :521 tests `fingerprint`; timing tests :593 (`retry_after`), :713 (10 ms interval), :740 (20 ms interval); :700 capabilities.
- `flows.rs`: `FlowEntry {slug,status,updated,plan_path,tasks_mtime}` (:11); `parse_flows(json: &str, mtime)` (:39); `list(root, tomlctl)` (:81) spawns `flow list`; `repo_root` (:106) spawns `git`.
- `model.rs:15` `Snapshot` derives Deserialize, all `#[serde(default)]` → `serde_json::from_value` works as-is.
- `runtime.rs`: `run_loop` (:262) waits `TICK` while activity has a running agent (:274-278) else `app.tick_interval()` (1 s / 30 s, `app.rs:506`), blocks on `recv` when none (:287). Transcript tail runs on the main thread: `Screen::refresh_tail` (:215) → `tail.retarget(agent.transcript_path)` + `tail.refresh()`, called :270 (first frame) and :314 (before each redraw); test :514. `Screen.tail: TailState` (:190), built :83 / `render_once` :141. `view::render`/`activity::render` borrow `&TailState` (`view/mod.rs:51,219`; `activity.rs:40`). `app.rs:587 refresh_stale` stats transcripts via `transcript_mtime` (:665) on the main thread too.
- `transcript.rs`: `TailState {path, offset, entries, tokens, rejected, accepted, root}` (:47); `new` (:66) confines to `claude_dir()`; `retarget` (:82), `refresh() -> bool` (:95); `READ_MAX` 64 KiB, `KEEP_ENTRIES` 64. Tails ONE transcript — newest running agent on the selected task (`view/activity.rs:23`), path from snapshot `Agent.transcript_path`; retarget resets.
- `config.rs`: `poll_ms` (:245, default 500 :275, `positive_int` :324), `tomlctl` (:246, default "tomlctl" :276); tests :516-517, :572-573, :581-585, :596. `hook.rs:291-327` spawns `tomlctl agents record` (a separate short-lived process). `flows.rs:82` also spawns.
- `glimpse/tests/cli.rs`: sandbox names `glimpse-test-no-such-tomlctl` (:30); only the two hook cases (:125, :252) depend on the spawn failing; `--once` cases use `--snapshot`.

**Existing watcher pattern** — lumina: `notify = "8.2.0"` (no serde; `lumina/core/Cargo.toml:22-24`); `lumina/core/src/jsonl_tail/mod.rs:250-460`: `notify::recommended_watcher` (:262), watch parent dir `NonRecursive` (:283), callback → mpsc (:255-260), match `Create(File|Any)` then filter `Modify|Create|Any` by path (:312, :386-389).

**tomlctl write sequence a watcher sees** (`io.rs:806-841 → 1954-1988 → 2028-2092`): lock file in `<root>/.claude/.locks/` (outside the flow dir); sidecar `.tmpXXXXXX` create/write/sync/rename → `X.toml.sha256`; then TOML `.tmpXXXXXX` → `X.toml`; unix parent-dir fsync. Retries on sharing violation 5/32/33 ×3 @50 ms. Two tmp+rename pairs per write, sidecar first.

**Prose surface to update**: glimpse `README.md` :5-6, :17-19, :185-186, :232-237, :250-254; glimpse `CLAUDE.md` :3, :5, :13, :18, :19, :26, :34; module docs `source.rs:1-5,129-130`, `runtime.rs:3-6,257-261`, `transcript.rs:4-5`, `view/activity.rs:3`, `model.rs:1-2`, `flows.rs:37`, `main.rs:107-108`, `cli.rs:28-29`; `glimpse/Cargo.toml` description; root `CLAUDE.md` (hook paragraph, Build & test bullet, sibling-crates glimpse bullet); snapshot contract docs `claude/skills/tomlctl/references/tasks.md:283-304`, `tasks-store.md:143-194`, `flow-contract-task-store/SKILL.md:249-253` (contract unchanged — likely no edit).

**Build/test commands**: tomlctl `cargo test|clippy --all-targets --manifest-path tomlctl/Cargo.toml`, `--test tasks_corpus -- --ignored`, `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`; glimpse `cargo test --manifest-path glimpse/Cargo.toml` (`--test cli`), `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`, `cargo fmt --manifest-path glimpse/Cargo.toml -- --check`, headless `glimpse --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40`, `--once --slug <slug>`. Global `~/.cargo/config.toml`: sccache wrapper; `CARGO_INCREMENTAL=0` for full runs.

**Pinned versions**: glimpse — ratatui 0.30.2, crossterm 0.29.0, serde 1.0.229, serde_json 1.0.151, toml 1.1.6, pulldown-cmark 0.13.4, sha2 0.10.9 (transitive), windows-sys 0.61.2, libc 0.2.189. tomlctl — clap 4.6.7, mimalloc 0.1.52 / libmimalloc-sys 0.1.49, sha2 0.11.0, tempfile 3.27.0, regex 1.13.1. lumina — notify 8.2.0. rustc 1.98.1.

**Backlog** — `.claude/backlog.toml` present, 197 rows, none live (196 resolved, 1 dismissed); `--area-prefix` glimpse / tomlctl/src/tasks / tomlctl/src/flow all `[]`. No fold-in candidates.

## Research Notes

Vet (no flow ledger exists in plan mode, so vet events are recorded here): `vet: Agent-1 (file-watching) — 4 findings sampled, 0 dropped, 0 downgraded`; `vet: Agent-2 (lib-packaging) — 4 findings sampled, 0 dropped, 0 downgraded`. Anchors confirmed: notify `windows.rs:40` `BUF_SIZE = 16384`, `lib.rs` `fsevent` gated on `not(feature="macos_kqueue")`, `Cargo.toml` `default = ["macos_fsevent"]`, inotify mask includes `OPEN`; glimpse `source.rs:349` halted loop; tomlctl `tasks/mod.rs:53` `pub(crate) use`, `cli/types.rs:163` `pub(crate) struct ReadIntegrityArgs`, `agents/record.rs:410` `set_current_dir`; no `cargo doc` in hooks/docs.

### File watching (notify) — research-deep
Searched: crates.io API (notify, debouncer-mini/full), notify CHANGELOG (main), notify-rs/notify#963/#964, microsoft/WSL#4739, OSV ×8, OpenSSF Scorecard — all 2026-09-30; installed `notify-8.2.0` source; scratch probes on Windows 11 NTFS.
- **Pin `notify = "8.2.0"` with default features** (high). 8.2.0 is the newest stable; 9.0 is at rc.5 (MSRV 1.88). Do NOT set `default-features = false`: `mod fsevent` compiles on macOS regardless of the `macos_fsevent` feature, so dropping defaults breaks the macOS build invisibly from Windows. std `mpsc::Sender` is an `EventHandler`; no crossbeam. No OSV advisories; CC0. Cost: duplicate `windows-sys` 0.60.2 beside 0.61.2 (gone in 9.0). *Impact*: one dep line; design so 9.0's fixes are upgrades, not requirements.
- **Windows overflow is silent in 8.2.0** (high). Zero-byte completion is parsed as empty; `ERROR_NOTIFY_ENUM_DIR` unwatches and logs; no Rescan emitted (fixed in 9.0.0-rc.5, #964). Probe: loss only when the callback stalls ≥300 ms under ≥200-file bursts; a watch survives. Realistic trigger: git checkout/pull/stash across `.claude/flows` (git-tracked, 45 flows). *Impact*: callback must be a non-blocking unbounded `send`; keep a safety poll.
- **Silent watch death** (high): overflow; watched dir deleted (Windows unwatches silently); read-start failure on SMB/`\\wsl$` while `watch()` returned `Ok`; WSL `/mnt/c` and Docker Desktop bind mounts deliver nothing. *Impact* (needs-plan): watcher only wakes the poller; fingerprint stays the decider; safety tick (~10 s) while watching; fall back to `poll_ms` polling when the watch cannot be established (e.g. `.claude/flows` absent at start → `PathNotFound` on all backends) and retry the watch each tick; on callback `Err`/`need_rescan()`/`Remove` of the root, tick now and re-establish. Missed-change detector: a safety tick finding a fingerprint change with no event since the last tick, twice running → treat watcher as dead, drop to `poll_ms` (OS-agnostic, no filesystem sniffing). Traps: `retry_due` (source.rs:309) is only checked on a tick — wait must be `min(safety, retry remaining)`; the halted loop `while let Ok(Control::SetSlug(_))` exits on the first non-SetSlug message.
- **One Recursive watch on `.claude/flows`, set up once** (high). FSEvents is always recursive and `watch()/unwatch()` restarts the stream from "now" (losing the gap), so per-slug re-watching on SetSlug is wrong on macOS; inotify costs 1 watch per dir (46 here, limit ≥8192); on Windows a per-slug watch died on delete+recreate while the Recursive one survived. NonRecursive on `flows` alone misses in-place edits and (Linux) all child events.
- **Recursive watch sees NTFS in-place edits** (high, observed) — but `flows_fingerprint` reuses a cached stat while the dir mtime is unchanged, so the wake must carry the slug (event path `strip_prefix` the watched root) and evict `flow_stats[slug]`; on prefix failure (macOS canonical `/private/...` paths) or a pathless rescan, evict all. Retires the CLAUDE.md "in-place edits" gotcha (which also applies on ext4, not only NTFS).
- **Event filtering** (high Windows observed; medium macOS): wake on any kind except `EventKind::Access(_)` — inotify's `OPEN` mask would otherwise wake glimpse on its own snapshot reads. Windows tomlctl write ≈ 18 raw events per write pair.
- **Hand-rolled coalescing, no debouncer crate** (high): debouncer-mini 0.7.0 keys by path and drops pathless Rescan events, and adds a thread. Coalesce in `Poller::run`: after the first wake, drain with `recv_timeout(~50 ms)` until quiet, capped ~250 ms, then tick once.
- **Own the watcher on the poller thread** (high): Windows/Linux drop is µs; macOS drop joins the runloop thread (deadlocks if the callback blocks). Linux drop `.unwrap()`s — panics abort under `panic = "abort"`. `detach` keeps any of this off the exit path. The callback's cloned `Sender` keeps the control channel alive, so `Stop` stays the shutdown signal (the `Disconnected` arm stops firing).

### tomlctl lib packaging + mimalloc — research-deep
Searched: OSV (mimalloc 0.1.52, libmimalloc-sys 0.1.49), deps.dev, OpenSSF Scorecard `purpleprotocol/mimalloc_rust` (2026-09-28 scan), local cargo 1.98.1/std docs; scratch crates; interleaved start-up benchmark on this machine.
- **Optional `mimalloc` feature, not a `cli` feature with `required-features`** (high). With `[[bin]] required-features` off: `cargo run` errors, `cargo install` finds no binaries, and `cargo test --no-default-features` builds the integration tests with `CARGO_BIN_EXE_*` set but not the bin → assert_cmd runs a stale exe or panics (224 call sites). Instead: `mimalloc = { version = "0.1", default-features = false, optional = true }`, `[features] default = ["mimalloc"]`, `#[cfg(feature = "mimalloc")]` on the allocator static in `main.rs`; glimpse takes `tomlctl = { path = "../tomlctl", default-features = false }`. Saving is compile time only (5 crates incl. a ~12 s C++ build script); an unreferenced mimalloc is not linked anyway. clap stays in the lib either way.
- **`#[global_allocator]` in a lib is legal and wins silently** (high) when the dependent declares none; conflict only errors if both declare. *Impact*: acceptance check that `global_allocator` appears only in `tomlctl/src/main.rs`.
- **Do not adopt mimalloc in glimpse** (medium — Windows+Sophos only). Start-up +3.8–6 ms median per process; parse+serialise of the fixture snapshot 79–89 µs → 35–41 µs (~40 µs saved per changed snapshot, invisible). Would add the `cc` chain + C++ compile and a VCRUNTIME140_1 import (dynamic CRT). Scorecard Maintained 0, Code-Review 2. Counter: a Linux/macOS or real `--once` measurement showing a visible win.
- **Facade via wrapper fns in `lib.rs`, not `pub use`** (high). Re-exporting a `pub(crate)` fn is E0364; a `pub fn` naming a `pub(crate)` type warns `private_interfaces`. Shape: `pub fn snapshot(slug: &str, store_path: &Path) -> anyhow::Result<serde_json::Value>` building `ReadIntegrityArgs { verify_integrity: false, strict_read: false }` (the verify path hits the process-wide `repo_or_cwd_root` `OnceLock` and glimpse must never verify). Flow list follows the same root-taking, no-integrity shape. Facade doc comments must not intra-doc-link private items (`cargo doc` fails; build/clippy silent). Private-module dead code is still reported.
- **lib target narrows rustdoc lints and adds doctests** (high mechanism, low impact). Same-named lib+bin → `cargo doc` documents the lib only, without private items, so broken links in private docs pass; full check is `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items`. All 11 fences in `tomlctl/src` are non-Rust → `[lib] doctest = false`.
- **Hook start-up is dominated by the tomlctl child** (medium, needs-plan): `tomlctl --version` 28 ms median / 38 ms p90 here vs the allocator's 4–6 ms. `agents::record` (`agents/record.rs:391`) already `set_current_dir`s for a one-shot process, safe in the short-lived hook but not in the TUI. Needs its own clap-free facade. Counter: two writers of `agents.toml` (glimpse's compiled-in tomlctl vs the installed CLI) under a schema change.
- **Harmless**: `toml/preserve_order` unification (glimpse never serialises a `toml::Table`); path dep needs no `version` key; tomlctl's `.cargo/config.toml` rustflags don't apply when compiled into glimpse (not a defect).
