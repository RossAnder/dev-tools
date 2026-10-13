//! `tomlctl review survey` against sandbox repos with test-local detector
//! vocabularies. Every run starts with `--builtin` pointing at a temp-dir
//! catalogue, so no test reads the developer's claude dir or the shipped
//! vocabulary.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

/// The concern and lens every fixture catalogue needs to validate.
const CONCERNS: &str = r#"
[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code"]
"#;

const CORRECTNESS: &str = r#"+++
id = "correctness"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["logic"]
activation = "floor"
slice = "per-subsystem"
v1_category = "quality"
+++
## Question
Does it compute the right thing?

## Owns
`logic` — wrong output.

## Probes — code
- off-by-one in ranges

## Does not flag
- confirmations
"#;

const STACKS: &str = r#"
[signals."lang:rust"]
kind = "stack"
meaning = "Rust crates"
detect = { files = ["**/Cargo.toml"] }

[signals."lang:ts"]
kind = "stack"
meaning = "TypeScript projects"
detect = { files = ["**/tsconfig.json"] }

[signals."lang:csharp"]
kind = "stack"
meaning = ".NET projects"
detect = { files = ["**/*.csproj"] }

[signals."framework:react"]
kind = "stack"
meaning = "React packages"
detect = { files = ["**/package.json"], manifest = ["dependencies.react", "devDependencies.react"] }

[signals."runtime:tokio"]
kind = "stack"
meaning = "tokio crates"
detect = { files = ["**/Cargo.toml"], manifest = ["dependencies.tokio", "workspace.dependencies.tokio"] }
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: PathBuf,
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = git_command(root)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl Fixture {
    /// A git sandbox and a catalogue whose vocabulary is `signals` plus the
    /// fixture concern.
    fn new(signals: &str) -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(builtin.join("lenses")).unwrap();
        fs::write(builtin.join("lenses/correctness.md"), CORRECTNESS).unwrap();
        let fixture = Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            builtin,
        };
        fixture.vocabulary(signals);
        Some(fixture)
    }

    fn vocabulary(&self, signals: &str) {
        fs::write(
            self.builtin.join("vocabulary.toml"),
            format!("{signals}\n{CONCERNS}"),
        )
        .unwrap();
    }

    fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        self
    }

    fn remove(&self, rel: &str) -> &Self {
        fs::remove_file(self.root.join(rel)).unwrap();
        self
    }

    /// Stages everything and commits; returns the new HEAD.
    fn commit(&self, message: &str) -> String {
        git(&self.root, &["add", "-A"]);
        git(&self.root, &["commit", "-q", "-m", message]);
        git(&self.root, &["rev-parse", "HEAD"])
    }

    fn tomlctl(&self) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    fn start(&self, base: &str) -> String {
        let out = self
            .tomlctl()
            .args([
                "review",
                "run",
                "start",
                "--scope",
                SCOPE,
                "--command",
                "review-v2",
                "--family",
                "code",
                "--depth",
                "quick",
                &format!("--base={base}"),
                "--builtin",
            ])
            .arg(&self.builtin)
            .assert()
            .success();
        let json: JsonValue =
            serde_json::from_slice(&out.get_output().stdout).expect("start prints JSON");
        json["run"].as_str().unwrap().to_string()
    }

    fn survey(&self, run: &str) -> JsonValue {
        let out = self
            .tomlctl()
            .args(["review", "survey", "--scope", SCOPE, "--run", run])
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("survey prints JSON")
    }

    fn stored_signals(&self) -> toml::Value {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        let doc: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        doc["runs"][0]
            .get("signals")
            .cloned()
            .unwrap_or(toml::Value::Array(Vec::new()))
    }
}

/// The roots `survey` printed for a true signal; `None` when it was not true.
fn roots(out: &JsonValue, name: &str) -> Option<Vec<String>> {
    out["signals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .map(|s| {
            s["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().to_string())
                .collect()
        })
}

fn unknown_reason(out: &JsonValue, name: &str) -> Option<String> {
    out["unknown"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .map(|s| s["reason"].as_str().unwrap().to_string())
}

fn stored(signals: &toml::Value, name: &str) -> Option<toml::Value> {
    signals
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"].as_str() == Some(name))
        .cloned()
}

#[test]
fn stack_signal_found_at_depth_three_with_roots() {
    let Some(fx) = Fixture::new(STACKS) else {
        return;
    };
    fx.write(
        "crates/inner/deep/Cargo.toml",
        "[package]\nname = \"deep\"\n\n[dependencies]\ntokio = \"1\"\n",
    )
    .write("crates/inner/deep/src/lib.rs", "pub fn f() {}\n")
    .write("README.md", "readme\n");
    let head = fx.commit("crate");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "lang:rust"),
        Some(vec!["crates/inner/deep".to_string()])
    );
    assert_eq!(
        roots(&out, "runtime:tokio"),
        Some(vec!["crates/inner/deep".to_string()]),
        "the manifest key is read from a Cargo.toml three levels down"
    );
    assert_eq!(roots(&out, "lang:ts"), None);
    let rust = stored(&fx.stored_signals(), "lang:rust").expect("lang:rust is stored");
    assert_eq!(rust["source"].as_str(), Some("detector"));
    assert_eq!(
        rust["roots"].as_array().unwrap()[0].as_str(),
        Some("crates/inner/deep")
    );
}

