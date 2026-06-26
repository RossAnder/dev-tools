//! CLI dispatch (Task 7; `--with-companion` co-launch added by Task 8).
//!
//! A clap `derive` `Cli` with an OPTIONAL subcommand. The bare invocation (no
//! subcommand) preserves the original behaviour — it starts the server via
//! `crate::app::serve`, optionally co-launching the `lumina-companion` binary
//! when `--with-companion` is set (a launcher convenience that lives entirely
//! in this cli layer; `app::serve`'s signature is untouched). `import-flow
//! <slug>` resolves the flow directory as `.claude/flows/<slug>/` (relative to
//! the CWD), opens a pool via `lumina_core::db::init`, imports the flow,
//! prints the summary, and returns.
//!
//! `main.rs` still only calls `cli::run()`; it is not edited by this task.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use clap::{Parser, Subcommand};

/// Default bind port for the lumina server, mirrored from `app.rs`'s private
/// `DEFAULT_PORT` (24817). Duplicated here as a local compile-time constant so
/// `init-hooks` can build the default ingest URL **offline** — it never
/// introspects a running server, and `app.rs` is outside this surface's edit
/// scope so its const cannot simply be re-exported. If the server default ever
/// changes, update both. The ingest URL is loopback-only, matching lumina's
/// loopback-only default bind (`DEFAULT_HOST = 127.0.0.1`).
const DEFAULT_BIND_PORT: u16 = 24817;

/// lumina — flow-tracking platform (vertical slice).
#[derive(Debug, Parser)]
#[command(name = "lumina", version, about)]
struct Cli {
    /// Co-launch the `lumina-companion` git-execution binary alongside the
    /// bare server invocation (ADR-0006 Step 1b launcher convenience). Only
    /// meaningful with NO subcommand; combining it with a subcommand is a
    /// hard error. The gate lives in code rather than clap's
    /// `args_conflicts_with_subcommands` (which would also reject the flag on
    /// the bare path in some arg orders — clap#5353).
    #[arg(long)]
    with_companion: bool,

    /// Explicit path to the companion binary. Defaults to the sibling of the
    /// current executable named `lumina-companion` (`.exe`-suffixed on
    /// Windows). Requires `--with-companion`.
    #[arg(long, value_name = "PATH", requires = "with_companion")]
    companion_bin: Option<PathBuf>,

    /// Spawn the in-process tokio SCHEDULER engine loop (focus 1C.3) alongside
    /// the bare server invocation. Equivalent to setting `LUMINA_SCHEDULER` — the
    /// scheduler spawns when EITHER is set; with neither, no scheduler task spawns
    /// (default-server + e2e behaviour). Only meaningful with NO subcommand;
    /// combining it with a subcommand is a hard error (mirroring
    /// `--with-companion`). The spawned loop starts ENABLED; the operator can
    /// toggle / scope / kill it at runtime via `POST /api/scheduler/control`.
    #[arg(long)]
    with_scheduler: bool,

    /// Optional subcommand. With none, lumina starts the axum server (the
    /// original default behaviour).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Import one `.claude/flows/<slug>/` flow directory into the DB.
    ImportFlow {
        /// The flow slug — resolved to `.claude/flows/<slug>/` under the CWD.
        slug: String,
    },

    /// Write (read-modify-merge) a SessionEnd http-hook into a project's
    /// `.claude/settings.json` so terminal sessions POST their transcript to
    /// lumina's ingest route. Runs entirely offline (pure FS + JSON; never
    /// introspects a running server). Re-running is idempotent.
    InitHooks {
        /// The ingest URL the http-hook POSTs to. Defaults to lumina's
        /// loopback default bind, `http://127.0.0.1:<DEFAULT_BIND_PORT>/api/sessions/ingest`.
        #[arg(long)]
        url: Option<String>,

        /// The project directory whose `.claude/settings.json` is written.
        /// Defaults to the current directory (`.`). The settings file is
        /// `<project-dir>/.claude/settings.json`.
        #[arg(long, default_value = ".")]
        project_dir: PathBuf,
    },
}

