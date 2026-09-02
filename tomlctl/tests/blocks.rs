//! Black-box integration tests for the `blocks` subcommand.

use assert_cmd::Command;

mod common;

/// `blocks verify` must NOT accept any of the four integrity/containment
/// flags (`--verify-integrity`, `--no-write-integrity`, `--strict-integrity`,
/// `--allow-outside`): it scans markdown, not the TOML + sidecar pair, so
/// they have no semantic hook here. They live on each TOML-touching
/// subcommand via a flattened `IntegrityArgs`, which structurally keeps them
/// off `blocks verify`; passing one errors at the clap layer. This test locks
/// that contract in so a refactor can't silently re-introduce the flag.
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
