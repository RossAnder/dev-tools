//! Installs the Claude Code hooks and the herdr keybinding.
//!
//! The merges are pure. `apply_settings` and `apply_herdr` take their path explicitly,
//! so only `setup` touches the user's real files. Each write is atomic (temp file in
//! the same directory, then rename) and is preceded by a `<file>.bak-glimpse-<unix-ts>`
//! copy, taken only when the merge changed something.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

const HOOK_EVENTS: [&str; 3] = ["SubagentStart", "SubagentStop", "TeammateIdle"];
const KEYBINDING: &str = "prefix+alt+g";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileChange {
    pub(crate) path: PathBuf,
    /// One line per addition; empty when everything is already installed.
    pub(crate) planned: Vec<String>,
    pub(crate) backup: Option<PathBuf>,
}

impl FileChange {
    pub(crate) fn changed(&self) -> bool {
        !self.planned.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) dry_run: bool,
    pub(crate) settings: Result<FileChange, String>,
    pub(crate) herdr: Result<FileChange, String>,
    /// `None` when herdr's config was not written, so there was nothing to reload.
    pub(crate) reload: Option<Result<(), String>>,
}

impl Report {
    pub(crate) fn is_ok(&self) -> bool {
        self.settings.is_ok() && self.herdr.is_ok()
    }
}

/// Resolves the real paths and applies both merges. `dry_run` writes nothing.
pub(crate) fn setup(dry_run: bool) -> Result<Report, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate glimpse: {e}"))?;
    let exe = exe
        .to_str()
        .ok_or_else(|| format!("glimpse path is not UTF-8: {}", exe.display()))?
        .to_string();
    let claude = crate::config::claude_dir()
        .ok_or("cannot locate the Claude config directory: set CLAUDE_CONFIG_DIR or HOME")?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let settings = apply_settings(&claude.join("settings.json"), &exe, dry_run, ts);
    let herdr = match herdr_config_path() {
        Some(path) => apply_herdr(&path, &exe, dry_run, ts),
        None => Err("cannot locate herdr's config: set HERDR_CONFIG_PATH or APPDATA".to_string()),
    };
    let reload = match &herdr {
        Ok(change) if change.changed() && !dry_run => Some(reload_herdr()),
        _ => None,
    };
    Ok(Report {
        dry_run,
        settings,
        herdr,
        reload,
    })
}

