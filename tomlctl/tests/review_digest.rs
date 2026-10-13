//! `tomlctl review digest` and `review report` against a sandbox repo, a
//! temp-dir catalogue and a store seeded by hand with the rows each test
//! reads.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";
const STORE: &[&str] = &["--scope", SCOPE];
const REPORT: &str = ".claude/review-v2-runs/scope-t/U1/report.md";

const VOCABULARY: &str = r#"
[signals."trust-boundary"]
kind = "judged"
meaning = "input crosses a boundary"
hint = "parsers and auth"

[concerns.logic]
meaning = "wrong output"
families = ["code"]

[concerns.exploit]
meaning = "an exploitable input"
families = ["code"]
"#;

fn lens(id: &str, activation: &str, tag: &str, extra: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\nmode = \"judgement\"\n\
         produces = [\"defect\", \"cleanup\", \"advisory\"]\nowns = [\"{tag}\"]\n\
         activation = \"{activation}\"\nslice = \"per-subsystem\"\nv1_category = \"quality\"\n\
         {extra}+++\n## Question\nWhat does {id} ask?\n\n## Owns\n`{tag}` — owned here.\n\n\
         ## Probes — code\n- one probe\n\n## Does not flag\n- confirmations\n"
    )
}

/// A floor lens and a lens matched by the accepted `trust-boundary` signal.
fn write_catalogue(builtin: &Path) {
    let entries = [
        (
            "lenses/correctness.md",
            lens("correctness", "floor", "logic", ""),
        ),
        (
            "lenses/security.md",
            lens(
                "security",
                "signal",
                "exploit",
                "apply_when = { signals = [\"trust-boundary\"] }\n",
            ),
        ),
    ];
    for (rel, text) in entries {
        let path = builtin.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
}

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: PathBuf,
}

impl Repo {
    fn new(files: &[&str]) -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        write_catalogue(&builtin);
        for rel in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "fn f() {}\n").unwrap();
        }
        let out = git_command(&root).args(["add", "-A"]).output().unwrap();
        assert!(
            out.status.success(),
            "git add: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            builtin,
        })
    }

    /// Writes the `--scope t` store: run U1 over `src/**` with `waivers`,
    /// then `rows` as given.
    fn seed(&self, waivers: &str, rows: &str) {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        let text = format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
             depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\n\
             scope = [\"src/**\"]\n\
             signals = [{{ name = \"trust-boundary\", source = \"judged\", accepted = true, evidence = [\"src/a.rs\"] }}]\n\
             waivers = {waivers}\ncatalogue = {{ builtin = \"{}\" }}\n\n{rows}",
            self.builtin.to_string_lossy().replace('\\', "/"),
        );
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn tomlctl(&self) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    fn digest_text(&self, extra: &[&str]) -> String {
        let out = self
            .tomlctl()
            .args(["review", "digest", "--run", "U1"])
            .args(STORE)
            .args(extra)
            .assert()
            .success();
        String::from_utf8_lossy(&out.get_output().stdout).to_string()
    }

    fn digest(&self, extra: &[&str]) -> JsonValue {
        serde_json::from_str(&self.digest_text(extra)).expect("digest prints JSON")
    }

    fn report(&self) -> JsonValue {
        let out = self
            .tomlctl()
            .args(["review", "report", "--run", "U1", "--out", REPORT])
            .args(STORE)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("report prints JSON")
    }
}

/// A cell of run U1 over `src/a.rs` with the `correctness` concern bound.
fn cell(id: &str, wave: u32, agent: &str, status: &str) -> String {
    format!(
        "[[cells]]\nid = \"{id}\"\nrun = \"U1\"\nwave = {wave}\nslice = [\"src/a.rs\"]\n\
         concerns = [\"correctness\"]\nagent = \"{agent}\"\nreason = \"proposed\"\n\
         bound = [{{ concern = \"correctness\", hash = \"9c1e0123456789abcdef\", path = \"lenses/correctness.md\" }}]\n\
         status = \"{status}\"\nsealed = true\nnote = \"note of {id}\"\n\n"
    )
}

fn finding(id: &str, cell: &str, kind: &str, status: &str, extra: &str) -> String {
    format!(
        "[[findings]]\nid = \"{id}\"\nrun = \"U1\"\ncell = \"{cell}\"\nfamily = \"code\"\n\
         kind = \"{kind}\"\nlens = \"correctness\"\nfile = \"src/a.rs\"\nline = 1\n\
         summary = \"summary of {id}\"\nstatus = \"{status}\"\n{extra}\n"
    )
}

fn defect(id: &str, cell: &str, status: &str, extra: &str) -> String {
    finding(
        id,
        cell,
        "defect",
        status,
        &format!(
            "route = \"verifier\"\nseverity = \"major\"\nfailure_scenario = \"input x fails\"\n\
             fix = \"guard x\"\nfix_shape = \"edits\"\neffort = \"small\"\n{extra}"
        ),
    )
}

