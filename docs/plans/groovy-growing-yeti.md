# Plan: Fix PTY initial-prompt startup-hang (claude readiness gate)

**Plan path**: `docs/plans/groovy-growing-yeti.md`
**Created**: 2026-06-30
**Status**: Draft

## Context

A lumina-spawned `claude` PTY session (via `POST /api/sprints/{id}/run` or the SPA Launch
control) reaches status `Awaiting` and **never progresses**: claude produces no JSONL
transcript at all, yet the process stays alive. This blocks all autonomous sprint execution.
The repro is new under Claude Code **2.1.196** (the lumina PTY workarounds were last verified
against 2.1.156 — ~40 patch versions of drift).

**Root cause (confirmed by code reading, to be empirically validated in Task 1):** lumina
dispatches the initial prompt into claude's PTY before claude's TUI/readline is ready. The
spawn pipeline flips the session to `Idle` *synchronously* immediately after the child spawns
(`spawn.rs:264`), enqueues the initial prompt (`spawn.rs:300-327`), and the supervisor's next
250 ms tick pops and writes it (`supervisor.rs:238`) — all within ~137 ms of spawn. claude
2.1.196 takes >1 s to render its banner, load MCP servers, and activate readline, so the
prompt body + submitting Enter land in the PTY kernel buffer before readline is live and are
**dropped**. claude sits at an empty prompt, starts no turn, writes no JSONL → `Awaiting`
forever.

**Critical design constraint surfaced during exploration:** the obvious "gate on claude's
first JSONL record" is a **deadlock**, and the code already knows it. `spawn.rs:254-264`
documents that a *prior* version flipped `Idle` on the first JSONL record, but "interactive
claude doesn't write JSONL until the user submits a prompt, so deferring the transition leaves
the supervisor unable to dispatch the very prompt that would produce the first record." That
prior fix (flip `Idle` immediately) is exactly what introduced today's timing race. The
readiness signal therefore **must be PTY output**, never JSONL.

**Intended outcome:** a lumina-spawned session into a fresh worktree reaches a working claude
that receives the seeded `/lumina:run-sprint` prompt and produces JSONL (status progresses
past `Awaiting`), verified end-to-end against claude 2.1.196 — without regressing interactive
`POST /input` sessions or the just-merged workspace-trust pre-seed.

## Scope

- **In scope**: a throwaway ConPTY diagnostic probe to confirm/calibrate the diagnosis; a
  PTY-output readiness signal plumbed onto `Session`; a supervisor dispatch gate that holds
  the first prompt until claude is ready; a mark-Failed failsafe for a truly-stuck startup; a
  deterministic regression test for the gate; doc updates.
- **Out of scope**:
  - The trust pre-seed (`pty/trust.rs`, `spawn.rs` step 0) — do **not** touch (just merged).
  - The bypass-permissions dialog handling (`--settings skipDangerousModePermissionPrompt`) —
    already correct; the session hangs alive, it does not exit-1.
  - The "1 MCP server needs authentication · run /mcp" banner — confirmed by the user to be an
    artifact of the lumina server being offline *during the merge*; it is **not** a
    normal-operation problem and needs no guard here.
  - The pre-existing racy `POST /input` sequence allocation (`enqueue_input` uses
    `list.len()+1`, not transactional) — noted as a risk, not fixed (one-concern rule).
- **Affected areas**: `lumina/server/src/pty/` (`session.rs`, `transport.rs`,
  `pty_transport/mod.rs`, `spawn.rs`, `supervisor.rs`), `lumina/server/tests/` (new probe),
  `lumina/.config/nextest.toml`, `lumina/CLAUDE.md`.
- **Estimated file count**: ~8.

## Research Notes

External library/API research was **not** run: the only genuine unknown is claude 2.1.196's
empirical startup behaviour, which is resolved by the Task 1 probe (the task's prescribed
diagnostic instrument), not by documentation. portable-pty and tokio atomics are internal,
well-understood dependencies. Findings below are from direct code reading (Phase 2
exploration).

- **Spawn→dispatch ordering** (`lumina/server/src/pty/spawn.rs`): `Transport::spawn` returns;
  status flips `Spawning→Idle` synchronously at `spawn.rs:264` (no await on readiness); initial
  prompt enqueued to `pty_queue` at `spawn.rs:300-327`; JSONL bridge spawned at `spawn.rs:338+`
  and **waits unbounded** for the JSONL file to appear (`bind_jsonl_path(..., None)`).
