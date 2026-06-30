//! THROWAWAY ConPTY startup-readiness diagnostic probe (plan `groovy-growing-yeti`, Task 1).
//!
//! This is a `#[ignore]`'d, MANUAL diagnostic — it is NOT a regression test and
//! is excluded from every automated profile (`quick` excludes the binary; the
//! single test is `#[ignore]`). It spawns a REAL `claude` CLI under a real
//! Windows ConPTY / Unix98 PTY, so it is strictly environment-dependent: it
//! requires `claude` reachable (via `LUMINA_CLAUDE_BIN` or PATH), a logged-in /
//! authenticated `claude`, and a TRUSTED spawn cwd (see below). It exists to
//! confirm + calibrate the claude-2.1.196 PTY initial-prompt timing diagnosis:
//! lumina dispatches the seeded first prompt into the PTY BEFORE claude's
//! TUI/readline is live, so the prompt body + submitting Enter are dropped into
//! the kernel buffer and lost, and the session hangs at `Awaiting` with no
//! JSONL ever written.
//!
//! Run it (and read its recorded output) with:
//! ```text
//! cargo test --manifest-path lumina/Cargo.toml --test pty_readiness_probe -- --ignored --nocapture
//! ```
//!
//! ## What it records (printed to stdout AND appended to a scratch log under the
//! OS temp dir, so a manual run is auditable):
//! - (a) the raw first ~3 s of claude's PTY stdout (lossy-UTF8) — a dialog? a
//!   normal prompt? blank?
//! - (b) `T_first_output`: ms from spawn to the first non-empty PTY chunk.
//! - (c) whether startup output QUIESCES after the banner (goes silent) or
//!   REPAINTS continuously — the per-chunk timeline + max inter-chunk gap +
//!   tail-silence let a human judge which, and pick the readiness condition
//!   (output-quiesce vs fixed-grace) + calibrate `READY_DELAY_MS` for Tasks 2-3.
//! - (d) the suspect-#1 contrast: an IMMEDIATE-dispatch session (fire the prompt
//!   right after spawn, before readiness) should produce NO JSONL file, while a
//!   DELAYED-dispatch session (settle through startup, then submit body + a
//!   SEPARATE `\r` Enter — lumina's submission contract) should produce one.
//!
//! ## Faithfulness notes / deviations from a perfect lumina-spawn replica
//! (deliberate; see the Task-1 report):
//! - The static flag set is reproduced 1:1 from `pty_transport/mod.rs` (the
//!   `--session-id`, `--permission-mode bypassPermissions`, `--settings`
//!   `{skipDangerousModePermissionPrompt, env}`, the TWO `--append-system-prompt`
//!   args, `--mcp-config <temp>`, and `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` +
//!   the autonomous-mode env token). The three system-prompt / mcp-config
//!   producers are `pub(crate)` in `pty_transport::config`, so their bodies are
//!   replicated VERBATIM below; `resolve_claude_bin` is a private `fn`, so its
//!   resolution logic is replicated too. `LUMINA_AUTONOMOUS` / `autonomous_secret`
//!   and `sanitise_cwd` / `resolve_projects_root` ARE `pub`, so they are reused.
//! - The probe does NOT pre-seed workspace-trust (that is `pty/trust.rs` step 0,
//!   out of scope for this task). To reproduce lumina's WORKING (trust-seeded)
//!   spawn, it defaults the spawn cwd to the process cwd (run it from a dir
//!   claude already trusts — e.g. the repo root) and lets it be overridden with
//!   `LUMINA_PROBE_CWD`. If the cwd is untrusted, claude's "Do you trust the
//!   files in this folder?" dialog appears in the (a) raw dump and will block
//!   the turn — that is itself diagnostic, but confounds (d).
//! - It watches the DIR REAL claude writes into — `resolve_projects_root()` (i.e.
//!   `~/.claude/projects` unless `LUMINA_PTY_PROJECTS_ROOT` is set) joined with
//!   `sanitise_cwd(cwd)` — and detects a turn by any `.jsonl` in that dir with an
//!   mtime at/after spawn. It does NOT guess `<session_id>.jsonl`: interactive
//!   claude names the transcript with its OWN minted UUID, not our `--session-id`
//!   (GitHub #44607 — the very reason lumina binds via `bind_jsonl_path`, and the
//!   reason the readiness signal must be PTY output, not JSONL). (pty_e2e's
//!   `LUMINA_PTY_PROJECTS_ROOT` + side-writer mechanism redirects only lumina's
//!   WATCHER, not real claude, so it does not transfer to a real-claude probe —
//!   the faithful detection is to watch claude's actual config dir.)

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, SlavePty, native_pty_system};

