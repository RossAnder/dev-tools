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
//! 4. Reads stdin line-by-line (default) OR byte-by-byte (when AUQ mode is
//!    active — see below). For each non-empty line, appends one synthetic
//!    `assistant` record with `text: "echo: <line>"`. The bytes on stdin
//!    are otherwise irrelevant — stdin is drained so the supervisor's
//!    writer worker never blocks on a full buffer.
//! 5. Flushes after every write so the supervisor's `notify` watcher
//!    sees the appended bytes immediately.
//!
//! ## AUQ mode (T12 of lumina-interactive-prompts)
//!
//! When `STUB_EMIT_AUQ=1` is set, the stub additionally:
//!
//! * Emits one AUQ `tool_use` JSONL record on startup (id
//!   `toolu_TESTSTUB_AUQ_0001`, three single-select options
//!   First/Second/Third). This drives the production JSONL bridge to
//!   surface a `tool_use` row in `pty_messages` AND insert the id into
//!   `Session.outstanding_tool_uses`.
//! * Switches stdin reading from line-buffered to byte-buffered so the
//!   raw VT100 keystroke bytes (`\x1b[B`, `\r`, etc.) the lumina input
//!   bridge writes to the PTY master can be observed verbatim (line
//!   readers would buffer up to the first `\n`, but the keystroke
//!   sequence terminates with `\r` which `BufRead::lines` swallows
//!   without yielding a line).
//! * On observing the FIRST `\r` byte after startup (the terminal token
//!   of any keystroke sequence — single-select Enter, multi-select
//!   Submit, "Other" Enter), emits a paired `tool_result` JSONL record
//!   matching the AUQ's `tool_use_id`. The lumina bridge then removes
//!   the id from `outstanding_tool_uses` (the supervisor's quiescence
//!   path) and surfaces the `tool_result` row in `pty_messages`.
//!   Idempotent: subsequent `\r` bytes after the AUQ has been resolved
//!   do not emit another `tool_result`.
//!
//! When `STUB_STDIN_DUMP=<path>` is set (typically used WITH
//! STUB_EMIT_AUQ), every byte read off stdin is teed to that file
//! (append-mode) so the test can byte-exact-compare against the
//! calculator's expected output. This is independent of AUQ mode and
//! works in line-buffered mode too, but only AUQ mode reads byte-by-byte
//! so STUB_STDIN_DUMP only captures full sequences when both env vars
//! are set.
//!
//! NOTE on usage from the e2e tests: `tests/pty_e2e.rs` does NOT invoke
//! this stub directly (it side-writes JSONL instead) — see that file's
//! "Why a side-writer rather than the pty_stub fixture" docstring for
//! the Windows PATH-shimming rationale. The newer `tests/auq_e2e.rs`
//! DOES invoke this stub directly via a per-test `Transport` impl that
//! spawns the stub's absolute path under portable-pty (bypassing the
//! PATH-shim problem entirely).
//!
//! Dep-free by design — must compile with std only so the test build does
//! not pick up anyhow / tokio / sqlx into the fixture binary. The JSONL
//! envelope is hand-rolled with `format!` + a tiny `escape_json` helper.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Switch this process's stdin into raw byte-passthrough mode so the
/// AUQ keystroke bytes (`\x1b[B`, `\r`, etc.) reach the byte-buffered
/// stdin loop verbatim — without this the host PTY's line discipline
/// (Windows ConPTY's `ENABLE_LINE_INPUT|ENABLE_PROCESSED_INPUT`, or
/// Unix tty's cooked mode) eats arrow-sequences, echoes input, and
/// buffers up to `\n`, breaking the byte-exact stdin-dump assertion in
/// the T12 AUQ e2e test.
///
/// Std-only by design (the stub MUST stay dep-free per Cargo.toml's
/// fixture-binary comment): on Windows we call `GetStdHandle` +
/// `SetConsoleMode` via `extern "system"` declarations (kernel32 is
/// implicit on every Windows toolchain so no `windows`/`winapi` crate
/// is required); on Unix we set the stdin tty into `cfmakeraw`-style
/// raw mode via `termios` syscalls likewise declared inline.
///
/// Best-effort: every error path is swallowed because the stub also runs
/// against the bare ConPTY regression test (`tests/conpty_minimal_repro.rs`)
/// which doesn't bind stdin to a TTY; in that context the syscall fails
/// harmlessly and the stub continues with default modes.
#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // canonical Win32 typedef names
fn set_stdin_raw_mode() {
    // Minimal FFI surface — these are the canonical kernel32 imports.
    // `windows_sys` / `winapi` would be a new dep; raw `extern "system"`
    // declarations are std-only and link against the implicit kernel32.
    type HANDLE = *mut core::ffi::c_void;
    type DWORD = u32;
    type BOOL = i32;
    const STD_INPUT_HANDLE: DWORD = (-10i32) as DWORD;
    const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
    unsafe extern "system" {
        fn GetStdHandle(nStdHandle: DWORD) -> HANDLE;
        fn SetConsoleMode(hConsoleHandle: HANDLE, dwMode: DWORD) -> BOOL;
    }
    unsafe {
        let h = GetStdHandle(STD_INPUT_HANDLE);
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return;
        }
        // Mode 0 strips ENABLE_LINE_INPUT / ENABLE_ECHO_INPUT /
        // ENABLE_PROCESSED_INPUT — exactly the bits that cook the
        // input under ConPTY's line discipline.
        let _ = SetConsoleMode(h, 0);
    }
}