/// Parse args and dispatch. No subcommand → serve (optionally co-launching
/// the companion); `import-flow` → import; `init-hooks` → settings merge.
pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(msg) = companion_flag_conflict(&cli) {
        anyhow::bail!(msg);
    }
    if let Some(msg) = scheduler_flag_conflict(&cli) {
        anyhow::bail!(msg);
    }
    match cli.command {
        None => {
            // Spawn the companion BEFORE serve and hold the `Child` binding
            // across `serve().await` so it drops only when the server returns
            // — that drop is what fires `kill_on_drop` and terminates the
            // child on graceful exit. (An underscore-PREFIXED binding still
            // lives to end of scope; a bare `_` pattern would drop — and kill
            // — the companion immediately.)
            let _companion: Option<tokio::process::Child> = if cli.with_companion {
                Some(spawn_companion(cli.companion_bin)?)
            } else {
                None
            };
            crate::app::serve(cli.with_scheduler).await
        }
        Some(Command::ImportFlow { slug }) => import_flow_cmd(&slug).await,
        Some(Command::InitHooks { url, project_dir }) => {
            let url = url.unwrap_or_else(default_ingest_url);
            init_hooks_cmd(&project_dir, &url)
        }
    }
}

/// The in-code gate replacing clap's `args_conflicts_with_subcommands` (per
/// clap#5353 guidance): the companion flags are launcher conveniences for the
/// bare server invocation only. Returns the error message when a subcommand
/// is combined with either flag, `None` when the invocation is fine.
fn companion_flag_conflict(cli: &Cli) -> Option<String> {
    if cli.command.is_some() && (cli.with_companion || cli.companion_bin.is_some()) {
        Some(
            "--with-companion/--companion-bin apply only to the bare `lumina` server \
             invocation; they cannot be combined with a subcommand"
                .to_string(),
        )
    } else {
        None
    }
}

/// The `--with-scheduler` analogue of [`companion_flag_conflict`]: the scheduler
/// boot flag is a convenience for the bare server invocation only (it threads
/// into `app::serve`), so combining it with a subcommand — which never reaches
/// `serve` — is a hard error rather than a silently-ignored flag. Returns the
/// error message on conflict, `None` otherwise.
fn scheduler_flag_conflict(cli: &Cli) -> Option<String> {
    if cli.command.is_some() && cli.with_scheduler {
        Some(
            "--with-scheduler applies only to the bare `lumina` server invocation; it \
             cannot be combined with a subcommand"
                .to_string(),
        )
    } else {
        None
    }
}

/// The platform file name of the companion binary.
const fn companion_file_name() -> &'static str {
    if cfg!(windows) {
        "lumina-companion.exe"
    } else {
        "lumina-companion"
    }
}

/// Resolve the companion binary path: an explicit `--companion-bin` override
/// wins; otherwise the sibling of the current executable named
/// [`companion_file_name`] (cargo places workspace bins side by side under
/// `target/<profile>/`). A resolved-but-missing path is an actionable error —
/// the common cause is `cargo run -p lumina-server`, which builds only the
/// server bin, not the companion.
///
/// Pure: the caller injects `current_exe` and the `exists` probe so unit
/// tests need no real filesystem or executable.
fn resolve_companion_bin(
    override_bin: Option<PathBuf>,
    current_exe: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let (candidate, source) = match override_bin {
        Some(p) => (p, "from --companion-bin"),
        None => (
            current_exe.with_file_name(companion_file_name()),
            "sibling of the current executable",
        ),
    };
    if exists(&candidate) {
        Ok(candidate)
    } else {
        Err(format!(
            "companion binary not found at {} ({source}). Build it with `cargo build \
             --workspace --manifest-path lumina/Cargo.toml` (a bare `cargo run -p \
             lumina-server` does not build the lumina-companion bin), or pass an explicit \
             --companion-bin <PATH>.",
            candidate.display()
        ))
    }
}