fn cleanup(id: &str, cell: &str, status: &str) -> String {
    finding(
        id,
        cell,
        "cleanup",
        status,
        "route = \"checker\"\nedit = \"rename y\"\ndirection = \"change\"\nold = \"fn f\"\neffort = \"trivial\"\n",
    )
}

fn advisory(id: &str, status: &str, extra: &str) -> String {
    finding(
        id,
        "C1",
        "advisory",
        status,
        &format!(
            "route = \"none\"\nangle = \"direction\"\nrationale = \"why {id}\"\n\
             adoption_cost = \"a day\"\n{extra}"
        ),
    )
}

fn verdict(finding: &str, by: &str, said: &str) -> String {
    format!(
        "[[verdicts]]\nfinding = \"{finding}\"\nrun = \"U1\"\nby = \"{by}\"\nseat = 1\n\
         verdict = \"{said}\"\ncitation = \"src/a.rs:1\"\nrationale = \"read it\"\n\n"
    )
}

fn ids(value: &JsonValue) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected an array: {value}"))
        .iter()
        .map(|row| row.get("id").unwrap_or(row).as_str().unwrap().to_string())
        .collect()
}

const NO_WAIVERS: &str = "[]";
const ALL_WAIVED: &str = r#"[{ concern = "security", reason = "no input handling" }]"#;

#[test]
fn digest_lists_unverified_ids_with_routes() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let rows = [
        cell("C1", 1, "review-finder", "covered"),
        defect("F1", "C1", "unverified", ""),
        cleanup("F2", "C1", "unverified"),
        defect("F3", "C1", "open", ""),
        verdict("F3", "verifier", "confirmed"),
    ]
    .concat();
    repo.seed(ALL_WAIVED, &rows);
    let digest = repo.digest(&[]);
    assert_eq!(
        digest["unverified"],
        json!([
            {"id": "F1", "kind": "defect", "route": "verifier", "cell": "C1"},
            {"id": "F2", "kind": "cleanup", "route": "checker", "cell": "C1"},
        ]),
        "{digest}"
    );
    assert_eq!(digest["counts"]["kind"], json!({"cleanup": 1, "defect": 2}));
}

#[test]
fn digest_lists_unwaived_unselected_concerns() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs"]) else {
        return;
    };
    let rows = cell("C1", 1, "review-finder", "covered");
    repo.seed(NO_WAIVERS, &rows);
    let digest = repo.digest(&[]);
    assert_eq!(digest["unselected"], json!(["security"]), "{digest}");
    assert_eq!(digest["unsliced"], json!(["src/b.rs"]), "{digest}");

    repo.seed(
        r#"[{ concern = "security", reason = "no input handling" }, { file = "src/b.rs", reason = "generated" }]"#,
        &rows,
    );
    let digest = repo.digest(&[]);
    assert_eq!(
        digest["unselected"],
        json!([]),
        "a waived concern is not due: {digest}"
    );
    assert_eq!(
        digest["unsliced"],
        json!([]),
        "a waived file is not due: {digest}"
    );
}

#[test]
fn digest_since_wave_limits_rows() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let rows = [
        cell("C1", 1, "review-finder", "thin"),
        cell("C2", 2, "review-finder", "uncovered"),
        defect("F1", "C1", "unverified", ""),
        defect("F2", "C2", "unverified", ""),
    ]
    .concat();
    repo.seed(ALL_WAIVED, &rows);

    let all = repo.digest(&[]);
    assert_eq!(ids(&all["unverified"]), ["F1", "F2"], "{all}");
    assert_eq!(ids(&all["cells"]["notes"]), ["C1", "C2"], "{all}");

    let since = repo.digest(&["--since-wave", "2"]);
    assert_eq!(since["since_wave"], 2);
    assert_eq!(ids(&since["unverified"]), ["F2"], "{since}");
    assert_eq!(ids(&since["cells"]["notes"]), ["C2"], "{since}");
    assert_eq!(since["counts"]["kind"], json!({"defect": 1}), "{since}");
}