use lumina_core::jsonl_tail::{resolve_projects_root, sanitise_cwd};
use lumina_server::pty::mode::{LUMINA_AUTONOMOUS, autonomous_secret};

// ---------------------------------------------------------------------------
// Tuning constants (bounded windows — NO unbounded waits, mirroring
// conpty_minimal_repro's 100ms recv_timeout poll discipline).
// ---------------------------------------------------------------------------

/// Initial raw-stdout capture window (observations a/b/c).
const INITIAL_CAPTURE_MS: u64 = 3_000;
/// Post-prompt read window (observation d — wait for a turn + JSONL).
const POST_PROMPT_READ_MS: u64 = 5_000;
/// Poll cadence for the mpsc reader (matches conpty_minimal_repro).
const POLL_MS: u64 = 100;
/// Settle between writing a prompt body and the separate submitting `\r`
/// (mirrors `pty_transport::PROMPT_SUBMIT_SETTLE_MS`).
const PROMPT_SUBMIT_SETTLE_MS: u64 = 220;
/// Tail-silence threshold (ms): if the last startup chunk landed at least this
/// long before the capture window closed, startup is judged to have QUIESCED.
const QUIESCE_TAIL_SILENCE_MS: u64 = 500;

/// A trivial prompt that forces claude to start a turn (which appends the user
/// record to the session JSONL — the file whose appearance the probe detects).
const TEST_PROMPT: &str = "Respond with exactly the single word: READY";

// --- constants replicated from `pty_transport::{mod,config}` (private there) ---

/// `--mcp-config` server name lumina registers (`pty_transport::ASK_MCP_SERVER_NAME`).
const ASK_MCP_SERVER_NAME: &str = "lumina-ask";
/// Default lumina HTTP port (`pty_transport::DEFAULT_LUMINA_PORT`).
const DEFAULT_LUMINA_PORT: u16 = 24817;
/// Ask-tool timeout advertised in the mcp-config (`pty_transport::ASK_MCP_TOOL_TIMEOUT_MS`).
const ASK_MCP_TOOL_TIMEOUT_MS: u64 = 1_860_000; // 31 min
/// `claude` binary override env (`pty_transport::CLAUDE_BIN_ENV`).
const CLAUDE_BIN_ENV: &str = "LUMINA_CLAUDE_BIN";

// ---------------------------------------------------------------------------
// claude-bin resolution — replicated from `pty_transport::{resolve_claude_bin,
// search_process_path}` (both PRIVATE there, so unreachable from this crate).
// ---------------------------------------------------------------------------

fn resolve_claude_bin() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os(CLAUDE_BIN_ENV) {
        let p = PathBuf::from(override_path);
        return if p.is_file() { Some(p) } else { None };
    }
    search_process_path("claude", std::env::var_os("PATH").as_deref(), |p| {
        p.is_file()
    })
}

