//! Black-box integration tests for the `blocks` subcommand. Split out of the
//! monolithic `integration.rs` by R23; the original `blocks_verify_rejects_integrity_flags`
//! test body is byte-identical to its pre-split form — `cargo test --test blocks`
//! runs it in isolation.

use assert_cmd::Command;

mod common;

/// R60: `blocks verify` must NOT accept any of the four integrity/containment
/// flags (`--verify-integrity`, `--no-write-integrity`, `--strict-integrity`,
/// `--allow-outside`). They were previously `global = true` on `Cli` and
/// silently ignored here (`blocks verify` scans markdown, not the TOML +
/// sidecar pair). The R60 refactor moves them onto each TOML-touching
/// subcommand via a flattened `IntegrityArgs`, which structurally keeps them
/// off `blocks verify`. Passing one now errors at the clap layer — this test
/// locks in that contract so a future refactor can't silently re-introduce
/// the flag on a subcommand where it has no semantic hook.
#[test]
fn blocks_verify_rejects_integrity_flags() {
    for flag in [
        "--verify-integrity",
        "--no-write-integrity",
        "--strict-integrity",
        "--allow-outside",
    ] {
        let assert = Command::cargo_bin("tomlctl")
            .unwrap()
            .arg("blocks")
            .arg("verify")
            .arg(flag)
            .arg("some-file.md")
            .write_stdin("")
            .assert()
            .failure();
        let err = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        assert!(
            err.contains("unexpected argument")
                || err.contains("argument '--")
                || err.contains("found argument"),
            "`blocks verify {flag}` must be rejected by clap as an unknown argument; got stderr:\n{err}"
        );
    }
}