- **Supervisor** (`lumina/server/src/pty/supervisor.rs`): 250 ms `TICK_PERIOD`; `tick_once`
  routes `Idle→dispatch_one`, `Awaiting→maybe_finalise_turn`. `dispatch_one` pops the queue,
  sends the frame via `session.input_tx.send` (`supervisor.rs:238`), then `set_status(Awaiting)`
  (`supervisor.rs:310`). `maybe_finalise_turn` watches only `last_record_at` (JSONL quiescence).
- **The readiness seam** (`lumina/server/src/pty/pty_transport/mod.rs`): the reader-blocking
  worker (`mod.rs:429-456`) forwards every PTY chunk to the **drain-and-discard bridge**
  (`mod.rs:489-494`), which currently drops them ("the canonical transcript flows out of
  `jsonl_tail::tail`, so we do NOT parse PTY bytes"). This bridge is the single tap point for
  "first PTY output byte." It lives inside `Transport::spawn`, which returns `TransportHandle`
  (`transport.rs:57`) — the seam to carry a readiness `Arc<AtomicI64>` out to `Session`.
- **`Session`** (`lumina/server/src/pty/session.rs`): already carries `last_record_at:
  AtomicI64` updated by the JSONL bridge; the new PTY-output stamp mirrors that pattern exactly.
- **Input-bridge** (`mod.rs:507-582`): translates `\n→\r`, and for a body with a trailing CR
  writes body → sleeps `PROMPT_SUBMIT_SETTLE_MS` (220 ms) → sends a *separate* `\r` Enter.
  This 220 ms settle is **orthogonal** to the new readiness gate (it spaces body-vs-Enter
  *after* dispatch; the gate decides *whether* dispatch runs).
- **Tests/verification** (`lumina/server/tests/`, `lumina/.config/nextest.toml`): existing PTY
  tests use a deterministic `pty_stub` (`CARGO_BIN_EXE_pty_stub`), not real claude; ConPTY repro
  pattern (`NativePtySystem` + `PtySize{24,80}` + Windows slave-keep-alive guard + mpsc reader
  thread + 100 ms `recv_timeout` poll) is in `conpty_minimal_repro.rs`; JSONL path is
  `<LUMINA_PTY_PROJECTS_ROOT>/<jsonl_tail::sanitise_cwd(cwd)>/<session_id>.jsonl` (`pty_e2e.rs`);
  the `quick` profile excludes nested-claude binaries via
  `default-filter = "not (binary(pty_e2e) | binary(conpty_minimal_repro))"`. Any `.rs` under
  `tests/` is an implicit test binary (no `[[test]]` registration needed).
- **Doc drift (minor):** `lumina/CLAUDE.md` and `mod.rs:589` say "portable-pty 0.9", but
  `lumina/server/Cargo.toml:65` pins `portable-pty = "=0.8.1"`. Fix the prose in Task 4.

## User Decisions

1. **Failsafe on no-output startup cap** → *Mark session Failed.* After a generous cap (≥30 s)
   with zero PTY output, mark the session `Failed` with a diagnostic so a stuck startup
   surfaces to `get_sprint_quiescence` / the SPA rather than hanging silently.
   *(Prompted by: the readiness gate needs a defined behaviour when `first_output_at` stays 0 —
   a truly-wedged claude — and autonomous sprint execution must not hang invisibly.)*
2. **Regression-test deliverable** → *Stub/unit gate test + throwaway probe.* A permanent,
   deterministic test of the gate logic (no real claude), plus the real-claude probe kept as an
   `#[ignore]`'d manual diagnostic. *(Prompted by: the existing `pty_e2e` uses a deterministic
   `pty_stub`, so the gate can be regression-tested without an environment-dependent real-claude
   run.)*