/// PATH walk skipping empty + relative entries (the 2026-06-10 spawn-failure
/// footgun guard), `.exe`/`.cmd`/`.bat`/`.com` extension probing on Windows.
fn search_process_path(
    name: &str,
    path_var: Option<&std::ffi::OsStr>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() || dir.is_relative() {
            continue;
        }
        let bare = dir.join(name);
        if cfg!(windows) {
            for ext in ["exe", "cmd", "bat", "com"] {
                let cand = bare.with_extension(ext);
                if is_file(&cand) {
                    return Some(cand);
                }
            }
        } else if is_file(&bare) {
            return Some(bare);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// System-prompt / mcp-config producers — replicated VERBATIM from
// `pty_transport::config` (all `pub(crate)` there). Kept byte-identical so the
// spawned `claude`'s startup work (MCP load, system-prompt size) matches lumina.
// ---------------------------------------------------------------------------

fn no_auq_system_prompt(session_id: &str) -> String {
    format!(
        "You are running inside lumina, a headless interface that CANNOT display \
claude's built-in AskUserQuestion picker. NEVER call the built-in AskUserQuestion \
tool. Whenever you need the operator to choose between options or decide between \
approaches, call the `mcp__{ASK_MCP_SERVER_NAME}__ask_user_question` tool (provided by \
the `{ASK_MCP_SERVER_NAME}` MCP server). Always set its `session_id` argument to \
exactly \"{session_id}\". Provide one or more `questions`, each with a short \
`header`, the `question` text, an `options` array (each `{{label, description}}`), \
and `multiSelect` true or false — do NOT add an \"Other\" option yourself (lumina's \
UI always offers a free-text row). The tool blocks until the operator answers in \
the lumina UI and returns their selections. Use it instead of asking the operator \
to type a choice in prose."
    )
}

fn autonomous_escalation_system_prompt() -> String {
    "When you are running AUTONOMOUSLY under lumina (a lumina-spawned session, no \
operator at a terminal), a HARD decision — one needing human judgement you cannot \
safely make alone — MUST be escalated DURABLY, not asked live. Do NOT call the \
built-in AskUserQuestion tool and do NOT block on the interactive \
`ask_user_question` tool: in a no-TTY / forked autonomous context no operator is \
watching to answer, so a live ask is structurally dead. Instead record the decision \
with `mcp__lumina__add_open_question` (story-scoped, with the candidate options) and \
PARK the deciding task with `mcp__lumina__block_task_on_question` so it leaves the \
ready queue and is not re-asked by a fresh agent. If no human answer has arrived, \
leave the task BLOCKED and defer to the clean-close-or-resume path once work \
quiesces — NEVER time out and then proceed on a guess; an unanswered hard decision \
is a stop, not a default. If the durable write itself FAILS (the open-question or \
block call errors), treat it as a HARD STOP: park or halt and surface the failure — \
do NOT proceed or degrade past an unrecorded decision. IRREVERSIBILITY FLOOR: a \
small set of DESTRUCTIVE operations — merging a worktree/branch (including \
`mcp__lumina__execute_worktree_merge`), deleting a branch, force-pushing, and \
deleting a work item — ALWAYS require a durable human decision FIRST, in EVERY mode. \
The autonomous license to take more decisions does NOT cover them: they cannot be \
undone, so before performing any of them you MUST raise an `add_open_question` and \
block on the answer, even when you would otherwise decide alone. This floor is \
mode-independent and overrides the self-decide posture."
        .to_string()
}

fn lumina_ask_mcp_url() -> String {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_LUMINA_PORT);
    format!("http://127.0.0.1:{port}/mcp-ask")
}

fn ask_mcp_config_json() -> String {
    format!(
        r#"{{"mcpServers":{{"{ASK_MCP_SERVER_NAME}":{{"type":"http","url":"{url}","timeout":{ASK_MCP_TOOL_TIMEOUT_MS}}}}}}}"#,
        url = lumina_ask_mcp_url()
    )
}

// ---------------------------------------------------------------------------
// Recorder — tee every observation to stdout AND an auditable scratch log.
// ---------------------------------------------------------------------------

struct Recorder {
    file: std::fs::File,
    path: PathBuf,
}

impl Recorder {
    fn new() -> Recorder {
        let path = std::env::temp_dir().join(format!(
            "lumina-pty-readiness-probe-{}.log",
            now_ms()
        ));
        let file = std::fs::File::create(&path)
            .unwrap_or_else(|e| panic!("create scratch log {}: {e}", path.display()));
        Recorder { file, path }
    }

    fn log(&mut self, line: impl AsRef<str>) {
        let line = line.as_ref();
        println!("{line}");
        let _ = writeln!(self.file, "{line}");
        let _ = self.file.flush();
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// ProbeSession — a single spawned `claude` under a PTY, with a guard that ALWAYS
// kills the child + drops the master (and cleans the temp mcp-config) on drop,
// on EVERY path including a panic.
// ---------------------------------------------------------------------------

struct ProbeSession {
    child: Box<dyn Child + Send + Sync>,
    // Field drop order is declaration order; the `Drop` impl below runs FIRST
    // (killing the child), THEN these fields drop. Keeping the slave (Windows
    // ConPTY routing, wezterm/wezterm#4206) + master alive for the read window;
    // dropping the master unblocks the reader thread's pending `read()`.
    _slave: Box<dyn SlavePty + Send>,
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    session_id: String,
    /// Candidate per-cwd projects DIRECTORIES claude may write its session JSONL
    /// into (canonical + bare cwd sanitisation). Interactive claude names the
    /// JSONL file with its OWN minted UUID — NOT the `--session-id` (GitHub
    /// #44607 / `bind_jsonl_path`'s note) — so detection scans these dirs for a
    /// `.jsonl` newer than `spawn_time` rather than guessing the filename.
    jsonl_dirs: Vec<PathBuf>,
    spawn_time: SystemTime,
    mcp_config_path: PathBuf,
}

impl Drop for ProbeSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.mcp_config_path);
    }
}

