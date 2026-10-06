//! Binary-level contracts: exit codes, which stream output lands on, and what each
//! subcommand writes to disk, asserted against the real process.
//!
//! Every case runs in its own temp sandbox: the Claude config directory, `CODEX_HOME`, the
//! glimpse config, herdr's config and the home directories all point inside it, and the
//! herdr pane variables are removed, so no case reads or writes the user's real files.
//! `CODEX_HOME` is left uncreated unless a case creates it. The sandbox names a herdr that
//! does not exist and removes `TOMLCTL_ROOT`, so no installed herdr runs and the in-process
//! recorder resolves flows from the sandbox's `cwd` alone.

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
        std::fs::write(dir.join("glimpse.toml"), "").expect("the sandbox config is writable");
        Sandbox { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// Runs the binary in the sandbox's `cwd`, writes `stdin` and closes it so the
    /// child sees EOF.
    fn run(&self, argv: &[&str], stdin: &str) -> Output {
        Self::finish(self.command(argv), stdin)
    }

    /// As [`Sandbox::run`] but in the sandbox directory `cwd`, with `PATH` naming only an
    /// empty directory, so the binary can start no other program by name.
    fn run_without_path(&self, cwd: &str, argv: &[&str], stdin: &str) -> Output {
        let bin = self.path("empty-bin");
        std::fs::create_dir_all(&bin).expect("the sandbox is writable");
        let mut cmd = self.command(argv);
        cmd.env("PATH", bin).current_dir(self.path(cwd));
        Self::finish(cmd, stdin)
    }

    /// The binary, by absolute path, with every config location inside the sandbox.
    fn command(&self, argv: &[&str]) -> Command {
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
            .env_remove("TOMLCTL_ROOT")
            // Set rather than removed: unset falls back to the `herdr` on PATH, which a
            // regressed `setup --dry-run` would ask to reload its config.
            .env("HERDR_BIN_PATH", self.path("no-such-herdr"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn finish(mut cmd: Command, stdin: &str) -> Output {
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

/// An empty payload is not JSON, so the hook fails, and that failure must reach the log
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

const REVIEW_LEDGER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/review-ledger.toml"
);

/// The sandbox holds no flow, so the frame can come only from the ledger file, over an
/// empty task graph.
#[test]
fn once_renders_a_review_ledger() {
    let sb = Sandbox::new("once-ledger");
    let out = sb.run_without_path(
        "cwd",
        &["--once", "--ledger", REVIEW_LEDGER, "--size", "120x30"],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("group none · sort id · closed hidden"),
        "{text}"
    );
    assert!(
        text.contains("R1 critical correctness"),
        "an open finding is listed:\n{text}"
    );
    assert!(!text.contains("R3 "), "a fixed finding is hidden:\n{text}");
    assert!(!text.contains('\u{1b}'), "no escape sequences");
}

#[test]
fn once_groups_sorts_and_lists_closed_rows() {
    let sb = Sandbox::new("once-arrange");
    let out = sb.run_without_path(
        "cwd",
        &[
            "--once",
            "--ledger",
            REVIEW_LEDGER,
            "--group",
            "severity",
            "--sort",
            "newest",
            "--closed",
            "--size",
            "120x30",
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("group severity · sort newest · closed shown"),
        "{text}"
    );
    assert!(text.contains("▾ critical (1)"), "a group header:\n{text}");
    assert!(text.contains("R3 "), "a fixed finding is listed:\n{text}");
}

#[test]
fn a_group_the_surface_lacks_is_a_runtime_error() {
    let sb = Sandbox::new("once-arrange-bad");
    let out = sb.run_without_path(
        "cwd",
        &["--once", "--ledger", REVIEW_LEDGER, "--group", "area"],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("--group area: the review surface offers"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn ledger_without_once_is_a_usage_error() {
    let sb = Sandbox::new("ledger-no-once");
    let out = sb.run(&["--ledger", REVIEW_LEDGER], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(stderr(&out).contains("--once"), "{}", stderr(&out));
}

#[test]
fn a_ledger_of_unknown_kind_is_a_runtime_error() {
    let sb = Sandbox::new("ledger-unknown");
    let path = sb.path("cwd/notes.toml");
    std::fs::write(&path, "[[items]]\nid = \"R1\"\n").expect("the sandbox is writable");
    let out = sb.run(&["--once", "--ledger", "notes.toml"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("cannot infer the ledger kind"),
        "{}",
        stderr(&out)
    );
}

const LIVE_STORE: &str = "schema_version = 1\n\n[[items]]\nid = 1\nref = \"seed-the-store\"\ntitle = \"Seed the store\"\neffort = \"S\"\nstatus = \"pending\"\nfiles = [\"src/a.rs\"]\n";

/// The only case that reads a real flow: with no `PATH`, the frame can come only from
/// glimpse's own in-process snapshot of the store, and the root, one level above the
/// working directory, only from the ancestor walk, since `git` cannot start.
#[test]
fn once_renders_a_live_flow_in_process() {
    let sb = Sandbox::new("once-live");
    let flow = sb.path("cwd/.claude/flows/live-flow-demo");
    std::fs::create_dir_all(&flow).expect("the sandbox is writable");
    std::fs::create_dir_all(sb.path("cwd/nested")).expect("the sandbox is writable");
    std::fs::write(flow.join("tasks.toml"), LIVE_STORE).expect("the sandbox is writable");
    std::fs::write(flow.join("context.toml"), "status = \"in-progress\"\n")
        .expect("the sandbox is writable");
    let out = sb.run_without_path(
        "cwd/nested",
        &["--once", "--slug", "live-flow-demo", "--size", "100x30"],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("live-flow-demo"), "{text}");
    assert!(text.contains("1 Seed"), "{text}");
}

/// Writes a flow's `tasks.toml` and `context.toml` under the sandbox's `cwd`.
fn stage_flow(sb: &Sandbox, slug: &str, store: &str) -> PathBuf {
    let flow = sb.path(&format!("cwd/.claude/flows/{slug}"));
    std::fs::create_dir_all(&flow).expect("the sandbox is writable");
    std::fs::write(flow.join("tasks.toml"), store).expect("the sandbox is writable");
    std::fs::write(flow.join("context.toml"), "status = \"in-progress\"\n")
        .expect("the sandbox is writable");
    flow
}

/// Without `--slug`, the flow comes from glimpse's in-process flow list: the one whose task
/// store changed last.
#[test]
fn once_without_a_slug_renders_the_freshest_flow() {
    let sb = Sandbox::new("once-freshest");
    let stale = stage_flow(
        &sb,
        "stale-flow-demo",
        &LIVE_STORE
            .replace("seed-the-store", "retire-the-store")
            .replace("Seed the store", "Retire the store"),
    );
    std::fs::File::options()
        .write(true)
        .open(stale.join("tasks.toml"))
        .and_then(|f| {
            f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100))
        })
        .expect("the stale store can be backdated");
    stage_flow(&sb, "live-flow-demo", LIVE_STORE);
    std::fs::create_dir_all(sb.path("cwd/nested")).expect("the sandbox is writable");
    let out = sb.run_without_path("cwd/nested", &["--once", "--size", "100x30"], "");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("live-flow-demo"), "{text}");
    assert!(text.contains("1 Seed"), "{text}");
    assert!(!text.contains("1 Retire"), "{text}");
}

/// A `SubagentStart` whose transcript names a task is recorded by the in-process recorder
/// into the flow's `agents.toml`, found from a payload `cwd` below the repo root. The
/// hand-made `.git` holds just what git itself requires of a repository, so the root is
/// found without a `git init`.
#[test]
fn a_hook_start_records_a_running_agent_in_process() {
    let sb = Sandbox::new("hook-record");
    let flow = stage_flow(&sb, "hook-flow-demo", LIVE_STORE);
    let git = sb.path("cwd/.git");
    for dir in ["objects", "refs"] {
        std::fs::create_dir_all(git.join(dir)).expect("the sandbox is writable");
    }
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("the sandbox is writable");
    std::fs::write(git.join("config"), "").expect("the sandbox is writable");
    let nested = sb.path("cwd/nested");
    std::fs::create_dir_all(&nested).expect("the sandbox is writable");

    // Claude Code's layout: the session transcript beside `<session>/subagents/`, which
    // holds the agent's transcript and its meta file.
    let projects = sb.path("claude/projects/p");
    let subagents = projects.join("s1").join("subagents");
    std::fs::create_dir_all(&subagents).expect("the sandbox is writable");
    std::fs::write(projects.join("s1.jsonl"), "").expect("the sandbox is writable");
    let dispatch = serde_json::json!({
        "parentUuid": null,
        "type": "user",
        "message": {
            "role": "user",
            "content": "DISPATCH: implement-deep\nFetch it: `tomlctl tasks show 1 --slug hook-flow-demo --with body,files,deps`"
        }
    });
    std::fs::write(subagents.join("agent-a1.jsonl"), format!("{dispatch}\n"))
        .expect("the sandbox is writable");
    std::fs::write(
        subagents.join("agent-a1.meta.json"),
        r#"{"agentType": "implement-deep", "description": "seed work"}"#,
    )
    .expect("the sandbox is writable");

    let payload = serde_json::json!({
        "hook_event_name": "SubagentStart",
        "session_id": "s1",
        "agent_id": "a1",
        "agent_type": "implement-deep",
        "transcript_path": projects.join("s1.jsonl").to_string_lossy(),
        "cwd": nested.to_string_lossy(),
    });
    let out = sb.run(&["hook"], &payload.to_string());
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    let log = sb.path("claude/glimpse/hook.log");
    assert!(
        !log.exists(),
        "{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    let agents = std::fs::read_to_string(flow.join("agents.toml"))
        .expect("the recorder wrote the flow's agents store");
    assert!(agents.contains("agent_id = \"a1\""), "{agents}");
    assert!(agents.contains("status = \"running\""), "{agents}");
    assert!(!nested.join(".claude").exists());
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
    let out = sb.run(&["hook", "--harness", "codex"], "not json");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    let log = std::fs::read_to_string(sb.path("claude/glimpse/hook.log"))
        .expect("the hook wrote its log in the sandbox");
    assert_eq!(log.lines().count(), 1, "{log}");
}

/// An event neither harness adapter handles is answered as not recorded: no failure to
/// log and no store created.
#[test]
fn a_hook_for_an_unsupported_event_records_nothing() {
    let sb = Sandbox::new("hook-unsupported");
    let out = sb.run(&["hook", "--harness", "codex"], "{}");
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).is_empty(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).is_empty(), "stderr: {}", stderr(&out));
    assert!(!sb.path("claude/glimpse/hook.log").exists());
    assert!(!sb.path("cwd/.claude").exists());
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
