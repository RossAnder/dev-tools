//! Stub binary used by the `pty_e2e` integration test in place of the real
//! interactive `claude` CLI.
//!
//! The PTY supervisor (T8) drives end-of-turn detection off the parser's
//! prompt-line heuristic: when the bottom line of the screen matches the
//! built-in prompt pattern (`> ` / `Human:` / `❯ ` / `› `) AND the parser
//! has been idle past `IDLE_THRESHOLD` (750 ms), the session is reported
//! `Idle`. We emit `> ` after every interaction so the supervisor's
//! `maybe_finalise_turn` loop transitions Awaiting → Idle and dispatches
//! the next queued input.
//!
//! On startup we emit a banner + one prompt line so the first parser feed
//! brings the session to `Idle`. For each line received on stdin, we emit a
//! pretend-assistant response (`Assistant: echo: <line>`) followed by a
//! fresh `> ` prompt.
//!
//! Dep-free by design — must compile with std only so the test build does
//! not pick up anyhow / tokio / sqlx into the fixture binary.

use std::io::{self, BufRead, Write};

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Initial banner + prompt. The trailing space after `>` matches the
    // parser's built-in pattern; the lack of a newline keeps the prompt on
    // the cursor row so the parser treats it as the "in-progress" prompt
    // rather than a finalised assistant line.
    writeln!(out, "Lumina PTY stub ready.").ok();
    write!(out, "> ").ok();
    out.flush().ok();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        writeln!(out, "Assistant: echo: {line}").ok();
        write!(out, "> ").ok();
        out.flush().ok();
    }
}