/// Spawn one `claude` under a fresh PTY with lumina's exact static flag set.
/// Panics on a setup failure (claude unresolved, openpty/spawn error) — the
/// probe is a manual diagnostic, so a loud failure is the right signal.
fn spawn_probe_session(cwd: &Path, projects_root: &Path) -> ProbeSession {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let claude_bin = resolve_claude_bin().expect(
        "claude executable not found — set LUMINA_CLAUDE_BIN to the binary or put claude on PATH",
    );
    let session_id = uuid::Uuid::now_v7().to_string();

    // Per-session temp --mcp-config (registers the `lumina-ask` HTTP MCP server,
    // exactly as pty_transport does). Cleaned up in ProbeSession::drop.
    let mcp_config_path =
        std::env::temp_dir().join(format!("lumina-ask-mcp-probe-{session_id}.json"));
    std::fs::write(&mcp_config_path, ask_mcp_config_json()).expect("write probe mcp-config");

    // ---- Build the command — 1:1 with pty_transport/mod.rs:233-361 ----------
    let mut cmd = CommandBuilder::new(&claude_bin);
    cmd.cwd(cwd);
    cmd.env("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");
    // Autonomous mode-signal token (server-minted; process-local secret reused
    // directly from the pub `mode` module — see file-level note).
    cmd.env(LUMINA_AUTONOMOUS, autonomous_secret());

    cmd.arg("--session-id");
    cmd.arg(&session_id);
    cmd.arg("--permission-mode");
    cmd.arg("bypassPermissions");
    cmd.arg("--settings");
    let mut settings_env = serde_json::Map::new();
    settings_env.insert(
        LUMINA_AUTONOMOUS.to_string(),
        serde_json::Value::from(autonomous_secret().to_string()),
    );
    cmd.arg(
        serde_json::json!({
            "skipDangerousModePermissionPrompt": true,
            "env": settings_env,
        })
        .to_string(),
    );
    cmd.arg("--append-system-prompt");
    cmd.arg(no_auq_system_prompt(&session_id));
    cmd.arg("--append-system-prompt");
    cmd.arg(autonomous_escalation_system_prompt());
    cmd.arg("--mcp-config");
    cmd.arg(&mcp_config_path);

    let child = pair.slave.spawn_command(cmd).expect("spawn claude");
    // Windows: keep the slave alive past spawn (ConPTY routing — wezterm#4206).
    let _slave = pair.slave;

    let reader = pair.master.try_clone_reader().expect("try_clone_reader");
    let writer = pair.master.take_writer().expect("take_writer");
    let master = pair.master;

    // Reader thread: 4 KiB reads → mpsc, exactly like conpty_minimal_repro.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Candidate per-cwd projects DIRS: claude sanitises the BARE user-visible
    // cwd, but pty_e2e canonicalises first — watch both so detection is robust
    // to which form claude derives the per-cwd dir from. We watch the DIR (not a
    // guessed filename) because interactive claude names the JSONL with its own
    // minted UUID, not our `--session-id`.
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut jsonl_dirs: Vec<PathBuf> = Vec::new();
    for base in [canonical, cwd.to_path_buf()] {
        let d = projects_root.join(sanitise_cwd(&base));
        if !jsonl_dirs.contains(&d) {
            jsonl_dirs.push(d);
        }
    }
    // Captured just before returning: any `.jsonl` in a watched dir with mtime
    // >= this was produced by THIS session's turn (a fresh session writes a
    // fresh transcript). Robust to claude minting its own JSONL filename.
    let spawn_time = SystemTime::now();

    ProbeSession {
        child,
        _slave,
        _master: master,
        writer,
        rx,
        session_id,
        jsonl_dirs,
        spawn_time,
        mcp_config_path,
    }
}