#[test]
fn central_package_management_detects_efcore() {
    let vocabulary = r#"
[signals."db:efcore"]
kind = "stack"
meaning = "Entity Framework Core"
detect = { files = ["**/*.csproj", "**/Directory.Packages.props", "**/packages.config"], contains = ['Microsoft\.EntityFrameworkCore'] }

[signals."db:dapper"]
kind = "stack"
meaning = "Dapper"
detect = { files = ["**/*.csproj", "**/Directory.Packages.props", "**/packages.config"], contains = ['(Include|id)="(Dapper|Microsoft\.Data\.SqlClient|System\.Data\.SqlClient)[",. ]'] }
"#;
    let Some(fx) = Fixture::new(vocabulary) else {
        return;
    };
    fx.write(
        "Directory.Packages.props",
        "<Project>\r\n  <PropertyGroup>\r\n    <ManagePackageVersionsCentrally>true</ManagePackageVersionsCentrally>\r\n  </PropertyGroup>\r\n  <ItemGroup>\r\n    <PackageVersion Include=\"Microsoft.EntityFrameworkCore.SqlServer\" Version=\"8.0.0\" />\r\n    <PackageVersion Include=\"Serilog\" Version=\"3.1.1\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    )
    .write(
        "src/App/App.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\">\r\n  <ItemGroup>\r\n    <PackageReference Include=\"Serilog\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    );
    let head = fx.commit("cpm");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "db:efcore"),
        Some(vec![".".to_string()]),
        "the version lives only in the root Directory.Packages.props"
    );
    assert_eq!(roots(&out, "db:dapper"), None);
}

#[test]
fn polyglot_repo_records_separate_roots_per_stack() {
    let Some(fx) = Fixture::new(STACKS) else {
        return;
    };
    fx.write("server/Cargo.toml", "[package]\nname = \"server\"\n")
        .write("server/src/main.rs", "fn main() {}\n")
        .write(
            "web/package.json",
            r#"{"name": "web", "dependencies": {"react": "^18.0.0"}}"#,
        )
        .write("web/tsconfig.json", "{}\n")
        .write(
            "tools/package.json",
            r#"{"name": "tools", "dependencies": {"left-pad": "1"}}"#,
        )
        .write("api/Api.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\" />\n");
    let head = fx.commit("polyglot");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(roots(&out, "lang:rust"), Some(vec!["server".to_string()]));
    assert_eq!(roots(&out, "lang:ts"), Some(vec!["web".to_string()]));
    assert_eq!(roots(&out, "lang:csharp"), Some(vec!["api".to_string()]));
    assert_eq!(
        roots(&out, "framework:react"),
        Some(vec!["web".to_string()]),
        "tools/package.json has no react dependency"
    );
    assert_eq!(roots(&out, "runtime:tokio"), None);
}