3. **MCP-auth banner scope** → *Non-issue.* The unauthenticated-MCP banner observed in the
   manual run was caused by the lumina server being offline during the merge; it will not occur
   in normal operation. No guard, no follow-up needed. *(Prompted by: the manual-run banner
   observation flagged for suspect #3.)*
4. **Readiness signal type** → *Let the probe decide.* Implement the gate so the signal is
   `first-output + fixed grace` by default, but Task 1's probe determines whether claude's
   startup output quiesces (favouring an output-quiesce variant) or repaints continuously
   (favouring fixed-grace), and calibrates the delay constant. *(Prompted by: the exact gate
   shape depends on claude 2.1.196's empirical render pattern, which only the probe reveals.)*

## Approach

**Diagnose, then fix.** Task 1 builds the prescribed throwaway ConPTY probe and runs it against
real claude 2.1.196 to (a) confirm suspect #1 and (b) calibrate the gate. The fix (Tasks 2–3)
adds a PTY-output readiness signal and gates the supervisor's first dispatch on it; Task 4
documents it.

**The readiness signal (PTY output, not JSONL).** Add `first_output_at: Arc<AtomicI64>` to
`Session`, mirroring the existing `last_record_at`. It is created in `Transport::spawn`, a clone
captured by the drain-and-discard bridge (`mod.rs:489-494`) which stamps it (wall-clock ms) on
the **first non-empty chunk** and continues draining, and returned on `TransportHandle` so
`spawn.rs` threads it onto the `Session`. Also stamp a `spawned_at` instant on `Session` (for the
failsafe cap). The signal is **one-way**: once set it never resets, so it is satisfied
permanently after claude's first byte (~1 s) — which is why it gates only the *first* prompt and
never regresses interactive `POST /input` (a human types long after claude has rendered) or
post-turn re-dispatch.

**The dispatch gate (supervisor).** Extract a pure predicate
`dispatch_gate(first_output_at, spawned_at, now) -> Gate` returning `Ready | Wait | StartupTimedOut`,
and consult it in `tick_once` before the `Idle→dispatch_one` branch:
- `Ready` (first byte seen **and** the chosen readiness condition met) → `dispatch_one` as today.
- `Wait` → skip this tick; the queued prompt stays pending, retried next tick.
- `StartupTimedOut` (`first_output_at == 0` and `now - spawned_at > MAX_STARTUP_MS`) → mark the
  session `Failed` with a diagnostic message, mirroring the existing failure path
  (`supervisor.rs:260`: `set_status(Failed)` + `repo::pty::update_pty_session_status(..,"failed",..)`).

The readiness condition inside `Ready` is the Task-1-calibrated choice (User Decision 4):
default `first_output_at > 0 && now - first_output_at >= READY_DELAY_MS`; switch to an
output-quiesce variant (adding a `last_output_at` stamp updated on every chunk) only if the
probe shows claude's startup output cleanly quiesces. `READY_DELAY_MS` (start ~1500 ms) and
`MAX_STARTUP_MS` (start ~45 s, generous to avoid false-failing a slow machine) are constants
calibrated from probe data.

**Reuse:** the `AtomicI64`/`now-ms` comparison pattern already exists in
`maybe_finalise_turn` (`last_record_at` vs `IDLE_THRESHOLD`); the mark-Failed path already
exists in `dispatch_one`. The probe reuses the `conpty_minimal_repro.rs` ConPTY scaffold and
`pty_transport::resolve_claude_bin` / `jsonl_tail::sanitise_cwd`.

**Branch:** implement on a fresh branch off `main` (e.g. `fix/pty-initial-prompt-readiness-gate`).

## Verification Commands

```
build: $env:LUMINA_SKIP_WEB_BUILD=1; cargo build --workspace --manifest-path lumina/Cargo.toml
test: cargo nextest run --manifest-path lumina/Cargo.toml --profile ci
lint: cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets
```

Sub-agent inner loop (narrow): `cargo clippy --manifest-path lumina/Cargo.toml -p lumina-server`
and `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'test(supervisor) + test(pty)'`.

## Tasks

### 1. Build + run the throwaway ConPTY diagnostic probe [M]
- **Files**: `lumina/server/tests/pty_readiness_probe.rs` (new), `lumina/.config/nextest.toml`
- **Depends on**: —
- **Action**: Add an `#[ignore]`'d real-claude probe that spawns claude under ConPTY with
  lumina's exact flags, dumps raw stdout for ~3 s, then writes a test prompt + Enter after a
  real delay and reads ~5 s, reporting what it observed.
- **Detail**: Model on `lumina/server/tests/conpty_minimal_repro.rs` (NativePtySystem,
  `PtySize{rows:24,cols:80}`, Windows slave-keep-alive guard, mpsc reader thread, 100 ms
  `recv_timeout` poll loop) and `pty_e2e.rs` (JSONL path via
  `jsonl_tail::sanitise_cwd(canonical_cwd)` under a temp `LUMINA_PTY_PROJECTS_ROOT`). Resolve
  claude via `pty_transport::resolve_claude_bin` (honours `LUMINA_CLAUDE_BIN`). Replicate the
  full static flag set from `pty_transport/mod.rs:233-361` (`--session-id`, `--permission-mode
  bypassPermissions`, `--settings {skipDangerousModePermissionPrompt,env}`, two
  `--append-system-prompt`, `--mcp-config`, env `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`). The
  probe must answer and **record** (printed to stdout / captured to a scratch file): (a) what
  the first ~3 s of raw stdout shows (dialog? normal prompt? blank?); (b) the time-to-first-byte
  `T_first_output`; (c) whether output **quiesces** after the banner or repaints continuously;
  (d) whether a prompt submitted *after a delay* produces a JSONL file while the current
  fire-immediately path does not. Mark `#[ignore]` (real claude, env-dependent). Add
  `| binary(pty_readiness_probe)` to the `quick` profile `default-filter` in `nextest.toml`.
- **Acceptance**: `cargo test --manifest-path lumina/Cargo.toml --test pty_readiness_probe -- --ignored`
  builds and runs against claude 2.1.196; the four observations (a)–(d) are recorded; suspect #1
  is **confirmed** (delayed prompt → JSONL appears; immediate dispatch → none) **or** the
  recorded evidence redirects to suspect #2/#3 (see Risks). The chosen signal type
  (fixed-grace vs quiesce) and `READY_DELAY_MS` value are written down for Tasks 2–3.
  `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick` does **not** pick up the
  new binary.

### 2. Plumb the PTY-output readiness signal onto `Session` [M]
- **Files**: `lumina/server/src/pty/session.rs`, `lumina/server/src/pty/transport.rs`,
  `lumina/server/src/pty/pty_transport/mod.rs`, `lumina/server/src/pty/spawn.rs`
- **Depends on**: 1
- **Action**: Add a `first_output_at` PTY-output timestamp (and a `spawned_at` instant) to
  `Session`, set by the drain bridge on first PTY output, threaded from `Transport::spawn`
  through `TransportHandle`.
- **Detail**: In `pty_transport/mod.rs`, create `let first_output_at = Arc::new(AtomicI64::new(0))`
  before the drain-and-discard bridge (`mod.rs:489-494`); inside the bridge, on the first
  non-empty chunk, CAS/store the current wall-clock ms (and, only if Task 1 picks the
  output-quiesce variant, also maintain a `last_output_at` updated every chunk). Continue
  draining unchanged (backpressure must be preserved). Return the `Arc<AtomicI64>` on
  `TransportHandle` (`transport.rs:57`). In `session.rs`, add `pub first_output_at:
  Arc<AtomicI64>` plus a `spawned_at` instant/ms field, mirroring `last_record_at`; accept the
  Arc via `Session::new` (update the few in-module/test call sites — keep the JSONL-bridge
  `last_record_at` pattern as the template). In `spawn.rs`, pass `handle.first_output_at` into
  `Session::new`. Do **not** change the `Idle` flip ordering or the trust pre-seed.
- **Acceptance**: `cargo clippy --manifest-path lumina/Cargo.toml -p lumina-server` passes;
  `first_output_at` is observably stamped within one chunk of claude's first PTY output (covered
  by Task 3's test); no signature churn outside `Session::new` call sites.

### 3. Add the supervisor dispatch gate + mark-Failed failsafe + regression test [M]
- **Files**: `lumina/server/src/pty/supervisor.rs`
- **Depends on**: 1, 2
- **Action**: Gate the `Idle→dispatch_one` branch in `tick_once` on a pure
  `dispatch_gate(...)` predicate; on startup timeout mark the session `Failed`; add a
  deterministic unit test of the gate.
- **Detail**: Add `READY_DELAY_MS` and `MAX_STARTUP_MS` consts (values from Task 1). Implement a
  pure `fn dispatch_gate(first_output_at: i64, spawned_at_ms: i64, now_ms: i64) -> Gate` returning
  `Ready | Wait | StartupTimedOut` per **Approach** (use the fixed-grace or quiesce condition
  Task 1 selected). In `tick_once`, for `Idle`: `Ready → dispatch_one`; `Wait → return` (leave the
  prompt queued); `StartupTimedOut → set_status(Failed)` + `repo::pty::update_pty_session_status
  (.., "failed", Some("claude produced no PTY output within startup cap"))` (mirror the existing
  failure path at `supervisor.rs:260`). Leave `Awaiting→maybe_finalise_turn` untouched. Add
  `#[cfg(test)]` tests: (i) pure-predicate unit tests for the three `Gate` arms (deterministic,
  no PTY); (ii) one `tick_once` integration assertion using an in-memory pool + a `Session` with
  a held `input_tx` receiver and a queued prompt — assert **no** frame is sent while
  `first_output_at == 0`, and the frame **is** sent (status → `Awaiting`) once `first_output_at`
  is set far enough in the past to satisfy the grace.
- **Acceptance**: the new tests pass via
  `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'test(supervisor)'`;
  `cargo clippy --manifest-path lumina/Cargo.toml -p lumina-server --all-targets` passes; a
  `first_output_at == 0` session past `MAX_STARTUP_MS` transitions to `Failed` (asserted in test).

### 4. Document the readiness gate + fix doc drift [S]
- **Files**: `lumina/CLAUDE.md`
- **Depends on**: 2, 3
- **Action**: Document the readiness gate as a 2.1.196-tuned PTY workaround and correct the
  portable-pty version drift.
- **Detail**: In `lumina/CLAUDE.md` § "PTY interaction" (near "Prompt submission needs a separate
  Enter"), add a short paragraph: the supervisor holds the initial-prompt dispatch until claude's
  first PTY output + grace (`READY_DELAY_MS`), because claude writes no JSONL until it processes a
  prompt (so a JSONL-based gate would deadlock — cite the `spawn.rs:254-264` rationale); a
  startup with zero PTY output by `MAX_STARTUP_MS` is marked `Failed`. Note these constants are
  calibrated against claude 2.1.196 and the re-verify procedure (run
  `tests/pty_readiness_probe.rs --ignored` after a Claude Code bump). Correct "portable-pty 0.9"
  → "0.8.1" in the prose (actual pin: `lumina/server/Cargo.toml:65`). Do not alter the
  trust-pre-seed or bypass-dialog sections.
- **Acceptance**: `lumina/CLAUDE.md` describes the gate, the JSONL-deadlock rationale, the
  failsafe, and the re-verify probe; no "portable-pty 0.9" string remains in `lumina/CLAUDE.md`.

## Dependency Graph

```
Task 1 (diagnose/calibrate)
   └─> Task 2 (signal plumbing)
          └─> Task 3 (gate + failsafe + test)
                 └─> Task 4 (docs)
```

Largely sequential — Task 1 is a diagnostic gate whose output calibrates Tasks 2–3, and the fix
tasks share `Session`/supervisor state. No parallel batches.

## Verification

- **Build**: `LUMINA_SKIP_WEB_BUILD=1 cargo build --workspace --manifest-path lumina/Cargo.toml`
  (PowerShell: `$env:LUMINA_SKIP_WEB_BUILD=1; cargo build ...`).
- **Test**: `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` (full suite,
  including the new deterministic supervisor gate tests; the `pty_readiness_probe` is `#[ignore]`
  and excluded from `quick`).
- **Lint**: `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets`.
- **Diagnostic (manual, real claude)**:
  `cargo test --manifest-path lumina/Cargo.toml --test pty_readiness_probe -- --ignored`.
- **End-to-end acceptance (manual)**: with the lumina server + companion running, `POST
  /api/sprints/{id}/run` into a **fresh worktree**; confirm the spawned session receives the
  seeded `/lumina:run-sprint <id>` prompt, a `<session_id>.jsonl` appears under
  `~/.claude/projects/<sanitised-cwd>/`, and the session status progresses past `Awaiting`.
  Confirm an interactive `POST /api/pty/sessions/{id}/input` prompt still submits normally (no
  gate regression).

## Risks

- **Probe disconfirms suspect #1** — if Task 1 shows the delayed prompt *also* produces no JSONL
  (the prompt is accepted but claude still never starts), the root cause is suspect #2 (ConPTY
  input quirk) or #3 (MCP init), not dispatch timing. *Mitigation*: Task 1 is an explicit gate;
  if (d) fails, stop and re-plan Tasks 2–3 around the actual signal the probe reveals (the probe
  output is the evidence for the next iteration).
- **`READY_DELAY_MS` too short** — a grace shorter than claude's true readline-ready time
  re-introduces the drop on slow machines / heavy MCP loads. *Mitigation*: calibrate from probe
  `T_first_output`→`T_prompt_accepted` plus margin; the gate is one-way and cheap, so err
  generous; the deterministic test pins the predicate, and the constant is documented for re-tune.
- **False mark-Failed on a slow startup** — a machine where claude takes >`MAX_STARTUP_MS` to
  emit any byte would be wrongly failed. *Mitigation*: cap is generous (~45 s) and keyed on
  **zero** PTY output (unambiguously stuck), not on slow-but-progressing output.
- **`Session::new` signature change ripple** — adding the readiness Arc touches test/construction
  call sites. *Mitigation*: few call sites (the JSONL-bridge `last_record_at` field is the
  precedent); keep the change confined to `Session::new` and update sites in the same task.
- **Pre-existing racy `POST /input` sequence** (`enqueue_input` uses non-transactional
  `list.len()+1`) — out of scope here; flagged for a separate fix.
- **ConPTY drain backpressure** — the drain bridge must keep draining at full rate; stamping
  `first_output_at` must not add latency that stalls the reader. *Mitigation*: the stamp is a
  single relaxed atomic store on the first chunk only; draining semantics are otherwise unchanged.
