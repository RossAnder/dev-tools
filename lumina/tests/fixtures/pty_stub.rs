//! Stub binary that stands in for the real interactive `claude` CLI.
//!
//! After the JSONL-tail rewrite (T6 of `lumina-pty-jsonl-tail`), the
//! canonical transcript source is no longer the PTY byte stream — it's the
//! per-session JSONL file Claude Code writes to
//! `~/.claude/projects/<sanitised-cwd>/<uuid>.jsonl`. The supervisor's
//! `jsonl_tail::bind_jsonl_path` watches that directory for the first
//! new `*.jsonl` file to appear, then `jsonl_tail::tail` streams records
//! off it onto a broadcast channel.
//!
//! Therefore this stub:
//!
//! 1. Reads `PTY_STUB_PROJECTS_DIR` (the watched-projects subdir computed
//!    by the harness as `LUMINA_PTY_PROJECTS_ROOT/<sanitised-cwd>`) and
//!    `PTY_STUB_SESSION_UUID` (the basename for the JSONL filename) from
//!    the environment. Both are required.
//! 2. Computes the JSONL output path as
//!    `<PTY_STUB_PROJECTS_DIR>/<PTY_STUB_SESSION_UUID>.jsonl`, creating the
//!    parent dir if missing.
//! 3. Writes ONE initial `assistant` JSONL banner record on startup so
//!    `bind_jsonl_path` sees a new file appear and the
//!    `Spawning -> Idle` gate in the bridge task flips on first record.
//! 4. Reads stdin line-by-line. For each non-empty line, appends one
//!    synthetic `assistant` record with `text: "echo: <line>"`. The bytes
//!    on stdin are otherwise irrelevant — stdin is drained so the
//!    supervisor's writer worker never blocks on a full buffer.
//! 5. Flushes after every write so the supervisor's `notify` watcher
//!    sees the appended bytes immediately.
//!
//! NOTE on usage from the e2e test: `tests/pty_e2e.rs` does NOT invoke
//! this stub in the current revision. On Windows, `portable-pty 0.9`'s
//! `CommandBuilder` reconstructs PATH from `HKEY_LOCAL_MACHINE` +
//! `HKEY_CURRENT_USER` registry hives at `new()`-time, discarding any
//! process-level PATH overlay we set up to redirect `claude` here. Rather
//! than fight that, the e2e test side-writes JSONL records itself and
//! treats the spawned child as opaque (see `tests/pty_e2e.rs` § Why a
//! side-writer rather than the `pty_stub` fixture). The stub is retained
//! as a future-use fixture: on Linux/macOS where PATH-shimming works,
//! a follow-up revision of the e2e test can drive the bind path via
//! `PTY_STUB_PROJECTS_DIR` and exercise stdin → JSONL fan-out end-to-end.
//!
//! Dep-free by design — must compile with std only so the test build does
//! not pick up anyhow / tokio / sqlx into the fixture binary. The JSONL
//! envelope is hand-rolled with `format!` + a tiny `escape_json` helper.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // ---- Stdout banner (required by tests/conpty_minimal_repro.rs) ---------
    //
    // The ConPTY regression test asserts that the bytes `Lumina PTY stub
    // ready.` reach the master reader within 5 s — that test exercises the
    // bare `portable-pty` API with no env vars set, so the stdout write MUST
    // happen unconditionally before any env-var check that could panic.
    {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = writeln!(out, "Lumina PTY stub ready.");
        let _ = out.flush();
    }

    // ---- Environment-driven JSONL path (optional) --------------------------
    //
    // When `PTY_STUB_PROJECTS_DIR` + `PTY_STUB_SESSION_UUID` are set we ALSO
    // mirror the banner + per-input echoes into a JSONL file the
    // jsonl_tail-driven e2e flow can consume. When EITHER is absent the
    // stub still runs (drains stdin so the writer worker doesn't block on
    // a full buffer) — this keeps the bare ConPTY regression test, which
    // sets neither env var, working.
    let jsonl_path: Option<PathBuf> =
        match (std::env::var("PTY_STUB_PROJECTS_DIR"), std::env::var("PTY_STUB_SESSION_UUID")) {
            (Ok(projects_dir), Ok(session_uuid)) => {
                let dir = PathBuf::from(&projects_dir);
                std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
                    panic!("pty_stub: failed to create {}: {e}", dir.display())
                });
                Some(dir.join(format!("{session_uuid}.jsonl")))
            }
            _ => None,
        };

    // ---- Banner JSONL record (if JSONL path was supplied) ------------------
    let banner_uuid = mint_uuid(0);
    if let Some(path) = jsonl_path.as_ref() {
        append_assistant_record(path, &banner_uuid, None, "Lumina PTY stub ready.");
    }

    // ---- Stdin loop --------------------------------------------------------
    //
    // The stub MUST drain stdin regardless of JSONL config — the supervisor
    // pipes input via the PTY writer worker, which can block on a full
    // buffer if nothing reads the other end. When JSONL path is set, each
    // non-empty input line also produces one synthetic assistant record.
    let stdin = io::stdin();
    let mut last_uuid = banner_uuid;
    let mut counter: u64 = 1;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        if let Some(path) = jsonl_path.as_ref() {
            let next_uuid = mint_uuid(counter);
            counter += 1;
            let text = format!("echo: {line}");
            append_assistant_record(path, &next_uuid, Some(&last_uuid), &text);
            last_uuid = next_uuid;
        }
    }
}

/// Append one `{"type":"assistant", ...}` JSONL record to `path`, flushing
/// the file after the write. Opens in append mode so successive calls
/// accumulate without truncating the banner.
fn append_assistant_record(
    path: &PathBuf,
    uuid: &str,
    parent_uuid: Option<&str>,
    text: &str,
) {
    let parent_field = match parent_uuid {
        Some(p) => format!("\"{}\"", escape_json(p)),
        None => "null".to_string(),
    };
    let record = format!(
        "{{\"type\":\"assistant\",\"uuid\":\"{}\",\"parentUuid\":{},\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}\n",
        escape_json(uuid),
        parent_field,
        escape_json(text),
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|e| panic!("pty_stub: failed to open {}: {e}", path.display()));
    file.write_all(record.as_bytes())
        .unwrap_or_else(|e| panic!("pty_stub: failed to write to {}: {e}", path.display()));
    file.flush()
        .unwrap_or_else(|e| panic!("pty_stub: failed to flush {}: {e}", path.display()));
}

/// Mint a unique-per-write string suitable for the JSONL `uuid` field. The
/// supervisor does not validate uuid shape — any unique-per-write value is
/// acceptable. We concatenate the wall-clock nanos since UNIX_EPOCH with a
/// per-call counter to guarantee uniqueness across rapid successive calls
/// within one clock tick.
fn mint_uuid(counter: u64) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("stub-{nanos}-{counter}")
}

/// JSON-escape a string for inclusion inside double-quoted JSON text.
///
/// Escapes `\\`, `"`, and the ASCII control range (`< 0x20`). Anything
/// outside that range is passed through verbatim — Rust strings are valid
/// UTF-8, and JSON permits raw multi-byte UTF-8 in string content.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}
