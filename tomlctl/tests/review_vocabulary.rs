//! `tomlctl review survey` with the shipped vocabulary as the built-in layer,
//! over sandbox repos shaped like the stacks it targets: Rust workspaces, Bun
//! and npm workspaces with React or Vue, and .NET solutions using central
//! package management or legacy references. A detector that drifts from those
//! layouts fails here rather than in a live review.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

/// The shipped catalogue, read in place so the test exercises the vocabulary
/// the catalogue gate validates.
fn shipped_catalogue() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join("claude/skills/review-v2-lenses")
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let catalogue = shipped_catalogue();
        let vocabulary = catalogue.join("vocabulary.toml");
        assert!(
            vocabulary.is_file(),
            "the shipped vocabulary is missing at {}",
            vocabulary.display()
        );
        let (dir, root) = sandbox();
        Some(Self { _dir: dir, root })
    }

    fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        self
    }

    fn git(&self, args: &[&str]) -> String {
        let out = git_command(&self.root)
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

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn tomlctl(&self) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    /// Commits the tree, starts a run based at that commit against the
    /// shipped catalogue, and returns the survey's JSON.
    fn survey(&self) -> JsonValue {
        let head = self.commit("fixture");
        let started = self
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
                &format!("--base={head}"),
                "--builtin",
            ])
            .arg(shipped_catalogue())
            .assert()
            .success();
        let started: JsonValue =
            serde_json::from_slice(&started.get_output().stdout).expect("start prints JSON");
        let run = started["run"].as_str().unwrap().to_string();
        let out = self
            .tomlctl()
            .args(["review", "survey", "--scope", SCOPE, "--run", &run])
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("survey prints JSON")
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

fn unknown(out: &JsonValue, name: &str) -> Option<String> {
    out["unknown"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .map(|s| s["reason"].as_str().unwrap_or_default().to_string())
}

fn one(root: &str) -> Option<Vec<String>> {
    Some(vec![root.to_string()])
}

#[test]
fn rust_crate_at_depth_three_detected_with_root() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write(
        "crates/engine/core/Cargo.toml",
        "[package]\nname = \"core\"\nedition = \"2024\"\n\n[dependencies]\ntokio = { version = \"1\", features = [\"rt\"] }\n",
    )
    .write("crates/engine/core/src/lib.rs", "pub fn f() {}\n")
    .write("README.md", "# workspace\n");
    let out = fx.survey();

    assert_eq!(roots(&out, "lang:rust"), one("crates/engine/core"));
    assert_eq!(
        roots(&out, "runtime:tokio"),
        one("crates/engine/core"),
        "a table-form dependency counts as the manifest key"
    );
    assert_eq!(roots(&out, "lang:ts"), None);
    assert_eq!(roots(&out, "lang:csharp"), None);
    assert_eq!(unknown(&out, "lang:rust"), None);
}

#[test]
fn efcore_detected_through_central_package_management() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write(
        "Directory.Packages.props",
        "<Project>\r\n  <PropertyGroup>\r\n    <ManagePackageVersionsCentrally>true</ManagePackageVersionsCentrally>\r\n  </PropertyGroup>\r\n  <ItemGroup>\r\n    <PackageVersion Include=\"Microsoft.EntityFrameworkCore.SqlServer\" Version=\"8.0.4\" />\r\n    <PackageVersion Include=\"Serilog.AspNetCore\" Version=\"8.0.1\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    )
    .write(
        "src/Shop.Api/Shop.Api.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk.Web\">\r\n  <PropertyGroup>\r\n    <TargetFramework>net8.0</TargetFramework>\r\n  </PropertyGroup>\r\n  <ItemGroup>\r\n    <PackageReference Include=\"Serilog.AspNetCore\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    )
    .write("src/Shop.Api/Program.cs", "var app = WebApplication.Create();\r\n");
    let out = fx.survey();

    assert_eq!(
        roots(&out, "db:efcore"),
        one("."),
        "the package is named only in the root Directory.Packages.props"
    );
    assert_eq!(roots(&out, "lang:csharp"), one("src/Shop.Api"));
    assert_eq!(roots(&out, "db:dapper"), None);
}

#[test]
fn react_detected_in_bun_workspace() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write(
        "package.json",
        r#"{
  "name": "monorepo",
  "private": true,
  "workspaces": ["apps/*", "packages/*"],
  "devDependencies": { "typescript": "^5.6.0" }
}
"#,
    )
    .write("bun.lock", "{\n  \"lockfileVersion\": 1\n}\n")
    .write(
        "apps/dashboard/package.json",
        r#"{
  "name": "@mono/dashboard",
  "type": "module",
  "dependencies": { "react": "^19.0.0", "react-dom": "^19.0.0", "@mono/shared": "workspace:*" }
}
"#,
    )
    .write(
        "apps/dashboard/tsconfig.json",
        "{ \"compilerOptions\": {} }\n",
    )
    .write(
        "packages/shared/package.json",
        r#"{ "name": "@mono/shared", "peerDependencies": { "zod": "^3" } }
"#,
    );
    let out = fx.survey();

    assert_eq!(
        roots(&out, "framework:react"),
        one("apps/dashboard"),
        "neither the workspace root nor the shared package depends on react"
    );
    assert_eq!(roots(&out, "lang:ts"), one("apps/dashboard"));
    assert_eq!(roots(&out, "framework:vue"), None);
    assert_eq!(unknown(&out, "framework:react"), None);
}