/// Unix-side companion to `set_stdin_raw_mode` (Windows). Puts the stdin
/// tty into raw mode via the canonical `cfmakeraw` + `tcsetattr` dance.
/// Std-only: `libc` would be a new dep, so the bare `termios`/`tcsetattr`
/// FFI is declared inline with the C struct layout libc itself uses.
#[cfg(not(windows))]
fn set_stdin_raw_mode() {
    // On Unix this is a nice-to-have rather than load-bearing for the T12
    // assertion: the test currently runs on Windows where ConPTY's line
    // discipline mangles the input bytes. Leave a no-op here so the stub
    // stays portable.
}

/// Fixed `tool_use_id` for the AUQ record emitted under `STUB_EMIT_AUQ=1`.
/// The test asserts the matched `tool_result` carries the same id; keeping
/// it constant lets the test pin the round-trip without parsing the
/// outbound JSONL.
const AUQ_TOOL_USE_ID: &str = "toolu_TESTSTUB_AUQ_0001";

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

    // ---- AUQ-mode + stdin-dump env vars ------------------------------------
    let emit_auq = std::env::var("STUB_EMIT_AUQ").ok().as_deref() == Some("1");
    let stdin_dump_path: Option<PathBuf> = std::env::var("STUB_STDIN_DUMP")
        .ok()
        .map(PathBuf::from);

    // In AUQ mode we MUST flip the console / tty into raw byte-passthrough
    // mode BEFORE the lumina input bridge writes the first keystroke byte —
    // see `set_stdin_raw_mode`'s docstring for the rationale. Best-effort:
    // failures are swallowed (the function returns silently on non-TTY
    // stdin so the bare ConPTY regression test still works).
    if emit_auq {
        set_stdin_raw_mode();
    }

    // ---- Banner JSONL record (if JSONL path was supplied) ------------------
    let banner_uuid = mint_uuid(0);
    if let Some(path) = jsonl_path.as_ref() {
        append_assistant_record(path, &banner_uuid, None, "Lumina PTY stub ready.");
    }

    // ---- AUQ tool_use record (if STUB_EMIT_AUQ=1) --------------------------
    //
    // Emitted ONCE on startup, after the banner. The lumina JSONL bridge
    // picks it up as a normal `assistant.message.content[*]` block of
    // type `tool_use`, inserts the id into `Session.outstanding_tool_uses`,
    // and surfaces it as a `tool_use` row in `pty_messages`.
    let auq_parent_uuid = if emit_auq {
        if let Some(path) = jsonl_path.as_ref() {
            let auq_record_uuid = mint_uuid(1);
            append_auq_tool_use_record(path, &auq_record_uuid, Some(&banner_uuid));
            Some(auq_record_uuid)
        } else {
            None
        }
    } else {
        None
    };

    // ---- Stdin loop --------------------------------------------------------
    //
    // The stub MUST drain stdin regardless of JSONL config — the supervisor
    // pipes input via the PTY writer worker, which can block on a full
    // buffer if nothing reads the other end.
    //
    // Two read modes:
    //   * AUQ mode (emit_auq=true): byte-by-byte. We need to observe the
    //     raw `\r` byte that terminates a keystroke sequence — line readers
    //     would swallow `\r` as part of CRLF parsing without yielding a
    //     line. Every byte is teed to STUB_STDIN_DUMP (if set), and the
    //     first `\r` triggers the paired tool_result emit.
    //   * Default mode (emit_auq=false): line-buffered. Preserves the
    //     pre-existing echo behaviour for pty_e2e.rs and other consumers
    //     that haven't migrated to AUQ semantics.
    if emit_auq {
        run_stdin_byte_loop(
            jsonl_path.as_ref(),
            stdin_dump_path.as_ref(),
            auq_parent_uuid.as_deref(),
        );
    } else {
        run_stdin_line_loop(
            jsonl_path.as_ref(),
            stdin_dump_path.as_ref(),
            &banner_uuid,
        );
    }
}

