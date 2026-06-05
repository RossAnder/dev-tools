//! CLI dispatch (Task 7).
//!
//! A clap `derive` `Cli` with an OPTIONAL subcommand. The bare invocation (no
//! subcommand) preserves the original behaviour — it starts the server via
//! `crate::app::serve`. `import-flow <slug>` resolves the flow directory as
//! `.claude/flows/<slug>/` (relative to the CWD), opens a pool via
//! `crate::db::init`, imports the flow, prints the summary, and returns.
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

/// Parse args and dispatch. No subcommand → serve; `import-flow` → import.
pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => crate::app::serve().await,
        Some(Command::ImportFlow { slug }) => import_flow_cmd(&slug).await,
        Some(Command::InitHooks { url, project_dir }) => {
            let url = url.unwrap_or_else(default_ingest_url);
            init_hooks_cmd(&project_dir, &url)
        }
    }
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
    let pool = crate::db::init(&database_url)
        .await
        .with_context(|| format!("initialising database at {database_url}"))?;

    let summary = crate::import::import_flow(&pool, &flow_dir)
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
}