#[test]
fn health_count_uses_per_stack_pattern_and_threshold() {
    let vocabulary = format!(
        "{STACKS}{}",
        r##"
[signals.test-ratio-high]
kind = "health"
meaning = "tests per kloc at HEAD"
[signals.test-ratio-high.detect]
threshold = { per_kloc = 250 }
count."lang:rust" = { files = ["**/*.rs"], patterns = ["#[test]"] }
count."lang:ts" = { files = ["**/*.ts"], patterns = ["\ntest("] }
count."lang:csharp" = { files = ["**/*.cs"], patterns = ["[Fact"] }

[signals.test-growth]
kind = "health"
meaning = "tests per kloc added in the window"
[signals.test-growth.detect]
window = 90
threshold = { per_kloc = 250 }
count."lang:rust" = { files = ["**/*.rs"], patterns = ["#[test]"] }
count."lang:ts" = { files = ["**/*.ts"], patterns = ["\ntest("] }

[signals.ts-tests]
kind = "health"
meaning = "at least two TypeScript tests"
[signals.ts-tests.detect]
threshold = 2
count."lang:ts" = { files = ["**/*.ts"], patterns = ["\ntest("] }
"##
    );
    let Some(fx) = Fixture::new(&vocabulary) else {
        return;
    };
    // core: 3 tests in 10 lines (300/kloc); ui: 1 test in 10 lines (100/kloc);
    // legacy: 1000/kloc of [Fact], but no .csproj makes lang:csharp false.
    fx.write("core/Cargo.toml", "[package]\nname = \"core\"\n")
        .write(
            "core/src/lib.rs",
            "#[test]\nfn a() {}\n#[test]\nfn b() {}\n#[test]\nfn c() {}\n\n\n\n// end\n",
        )
        .write("ui/tsconfig.json", "{}\n")
        .write(
            "ui/a.test.ts",
            "test('a', () => {});\nlatest();\nlatest();\n\n\n\n\n\n\n// end\n",
        )
        .write(
            "legacy/Tests.cs",
            "[Fact]\n[Fact]\n[Fact]\n[Fact]\n[Fact]\n",
        );
    let head = fx.commit("tests");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "test-ratio-high"),
        Some(vec!["core".to_string()]),
        "only the rust stack reaches 250/kloc; the csharp entry is skipped without its stack"
    );
    assert_eq!(
        roots(&out, "test-growth"),
        Some(vec!["core".to_string()]),
        "lines added in the window carry the same per-stack counts"
    );
    assert_eq!(
        roots(&out, "ts-tests"),
        None,
        "one TypeScript test is under the integer threshold of 2"
    );
}

#[test]
fn count_pattern_matches_literally() {
    let vocabulary = r#"
[signals.dotted]
kind = "health"
meaning = "a.b twice"
[signals.dotted.detect]
threshold = 2
count."*" = { files = ["notes/*.txt"], patterns = ["a.b"] }

[signals.line-start]
kind = "health"
meaning = "foo( at a line start once"
[signals.line-start.detect]
threshold = 1
count."*" = { files = ["notes/*.txt"], patterns = ["\nfoo("] }

[signals.line-start-twice]
kind = "health"
meaning = "foo( at a line start twice"
[signals.line-start-twice.detect]
threshold = 2
count."*" = { files = ["notes/*.txt"], patterns = ["\nfoo("] }
"#;
    let Some(fx) = Fixture::new(vocabulary) else {
        return;
    };
    fx.write("notes/n.txt", "axb axb a.b\nfoo(\r\nxfoo(\n foo(\n");
    let head = fx.commit("notes");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "dotted"),
        None,
        "`.` is a literal dot, so only one `a.b` counts"
    );
    assert_eq!(
        roots(&out, "line-start"),
        Some(vec!["notes".to_string()]),
        "a leading `\\n` matches the first line's start"
    );
    assert_eq!(
        roots(&out, "line-start-twice"),
        None,
        "`xfoo(` and ` foo(` are not at a line start"
    );
}

#[test]
fn diff_signal_from_base_range() {
    let vocabulary = r#"
[signals.rename-or-removal]
kind = "diff"
meaning = "a removed pub fn"
detect = { contains = ['^-[ \t]*pub fn'] }

[signals.docs-touched]
kind = "diff"
meaning = "docs"
detect = { files = ["**/*.md"] }

[signals.tests-touched]
kind = "diff"
meaning = "tests"
detect = { files = ["**/tests/**"] }

[signals.file-deleted]
kind = "diff"
meaning = "a deleted file"
detect = { contains = ['^deleted file mode '] }
"#;
    let Some(fx) = Fixture::new(vocabulary) else {
        return;
    };
    fx.write("src/lib.rs", "pub fn old() {}\npub fn keep() {}\n")
        .write("tests/t.rs", "#[test]\nfn t() {}\n")
        .write("tools/gone.txt", "bye\n");
    let base = fx.commit("first");
    fx.write("src/lib.rs", "pub fn keep() {}\n")
        .write("docs/readme.md", "# docs\n")
        .remove("tools/gone.txt");
    fx.commit("second");
    let run = fx.start(&base);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "rename-or-removal"),
        Some(vec!["src".to_string()])
    );
    assert_eq!(roots(&out, "docs-touched"), Some(vec!["docs".to_string()]));
    assert_eq!(
        roots(&out, "file-deleted"),
        Some(vec!["tools".to_string()]),
        "patch header lines are matched"
    );
    assert_eq!(
        roots(&out, "tests-touched"),
        None,
        "tests/t.rs changed only before the base"
    );
}