/// Read PTY chunks for a bounded window, returning `(elapsed_ms_from_start, bytes)`
/// per non-empty chunk. NO unbounded wait — bounded by `duration`, polled at
/// `POLL_MS` (conpty_minimal_repro's `recv_timeout` discipline).
fn read_window(
    rx: &Receiver<Vec<u8>>,
    start: Instant,
    duration: Duration,
) -> Vec<(u64, Vec<u8>)> {
    let deadline = Instant::now() + duration;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(POLL_MS)) {
            Ok(chunk) => out.push((start.elapsed().as_millis() as u64, chunk)),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    out
}

/// Submit a prompt the way lumina's input bridge does: write the body (with
/// `\n`→`\r`), settle `PROMPT_SUBMIT_SETTLE_MS`, then send a SEPARATE `\r`
/// Enter (the long-prompt paste-detect workaround / submission contract).
fn submit_prompt(sess: &mut ProbeSession, prompt: &str) {
    let body: Vec<u8> = prompt
        .bytes()
        .map(|b| if b == b'\n' { b'\r' } else { b })
        .collect();
    let _ = sess.writer.write_all(&body);
    let _ = sess.writer.flush();
    std::thread::sleep(Duration::from_millis(PROMPT_SUBMIT_SETTLE_MS));
    let _ = sess.writer.write_all(b"\r");
    let _ = sess.writer.flush();
}

/// Scan the candidate per-cwd projects dirs for ANY `.jsonl` written at or after
/// `since` — i.e. produced by this probe session's turn. Robust to interactive
/// claude minting its own JSONL filename (not our `--session-id`, GitHub #44607).
fn jsonl_present(dirs: &[PathBuf], since: SystemTime) -> Option<PathBuf> {
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = entry.metadata()
                && let Ok(modified) = meta.modified()
                && modified >= since
            {
                return Some(path);
            }
        }
    }
    None
}

fn dump_raw(chunks: &[(u64, Vec<u8>)]) -> String {
    let mut all = Vec::new();
    for (_, c) in chunks {
        all.extend_from_slice(c);
    }
    String::from_utf8_lossy(&all).into_owned()
}

