//! A reader that closes stdout before tomlctl writes (`tomlctl … | head`)
//! ends the run quietly with success. The pipe's read end is dropped before
//! the child starts, so its first stdout write always hits a closed pipe.

use std::process::{Command, Stdio};

mod common;
use common::{QUERY_FIXTURE, seed_ledger};

fn run_into_closed_pipe(cmd: &mut Command) {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{:?}: {stderr}", out.status);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn capabilities_into_a_closed_pipe_exits_quietly() {
    run_into_closed_pipe(Command::new(env!("CARGO_BIN_EXE_tomlctl")).arg("capabilities"));
}

#[test]
fn streamed_list_into_a_closed_pipe_exits_quietly() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    run_into_closed_pipe(
        Command::new(env!("CARGO_BIN_EXE_tomlctl"))
            .env("TOMLCTL_ROOT", dir.path())
            .args(["items", "list"])
            .arg(&ledger)
            .arg("--lines"),
    );
}