#[test]
fn commits_per_file_filters_by_subject_and_threshold() {
    let vocabulary = r#"
[signals.churn-hotspot]
kind = "health"
meaning = "touched by two commits"
detect = { files = ["**/*.rs"], window = 90, threshold = 2 }

[signals.finding-hotspot]
kind = "health"
meaning = "touched by two fix commits"
detect = { files = ["**/*.rs"], window = 90, threshold = 2, contains = ['^(?i-u:fix)(\(|!|:)'] }
"#;
    let Some(fx) = Fixture::new(vocabulary) else {
        return;
    };
    fx.write("a/hot.rs", "1\n").write("b/cold.rs", "1\n");
    fx.commit("feat: start");
    fx.write("a/hot.rs", "2\n");
    fx.commit("fix: one");
    fx.write("a/hot.rs", "3\n").write("b/cold.rs", "2\n");
    let head = fx.commit("Fix(core): two");
    let run = fx.start(&head);
    let out = fx.survey(&run);

    assert_eq!(
        roots(&out, "churn-hotspot"),
        Some(vec!["a".to_string(), "b".to_string()])
    );
    assert_eq!(
        roots(&out, "finding-hotspot"),
        Some(vec!["a".to_string()]),
        "b/cold.rs has one fix commit"
    );
    let churn = out["signals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "churn-hotspot")
        .unwrap()
        .clone();
    assert_eq!(
        churn["evidence"],
        serde_json::json!(["a/hot.rs", "b/cold.rs"]),
        "the evidence lists files by commit count, most first"
    );
}

#[test]
fn failed_detector_records_unknown() {
    let Some(fx) = Fixture::new(STACKS) else {
        return;
    };
    fx.write("Cargo.toml", "[package]\nname = \"x\"\n")
        .write("web/package.json", "{ \"dependencies\": { \"react\": ")
        .write("tools/package.json", r#"{"dependencies": {}}"#);
    let head = fx.commit("broken manifest");
    let run = fx.start(&head);
    // Written after the start, which would refuse it: the survey must still
    // evaluate every other detector.
    fx.vocabulary(&format!(
        "{STACKS}\n[signals.bad]\nkind = \"diff\"\nmeaning = \"broken\"\ndetect = {{ contains = ['('] }}\n"
    ));
    let out = fx.survey(&run);

    assert_eq!(roots(&out, "lang:rust"), Some(vec![".".to_string()]));
    let react = unknown_reason(&out, "framework:react").expect("react is unknown, not false");
    assert!(react.contains("web/package.json"), "{react}");
    assert!(react.contains("JSON"), "{react}");
    let bad = unknown_reason(&out, "bad").expect("a bad regex is unknown, not false");
    assert!(bad.contains("does not compile"), "{bad}");

    let signals = fx.stored_signals();
    let stored_react = stored(&signals, "framework:react").expect("an unknown signal is stored");
    assert_eq!(stored_react["source"].as_str(), Some("unknown"));
    assert!(stored_react.get("roots").is_none());
    assert!(
        stored(&signals, "lang:ts").is_none(),
        "a false signal is not stored"
    );
}

#[test]
fn survey_rerun_is_idempotent_and_keeps_judged_signals() {
    let Some(fx) = Fixture::new(STACKS) else {
        return;
    };
    fx.write("Cargo.toml", "[package]\nname = \"x\"\n");
    let head = fx.commit("crate");
    let run = fx.start(&head);
    fx.tomlctl()
        .args([
            "review",
            "run",
            "update",
            &run,
            "--scope",
            SCOPE,
            "--set-json",
        ])
        .arg(r#"signals=[{"name": "trust-boundary", "evidence": ["src/io.rs"]}]"#)
        .assert()
        .success();

    let first_out = fx.survey(&run);
    let first = fx.stored_signals();
    let second_out = fx.survey(&run);
    let second = fx.stored_signals();
    assert_eq!(first, second, "a second survey writes the same signals");
    assert_eq!(first_out["signals"], second_out["signals"]);
    assert_eq!(
        first
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["name"].as_str() == Some("lang:rust"))
            .count(),
        1
    );
    let judged = stored(&second, "trust-boundary").expect("the judged candidate survives");
    assert_eq!(judged["source"].as_str(), Some("judged"));

    fx.tomlctl()
        .args([
            "review",
            "run",
            "update",
            &run,
            "--scope",
            SCOPE,
            "--set-json",
        ])
        .arg(r#"signals=[{"name": "lang:rust", "accepted": true}]"#)
        .assert()
        .failure();
}

#[test]
fn survey_refuses_an_unknown_run() {
    let Some(fx) = Fixture::new(STACKS) else {
        return;
    };
    fx.write("Cargo.toml", "[package]\nname = \"x\"\n");
    let head = fx.commit("crate");
    fx.start(&head);
    fx.tomlctl()
        .args(["review", "survey", "--scope", SCOPE, "--run", "U9"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no run U9"));
}