/// Derive the port the companion should dial, mirroring `app::serve`'s bind
/// resolution: the `PORT` env var when set AND parseable, else the default
/// (24817). An unparseable set value falls back exactly like `app.rs`'s
/// `parse_env_or_default`, so the two layers can never disagree. Pure for
/// testability — the caller passes the env lookup result.
///
/// The HOST is deliberately NOT mirrored: even when the server binds
/// `0.0.0.0`/`::`, the companion always dials loopback (`127.0.0.1`) — the
/// companion WS endpoint is loopback-enforced in `http::companion` anyway.
fn companion_dial_port(env_port: Option<&str>) -> u16 {
    env_port
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_BIND_PORT)
}

/// Resolve and spawn the companion binary with `kill_on_drop(true)`.
///
/// Lifecycle (the accepted v1 posture from the serene-jumping-kitten plan):
/// `kill_on_drop` fires only when the in-process `Child` is DROPPED — i.e. on
/// a graceful server exit. tokio sets up no job-object/process-group tie, so
/// a parent CRASH (abort, SIGKILL, power loss) orphans the companion; the
/// orphan then self-terminates once its WS redial loop permanently fails to
/// reach a server. That pairing — kill_on_drop for graceful exit + companion
/// self-exit on permanent WS loss — is the whole v1 co-launch contract.
fn spawn_companion(override_bin: Option<PathBuf>) -> anyhow::Result<tokio::process::Child> {
    let current_exe =
        std::env::current_exe().context("resolving the current executable path")?;
    let bin = resolve_companion_bin(override_bin, &current_exe, |p| p.exists())
        .map_err(|e| anyhow::anyhow!(e))?;

    let port = companion_dial_port(std::env::var("PORT").ok().as_deref());
    let server_url = format!("ws://127.0.0.1:{port}/api/companion/ws");
    let cwd = std::env::current_dir().context("resolving the current directory")?;

    let child = tokio::process::Command::new(&bin)
        .arg("--server-url")
        .arg(&server_url)
        .arg("--repo-root")
        .arg(&cwd)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning companion binary {}", bin.display()))?;

    eprintln!(
        "lumina: co-launched companion {} (pid {:?}) dialing {server_url}",
        bin.display(),
        child.id()
    );
    Ok(child)
}

/// The compile-time default ingest URL, built from [`DEFAULT_BIND_PORT`].
fn default_ingest_url() -> String {
    format!("http://127.0.0.1:{DEFAULT_BIND_PORT}/api/sessions/ingest")
}

/// Resolve the flow dir, open a pool, import, and print the summary.
async fn import_flow_cmd(slug: &str) -> anyhow::Result<()> {
    let flow_dir: PathBuf = PathBuf::from(".claude").join("flows").join(slug);
    if !flow_dir.is_dir() {
        anyhow::bail!(
            "flow directory not found: {} (run from the repo root)",
            flow_dir.display()
        );
    }

    // Reuse the runtime DB the server uses, honouring DATABASE_URL with the same
    // local default as `app::serve`.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://lumina.db".to_string());
    let pool = lumina_core::db::init(&database_url)
        .await
        .with_context(|| format!("initialising database at {database_url}"))?;

    let summary = lumina_core::import::import_flow(&pool, &flow_dir)
        .await
        .with_context(|| format!("importing flow '{slug}'"))?;

    println!(
        "imported flow '{slug}': {} scaffold work-items, {} tasks, {} findings ({} execution-record items dropped)",
        summary.scaffold_created,
        summary.tasks_created,
        summary.findings_created,
        summary.items_dropped,
    );

    Ok(())
}

