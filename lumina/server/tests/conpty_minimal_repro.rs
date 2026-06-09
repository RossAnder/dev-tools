//! Regression test for the Windows ConPTY child-stdout delivery bug.
//!
//! Background: portable-pty 0.9.0 introduced a Windows-specific regression
//! (wezterm/wezterm#6783) where ConPTY emits `\x1b[6n` (DSR cursor-position
//! query) at startup and blocks all child output until the parent responds
//! on the master writer side. Without a custom DSR handler the child's
//! stdout never flows, indistinguishable from a hung child.
//!
//! Additional Windows-specific issue (wezterm/wezterm#4206):
//! `drop(pair.slave)` immediately after `spawn_command` severs the slave-side
//! pipe ConPTY uses to route child output. The established fix is to keep
//! the slave handle alive for the lifetime of the session.
//!
//! Combined fix: pin `portable-pty = "=0.8.1"` (which does not set
//! `PSEUDOCONSOLE_INHERIT_CURSOR` and therefore does not emit the DSR query),
//! AND on Windows keep the slave alive past the spawn boundary.
//!
//! This test exercises the bare `portable-pty` API with no `lumina::*`
//! infrastructure; a failure here means a regression in the dependency
//! version or in the Windows-specific lifetime handling.

use std::io::Read;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[test]
fn conpty_child_stdout_reaches_master_reader() {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    // Use the pty_stub Rust binary — writes a deterministic banner via
    // writeln! + flush() then blocks on stdin. Output is line-buffered when
    // attached to a terminal, so the banner reaches the master before the
    // stdin block.
    let stub = std::path::PathBuf::from(env!("CARGO_BIN_EXE_pty_stub"));
    assert!(stub.exists(), "pty_stub binary missing");
    let cmd = CommandBuilder::new(stub.to_string_lossy().to_string());

    let mut child = pair.slave.spawn_command(cmd).expect("spawn_command");

    // Windows-specific: keep the slave handle alive for the read window.
    // Dropping it immediately would sever the ConPTY routing on Windows
    // (wezterm/wezterm#4206). Unix can drop here safely, but the test
    // matches the production code's #[cfg(windows)] keep-alive pattern.
    let _slave_keep_alive = pair.slave;

    let mut reader = pair.master.try_clone_reader().expect("try_clone_reader");
    let _master_keep_alive = pair.master;

    let (tx, rx) = std::sync::mpsc::channel::<std::io::Result<Vec<u8>>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(Ok(Vec::new()));
                    break;
                }
                Ok(n) => {
                    if tx.send(Ok(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected: Vec<u8> = Vec::new();
    let marker = b"Lumina PTY stub ready.";

    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(chunk)) if chunk.is_empty() => break,
            Ok(Ok(chunk)) => {
                collected.extend_from_slice(&chunk);
                if collected.windows(marker.len()).any(|w| w == marker) {
                    break;
                }
            }
            Ok(Err(e)) => panic!("reader error: {e}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        collected.windows(marker.len()).any(|w| w == marker),
        "stub banner did not reach reader within 5s; got {} bytes: {:?}",
        collected.len(),
        String::from_utf8_lossy(&collected)
    );
}