/// Appends a matcher-less async exec-form group for each hook event that has no glimpse
/// hook yet, and returns the events it added. A hook counts as glimpse's when its
/// `command` names glimpse and its `args` or `command` mention `hook`, so a shell-form
/// `glimpse hook` entry is left alone.
pub(crate) fn merge_settings(
    mut settings: Value,
    exe: &str,
) -> Result<(Value, Vec<&'static str>), String> {
    let root = settings
        .as_object_mut()
        .ok_or("settings.json is not a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`hooks` in settings.json is not an object")?;
    let mut added = Vec::new();
    for event in HOOK_EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{event}` in settings.json is not an array"))?;
        if groups.iter().any(group_has_glimpse_hook) {
            continue;
        }
        groups.push(json!({
            "hooks": [{"type": "command", "command": exe, "args": ["hook"], "async": true}]
        }));
        added.push(event);
    }
    Ok((settings, added))
}

fn group_has_glimpse_hook(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_glimpse_hook))
}

fn is_glimpse_hook(hook: &Value) -> bool {
    let Some(command) = hook.get("command").and_then(Value::as_str) else {
        return false;
    };
    let command = command.to_lowercase();
    if !command.contains("glimpse") {
        return false;
    }
    let in_args = hook
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .any(|a| a.to_lowercase().contains("hook"))
        });
    in_args || command.contains("hook")
}

/// Appends the keybinding as text so the user's comments and layout survive, unless the
/// config already mentions glimpse. Follows the file's line ending.
pub(crate) fn merge_herdr(text: &str, exe: &str) -> (String, bool) {
    if text.contains("glimpse") {
        return (text.to_string(), false);
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let command = toml_string(&format!("{exe} ensure-pane --focus"));
    let block = [
        "[[keys.command]]".to_string(),
        format!("key = \"{KEYBINDING}\""),
        "type = \"popup\"".to_string(),
        format!("command = {command}"),
        "description = \"glimpse task graph\"".to_string(),
        "width = 40".to_string(),
        "height = 6".to_string(),
    ];
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push_str(nl);
        }
        out.push_str(nl);
    }
    for line in block {
        out.push_str(&line);
        out.push_str(nl);
    }
    (out, true)
}

/// A literal string where one is possible, since Windows paths are full of backslashes;
/// otherwise an escaped basic string.
fn toml_string(s: &str) -> String {
    if s.contains('\'') || s.chars().any(char::is_control) {
        toml::Value::String(s.to_string()).to_string()
    } else {
        format!("'{s}'")
    }
}

pub(crate) fn apply_settings(
    path: &Path,
    exe: &str,
    dry_run: bool,
    ts: u64,
) -> Result<FileChange, String> {
    let original = read_optional(path)?;
    let body = original
        .as_deref()
        .map(|t| t.trim_start_matches('\u{feff}'))
        .unwrap_or("");
    let value = if body.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(body).map_err(|e| format!("{}: {e}", path.display()))?
    };
    let (merged, added) =
        merge_settings(value, exe).map_err(|e| format!("{}: {e}", path.display()))?;
    let planned: Vec<String> = added
        .iter()
        .map(|event| format!("async `{event}` hook: {exe} hook"))
        .collect();
    let mut backup = None;
    if !planned.is_empty() && !dry_run {
        let mut out = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
        out.push('\n');
        backup = write_with_backup(path, original.is_some(), &out, ts)?;
    }
    Ok(FileChange {
        path: path.to_path_buf(),
        planned,
        backup,
    })
}

/// Refuses to write a merge that breaks a config which parsed before it.
pub(crate) fn apply_herdr(
    path: &Path,
    exe: &str,
    dry_run: bool,
    ts: u64,
) -> Result<FileChange, String> {
    let original = read_optional(path)?;
    let text = original.as_deref().unwrap_or("");
    let (merged, changed) = merge_herdr(text, exe);
    let mut planned = Vec::new();
    if changed {
        if text.parse::<toml::Table>().is_ok()
            && let Err(e) = merged.parse::<toml::Table>()
        {
            return Err(format!(
                "{}: appending the keybinding would break the file: {e}",
                path.display()
            ));
        }
        planned.push(format!(
            "`{KEYBINDING}` popup keybinding: {exe} ensure-pane --focus"
        ));
    }
    let mut backup = None;
    if changed && !dry_run {
        backup = write_with_backup(path, original.is_some(), &merged, ts)?;
    }
    Ok(FileChange {
        path: path.to_path_buf(),
        planned,
        backup,
    })
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_with_backup(
    path: &Path,
    existed: bool,
    contents: &str,
    ts: u64,
) -> Result<Option<PathBuf>, String> {
    let backup = if existed {
        let bak = sibling(path, &format!(".bak-glimpse-{ts}"), "");
        std::fs::copy(path, &bak).map_err(|e| format!("{}: backup failed: {e}", bak.display()))?;
        Some(bak)
    } else {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        None
    };
    write_atomic(path, contents)?;
    Ok(backup)
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = sibling(path, &format!(".glimpse-tmp-{}", std::process::id()), ".");
    std::fs::write(&tmp, contents).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// `path`'s file name wrapped as `<prefix><name><suffix>`, in the same directory.
fn sibling(path: &Path, suffix: &str, prefix: &str) -> PathBuf {
    let mut name = OsString::from(prefix);
    name.push(path.file_name().unwrap_or_default());
    name.push(suffix);
    path.with_file_name(name)
}

/// herdr's own order: `HERDR_CONFIG_PATH`, then `$XDG_CONFIG_HOME/herdr`, then `%APPDATA%\herdr`
/// on Windows, then `~/.config/herdr`. Empty values count as unset.
fn herdr_config_path_from(
    config_path: Option<&str>,
    xdg_config_home: Option<&str>,
    appdata: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    let set = |v: Option<&str>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = set(config_path) {
        return Some(p);
    }
    let dir = set(xdg_config_home)
        .or_else(|| set(appdata))
        .or_else(|| set(home).map(|h| h.join(".config")))?;
    Some(dir.join("herdr").join("config.toml"))
}

fn herdr_config_path() -> Option<PathBuf> {
    let var = |k: &str| std::env::var(k).ok();
    let appdata = if cfg!(windows) {
        var("APPDATA").filter(|v| !v.is_empty()).or_else(|| {
            var("USERPROFILE")
                .filter(|v| !v.is_empty())
                .map(|p| format!(r"{p}\AppData\Roaming"))
        })
    } else {
        None
    };
    let home = var("HOME");
    herdr_config_path_from(
        var("HERDR_CONFIG_PATH").as_deref(),
        var("XDG_CONFIG_HOME").as_deref(),
        appdata.as_deref(),
        home.as_deref(),
    )
}

/// Same binary resolution as `Herdr::from_env`.
fn reload_herdr() -> Result<(), String> {
    let bin = std::env::var_os("HERDR_BIN_PATH")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("herdr"));
    let out = Command::new(&bin)
        .args(["server", "reload-config"])
        .output()
        .map_err(|e| format!("cannot run herdr: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("herdr exited with {}", out.status)
        } else {
            stderr
        })
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verb = if self.dry_run { "would add" } else { "added" };
        if self.dry_run {
            writeln!(f, "glimpse setup --dry-run: nothing written")?;
        }
        for (title, outcome) in [
            ("Claude Code settings", &self.settings),
            ("herdr config", &self.herdr),
        ] {
            match outcome {
                Ok(change) => {
                    writeln!(f, "{title}: {}", change.path.display())?;
                    if !change.changed() {
                        writeln!(f, "  already installed")?;
                    }
                    for line in &change.planned {
                        writeln!(f, "  {verb} {line}")?;
                    }
                    if let Some(bak) = &change.backup {
                        writeln!(f, "  backup: {}", bak.display())?;
                    }
                }
                Err(e) => writeln!(f, "{title}: error: {e}")?,
            }
        }
        match &self.reload {
            Some(Ok(())) => writeln!(f, "herdr: config reloaded")?,
            Some(Err(e)) => writeln!(
                f,
                "herdr: reload failed ({e}); run `herdr server reload-config` once herdr is running"
            )?,
            None => {}
        }
        if matches!(&self.settings, Ok(c) if c.changed()) {
            writeln!(
                f,
                "Claude Code picks up hook edits in running sessions; restart it only if the hooks do not fire."
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\me\.cargo\bin\glimpse.exe";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glimpse-setup-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn keys(v: &Value) -> Vec<&str> {
        v.as_object().unwrap().keys().map(String::as_str).collect()
    }

    #[test]
    fn adds_exec_form_async_hooks_keeping_unrelated_keys_in_order() {
        let settings: Value =
            serde_json::from_str(r#"{"theme":"dark","model":"x","permissions":{"allow":[]}}"#)
                .unwrap();
        let (merged, added) = merge_settings(settings, EXE).unwrap();
        assert_eq!(added, HOOK_EVENTS.to_vec());
        assert_eq!(keys(&merged), ["theme", "model", "permissions", "hooks"]);
        assert_eq!(keys(&merged["hooks"]), HOOK_EVENTS.to_vec());
        for event in HOOK_EVENTS {
            let groups = merged["hooks"][event].as_array().unwrap();
            assert_eq!(groups.len(), 1);
            assert!(groups[0].get("matcher").is_none());
            assert_eq!(
                groups[0]["hooks"][0],
                json!({"type": "command", "command": EXE, "args": ["hook"], "async": true})
            );
        }
    }

    #[test]
    fn a_second_settings_merge_is_a_no_op() {
        let (once, _) = merge_settings(json!({"theme": "dark"}), EXE).unwrap();
        let (twice, added) = merge_settings(once.clone(), EXE).unwrap();
        assert!(added.is_empty());
        assert_eq!(twice, once);
    }

    #[test]
    fn an_existing_shell_form_glimpse_hook_counts_as_installed() {
        let hook =
            json!([{"hooks": [{"type": "command", "command": "glimpse hook", "async": true}]}]);
        let settings = json!({"hooks": {
            "SubagentStart": hook, "SubagentStop": hook, "TeammateIdle": hook
        }});
        let (merged, added) = merge_settings(settings.clone(), EXE).unwrap();
        assert!(added.is_empty());
        assert_eq!(merged, settings);
    }

    #[test]
    fn an_unrelated_hook_on_the_same_event_does_not_count() {
        let settings = json!({"hooks": {"SubagentStop": [
            {"hooks": [{"type": "command", "command": "glimpse-notes.exe", "args": ["sync"]}]}
        ]}});
        let (merged, added) = merge_settings(settings, EXE).unwrap();
        assert_eq!(added, HOOK_EVENTS.to_vec());
        assert_eq!(merged["hooks"]["SubagentStop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn an_existing_session_start_hook_is_untouched() {
        let session_start = json!([{"matcher": "startup", "hooks": [
            {"type": "command", "command": "pwsh -File herdr-agent-state.ps1"}
        ]}]);
        let settings = json!({"hooks": {"SessionStart": session_start}});
        let (merged, added) = merge_settings(settings, EXE).unwrap();
        assert_eq!(added.len(), 3);
        assert_eq!(merged["hooks"]["SessionStart"], session_start);
        assert_eq!(
            keys(&merged["hooks"]),
            [
                "SessionStart",
                "SubagentStart",
                "SubagentStop",
                "TeammateIdle"
            ]
        );
    }

    #[test]
    fn rejects_settings_of_the_wrong_shape() {
        assert!(merge_settings(json!([]), EXE).is_err());
        assert!(merge_settings(json!({"hooks": []}), EXE).is_err());
        assert!(merge_settings(json!({"hooks": {"SubagentStart": {}}}), EXE).is_err());
    }

    #[test]
    fn the_appended_herdr_block_parses_and_keeps_existing_text() {
        let existing = "# my herdr config\n[ui]\ntheme = \"kanso\"";
        let (merged, changed) = merge_herdr(existing, EXE);
        assert!(changed);
        assert!(merged.starts_with(existing));
        let table: toml::Table = merged.parse().expect("merged config parses");
        assert_eq!(table["ui"]["theme"].as_str(), Some("kanso"));
        let binding = &table["keys"]["command"][0];
        assert_eq!(binding["key"].as_str(), Some("prefix+alt+g"));
        assert_eq!(binding["type"].as_str(), Some("popup"));
        let expected = format!("{EXE} ensure-pane --focus");
        assert_eq!(binding["command"].as_str(), Some(expected.as_str()));
        assert_eq!(binding["width"].as_integer(), Some(40));
        assert_eq!(binding["height"].as_integer(), Some(6));
    }

    #[test]
    fn a_second_herdr_merge_is_a_no_op() {
        let (once, _) = merge_herdr("", EXE);
        let (twice, changed) = merge_herdr(&once, EXE);
        assert!(!changed);
        assert_eq!(twice, once);
    }

    #[test]
    fn herdr_block_follows_crlf_and_escapes_a_quote_in_the_path() {
        let exe = r"C:\it's\glimpse.exe";
        let (merged, _) = merge_herdr("[ui]\r\nx = 1\r\n", exe);
        assert!(!merged.replace("\r\n", "").contains('\n'), "{merged:?}");
        let table: toml::Table = merged.parse().expect("merged config parses");
        let expected = format!("{exe} ensure-pane --focus");
        assert_eq!(
            table["keys"]["command"][0]["command"].as_str(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn herdr_config_path_follows_herdrs_order() {
        let p = |a, b, c, d| herdr_config_path_from(a, b, c, d);
        assert_eq!(
            p(Some("/x.toml"), Some("/xdg"), Some("/ad"), Some("/h")),
            Some(PathBuf::from("/x.toml"))
        );
        assert_eq!(
            p(Some(""), Some("/xdg"), Some("/ad"), Some("/h")),
            Some(PathBuf::from("/xdg").join("herdr").join("config.toml"))
        );
        assert_eq!(
            p(None, None, Some("/ad"), Some("/h")),
            Some(PathBuf::from("/ad").join("herdr").join("config.toml"))
        );
        assert_eq!(
            p(None, None, None, Some("/h")),
            Some(
                PathBuf::from("/h")
                    .join(".config")
                    .join("herdr")
                    .join("config.toml")
            )
        );
        assert_eq!(p(None, None, None, None), None);
    }

    #[test]
    fn apply_writes_atomically_with_a_backup_and_dry_run_writes_nothing() {
        let dir = temp_dir("apply");
        let settings = dir.join("settings.json");
        let original = "{\"theme\": \"dark\"}";
        std::fs::write(&settings, original).unwrap();

        let dry = apply_settings(&settings, EXE, true, 7).unwrap();
        assert_eq!(dry.planned.len(), 3);
        assert_eq!(dry.backup, None);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);

        let wet = apply_settings(&settings, EXE, false, 7).unwrap();
        let bak = dir.join("settings.json.bak-glimpse-7");
        assert_eq!(wet.backup.as_deref(), Some(bak.as_path()));
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), original);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(keys(&written), ["theme", "hooks"]);

        let again = apply_settings(&settings, EXE, false, 8).unwrap();
        assert!(!again.changed());
        assert!(!dir.join("settings.json.bak-glimpse-8").exists());

        let herdr = dir.join("herdr").join("config.toml");
        let created = apply_herdr(&herdr, EXE, false, 7).unwrap();
        assert!(created.changed());
        assert_eq!(created.backup, None, "a new file has nothing to back up");
        assert!(!apply_herdr(&herdr, EXE, false, 8).unwrap().changed());

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("glimpse-tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
