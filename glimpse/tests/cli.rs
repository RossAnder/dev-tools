//! Binary-level contracts: exit codes, which stream output lands on, and what each
//! subcommand writes to disk, asserted against the real process.
//!
//! Every case runs in its own temp sandbox: the Claude config directory, `CODEX_HOME`, the
//! glimpse config, herdr's config and the home directories all point inside it, and the
//! herdr pane variables are removed, so no case reads or writes the user's real files.
//! `CODEX_HOME` is left uncreated unless a case creates it. The sandbox
//! names a tomlctl and a herdr that do not exist, so neither installed binary ever runs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/snapshot.json");

/// A per-case directory tree, removed on drop.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(case: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("glimpse-cli-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["claude", "home", "appdata", "xdg", "cwd"] {
            std::fs::create_dir_all(dir.join(sub)).expect("the sandbox is writable");
        }
        std::fs::write(
            dir.join("glimpse.toml"),
            "tomlctl = \"glimpse-test-no-such-tomlctl\"\n",
        )
        .expect("the sandbox config is writable");
        Sandbox { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// Runs the binary in the sandbox's `cwd`, writes `stdin` and closes it so the
    /// child sees EOF.
    fn run(&self, argv: &[&str], stdin: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_glimpse"));
        cmd.args(argv)
            .current_dir(self.path("cwd"))
            .env("CLAUDE_CONFIG_DIR", self.path("claude"))
            .env("CODEX_HOME", self.path("codex"))
            .env("GLIMPSE_CONFIG", self.path("glimpse.toml"))
            .env("APPDATA", self.path("appdata"))
            .env("HERDR_CONFIG_PATH", self.path("herdr/config.toml"))
            .env("XDG_CONFIG_HOME", self.path("xdg"))
            .env("USERPROFILE", self.path("home"))
            .env("HOME", self.path("home"))
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_ACTIVE_PANE_ID")
            // Set rather than removed: unset falls back to the `herdr` on PATH, which a
            // regressed `setup --dry-run` would ask to reload its config.
            .env("HERDR_BIN_PATH", self.path("no-such-herdr"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("the binary spawns");
        {
            let mut pipe = child.stdin.take().expect("stdin was piped");
            pipe.write_all(stdin.as_bytes())
                .expect("stdin accepts the input");
        }
        child.wait_with_output().expect("the binary exits")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is UTF-8")
}

/// Every file under `dir`, relative to it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn help_exits_zero_on_stdout() {
    let sb = Sandbox::new("help");
    let out = sb.run(&["--help"], "");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("USAGE"), "{}", stdout(&out));
    assert!(stderr(&out).is_empty());
}

#[test]
fn an_unknown_flag_is_a_usage_error() {
    let sb = Sandbox::new("unknown");
    let out = sb.run(&["--no-such-flag"], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("--no-such-flag"), "{}", stderr(&out));
}

/// With a tomlctl that cannot run, the hook fails, and that failure must reach the log
/// and nowhere else.
#[test]
fn hook_is_silent_and_logs_one_line() {
    let sb = Sandbox::new("hook");
    let out = sb.run(&["hook"], "");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    let log = std::fs::read_to_string(sb.path("claude/glimpse/hook.log"))
        .expect("the hook wrote its log in the sandbox");
    assert_eq!(log.lines().count(), 1, "{log}");
}

#[test]
fn once_renders_every_task_of_a_snapshot_file() {
    let sb = Sandbox::new("once");
    let out = sb.run(&["--once", "--snapshot", FIXTURE, "--size", "100x30"], "");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    // Each id paired with its title's first word, so a stray digit elsewhere in the
    // frame cannot stand in for a task row.
    for row in [
        "1 Scaffold",
        "2 Define",
        "3 Load",
        "4 Render",
        "5 Style",
        "6 Bind",
        "7 Assemble",
        "8 Wire",
    ] {
        assert!(text.contains(row), "`{row}` missing:\n{text}");
    }
    assert!(!text.contains('\u{1b}'), "no escape sequences");
}

#[test]
fn once_renders_the_diagram_view() {
    let sb = Sandbox::new("diagram");
    let out = sb.run(
        &[
            "--once",
            "--snapshot",
            FIXTURE,
            "--view",
            "diagram",
            "--size",
            "100x30",
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("[1]"), "{}", stdout(&out));
}

#[test]
fn setup_dry_run_writes_nothing() {
    let sb = Sandbox::new("setup");
    let before = files_under(&sb.dir);
    let out = sb.run(&["setup", "--dry-run"], "");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let text = stdout(&out);
    assert!(text.contains("nothing written"), "{text}");
    assert!(text.contains("Codex hooks: codex not found at"), "{text}");
    assert!(!sb.path("claude/settings.json").exists());
    assert!(!sb.path("herdr/config.toml").exists());
    assert!(!sb.path("codex").exists());
    assert_eq!(files_under(&sb.dir), before);
}

/// The lines of `setup`'s report from the Codex heading up to the next section.
fn codex_section(report: &str) -> String {
    report
        .lines()
        .skip_while(|l| !l.starts_with("Codex hooks:"))
        .take_while(|l| !l.starts_with("herdr config:"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn setup_dry_run_plans_the_codex_hooks_when_codex_exists() {
    let sb = Sandbox::new("setup-codex");
    std::fs::create_dir_all(sb.path("codex")).expect("the sandbox is writable");
    let before = files_under(&sb.dir);
    let out = sb.run(&["setup", "--dry-run"], "");
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}{}", stderr(&out));
    let codex = codex_section(&text);
    for event in ["SubagentStart", "SubagentStop"] {
        assert!(
            codex.contains(&format!("would add async `{event}` hook: ")),
            "{codex}"
        );
    }
    assert!(codex.contains("hook --harness codex"), "{codex}");
    assert!(!codex.contains("TeammateIdle"), "{codex}");
    assert!(text.contains("only once it is trusted"), "{text}");
    assert!(!sb.path("codex/hooks.json").exists());
    assert_eq!(files_under(&sb.dir), before);
}

#[test]
fn setup_dry_run_leaves_an_installed_codex_hook_alone() {
    let sb = Sandbox::new("setup-codex-installed");
    std::fs::create_dir_all(sb.path("codex")).expect("the sandbox is writable");
    let hook = r#"[{"hooks": [{"type": "command", "command": "glimpse hook --harness codex", "async": true}]}]"#;
    let existing = format!(r#"{{"hooks": {{"SubagentStart": {hook}, "SubagentStop": {hook}}}}}"#);
    std::fs::write(sb.path("codex/hooks.json"), &existing).expect("the sandbox is writable");
    let out = sb.run(&["setup", "--dry-run"], "");
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}{}", stderr(&out));
    let codex = codex_section(&text);
    assert!(codex.contains("already installed"), "{codex}");
    assert!(!codex.contains("would add"), "{codex}");
    assert!(!text.contains("only once it is trusted"), "{text}");
    assert_eq!(
        std::fs::read_to_string(sb.path("codex/hooks.json")).expect("still there"),
        existing
    );
}

#[test]
fn a_codex_hook_is_silent_and_logs_one_line() {
    let sb = Sandbox::new("hook-codex");
    let out = sb.run(&["hook", "--harness", "codex"], "{}");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    let log = std::fs::read_to_string(sb.path("claude/glimpse/hook.log"))
        .expect("the hook wrote its log in the sandbox");
    assert_eq!(log.lines().count(), 1, "{log}");
}

/// A mistyped hook entry in a harness config must land in the log like any other failure.
#[test]
fn a_hook_usage_error_is_logged_not_printed() {
    let sb = Sandbox::new("hook-usage");
    let out = sb.run(&["hook", "--harness", "manual"], "");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    let log = std::fs::read_to_string(sb.path("claude/glimpse/hook.log"))
        .expect("the hook wrote its log in the sandbox");
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("unknown harness `manual`"), "{log}");
}

#[test]
fn ensure_pane_outside_herdr_fails_with_a_message() {
    let sb = Sandbox::new("ensure");
    let out = sb.run(&["ensure-pane"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("herdr"), "{}", stderr(&out));
}