#[test]
fn dapper_detected_through_legacy_reference() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write(
        "Legacy/Orders.Data/Orders.Data.csproj",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Project ToolsVersion=\"15.0\" xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\r\n  <ItemGroup>\r\n    <Reference Include=\"Dapper, Version=2.0.0.0, Culture=neutral, processorArchitecture=MSIL\">\r\n      <HintPath>..\\packages\\Dapper.2.0.123\\lib\\net461\\Dapper.dll</HintPath>\r\n    </Reference>\r\n    <Reference Include=\"System.Data\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    )
    .write(
        "Legacy/Orders.Web/Orders.Web.csproj",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Project ToolsVersion=\"15.0\" xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\r\n  <ItemGroup>\r\n    <Reference Include=\"DapperExtensions.Helpers\" />\r\n  </ItemGroup>\r\n</Project>\r\n",
    );
    let out = fx.survey();

    assert_eq!(
        roots(&out, "db:dapper"),
        one("Legacy/Orders.Data"),
        "a package whose name only starts with Dapper is not Dapper"
    );
    assert_eq!(roots(&out, "db:efcore"), None);
    assert_eq!(
        roots(&out, "lang:csharp"),
        Some(vec![
            "Legacy/Orders.Data".to_string(),
            "Legacy/Orders.Web".to_string()
        ])
    );
}

#[test]
fn vue_root_is_its_subdirectory_only() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"server\"\nedition = \"2024\"\n",
    )
    .write("src/main.rs", "fn main() {}\n")
    .write(
        "web/package.json",
        r#"{
  "name": "web",
  "dependencies": { "vue": "^3.5.0" },
  "devDependencies": { "vite": "^6.0.0", "vue-tsc": "^2.1.0" }
}
"#,
    )
    .write("web/tsconfig.json", "{}\n")
    .write("web/tsconfig.app.json", "{}\n")
    .write("web/src/App.vue", "<template><div /></template>\n");
    let out = fx.survey();

    assert_eq!(roots(&out, "framework:vue"), one("web"));
    assert_eq!(roots(&out, "lang:rust"), one("."));
    assert_eq!(
        roots(&out, "lang:ts"),
        one("web"),
        "two tsconfigs in one directory give one root"
    );
    assert_eq!(roots(&out, "framework:react"), None);
}

#[test]
fn rust_test_growth_counts_test_attributes() {
    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write("Cargo.toml", "[package]\nname = \"lib\"\n").write(
        "src/lib.rs",
        "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() {}\n}\n",
    );
    let out = fx.survey();
    assert_eq!(
        roots(&out, "test-growth"),
        one("."),
        "one #[test] among the lines added in the window clears 40 per kloc"
    );

    let Some(fx) = Fixture::new() else {
        return;
    };
    fx.write("Cargo.toml", "[package]\nname = \"lib\"\n")
        .write(
            "src/lib.rs",
            "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    #[testing]\n    #[test_case(1)]\n    fn a() {}\n}\n",
        );
    let out = fx.survey();
    assert_eq!(roots(&out, "lang:rust"), one("."));
    assert_eq!(
        roots(&out, "test-growth"),
        None,
        "attributes that only start with #[test are not counted"
    );
    assert_eq!(unknown(&out, "test-growth"), None);
}