/// The probe's main diagnostic. `#[ignore]`'d — spawns REAL claude.
///
/// Run manually:
/// `cargo test --manifest-path lumina/Cargo.toml --test pty_readiness_probe -- --ignored --nocapture`
#[test]
#[ignore = "spawns REAL claude under a PTY (env-dependent; manual startup-timing diagnostic). \
Run with `-- --ignored --nocapture`; needs claude reachable + a trusted spawn cwd."]
fn pty_readiness_probe() {
    let mut rec = Recorder::new();
    let scratch = rec.path.clone();

    rec.log("=== lumina PTY readiness probe (claude 2.1.196 startup-timing diagnosis) ===");
    rec.log(format!("scratch log: {}", scratch.display()));

    let cwd = probe_cwd();
    let projects_root = resolve_projects_root();
    rec.log(format!("spawn cwd          : {}", cwd.display()));
    rec.log(format!("claude projects dir: {}", projects_root.display()));
    rec.log(format!(
        "sanitise_cwd(cwd)  : {}",
        sanitise_cwd(&std::fs::canonicalize(&cwd).unwrap_or_else(|_| cwd.clone()))
    ));
    if std::env::var_os("LUMINA_PTY_PROJECTS_ROOT").is_some() {
        rec.log(
            "WARNING: LUMINA_PTY_PROJECTS_ROOT is set — real claude IGNORES it and writes to \
             ~/.claude/projects; JSONL detection may miss. Unset it for a faithful probe.",
        );
    }

    // =====================================================================
    // Phase A — IMMEDIATE dispatch (suspect #1: prompt lands before readline →
    // dropped → NO JSONL). Fire the prompt right after spawn, before any grace.
    // =====================================================================
    rec.log("");
    rec.log("--- Phase A: IMMEDIATE dispatch (expect: NO JSONL — prompt dropped) ---");
    let phase_a_appeared = {
        let start = Instant::now();
        let mut sess = spawn_probe_session(&cwd, &projects_root);
        rec.log(format!("[A] session_id: {}", sess.session_id));
        for d in &sess.jsonl_dirs {
            rec.log(format!("[A] watching jsonl dir: {}", d.display()));
        }
        // Fire immediately — this is the lumina-today timing (dispatch ~137ms
        // post-spawn, before claude's readline is live).
        submit_prompt(&mut sess, TEST_PROMPT);
        rec.log("[A] prompt fired immediately (no readiness wait)");
        let chunks = read_window(&sess.rx, start, Duration::from_millis(POST_PROMPT_READ_MS));
        let appeared = jsonl_present(&sess.jsonl_dirs, sess.spawn_time);
        rec.log(format!(
            "[A] post-fire raw stdout ({} chunks, {} bytes):",
            chunks.len(),
            chunks.iter().map(|(_, c)| c.len()).sum::<usize>()
        ));
        rec.log(format!(">>>>>\n{}\n<<<<<", dump_raw(&chunks)));
        rec.log(format!(
            "[A] JSONL appeared: {}",
            appeared
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "NO".to_string())
        ));
        appeared.is_some()
        // sess drops here -> child killed, master/slave dropped, mcp-config removed
    };

    // =====================================================================
    // Phase B — DELAYED dispatch. Capture startup (a/b/c), THEN submit after a
    // real settle (suspect #1 fix: prompt lands after readline → JSONL appears).
    // =====================================================================
    rec.log("");
    rec.log("--- Phase B: startup capture + DELAYED dispatch (expect: JSONL appears) ---");
    let phase_b_appeared = {
        let start = Instant::now();
        let mut sess = spawn_probe_session(&cwd, &projects_root);
        rec.log(format!("[B] session_id: {}", sess.session_id));
        for d in &sess.jsonl_dirs {
            rec.log(format!("[B] watching jsonl dir: {}", d.display()));
        }

        // (a)(b)(c): capture the first ~3 s of raw startup output.
        let startup = read_window(&sess.rx, start, Duration::from_millis(INITIAL_CAPTURE_MS));

        // (a) raw dump
        rec.log(format!(
            "[B][a] first {}ms raw startup ({} chunks, {} bytes):",
            INITIAL_CAPTURE_MS,
            startup.len(),
            startup.iter().map(|(_, c)| c.len()).sum::<usize>()
        ));
        rec.log(format!(">>>>>\n{}\n<<<<<", dump_raw(&startup)));

        // (b) T_first_output
        match startup.first() {
            Some((t, _)) => rec.log(format!("[B][b] T_first_output = {t} ms")),
            None => rec.log(
                "[B][b] T_first_output = NONE — claude produced NO PTY output in the window \
                 (a wedged/blocked startup; suspect #2/#3 — see plan Risks)",
            ),
        }

        // (c) quiesce-vs-repaint: timeline + max inter-chunk gap + tail silence.
        rec.log("[B][c] per-chunk timeline (elapsed_ms : byte_len):");
        let timeline: String = startup
            .iter()
            .map(|(t, c)| format!("{t}:{}", c.len()))
            .collect::<Vec<_>>()
            .join("  ");
        rec.log(format!("       {timeline}"));
        let mut max_gap = 0u64;
        for w in startup.windows(2) {
            max_gap = max_gap.max(w[1].0.saturating_sub(w[0].0));
        }
        let last_ms = startup.last().map(|(t, _)| *t).unwrap_or(0);
        let tail_silence = INITIAL_CAPTURE_MS.saturating_sub(last_ms);
        rec.log(format!(
            "[B][c] max inter-chunk gap = {max_gap} ms; last chunk at {last_ms} ms; \
             tail silence = {tail_silence} ms"
        ));
        if !startup.is_empty() && tail_silence >= QUIESCE_TAIL_SILENCE_MS {
            rec.log(format!(
                "[B][c] VERDICT: output QUIESCED (tail silent ≥{QUIESCE_TAIL_SILENCE_MS}ms) — \
                 favours an output-QUIESCE readiness condition; READY_DELAY_MS can be modest \
                 (≈ T_first_output + a small margin past last_ms)."
            ));
        } else if !startup.is_empty() {
            rec.log(
                "[B][c] VERDICT: output STILL REPAINTING at window end — favours a FIXED-GRACE \
                 readiness condition; widen the capture window in a re-run to find when it \
                 settles, and set READY_DELAY_MS past that.",
            );
        }

        // (d) delayed dispatch — submit after startup has settled.
        submit_prompt(&mut sess, TEST_PROMPT);
        rec.log("[B][d] prompt submitted AFTER startup settle (body, 220ms, separate \\r)");
        let post = read_window(&sess.rx, start, Duration::from_millis(POST_PROMPT_READ_MS));
        rec.log(format!(
            "[B][d] post-prompt raw stdout ({} chunks, {} bytes):",
            post.len(),
            post.iter().map(|(_, c)| c.len()).sum::<usize>()
        ));
        rec.log(format!(">>>>>\n{}\n<<<<<", dump_raw(&post)));
        let appeared = jsonl_present(&sess.jsonl_dirs, sess.spawn_time);
        rec.log(format!(
            "[B][d] JSONL appeared: {}",
            appeared
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "NO".to_string())
        ));
        appeared.is_some()
    };

    // =====================================================================
    // Summary — the suspect-#1 contrast (d).
    // =====================================================================
    rec.log("");
    rec.log("=== SUMMARY ===");
    rec.log(format!("[A] immediate-dispatch JSONL : {phase_a_appeared}"));
    rec.log(format!("[B] delayed-dispatch JSONL   : {phase_b_appeared}"));
    rec.log(
        "NOTE: JSONL is a SECONDARY/unreliable signal for THIS probe — claude flushes its \
         transcript incrementally over a LIVE session, but the probe HARD-KILLS the child a few \
         seconds in (Drop), often before the first flush lands on disk. So an absent JSONL here \
         does NOT mean no turn ran. The AUTHORITATIVE signal is the raw PTY dump above (this is \
         the whole reason the fix gates on PTY OUTPUT, not JSONL): in [A] a DROPPED prompt sits \
         in the input box (`❯ <prompt text>`) with NO spinner / NO model bullet `●` / NO `Cooked \
         for …`; in [B] a SUBMITTED prompt shows a spinner (e.g. `Burrowing…`) + a model response \
         + a token count. [A]=no-turn AND [B]=turn ⇒ suspect #1 CONFIRMED.",
    );
    if !phase_a_appeared && phase_b_appeared {
        rec.log(
            "JSONL VERDICT: suspect #1 CONFIRMED by JSONL too — immediate dispatch produced none \
             while a delayed dispatch did. Proceed with the PTY-output readiness gate (Tasks 2-3); \
             calibrate READY_DELAY_MS from the [B][b]/[B][c] numbers above.",
        );
    } else if phase_a_appeared {
        rec.log(
            "JSONL VERDICT: immediate dispatch ALSO produced JSONL — if the [A] raw dump shows a \
             real turn (spinner + response), the timing race may not be the sole cause; re-examine \
             before building the gate.",
        );
    } else {
        rec.log(
            "JSONL VERDICT: INCONCLUSIVE (neither flushed JSONL in the short killed window — \
             expected for this probe; see NOTE). DEFER TO THE RAW PTY DUMPS: if [A] shows the \
             prompt wedged in the input box with no turn and [B] shows a spinner + model response, \
             suspect #1 is CONFIRMED despite no JSONL. Only if [B] ALSO shows no turn (a dialog / \
             wedged prompt in the [A]/[B][a] dumps) is it suspect #2/#3 — then re-plan Tasks 2-3 \
             (plan Risks: 'Probe disconfirms #1').",
        );
    }
    rec.log(format!("Full recorded output: {}", scratch.display()));
}

fn probe_cwd() -> PathBuf {
    if let Some(d) = std::env::var_os("LUMINA_PROBE_CWD") {
        return PathBuf::from(d);
    }
    std::env::current_dir().expect("current_dir")
}