/// `lumina init-hooks` — merge a SessionEnd http-hook into
/// `<project_dir>/.claude/settings.json`.
///
/// Pure filesystem + JSON, no DB / network / server introspection. The merge
/// is read-modify-merge and idempotent: an existing http-hook with the same
/// `url` is never duplicated, and unrelated hook events / settings keys are
/// preserved untouched. The settings dir (`.claude/`) is created if absent.
fn init_hooks_cmd(project_dir: &Path, url: &str) -> anyhow::Result<()> {
    let claude_dir = project_dir.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    // Read-or-start-empty. A present-but-unparseable file is a hard error
    // (refuse to clobber a file we cannot understand); an absent file means a
    // fresh `{}`.
    let mut root: serde_json::Value = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("reading {}", settings_path.display()))?;
        serde_json::from_str(&raw).with_context(|| {
            format!(
                "parsing {} as JSON (refusing to overwrite an unparseable settings file)",
                settings_path.display()
            )
        })?
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    let outcome = merge_session_end_http_hook(&mut root, url)?;

    // Pretty-print 2-space (Claude Code's settings.json convention) + trailing
    // newline. serde_json's pretty formatter defaults to 2-space indent.
    std::fs::create_dir_all(&claude_dir)
        .with_context(|| format!("creating {}", claude_dir.display()))?;
    let mut serialized = serde_json::to_string_pretty(&root)
        .context("serialising merged settings.json")?;
    serialized.push('\n');

    // Write atomically: a plain `fs::write` truncates-then-writes, so an
    // interruption mid-write (crash, Ctrl-C, disk-full) would leave the
    // operator's settings.json truncated and the next init-hooks run would
    // hard-error on the unparseable file. Instead write to a temp file in the
    // SAME directory (so the rename stays on one filesystem) then atomically
    // replace the target. `tempfile::NamedTempFile::persist` is a cross-platform
    // atomic replace that handles the Windows "rename fails if dest exists"
    // gotcha for us.
    let mut tmp = tempfile::NamedTempFile::new_in(&claude_dir)
        .with_context(|| format!("creating temp file in {}", claude_dir.display()))?;
    {
        use std::io::Write as _;
        tmp.write_all(serialized.as_bytes())
            .with_context(|| format!("writing temp file for {}", settings_path.display()))?;
        tmp.flush()
            .with_context(|| format!("flushing temp file for {}", settings_path.display()))?;
    }
    tmp.persist(&settings_path)
        .map_err(|e| e.error)
        .with_context(|| format!("writing {}", settings_path.display()))?;

    // Surface the gate caveat whenever we did not extend a LOCAL
    // `allowedHttpHookUrls` gate. We deliberately cannot read managed/user
    // settings from here, so we cannot tell whether a gate exists upstream
    // (real risk) or nowhere at all (the common fresh-project case, no risk).
    // The note is therefore conditional/informational — not an assertion that
    // delivery WILL be blocked.
    if !outcome.local_gate_extended {
        eprintln!(
            "note: `allowedHttpHookUrls` MERGES across managed/user/project settings sources, and \
             this project-level write did not extend a local gate (none present in {}). If — and \
             only if — an `allowedHttpHookUrls` gate is configured in your managed or user \
             settings, ensure {url} is allowlisted there, or the SessionEnd http-hook would be \
             blocked. If no such gate exists anywhere, no action is needed.",
            settings_path.display()
        );
    }

    // Print the written SessionEnd hook block so the operator sees exactly what
    // landed on disk.
    let session_end = root
        .get("hooks")
        .and_then(|h| h.get("SessionEnd"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let pretty =
        serde_json::to_string_pretty(&session_end).unwrap_or_else(|_| "<unprintable>".to_string());
    if outcome.added {
        println!(
            "init-hooks: wrote SessionEnd http-hook ({url}) to {}",
            settings_path.display()
        );
    } else {
        println!(
            "init-hooks: SessionEnd http-hook ({url}) already present in {} (no change)",
            settings_path.display()
        );
    }
    println!("hooks.SessionEnd =\n{pretty}");

    Ok(())
}

/// Result of an in-place merge of the SessionEnd http-hook.
struct MergeOutcome {
    /// True when a new http-hook entry was appended (false ⇒ idempotent no-op).
    added: bool,
    /// True when a pre-existing local `allowedHttpHookUrls` array was extended
    /// (or already contained) the URL. False ⇒ no local gate present, so the
    /// caller emits the cross-source-gate warning.
    local_gate_extended: bool,
}

/// Read-modify-merge the SessionEnd http-hook entry into `root` (a parsed
/// settings.json object).
///
/// Navigates/creates `hooks` (object) → `SessionEnd` (array). The http-hook
/// entry is `{ "type":"http", "url":<url>, "timeout":30 }`, appended to the
/// FIRST SessionEnd group's `hooks` array (creating a group if none exists).
/// NEVER overwrites the `hooks` object wholesale or any sibling event key
/// (PreToolUse, etc.). Idempotent: if an http-hook with the same `url` already
/// exists in ANY SessionEnd group, it is not duplicated.
///
/// `allowedHttpHookUrls`: if `root` already carries that array, the URL is
/// appended (deduplicated) so the hook is not silently blocked. A gate is
/// NEVER created if absent — an empty/restrictive gate would block all other
/// http hooks too.
fn merge_session_end_http_hook(
    root: &mut serde_json::Value,
    url: &str,
) -> anyhow::Result<MergeOutcome> {
    use serde_json::Value;

    // The root must be a JSON object to host a `hooks` key.
    let root_obj = root
        .as_object_mut()
        .context("settings.json root is not a JSON object")?;

    // Extend a pre-existing allowedHttpHookUrls gate (never create one).
    let local_gate_extended = match root_obj.get_mut("allowedHttpHookUrls") {
        Some(Value::Array(arr)) => {
            let already = arr.iter().any(|v| v.as_str() == Some(url));
            if !already {
                arr.push(Value::String(url.to_string()));
            }
            true
        }
        // A non-array gate value is malformed; leave it untouched and treat as
        // "no usable local gate" so the warning still fires.
        Some(_) => false,
        None => false,
    };

    // hooks (object) — create if absent, never clobber if present.
    let hooks = root_obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let hooks_obj = hooks
        .as_object_mut()
        .context("settings.json `hooks` is not a JSON object")?;

    // SessionEnd (array of groups) — create if absent.
    let session_end = hooks_obj
        .entry("SessionEnd")
        .or_insert_with(|| Value::Array(Vec::new()));
    let groups = session_end
        .as_array_mut()
        .context("settings.json `hooks.SessionEnd` is not a JSON array")?;

    // Idempotency: scan every group's `hooks` array for an existing http-hook
    // with the same url.
    let already_present = groups.iter().any(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .map(|entries| {
                entries.iter().any(|entry| {
                    entry.get("type").and_then(Value::as_str) == Some("http")
                        && entry.get("url").and_then(Value::as_str) == Some(url)
                })
            })
            .unwrap_or(false)
    });

    if already_present {
        return Ok(MergeOutcome {
            added: false,
            local_gate_extended,
        });
    }

    let hook_entry = serde_json::json!({
        "type": "http",
        "url": url,
        "timeout": 30,
    });

    // Append to the first existing group with a `hooks` array, else create a
    // fresh matcher-less group. (SessionEnd has no meaningful matcher; an
    // omitted matcher applies to all session-end reasons.)
    if let Some(group) = groups
        .iter_mut()
        .find(|g| g.get("hooks").and_then(Value::as_array).is_some())
    {
        // Safe: the `find` predicate guarantees `hooks` is an array.
        if let Some(entries) = group
            .get_mut("hooks")
            .and_then(Value::as_array_mut)
        {
            entries.push(hook_entry);
        }
    } else {
        groups.push(serde_json::json!({
            "hooks": [hook_entry],
        }));
    }

    Ok(MergeOutcome {
        added: true,
        local_gate_extended,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const URL: &str = "http://127.0.0.1:24817/api/sessions/ingest";

    /// Read the settings.json back as a parsed Value.
    fn read_settings(dir: &Path) -> Value {
        let raw = std::fs::read_to_string(dir.join(".claude").join("settings.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// Extract the SessionEnd http-hook entries with the given url.
    fn session_end_http_hooks<'a>(root: &'a Value, url: &str) -> Vec<&'a Value> {
        root["hooks"]["SessionEnd"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap().iter())
            .filter(|e| {
                e.get("type").and_then(Value::as_str) == Some("http")
                    && e.get("url").and_then(Value::as_str) == Some(url)
            })
            .collect()
    }

    #[test]
    fn writes_valid_session_end_hook_from_scratch() {
        let tmp = tempfile::tempdir().unwrap();
        init_hooks_cmd(tmp.path(), URL).unwrap();

        let root = read_settings(tmp.path());
        let hooks = session_end_http_hooks(&root, URL);
        assert_eq!(hooks.len(), 1, "exactly one http-hook entry");
        assert_eq!(hooks[0]["type"], json!("http"));
        assert_eq!(hooks[0]["url"], json!(URL));
        assert_eq!(hooks[0]["timeout"], json!(30));
    }

    #[test]
    fn rerun_is_idempotent_no_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        init_hooks_cmd(tmp.path(), URL).unwrap();
        init_hooks_cmd(tmp.path(), URL).unwrap();
        init_hooks_cmd(tmp.path(), URL).unwrap();

        let root = read_settings(tmp.path());
        assert_eq!(
            session_end_http_hooks(&root, URL).len(),
            1,
            "re-running must not duplicate the http-hook entry"
        );
    }

    #[test]
    fn preserves_existing_unrelated_hooks_and_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        // A pre-existing PreToolUse hook + an unrelated top-level key.
        let pre = json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "echo hi" }
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            claude.join("settings.json"),
            serde_json::to_string_pretty(&pre).unwrap(),
        )
        .unwrap();

        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root = read_settings(tmp.path());

        // Unrelated top-level key survives.
        assert_eq!(root["model"], json!("opus"));
        // The PreToolUse hook survives untouched.
        let pre_tool = &root["hooks"]["PreToolUse"];
        assert_eq!(pre_tool[0]["matcher"], json!("Bash"));
        assert_eq!(pre_tool[0]["hooks"][0]["command"], json!("echo hi"));
        // The SessionEnd http-hook landed.
        assert_eq!(session_end_http_hooks(&root, URL).len(), 1);
    }

    #[test]
    fn extends_existing_allowed_http_hook_urls_gate() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let pre = json!({
            "allowedHttpHookUrls": ["https://hooks.example.com/*"]
        });
        std::fs::write(
            claude.join("settings.json"),
            serde_json::to_string_pretty(&pre).unwrap(),
        )
        .unwrap();

        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root = read_settings(tmp.path());

        let gate = root["allowedHttpHookUrls"].as_array().unwrap();
        assert!(
            gate.iter().any(|v| v.as_str() == Some(URL)),
            "the ingest URL must be appended to the existing gate"
        );
        assert!(
            gate.iter()
                .any(|v| v.as_str() == Some("https://hooks.example.com/*")),
            "the pre-existing gate entry must be preserved"
        );

        // Re-running must not duplicate the gate entry either.
        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root2 = read_settings(tmp.path());
        let gate2 = root2["allowedHttpHookUrls"].as_array().unwrap();
        assert_eq!(
            gate2.iter().filter(|v| v.as_str() == Some(URL)).count(),
            1,
            "gate URL must not be duplicated on re-run"
        );
    }

    #[test]
    fn does_not_create_gate_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root = read_settings(tmp.path());
        assert!(
            root.get("allowedHttpHookUrls").is_none(),
            "init-hooks must NOT create a gate (an empty/restrictive gate would block all http hooks)"
        );
    }

    #[test]
    fn unparseable_settings_errors_and_never_clobbers() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let settings = claude.join("settings.json");
        // Deliberately invalid JSON.
        let original = "{ not json";
        std::fs::write(&settings, original).unwrap();

        // init-hooks must refuse to overwrite a file it cannot parse.
        let result = init_hooks_cmd(tmp.path(), URL);
        assert!(
            result.is_err(),
            "an unparseable settings.json must be a hard error"
        );

        // The original bytes must survive untouched (never-clobber).
        let after = std::fs::read_to_string(&settings).unwrap();
        assert_eq!(
            after, original,
            "the unparseable file must be left byte-for-byte unchanged"
        );
    }

    #[test]
    fn preserves_existing_session_end_command_hook() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        // A pre-existing SessionEnd group carrying a `command` hook.
        let pre = json!({
            "hooks": {
                "SessionEnd": [
                    {
                        "hooks": [
                            { "type": "command", "command": "echo bye" }
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            claude.join("settings.json"),
            serde_json::to_string_pretty(&pre).unwrap(),
        )
        .unwrap();

        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root = read_settings(tmp.path());

        let groups = root["hooks"]["SessionEnd"].as_array().unwrap();
        // No duplicate group: the http entry is appended into the existing
        // group's `hooks` array rather than spawning a second group.
        assert_eq!(groups.len(), 1, "no duplicate SessionEnd group");

        let entries = groups[0]["hooks"].as_array().unwrap();
        // The pre-existing command hook survives untouched.
        assert!(
            entries.iter().any(|e| {
                e.get("type").and_then(Value::as_str) == Some("command")
                    && e.get("command").and_then(Value::as_str) == Some("echo bye")
            }),
            "the existing command hook must survive"
        );
        // The http entry landed alongside it.
        assert_eq!(
            session_end_http_hooks(&root, URL).len(),
            1,
            "the http-hook must be appended alongside the command hook"
        );
    }

    #[test]
    fn non_array_allowed_http_hook_urls_is_left_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        // A malformed (non-array) gate value — here a string.
        let pre = json!({
            "allowedHttpHookUrls": "https://hooks.example.com/*"
        });
        std::fs::write(
            claude.join("settings.json"),
            serde_json::to_string_pretty(&pre).unwrap(),
        )
        .unwrap();

        // init-hooks must still complete (it does not own/repair this key).
        init_hooks_cmd(tmp.path(), URL).unwrap();
        let root = read_settings(tmp.path());

        // The malformed value is left exactly as-is — neither coerced to an
        // array nor extended.
        assert_eq!(
            root["allowedHttpHookUrls"],
            json!("https://hooks.example.com/*"),
            "a non-array gate value must be left untouched"
        );
        // The hook itself still landed.
        assert_eq!(session_end_http_hooks(&root, URL).len(), 1);
    }

    #[test]
    fn default_ingest_url_uses_bind_port_constant() {
        assert_eq!(
            default_ingest_url(),
            format!("http://127.0.0.1:{DEFAULT_BIND_PORT}/api/sessions/ingest")
        );
    }

    // --- Task 8: --with-companion co-launch ---

    #[test]
    fn companion_bin_override_wins_when_present() {
        let override_path = PathBuf::from("/custom/companion");
        let resolved = resolve_companion_bin(
            Some(override_path.clone()),
            Path::new("/target/debug/lumina"),
            |p| p == override_path.as_path(),
        )
        .unwrap();
        assert_eq!(resolved, override_path);
    }

    #[test]
    fn companion_bin_missing_override_is_actionable_error() {
        let err = resolve_companion_bin(
            Some(PathBuf::from("/nowhere/companion")),
            Path::new("/target/debug/lumina"),
            |_| false,
        )
        .unwrap_err();
        assert!(
            err.contains("/nowhere/companion"),
            "error must name the missing path: {err}"
        );
        assert!(
            err.contains("cargo build --workspace --manifest-path lumina/Cargo.toml"),
            "error must name the workspace build command: {err}"
        );
    }

    #[test]
    fn companion_bin_defaults_to_sibling_with_platform_suffix() {
        let exe = Path::new("/target/debug").join(if cfg!(windows) {
            "lumina.exe"
        } else {
            "lumina"
        });
        let expected = Path::new("/target/debug").join(companion_file_name());
        let resolved = resolve_companion_bin(None, &exe, |p| p == expected.as_path()).unwrap();
        assert_eq!(resolved, expected);
        if cfg!(windows) {
            assert!(
                resolved.to_string_lossy().ends_with("lumina-companion.exe"),
                "Windows sibling must carry the .exe suffix: {}",
                resolved.display()
            );
        }
    }

    #[test]
    fn companion_bin_missing_sibling_is_actionable_error() {
        let err =
            resolve_companion_bin(None, Path::new("/target/debug/lumina"), |_| false)
                .unwrap_err();
        assert!(
            err.contains("sibling of the current executable"),
            "error must say how the path was derived: {err}"
        );
        assert!(
            err.contains("cargo build --workspace --manifest-path lumina/Cargo.toml"),
            "error must name the workspace build command: {err}"
        );
        assert!(
            err.contains("--companion-bin"),
            "error must mention the override escape hatch: {err}"
        );
    }

    #[test]
    fn companion_dial_port_mirrors_serve_resolution() {
        // Unset → default; parseable → that port; unparseable → default
        // (matching app.rs's parse_env_or_default fallback).
        assert_eq!(companion_dial_port(None), DEFAULT_BIND_PORT);
        assert_eq!(companion_dial_port(Some("9000")), 9000);
        assert_eq!(companion_dial_port(Some("8080a")), DEFAULT_BIND_PORT);
    }

    #[test]
    fn companion_flags_parse_alongside_subcommand_but_gate_rejects() {
        // clap#5353: flags + optional subcommand COEXIST at parse time (we do
        // not use args_conflicts_with_subcommands)…
        let cli =
            Cli::try_parse_from(["lumina", "--with-companion", "import-flow", "some-slug"])
                .expect("flags must parse alongside a subcommand");
        // …and the in-code gate is what rejects the combination.
        let msg = companion_flag_conflict(&cli).expect("gate must reject subcommand + flag");
        assert!(msg.contains("--with-companion"), "message names the flag: {msg}");
        assert!(msg.contains("subcommand"), "message names the conflict: {msg}");
    }

    #[test]
    fn companion_flags_on_bare_invocation_pass_the_gate() {
        let cli = Cli::try_parse_from([
            "lumina",
            "--with-companion",
            "--companion-bin",
            "/custom/companion",
        ])
        .expect("bare invocation with companion flags must parse");
        assert!(cli.with_companion);
        assert_eq!(cli.companion_bin, Some(PathBuf::from("/custom/companion")));
        assert!(companion_flag_conflict(&cli).is_none());
    }

    #[test]
    fn bare_invocation_without_flags_passes_the_gate() {
        let cli = Cli::try_parse_from(["lumina"]).unwrap();
        assert!(companion_flag_conflict(&cli).is_none());
    }

    // --- focus 1C.3: --with-scheduler boot flag ---

    #[test]
    fn with_scheduler_parses_on_bare_invocation() {
        let cli = Cli::try_parse_from(["lumina", "--with-scheduler"])
            .expect("bare invocation with --with-scheduler must parse");
        assert!(cli.with_scheduler);
        assert!(scheduler_flag_conflict(&cli).is_none());
    }

    #[test]
    fn bare_invocation_defaults_with_scheduler_off() {
        let cli = Cli::try_parse_from(["lumina"]).unwrap();
        assert!(!cli.with_scheduler, "default is off — no scheduler spawn");
        assert!(scheduler_flag_conflict(&cli).is_none());
    }

    #[test]
    fn with_scheduler_parses_alongside_subcommand_but_gate_rejects() {
        // Like the companion flag (clap#5353): the flag parses alongside an
        // optional subcommand, and the in-code gate is what rejects it.
        let cli = Cli::try_parse_from(["lumina", "--with-scheduler", "import-flow", "some-slug"])
            .expect("flag must parse alongside a subcommand");
        let msg = scheduler_flag_conflict(&cli).expect("gate must reject subcommand + flag");
        assert!(msg.contains("--with-scheduler"), "message names the flag: {msg}");
        assert!(msg.contains("subcommand"), "message names the conflict: {msg}");
    }

    #[test]
    fn with_scheduler_and_companion_coexist_on_bare_invocation() {
        let cli = Cli::try_parse_from(["lumina", "--with-scheduler", "--with-companion"])
            .expect("both boot flags parse on the bare invocation");
        assert!(cli.with_scheduler && cli.with_companion);
        assert!(scheduler_flag_conflict(&cli).is_none());
        assert!(companion_flag_conflict(&cli).is_none());
    }

    #[test]
    fn companion_bin_requires_with_companion_at_parse_time() {
        // The `requires` clap relation: --companion-bin alone is a parse
        // error, not a silently-ignored flag.
        let err = Cli::try_parse_from(["lumina", "--companion-bin", "/custom/companion"])
            .expect_err("--companion-bin without --with-companion must fail to parse");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }
}
