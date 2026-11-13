//! `review anchor` against a sandbox seeded by hand: exit 0 while a finding's
//! anchor holds, 1 once it has drifted, and `--gone` only for a `remove`
//! cleanup whose `old` has left the whole edit target file.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox, stdout_json};

const SCOPE: &str = "t";

const HEADER: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-01T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness", "dry"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"
"#;

/// The source the findings anchor into: `limit` on line 3, the `--fast`
/// line on line 30.
fn source() -> String {
    let mut lines = vec![
        "use std::fmt;".to_string(),
        String::new(),
        "pub fn limit(n: u32) -> u32 {".to_string(),
        "    n + 1".to_string(),
        "}".to_string(),
    ];
    while lines.len() < 29 {
        lines.push("// filler".to_string());
    }
    lines.push("let fast = \"--fast\";".to_string());
    lines.push("// end".to_string());
    lines.join("\n") + "\n"
}

fn defect(id: &str, symbol: &str) -> String {
    format!(
        r#"
[[findings]]
id = "{id}"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 3
symbol = "{symbol}"
summary = "the limit is off by one"
severity = "major"
status = "open"
"#
    )
}

fn cleanup(id: &str, direction: &str, old: Option<&str>, line: u32) -> String {
    let old = old.map(|o| format!("old = {o:?}\n")).unwrap_or_default();
    format!(
        r#"
[[findings]]
id = "{id}"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "cleanup"
lens = "dry"
route = "checker"
file = "src/lib.rs"
line = {line}
summary = "tidy the fast flag"
edit = "drop the --fast line"
direction = "{direction}"
effort = "trivial"
status = "open"
{old}"#
    )
}

const ADVISORY: &str = r#"
[[findings]]
id = "F9"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "src/lib.rs"
summary = "consider a newtype"
status = "open"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new(body: &[String]) -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, format!("{HEADER}{}", body.concat())).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), source()).unwrap();
        Self { _dir: dir, root }
    }

    fn source(&self, text: &str) {
        fs::write(self.root.join("src/lib.rs"), text).unwrap();
    }

    fn anchor(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "anchor"])
            .args(args)
            .args(["--scope", SCOPE])
            .assert()
    }

    fn store_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(".claude/review-v2/t.toml")).unwrap()
    }
}

fn fixtures() -> Vec<String> {
    vec![
        defect("F1", "lib::limit"),
        cleanup("F2", "change", Some("let fast = \"--fast\";"), 30),
        cleanup("F3", "remove", Some("let fast = \"--fast\";"), 30),
        cleanup("F4", "add", None, 30),
        defect("F5", "lib::missing"),
        ADVISORY.to_string(),
    ]
}

#[test]
fn a_matching_defect_and_cleanup_anchor_holds() {
    let repo = Repo::new(&fixtures());
    let before = repo.store_bytes();
    for id in ["F1", "F2", "F3", "F4"] {
        let out = repo.anchor(&[id]).success();
        let json = stdout_json(&out);
        assert_eq!(json["ok"], true, "{id}: {json}");
        assert_eq!(json["id"], id);
    }
    assert_eq!(repo.store_bytes(), before, "the verb writes nothing");
}

#[test]
fn a_drifted_anchor_exits_one() {
    let repo = Repo::new(&fixtures());
    let out = repo.anchor(&["F5"]).code(1);
    let json = stdout_json(&out);
    assert_eq!(json["ok"], false);
    assert!(
        json["reason"].as_str().unwrap().contains("missing"),
        "{json}"
    );

    let shifted: String = "// pad\n".repeat(40) + &source();
    repo.source(&shifted);
    repo.anchor(&["F2"]).code(1);
    repo.anchor(&["F1"]).success();

    repo.source("pub fn limited(n: u32) -> u32 { n }\n");
    repo.anchor(&["F1"]).code(1);

    fs::remove_file(repo.root.join("src/lib.rs")).unwrap();
    repo.anchor(&["F1"]).code(1);
    repo.anchor(&["F4"]).code(1);
}

#[test]
fn a_crlf_file_matches_as_review_add_does() {
    let repo = Repo::new(&fixtures());
    repo.source(&source().replace('\n', "\r\n"));
    repo.anchor(&["F2"]).success();
}

#[test]
fn gone_exits_zero_only_when_old_has_left_the_file() {
    let repo = Repo::new(&fixtures());
    repo.anchor(&["F3", "--gone"]).code(1);

    let shifted: String = "// pad\n".repeat(40) + &source();
    repo.source(&shifted);
    let out = repo.anchor(&["F3", "--gone"]).code(1);
    assert_eq!(stdout_json(&out)["ok"], false, "shifted text is not gone");

    repo.source(&source().replace("let fast = \"--fast\";\n", ""));
    let out = repo.anchor(&["F3", "--gone"]).success();
    assert_eq!(stdout_json(&out)["ok"], true);
}

#[test]
fn gone_is_refused_on_a_defect_an_add_and_a_change() {
    let repo = Repo::new(&fixtures());
    for id in ["F1", "F2", "F4"] {
        let out = repo.anchor(&[id, "--gone"]).failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(stderr.contains("--gone applies only"), "{id}: {stderr}");
        assert!(
            out.get_output().stdout.is_empty(),
            "{id}: refused, not answered"
        );
    }
}

#[test]
fn an_advisory_and_an_unknown_id_are_refused() {
    let repo = Repo::new(&fixtures());
    let out = repo.anchor(&["F9"]).failure();
    assert!(out.get_output().stdout.is_empty());
    let out = repo.anchor(&["F99"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("no finding F99"), "{stderr}");
}