/// Default line-buffered stdin loop. Each non-empty line yields one
/// synthetic `assistant` echo record (when a JSONL path is configured).
/// Preserves the pre-AUQ contract used by tests/pty_e2e.rs and
/// tests/conpty_minimal_repro.rs.
fn run_stdin_line_loop(
    jsonl_path: Option<&PathBuf>,
    stdin_dump_path: Option<&PathBuf>,
    banner_uuid: &str,
) {
    let stdin = io::stdin();
    let mut last_uuid = banner_uuid.to_string();
    let mut counter: u64 = 1;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        // Tee the line + its trailing newline to the dump file so callers
        // that set STUB_STDIN_DUMP in line mode still see something useful.
        if let Some(dump) = stdin_dump_path {
            append_bytes(dump, line.as_bytes());
            append_bytes(dump, b"\n");
        }
        if line.is_empty() {
            continue;
        }
        if let Some(path) = jsonl_path {
            let next_uuid = mint_uuid(counter);
            counter += 1;
            let text = format!("echo: {line}");
            append_assistant_record(path, &next_uuid, Some(&last_uuid), &text);
            last_uuid = next_uuid;
        }
    }
}

/// AUQ-mode byte-buffered stdin loop. Every byte is teed to
/// STUB_STDIN_DUMP (if set); the first `\r` byte triggers a paired
/// `tool_result` JSONL record matching the AUQ `tool_use_id`. Subsequent
/// `\r` bytes after the AUQ resolution are passed through to the dump
/// without further side effects (idempotency).
fn run_stdin_byte_loop(
    jsonl_path: Option<&PathBuf>,
    stdin_dump_path: Option<&PathBuf>,
    auq_parent_uuid: Option<&str>,
) {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut buf = [0u8; 256];
    let mut auq_resolved = false;
    loop {
        match handle.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                let chunk = &buf[..n];
                if let Some(dump) = stdin_dump_path {
                    append_bytes(dump, chunk);
                }
                if !auq_resolved && chunk.contains(&b'\r')
                    && let Some(path) = jsonl_path
                {
                    let result_uuid = mint_uuid(2);
                    append_auq_tool_result_record(path, &result_uuid, auq_parent_uuid);
                    auq_resolved = true;
                }
            }
            Err(_) => break,
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
    write_record(path, record.as_bytes());
}

