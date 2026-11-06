//! The integrity surface's two refusals: stamping a sidecar over a CRLF `.toml`,
//! and a writer's `--verify-integrity` check on a store that fails it.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const LF_BODY: &str = "schema_version = 1\n\n[[items]]\nid = \"R1\"\nstatus = \"open\"\n";

fn fresh_store(name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let store = claude.join(name);
    fs::write(&store, body).unwrap();
    (dir, store)
}

fn sidecar(file: &Path) -> PathBuf {
    let mut path = file.as_os_str().to_os_string();
    path.push(".sha256");
    PathBuf::from(path)
}

fn digest_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_sidecar_over(file: &Path, bytes: &[u8]) {
    let name = file.file_name().unwrap().to_string_lossy();
    fs::write(sidecar(file), format!("{}  {name}\n", digest_of(bytes))).unwrap();
}

fn tomlctl(dir: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .args(["--error-format", "json"])
        .write_stdin("");
    cmd
}

fn error_of(cmd: &mut Command) -> JsonValue {
    let out = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: JsonValue = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr must be JSON; err={e}; stderr:\n{stderr}"));
    envelope["error"].clone()
}

fn assert_names_the_remedy(message: &str) {
    for needle in [".gitattributes", "eol=lf", "git checkout", "remove"] {
        assert!(message.contains(needle), "`{needle}` absent: {message}");
    }
}