#[test]
fn digest_omits_evidence() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let evidence = "evidence = [\"src/a.rs:1 SECRET-PROOF\"]\n";
    let rows = [
        cell("C1", 1, "review-finder", "covered"),
        defect("F1", "C1", "open", evidence),
        defect(
            "F2",
            "C1",
            "merged",
            &format!("{evidence}same_as = \"F1\"\noutcome = \"contested\"\nmerged_into = \"F1\"\n"),
        ),
        defect("F3", "C1", "refuted", evidence).replace("\"major\"", "\"critical\""),
        advisory("F4", "open", "evidence = [\"src/a.rs:1 SECRET-PROOF\"]\n"),
        verdict("F1", "verifier", "confirmed"),
        verdict("F2", "verifier", "refuted"),
        verdict("F3", "verifier", "refuted"),
    ]
    .concat();
    repo.seed(ALL_WAIVED, &rows);
    let text = repo.digest_text(&[]);
    let digest: JsonValue = serde_json::from_str(&text).unwrap();
    assert_eq!(ids(&digest["contested"]), ["F2"], "{digest}");
    assert_eq!(digest["contested"][0]["target"]["id"], "F1", "{digest}");
    assert_eq!(ids(&digest["refuted_criticals"]), ["F3"], "{digest}");
    assert_eq!(ids(&digest["advisory"]), ["F4"], "{digest}");
    assert!(!text.contains("evidence"), "no evidence key: {text}");
    assert!(!text.contains("SECRET-PROOF"), "no evidence value: {text}");
}

#[test]
fn digest_lists_advisory_only_until_triaged() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let rows = [
        cell("C1", 1, "review-finder", "covered"),
        advisory("F1", "open", ""),
        advisory("F2", "parked", "park_reason = \"later\"\n"),
    ]
    .concat();
    repo.seed(ALL_WAIVED, &rows);
    let digest = repo.digest(&[]);
    assert_eq!(ids(&digest["advisory"]), ["F1"], "{digest}");
    assert_eq!(digest["advisory"][0]["rationale"], "why F1", "{digest}");
}

#[test]
fn digest_audit_sample_is_chosen_by_index() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let mut rows = [
        cell("C1", 1, "review-finder", "covered"),
        cell("C2", 1, "review-sweeper", "covered"),
    ]
    .concat();
    for id in ["F1", "F2", "F3", "F4"] {
        rows.push_str(&defect(id, "C1", "open", ""));
        rows.push_str(&verdict(id, "verifier", "confirmed"));
    }
    for id in ["F5", "F6", "F7"] {
        rows.push_str(&cleanup(id, "C1", "open"));
        rows.push_str(&verdict(id, "checker", "confirmed"));
    }
    rows.push_str(&cleanup("F8", "C2", "open"));
    rows.push_str(&verdict("F8", "checker", "confirmed"));
    rows.push_str(&defect("F9", "C1", "refuted", "").replace("\"major\"", "\"minor\""));
    rows.push_str(&verdict("F9", "verifier", "refuted"));
    rows.push_str(&defect("F10", "C1", "refuted", "").replace("\"major\"", "\"critical\""));
    rows.push_str(&verdict("F10", "verifier", "refuted"));
    repo.seed(ALL_WAIVED, &rows);

    let sample = &repo.digest(&[])["audit_sample"];
    assert_eq!(sample["confirmed_defects"], json!(["F2", "F4"]), "{sample}");
    assert_eq!(
        sample["refuted_defects"],
        json!(["F9"]),
        "a refuted critical is audited in full, not sampled: {sample}"
    );
    assert_eq!(
        sample["checked_cleanup"],
        json!(["F6", "F8"]),
        "one pick comes from the sweeper cell: {sample}"
    );
}

fn report_rows() -> String {
    [
        cell("C1", 1, "review-finder", "covered"),
        defect("F1", "C1", "open", ""),
        cleanup("F2", "C1", "open"),
        advisory("F3", "parked", "park_reason = \"later\"\n"),
        defect("F4", "C1", "refuted", ""),
        verdict("F1", "verifier", "confirmed"),
        verdict("F2", "checker", "confirmed"),
        verdict("F4", "verifier", "refuted"),
    ]
    .concat()
}

#[test]
fn report_writes_file_and_prints_counts() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(ALL_WAIVED, &report_rows());
    let printed = repo.report();
    assert_eq!(
        printed,
        json!({
            "path": REPORT,
            "counts": "defect 1 · cleanup 1 · advisory 1 · refuted 1 · suppressed 0 · unverified 0 · cells 1 · uncovered 0",
        }),
        "only the path and a counts line"
    );
    let text = fs::read_to_string(repo.root.join(REPORT)).unwrap();
    for part in [
        "# Review report — U1",
        "### Major",
        "**F1**",
        "### `src/a.rs`",
        "**F2**",
        "park_reason: later",
        "correctness@9c1e01234567",
        "- concern `security` — no input handling",
    ] {
        assert!(text.contains(part), "missing {part:?}:\n{text}");
    }
    assert!(
        !text.contains("**F4**"),
        "a refuted defect is counted, not listed:\n{text}"
    );
}

#[test]
fn report_is_byte_stable() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(ALL_WAIVED, &report_rows());
    repo.report();
    let first = fs::read(repo.root.join(REPORT)).unwrap();
    repo.report();
    let second = fs::read(repo.root.join(REPORT)).unwrap();
    assert_eq!(first, second);
}