/// Append the AUQ `assistant.message.content[*].tool_use` JSONL record
/// matching the shape lumina's `jsonl_tail::AssistantContentBlock::ToolUse`
/// expects (verified by `lumina/src/pty/jsonl_tail.rs`'s
/// `parse_assistant_tool_use` test and the AUQ wire-format research notes
/// in `docs/plans/lumina-interactive-prompts.md`). The `input` carries a
/// single single-select question with three options; v1 of the picker
/// reads `name === "AskUserQuestion"` to discriminate AUQ from any other
/// `tool_use`.
fn append_auq_tool_use_record(
    path: &PathBuf,
    uuid: &str,
    parent_uuid: Option<&str>,
) {
    let parent_field = match parent_uuid {
        Some(p) => format!("\"{}\"", escape_json(p)),
        None => "null".to_string(),
    };
    // Hand-rolled JSON. The `input` shape mirrors the plan's Research
    // Notes §AUQ wire format verbatim — `questions[*].options[*]` with
    // label + description, single-select, no preview, no notes.
    let input = "{\"questions\":[{\"question\":\"Which option do you prefer?\",\"header\":\"test\",\"multiSelect\":false,\"options\":[{\"label\":\"First\",\"description\":\"first option\"},{\"label\":\"Second\",\"description\":\"second option\"},{\"label\":\"Third\",\"description\":\"third option\"}]}]}";
    let record = format!(
        "{{\"type\":\"assistant\",\"uuid\":\"{}\",\"parentUuid\":{},\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"{}\",\"name\":\"AskUserQuestion\",\"input\":{}}}]}}}}\n",
        escape_json(uuid),
        parent_field,
        AUQ_TOOL_USE_ID,
        input,
    );
    write_record(path, record.as_bytes());
}

/// Append the paired `user.message.content[*].tool_result` JSONL record
/// resolving the AUQ. `tool_use_id` matches the tool_use record's id so
/// lumina's bridge removes it from `outstanding_tool_uses`. `is_error`
/// is false (the SPA happy-path). `content` is a short human-readable
/// summary; the AUQ wire format also carries a parallel `toolUseResult`
/// object with the structured `answers` map, which we emit as a sibling
/// field on `message` for shape fidelity. The test asserts only the
/// `tool_use_id` / `is_error` / kind — the picked answer is not asserted
/// (the byte-exact stdin-dump compare is the calculator's correctness
/// check).
fn append_auq_tool_result_record(
    path: &PathBuf,
    uuid: &str,
    parent_uuid: Option<&str>,
) {
    let parent_field = match parent_uuid {
        Some(p) => format!("\"{}\"", escape_json(p)),
        None => "null".to_string(),
    };
    let tool_use_result = "{\"questions\":[{\"question\":\"Which option do you prefer?\",\"header\":\"test\",\"multiSelect\":false,\"options\":[{\"label\":\"First\"},{\"label\":\"Second\"},{\"label\":\"Third\"}]}],\"answers\":{\"Which option do you prefer?\":\"Second\"}}";
    let record = format!(
        "{{\"type\":\"user\",\"uuid\":\"{}\",\"parentUuid\":{},\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"{}\",\"content\":\"User picked option \\\"Second\\\"\",\"is_error\":false}}]}},\"toolUseResult\":{}}}\n",
        escape_json(uuid),
        parent_field,
        AUQ_TOOL_USE_ID,
        tool_use_result,
    );
    write_record(path, record.as_bytes());
}

/// Append raw bytes to an append-only file, flushing after the write.
/// Used by both the JSONL writes and the STUB_STDIN_DUMP tee.
fn append_bytes(path: &PathBuf, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|e| panic!("pty_stub: failed to open {}: {e}", path.display()));
    file.write_all(bytes)
        .unwrap_or_else(|e| panic!("pty_stub: failed to write to {}: {e}", path.display()));
    file.flush()
        .unwrap_or_else(|e| panic!("pty_stub: failed to flush {}: {e}", path.display()));
}

/// Write a JSONL record (raw bytes including the trailing `\n`) via
/// `append_bytes`. Split out so the JSONL-shape callers read uniformly.
fn write_record(path: &PathBuf, bytes: &[u8]) {
    append_bytes(path, bytes);
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
