//! Binary-level contracts: what the *process* does, as Claude Code sees it.
//!
//! Every other test in this crate calls an internal function, so the parts that
//! only exist at the process boundary — which stream a byte lands on, the exit
//! code, and the absence of a trailing newline — are asserted nowhere else. A
//! `write!` silently becoming `writeln!`, or a diagnostic leaking onto the row
//! protocol, would leave the whole unit suite green.
//!
//! `CARGO_BIN_EXE_statusline` is set by cargo for integration tests, so this
//! runs the real binary with no dev-dependencies.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Run the binary with `argv`, write `stdin` to it, and close stdin.
///
/// Closing matters: the binary reads stdin to EOF (even under `--doctor`), so a
/// child stdin left open deadlocks `wait_with_output`. Dropping the handle is
/// what signals EOF.
fn run(argv: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_statusline"))
        .args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary spawns");
    {
        let mut pipe = child.stdin.take().expect("stdin was piped");
        pipe.write_all(stdin.as_bytes()).expect("stdin accepts the payload");
    } // dropped here — EOF.
    child.wait_with_output().expect("the binary exits")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is UTF-8")
}

/// A payload with no `cwd` and no `workspace.current_dir`, so `Payload::dir`
/// returns `None` and the binary never touches git — no `.git/HEAD` walk and no
/// `git diff` spawn, whatever directory the test harness happens to run in.
const PAYLOAD: &str = r#"{
    "workspace": {"repo": {"name": "dev-tools"}},
    "model": {"display_name": "Claude Opus 5"},
    "effort": {"level": "xhigh"},
    "context_window": {"total_input_tokens": 31000, "context_window_size": 200000,
                       "used_percentage": 15},
    "rate_limits": {"five_hour": {"used_percentage": 26},
                    "seven_day": {"used_percentage": 7}}
}"#;

const SUBAGENT_PAYLOAD: &str = r#"{"columns": 60, "tasks": [
    {"id": "a", "name": "t10-css-face-split", "status": "running"}
]}"#;

/// The status line is written with `write!`, not `writeln!` — Claude Code
/// composes the returned bytes into its own frame, so a trailing newline
/// misaligns it. Deliberately asserted on the untrimmed bytes: trimming first
/// would erase exactly the thing under test.
#[test]
fn the_rendered_status_line_carries_no_trailing_newline_in_either_style() {
    // The label differs by style, and deliberately so: `min` draws the repo
    // name, while `full` draws `path_leaf(cwd)` — which this payload withholds
    // to keep git off the path. Assert a marker each style actually emits.
    for (style, marker) in [("full", "Opus 5"), ("min", "dev-tools")] {
        let out = run(&["--style", style, "--columns", "120"], PAYLOAD);
        let s = stdout(&out);
        assert_eq!(out.status.code(), Some(0), "{style}: {}", stderr(&out));
        assert!(!s.is_empty(), "{style} rendered nothing");
        assert!(
            !s.ends_with('\n'),
            "{style} ends with a newline: {:?}",
            &s[s.len().saturating_sub(16)..]
        );
        assert!(s.contains(marker), "{style} lost `{marker}`: {s:?}");
    }
}

/// The subagent rows are NDJSON, one row per task, and the last row is *not*
/// newline-terminated either.
#[test]
fn the_subagent_rows_carry_no_trailing_newline() {
    let out = run(&["subagent", "--columns", "60"], SUBAGENT_PAYLOAD);
    let s = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(s.starts_with(r#"{"id":"a","content":"#), "{s:?}");
    assert!(!s.ends_with('\n'), "the last row is newline-terminated: {s:?}");
}

/// A usage error must not put a single byte on the row protocol — stdout is
/// parsed by Claude Code, so a help dump there would render as a status line.
#[test]
fn an_unknown_argument_exits_two_with_its_complaint_on_stderr_and_nothing_on_stdout() {
    let out = run(&["--nope"], "");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "stdout was not empty: {:?}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("unknown argument"), "{err:?}");
    assert!(err.contains("--nope"), "the complaint never names the argument: {err:?}");
}

#[test]
fn help_and_version_exit_zero_on_stdout_with_a_silent_stderr() {
    let help = run(&["--help"], "");
    assert_eq!(help.status.code(), Some(0));
    assert!(stdout(&help).contains("USAGE"), "{:?}", stdout(&help));
    assert!(help.stderr.is_empty(), "{:?}", stderr(&help));

    let version = run(&["-V"], "");
    assert_eq!(version.status.code(), Some(0));
    assert!(
        stdout(&version).starts_with("statusline "),
        "{:?}",
        stdout(&version)
    );
    assert!(version.stderr.is_empty(), "{:?}", stderr(&version));
}

/// `--doctor` reports to stderr *specifically* because stdout is the row
/// protocol. Nothing but a process-level test can check that separation.
#[test]
fn the_doctor_report_goes_to_stderr_and_leaves_stdout_completely_empty() {
    for argv in [
        vec!["--doctor", "--columns", "120"],
        vec!["subagent", "--doctor"],
    ] {
        let payload = if argv[0] == "subagent" { SUBAGENT_PAYLOAD } else { PAYLOAD };
        let out = run(&argv, payload);
        assert_eq!(out.status.code(), Some(0), "{argv:?}: {}", stderr(&out));
        assert_eq!(
            out.stdout.len(),
            0,
            "{argv:?} put {} bytes on the row protocol: {:?}",
            out.stdout.len(),
            stdout(&out)
        );
        let err = stderr(&out);
        assert!(err.starts_with("statusline --doctor"), "{argv:?}: {err:?}");
        assert!(err.contains("claude dir"), "{argv:?}: {err:?}");
    }
}

/// Both degradations, at the process boundary: the main line falls back to the
/// bare label, the subagent line emits nothing at all (which Claude Code reads
/// as "keep the default rows"). Neither is an error exit — a non-zero status
/// would show up in `claude --debug` as a broken hook.
#[test]
fn an_empty_or_malformed_payload_degrades_quietly_and_still_exits_zero() {
    for input in ["", "   ", "not json", "{oops"] {
        let main = run(&["--columns", "120"], input);
        assert_eq!(main.status.code(), Some(0), "main {input:?}");
        assert_eq!(stdout(&main), "Claude", "main {input:?}");

        let sub = run(&["subagent"], input);
        assert_eq!(sub.status.code(), Some(0), "subagent {input:?}");
        assert!(
            sub.stdout.is_empty(),
            "subagent {input:?} drew rows: {:?}",
            stdout(&sub)
        );
    }
}

/// `STATUSLINE_DUMP` exists so a bad line in situ becomes an offline repro; the
/// round trip is only meaningful end-to-end. `CARGO_TARGET_TMPDIR` is a real
/// directory cargo provides to integration tests.
#[test]
fn the_dump_env_var_writes_the_raw_stdin_payload_verbatim() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dump-{}-{}.json", std::process::id(), line!()));
    let _ = std::fs::remove_file(&path);

    let out = Command::new(env!("CARGO_BIN_EXE_statusline"))
        .args(["--columns", "120"])
        .env("STATUSLINE_DUMP", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .take()
                .expect("stdin was piped")
                .write_all(PAYLOAD.as_bytes())?;
            child.wait_with_output()
        })
        .expect("the binary runs");

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let dumped = std::fs::read_to_string(&path).expect("the dump was written");
    let _ = std::fs::remove_file(&path);
    assert_eq!(dumped, PAYLOAD, "the dump is the raw stdin bytes, unmodified");
    // The dump is a side channel, not a substitute for drawing the line.
    assert!(stdout(&out).contains("Opus 5"), "{:?}", stdout(&out));
}