#[test]
fn refresh_refuses_a_toml_holding_crlf_and_leaves_the_sidecar_alone() {
    let (dir, store) = fresh_store("data.toml", &LF_BODY.replace('\n', "\r\n"));
    let stale = "stale sidecar bytes\n";
    fs::write(sidecar(&store), stale).unwrap();

    let err = error_of(tomlctl(&dir).args(["integrity", "refresh"]).arg(&store));

    assert_eq!(err["kind"], "integrity", "{err}");
    assert!(err["file"].as_str().unwrap().contains("data.toml"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert_names_the_remedy(message);
    assert!(!message.contains('\n'), "{message}");
    assert_eq!(fs::read_to_string(sidecar(&store)).unwrap(), stale);
}

#[test]
fn refresh_refuses_a_toml_with_a_single_crlf_and_writes_no_sidecar() {
    let (dir, store) = fresh_store("data.toml", "a = 1\r\nb = 2\n");

    let err = error_of(tomlctl(&dir).args(["integrity", "refresh"]).arg(&store));

    assert_eq!(err["kind"], "integrity", "{err}");
    assert!(!sidecar(&store).exists());
}

#[test]
fn refresh_stamps_a_non_toml_file_holding_crlf_over_its_raw_bytes() {
    let body = "{\r\n  \"a\": 1\r\n}\r\n";
    let (dir, store) = fresh_store("data.json", body);

    tomlctl(&dir)
        .args(["integrity", "refresh"])
        .arg(&store)
        .assert()
        .success();

    let stamped = fs::read_to_string(sidecar(&store)).unwrap();
    assert!(
        stamped.starts_with(&digest_of(body.as_bytes())),
        "{stamped}"
    );
}

#[test]
fn refresh_stamps_an_lf_toml_with_a_lone_cr_over_its_raw_bytes() {
    let body = "a = \"x\ty\"\n# a lone \r stays content\n";
    let (dir, store) = fresh_store("data.toml", body);

    tomlctl(&dir)
        .args(["integrity", "refresh"])
        .arg(&store)
        .assert()
        .success();

    let stamped = fs::read_to_string(sidecar(&store)).unwrap();
    assert!(
        stamped.starts_with(&digest_of(body.as_bytes())),
        "{stamped}"
    );
}

/// A value holding CRLF reaches the file escaped, so a store this tool wrote can
/// always be refreshed afterwards.
#[test]
fn a_value_holding_crlf_does_not_make_the_written_store_refuse_a_refresh() {
    let (dir, store) = fresh_store("data.toml", LF_BODY);

    tomlctl(&dir)
        .args(["items", "update"])
        .arg(&store)
        .arg("R1")
        .args(["--json", r#"{"summary":"first\r\nsecond\r\n"}"#])
        .assert()
        .success();

    tomlctl(&dir)
        .args(["integrity", "refresh"])
        .arg(&store)
        .assert()
        .success();
}

#[test]
fn a_mismatch_on_a_crlf_copy_of_the_lf_digest_names_line_endings_on_one_line() {
    let (dir, store) = fresh_store("data.toml", &LF_BODY.replace('\n', "\r\n"));
    write_sidecar_over(&store, LF_BODY.as_bytes());

    let err = error_of(
        tomlctl(&dir)
            .args(["get"])
            .arg(&store)
            .args(["schema_version", "--verify-integrity"]),
    );

    assert_eq!(err["kind"], "integrity", "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("integrity check failed"), "{message}");
    assert!(message.contains("line endings"), "{message}");
    assert_names_the_remedy(message);
    assert!(!message.contains('\n'), "{message}");
}

#[test]
fn a_crlf_store_whose_folded_bytes_differ_from_the_digest_gets_no_clause() {
    let (dir, store) = fresh_store("data.toml", &LF_BODY.replace('\n', "\r\n"));
    write_sidecar_over(&store, b"schema_version = 2\n");

    let err = error_of(
        tomlctl(&dir)
            .args(["get"])
            .arg(&store)
            .args(["schema_version", "--verify-integrity"]),
    );

    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("integrity check failed"), "{message}");
    assert!(!message.contains("line endings"), "{message}");
}

#[test]
fn a_digest_over_the_crlf_bytes_still_verifies() {
    let crlf = LF_BODY.replace('\n', "\r\n");
    let (dir, store) = fresh_store("data.toml", &crlf);
    write_sidecar_over(&store, crlf.as_bytes());

    tomlctl(&dir)
        .args(["get"])
        .arg(&store)
        .args(["schema_version", "--verify-integrity"])
        .assert()
        .success();
}

/// One verb per `mutate_doc` sibling: `set`, then `items add` and `items remove`.
fn writer_commands(dir: &tempfile::TempDir, store: &Path) -> Vec<Command> {
    let mut set = tomlctl(dir);
    set.arg("set").arg(store).args(["key", "value"]);
    let mut add = tomlctl(dir);
    add.args(["items", "add"])
        .arg(store)
        .args(["--json", r#"{"id":"R2","status":"open"}"#]);
    let mut remove = tomlctl(dir);
    remove.args(["items", "remove"]).arg(store).arg("R1");
    vec![set, add, remove]
}

#[test]
fn every_writer_refuses_a_mismatched_store_and_repairs_nothing() {
    let (dir, store) = fresh_store("data.toml", LF_BODY);
    let bogus = format!("{}  data.toml\n", "0".repeat(64));
    fs::write(sidecar(&store), &bogus).unwrap();

    for mut writer in writer_commands(&dir, &store) {
        let err = error_of(writer.arg("--verify-integrity"));
        assert_eq!(err["kind"], "integrity", "{err}");
        assert!(err["file"].as_str().unwrap().contains("data.toml"), "{err}");
        assert_eq!(fs::read_to_string(&store).unwrap(), LF_BODY);
        assert_eq!(fs::read_to_string(sidecar(&store)).unwrap(), bogus);
    }
}

#[test]
fn every_writer_refuses_a_store_with_no_sidecar_as_a_read_does() {
    let (dir, store) = fresh_store("data.toml", LF_BODY);

    for mut writer in writer_commands(&dir, &store) {
        let err = error_of(writer.arg("--verify-integrity"));
        assert_eq!(err["kind"], "integrity", "{err}");
        assert!(
            err["message"].as_str().unwrap().contains("is missing"),
            "{err}"
        );
        assert_eq!(fs::read_to_string(&store).unwrap(), LF_BODY);
        assert!(!sidecar(&store).exists());
    }
}

#[test]
fn a_writer_passes_a_matching_store_and_the_flag_is_off_by_default() {
    let (dir, store) = fresh_store("data.toml", LF_BODY);
    write_sidecar_over(&store, LF_BODY.as_bytes());

    tomlctl(&dir)
        .arg("set")
        .arg(&store)
        .args(["key", "value", "--verify-integrity"])
        .assert()
        .success();

    fs::write(sidecar(&store), format!("{}  data.toml\n", "0".repeat(64))).unwrap();
    tomlctl(&dir)
        .arg("set")
        .arg(&store)
        .args(["key", "other"])
        .assert()
        .success();
}

#[test]
fn a_writer_creating_its_target_has_nothing_to_verify() {
    let (dir, store) = fresh_store("data.toml", LF_BODY);
    let target = store.with_file_name("new.toml");

    tomlctl(&dir)
        .arg("set")
        .arg(&target)
        .args(["key", "value", "--verify-integrity"])
        .assert()
        .success();

    assert!(target.exists());
    assert!(sidecar(&target).exists());
}
