//! Replays two recorded code runs through `review plan --propose` against the
//! live lens catalogue, and checks the proposal's shape: judgement cells keep
//! file kinds apart, security stays where its signals point, test-gaps reads
//! code and tests, sweepers stay within their pool, and a matched lens is
//! never cut while a repeat pass is proposed.
//!
//! Each fixture holds a run record with no cells and the run head's tracked
//! path list; the U5 paths are anonymised segment by segment, keeping the
//! names the routing globs read.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use common::{cli, git_available, git_command, sandbox};
use globset::{Glob, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use serde_json::Value as JsonValue;

const SWEEPER: &str = "review-sweeper";
const SCOPE: &str = "replay";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn builtin() -> PathBuf {
    manifest_dir().join("../claude/skills/review-v2-lenses")
}

fn fixture(name: &str) -> PathBuf {
    manifest_dir()
        .join("tests/fixtures/review_replay")
        .join(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Lockfile,
    Test,
    Code,
    Other,
}

/// The live catalogue's `[routing]` lists, applied lockfiles first, then
/// tests, then executables.
struct Routing {
    lockfiles: GlobSet,
    tests: GlobSet,
    executable: GlobSet,
}

fn glob_set(list: &toml::Value) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in list.as_array().expect("a routing glob list") {
        builder.add(Glob::new(pattern.as_str().unwrap()).unwrap());
    }
    builder.build().unwrap()
}

impl Routing {
    fn live() -> Self {
        let text = fs::read_to_string(builtin().join("vocabulary.toml")).unwrap();
        let doc: toml::Value = toml::from_str(&text).unwrap();
        let routing = &doc["routing"];
        Self {
            lockfiles: glob_set(&routing["lockfiles"]),
            tests: glob_set(&routing["tests"]),
            executable: glob_set(&routing["executable"]),
        }
    }

    fn kind(&self, path: &str) -> Kind {
        if self.lockfiles.is_match(path) {
            Kind::Lockfile
        } else if self.tests.is_match(path) {
            Kind::Test
        } else if self.executable.is_match(path) {
            Kind::Code
        } else {
            Kind::Other
        }
    }
}

/// One lens's frontmatter fields the checks read.
struct Lens {
    mode: String,
    activation: String,
    signals: Vec<String>,
}

fn live_lenses() -> BTreeMap<String, Lens> {
    let mut lenses = BTreeMap::new();
    for entry in fs::read_dir(builtin().join("lenses")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let front = text
            .strip_prefix("+++\n")
            .and_then(|rest| rest.split_once("\n+++"))
            .map(|(front, _)| front)
            .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
        let doc: toml::Value = toml::from_str(front).unwrap();
        let text_of = |key: &str| {
            doc.get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let signals = doc
            .get("apply_when")
            .and_then(|a| a.get("signals"))
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|s| s.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default();
        lenses.insert(
            text_of("id"),
            Lens {
                mode: text_of("mode"),
                activation: text_of("activation"),
                signals,
            },
        );
    }
    lenses
}

/// A signal as the run records it: active when a detector raised it or the
/// orchestrator accepted a judged one.
struct Signal {
    name: String,
    roots: Vec<String>,
    evidence: Vec<String>,
}

fn active_signals(run: &toml::Table) -> Vec<Signal> {
    let strings = |s: &toml::Value, key: &str| -> Vec<String> {
        s.get(key)
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .unwrap()
                            .trim_start_matches("./")
                            .trim_end_matches('/')
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    run["signals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| match s["source"].as_str().unwrap() {
            "detector" => true,
            "judged" => s.get("accepted").and_then(toml::Value::as_bool) == Some(true),
            _ => false,
        })
        .map(|s| Signal {
            name: s["name"].as_str().unwrap().to_string(),
            roots: strings(s, "roots"),
            evidence: strings(s, "evidence"),
        })
        .collect()
}

/// A slice entry as `git`'s `:(glob)` pathspec reads it: `*` stays within a
/// segment, and a plain entry also matches everything under it.
struct Entry {
    text: String,
    glob: Option<GlobMatcher>,
}

impl Entry {
    fn new(raw: &str) -> Self {
        let text = raw
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        let glob = GlobBuilder::new(&text)
            .literal_separator(true)
            .build()
            .ok()
            .map(|g| g.compile_matcher());
        Self { text, glob }
    }

    fn matches(&self, path: &str) -> bool {
        under(path, &self.text) || self.glob.as_ref().is_some_and(|g| g.is_match(path))
    }
}

fn under(path: &str, dir: &str) -> bool {
    !dir.is_empty()
        && (path == dir
            || path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')))
}

/// One replayed run: its record, tracked list, proposal and summary.
struct Replay {
    run: toml::Table,
    tracked: Vec<String>,
    cells: Vec<JsonValue>,
    summary: JsonValue,
}

impl Replay {
    /// Loads `name`'s fixture into a git sandbox of empty tracked files and
    /// proposes cells for its run; `None` without git.
    fn of(name: &str) -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let dir = fixture(name);
        let tracked: Vec<String> = fs::read_to_string(dir.join("tracked.txt"))
            .unwrap()
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        let run: toml::Table =
            toml::from_str(&fs::read_to_string(dir.join("run.toml")).unwrap()).unwrap();

        let (_dir, root) = sandbox();
        // The sandbox's own ignore file would add a tracked path the run's head
        // never had, and could hide some that it did.
        fs::remove_file(root.join(".gitignore")).unwrap();
        for rel in &tracked {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let out = git_command(&root)
            .args(["add", "-A", "-f", "--", "."])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git add: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let listed = git_command(&root)
            .args(["-c", "core.quotePath=false", "ls-files"])
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout).lines().count();
        assert_eq!(
            listed,
            tracked.len(),
            "{name}: every fixture path is tracked"
        );

        let store = Self::store(&run);
        let path = root.join(".claude/review-v2").join(format!("{SCOPE}.toml"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, store).unwrap();

        let id = run["id"].as_str().unwrap().to_string();
        let out = cli(&root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args([
                "review",
                "plan",
                "--scope",
                SCOPE,
                "--run",
                &id,
                "--propose",
            ])
            .args([
                "--lens",
                "+intent-fidelity",
                "--context-free",
                "--reserve",
                "1",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
        let mut cells: Vec<JsonValue> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("every line is JSON"))
            .collect();
        let summary = cells.pop().expect("a summary line")["proposal"].clone();
        assert!(summary.is_object(), "{name}: the last line is the summary");
        Some(Self {
            run,
            tracked,
            cells,
            summary,
        })
    }

    /// A one-run store whose catalogue is the repository's built-in one,
    /// pinned by no sha so catalogue edits flow into the replay.
    fn store(run: &toml::Table) -> String {
        let mut run = run.clone();
        let builtin = builtin().to_string_lossy().replace('\\', "/");
        let mut catalogue = toml::Table::new();
        catalogue.insert("builtin".into(), toml::Value::String(builtin));
        run.insert("catalogue".into(), toml::Value::Table(catalogue));
        let mut doc = toml::Table::new();
        doc.insert("schema_version".into(), toml::Value::Integer(1));
        doc.insert(
            "runs".into(),
            toml::Value::Array(vec![toml::Value::Table(run)]),
        );
        toml::to_string(&doc).unwrap()
    }

    fn name(&self) -> &str {
        self.run["id"].as_str().unwrap()
    }

    /// The tracked files a cell's slice entries match.
    fn files(&self, cell: &JsonValue) -> Vec<&str> {
        let entries: Vec<Entry> = cell["slice"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| Entry::new(e.as_str().unwrap()))
            .collect();
        self.tracked
            .iter()
            .filter(|f| entries.iter().any(|e| e.matches(f)))
            .map(String::as_str)
            .collect()
    }

    fn ceiling(&self) -> usize {
        self.summary["ceiling"].as_u64().unwrap() as usize
    }

    fn cut_for_ceiling(&self) -> Vec<String> {
        self.summary["unselected"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|u| u["reason"] == "ceiling")
            .map(|u| u["concern"].as_str().unwrap().to_string())
            .collect()
    }
}

fn concerns(cell: &JsonValue) -> Vec<&str> {
    cell["concerns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect()
}

fn pass(cell: &JsonValue) -> u64 {
    cell.get("pass").and_then(JsonValue::as_u64).unwrap_or(1)
}

/// Every proposal success criterion the replays can state as a property.
fn assert_proposal_criteria(replay: &Replay) {
    let name = replay.name();
    let routing = Routing::live();
    let lenses = live_lenses();
    let signals = active_signals(&replay.run);
    let active: BTreeSet<&str> = signals.iter().map(|s| s.name.as_str()).collect();
    let matched = |id: &str| {
        lenses.get(id).is_some_and(|lens| {
            matches!(lens.activation.as_str(), "signal" | "candidate")
                && lens.signals.iter().any(|s| active.contains(s.as_str()))
        })
    };
    let cells = &replay.cells;
    assert!(!cells.is_empty(), "{name}: a proposal with cells");

    for cell in cells {
        let held = concerns(cell);
        let files = replay.files(cell);
        let kinds: Vec<(&str, Kind)> = files.iter().map(|f| (*f, routing.kind(f))).collect();
        if held.contains(&"correctness") || held.contains(&"test-gaps") {
            let locked: Vec<&str> = kinds
                .iter()
                .filter(|(_, k)| *k == Kind::Lockfile)
                .map(|(f, _)| *f)
                .collect();
            assert!(
                locked.is_empty(),
                "{name}: a {held:?} cell holds lockfiles {locked:?}"
            );
            let code = kinds.iter().any(|(_, k)| *k == Kind::Code);
            let other = kinds.iter().any(|(_, k)| *k == Kind::Other);
            assert!(
                !(code && other),
                "{name}: a {held:?} cell mixes code and other files: {:?}",
                cell["slice"]
            );
        }
        if held.contains(&"test-gaps") {
            let stray: Vec<&str> = kinds
                .iter()
                .filter(|(_, k)| !matches!(k, Kind::Code | Kind::Test))
                .map(|(f, _)| *f)
                .collect();
            assert!(
                stray.is_empty(),
                "{name}: a test-gaps cell reads files that are neither code nor tests: {stray:?}"
            );
        }
        if held.contains(&"security") {
            let pointing: Vec<&Signal> = signals
                .iter()
                .filter(|s| lenses["security"].signals.contains(&s.name))
                .collect();
            let stray: Vec<&str> = kinds
                .iter()
                .filter(|(f, k)| {
                    *k != Kind::Lockfile
                        && !pointing.iter().any(|s| {
                            s.evidence.iter().any(|e| e == f)
                                || s.roots.iter().any(|r| r != "." && under(f, r))
                        })
                })
                .map(|(f, _)| *f)
                .collect();
            assert!(
                stray.is_empty(),
                "{name}: a security cell reads files no security signal points at: {stray:?}"
            );
        }
    }

    let sweepers: Vec<&JsonValue> = cells.iter().filter(|c| c["agent"] == SWEEPER).collect();
    let ceiling = replay.ceiling();
    assert!(
        sweepers.len() <= ceiling.div_ceil(4),
        "{name}: {} sweeper cells past a pool of {}",
        sweepers.len(),
        ceiling.div_ceil(4)
    );
    assert!(
        sweepers.iter().all(|c| pass(c) == 1),
        "{name}: a sweeper cell repeats"
    );

    let cut = replay.cut_for_ceiling();
    if cells.iter().any(|c| pass(c) == 2) {
        let matched_cut: Vec<&String> = cut.iter().filter(|c| matched(c)).collect();
        assert!(
            matched_cut.is_empty(),
            "{name}: signal-matched lenses {matched_cut:?} cut while a repeat pass is proposed"
        );
    }
    assert!(
        cells.len() < ceiling,
        "{name}: {} cells against a ceiling of {ceiling} with one cell reserved",
        cells.len()
    );

    let judgement_cut = cut
        .iter()
        .any(|c| lenses.get(c).is_some_and(|l| l.mode == "judgement"));
    if judgement_cut {
        let mut per_lens: BTreeMap<&str, usize> = BTreeMap::new();
        for cell in &sweepers {
            for concern in concerns(cell) {
                *per_lens.entry(concern).or_default() += 1;
            }
        }
        for (lens, count) in per_lens {
            assert_eq!(
                count, 1,
                "{name}: sweeper lens {lens} holds {count} cells after a judgement lens was cut"
            );
        }
    }
}

#[test]
fn u3_proposal_meets_the_success_criteria() {
    let Some(replay) = Replay::of("u3") else {
        return;
    };
    assert_proposal_criteria(&replay);
}

#[test]
fn u5_proposal_meets_the_success_criteria() {
    let Some(replay) = Replay::of("u5") else {
        return;
    };
    assert_proposal_criteria(&replay);
    assert!(
        replay
            .cells
            .iter()
            .any(|c| concerns(c).contains(&"security")),
        "U5: security is admitted; cut for the ceiling: {:?}",
        replay.cut_for_ceiling()
    );
}

#[test]
fn u5_fixture_keeps_every_file_kind() {
    let tracked = fs::read_to_string(fixture("u5").join("tracked.txt")).unwrap();
    let routing = Routing::live();
    let kinds: BTreeSet<String> = tracked
        .lines()
        .map(|p| format!("{:?}", routing.kind(p)))
        .collect();
    // The anonymised tree still holds every kind the slicing keeps apart.
    for kind in ["Lockfile", "Test", "Code", "Other"] {
        assert!(kinds.contains(kind), "U5 tracks a {kind} file");
    }
}
