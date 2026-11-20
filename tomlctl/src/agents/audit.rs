//! `tomlctl agents audit` — the Bash commands a launch's agents ran that match
//! a pattern, read from the `agent-*.jsonl` transcripts in one directory.
//! Read-only: nothing is written and no subdirectory is entered.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value as JsonValue, json};

/// The patterns used when none is given: the build and whole-suite test runs
/// of the common toolchains, which a milestone gate owns, and process kills by
/// image name, which reach processes the agent did not start. Compile checks
/// such as `cargo clippy`, and kills by PID, pass.
pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    r"\bcargo\s+(?:\+\S+\s+)?(?:build|test|nextest)\b",
    r"\b(?:bun|npm|pnpm|yarn)\s+(?:run\s+)?(?:test|build)\b",
    r"\b(?:npx|bunx|pnpm\s+exec)\s+(?:vitest|jest|playwright\s+test)\b",
    r"\bdocker\s+(?:compose\s+|buildx\s+)?build\b",
    r"\bgo\s+(?:build|test)\b",
    r"\bdotnet\s+(?:build|test)\b",
    r"\bpytest\b",
    r"\b(?:bun|node)\s+(?:-e|--eval)\b.*\bscripts/\S",
    // `//IM` is the Git Bash spelling of `/IM`.
    r"(?i)\btaskkill(?:\.exe)?\b[^|;&\n]*\s/{1,2}im\b",
    r"\b(?:pkill|killall)\b",
    r"(?i)\bStop-Process\b[^;&\n]*\s-(?:Name|ProcessName)\b",
    r"(?i)\bGet-Process\b[^|\n]*\|\s*Stop-Process\b",
];

/// The longest command a row carries, in characters, besides the `...`
/// marking each end a longer command was cut at.
pub(crate) const COMMAND_CAP: usize = 200;

/// One `{agent, command}` row per matching Bash command, ordered by
/// transcript file name and then by position in the transcript. A command
/// also matches when it contains one of `gates`, whitespace runs collapsed.
pub(crate) fn rows(dir: &Path, patterns: &[String], gates: &[String]) -> Result<JsonValue> {
    let regexes = compile(patterns)?;
    let gates: Vec<String> = gates
        .iter()
        .map(|g| collapse(g))
        .filter(|g| !g.is_empty())
        .collect();
    let mut files: Vec<(String, std::path::PathBuf)> = fs::read_dir(dir)
        .with_context(|| format!("agents audit: reading {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".jsonl")?;
            stem.starts_with("agent-")
                .then(|| (stem.to_owned(), entry.path()))
        })
        .collect();
    files.sort();

    let mut out = Vec::new();
    for (agent, path) in files {
        let text =
            fs::read(&path).with_context(|| format!("agents audit: reading {}", path.display()))?;
        let text = String::from_utf8_lossy(&text);
        for command in bash_commands(&text) {
            if let Some(shown) = matched(&command, &regexes, &gates) {
                out.push(json!({"agent": agent, "command": shown}));
            }
        }
    }
    Ok(JsonValue::Array(out))
}

fn compile(patterns: &[String]) -> Result<Vec<Regex>> {
    if patterns.is_empty() {
        return Ok(DEFAULT_PATTERNS
            .iter()
            .map(|p| Regex::new(p).expect("default audit pattern compiles"))
            .collect());
    }
    patterns
        .iter()
        .map(|p| Regex::new(p).with_context(|| format!("agents audit: invalid --pattern `{p}`")))
        .collect()
}

/// Every Bash `tool_use` command in a transcript. A line that is not JSON, or
/// has no `message.content` array, is skipped.
fn bash_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<JsonValue>(line) else {
            continue;
        };
        let Some(content) = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(JsonValue::as_array)
        else {
            continue;
        };
        for entry in content {
            if entry.get("type").and_then(JsonValue::as_str) != Some("tool_use")
                || entry.get("name").and_then(JsonValue::as_str) != Some("Bash")
            {
                continue;
            }
            if let Some(command) = entry
                .get("input")
                .and_then(|i| i.get("command"))
                .and_then(JsonValue::as_str)
            {
                out.push(command.to_owned());
            }
        }
    }
    out
}

/// The row text for a command that matches a pattern or contains a gate, or
/// `None` when it does neither. A gate is found in the whitespace-collapsed
/// command, so a long gate-matched command is windowed over that form.
fn matched(command: &str, regexes: &[Regex], gates: &[String]) -> Option<String> {
    if let Some(m) = regexes
        .iter()
        .filter_map(|re| re.find(command))
        .min_by_key(|m| m.start())
    {
        return Some(window(command, m.start(), m.end()));
    }
    let collapsed = collapse(command);
    let (start, end) = gates
        .iter()
        .filter_map(|g| collapsed.find(g.as_str()).map(|s| (s, s + g.len())))
        .min_by_key(|&(s, _)| s)?;
    if command.chars().count() <= COMMAND_CAP {
        return Some(command.to_owned());
    }
    Some(window(&collapsed, start, end))
}

/// `text` whole when it fits `COMMAND_CAP` characters; otherwise a window of
/// that many characters around the match at byte offsets `start..end`, each
/// cut end marked `...`, so a match at the tail of a long command survives.
fn window(text: &str, start: usize, end: usize) -> String {
    let total = text.chars().count();
    if total <= COMMAND_CAP {
        return text.to_owned();
    }
    let match_start = text[..start].chars().count();
    let match_len = text[start..end].chars().count();
    let before = COMMAND_CAP.saturating_sub(match_len) / 2;
    let from = match_start.saturating_sub(before).min(total - COMMAND_CAP);
    let mut out = String::new();
    if from > 0 {
        out.push_str("...");
    }
    out.extend(text.chars().skip(from).take(COMMAND_CAP));
    if from + COMMAND_CAP < total {
        out.push_str("...");
    }
    out
}

fn collapse(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flagged(command: &str) -> bool {
        compile(&[]).unwrap().iter().any(|re| re.is_match(command))
    }

    #[test]
    fn default_patterns_flag_gate_owned_runs() {
        for command in [
            "cargo test --manifest-path tomlctl/Cargo.toml",
            "CARGO_INCREMENTAL=0 cargo build",
            "cargo nextest run -E 'test(x)'",
            "cargo +nightly test",
            "bun test",
            "cd web && bun run build",
            "npm test -- --watch=false",
            "pnpm exec vitest run",
            "docker build -t app .",
            "docker compose build",
            "go test ./...",
            "dotnet build",
            "python -m pytest",
            "bun -e \"await import('./scripts/preview-deploy.ts')\"",
            "node --eval \"require('./scripts/x.js')\"",
            "taskkill //F //IM workerd.exe",
            "taskkill.exe /f /im node.exe /t",
            "pkill -f vite",
            "killall node",
            "Stop-Process -Name workerd -Force",
            "Get-Process node | Stop-Process",
        ] {
            assert!(flagged(command), "{command}");
        }
    }

    #[test]
    fn default_patterns_pass_narrow_checks_and_reading() {
        for command in [
            "cargo clippy --manifest-path tomlctl/Cargo.toml",
            "bun run type-check",
            "rg -n foo src",
            "grep -rn foo node_modules/pkg",
            "grep --include=*.rs foo file",
            "rustfmt --edition 2024 --check src/a.rs",
            "bun run scripts/x.ts",
            "bun -e \"console.log(1)\"",
            "rg -n foo scripts/",
            "taskkill /T /PID 123",
            "taskkill //F //PID 4567",
            "kill 4567",
            "Stop-Process -Id 4567",
        ] {
            assert!(!flagged(command), "{command}");
        }
    }

    fn shown(command: &str, gates: &[&str]) -> Option<String> {
        let gates: Vec<String> = gates.iter().map(|g| collapse(g)).collect();
        matched(command, &compile(&[]).unwrap(), &gates)
    }

    #[test]
    fn window_counts_characters() {
        let long = format!(
            "{} cargo test {}",
            "é".repeat(COMMAND_CAP),
            "é".repeat(COMMAND_CAP)
        );
        let row = shown(&long, &[]).unwrap();
        let inner = row
            .strip_prefix("...")
            .unwrap()
            .strip_suffix("...")
            .unwrap();
        assert_eq!(inner.chars().count(), COMMAND_CAP);
        assert!(inner.contains("cargo test"), "{row}");
    }

    #[test]
    fn short_command_is_whole() {
        let command = "cd a  &&  cargo test";
        assert_eq!(shown(command, &[]).unwrap(), command);
        assert_eq!(shown(command, &["cd a && cargo"]).unwrap(), command);
        assert_eq!(shown("rg foo", &[]), None);
    }

    #[test]
    fn long_command_keeps_a_match_at_its_tail() {
        let command = format!(
            "{} && cargo nextest run -E 'test(x)'",
            "x".repeat(COMMAND_CAP * 2)
        );
        let row = shown(&command, &[]).unwrap();
        assert!(row.starts_with("..."), "{row}");
        assert!(!row.ends_with("..."), "{row}");
        assert!(row.ends_with("cargo nextest run -E 'test(x)'"), "{row}");
        assert_eq!(row.chars().count(), COMMAND_CAP + 3);
    }

    #[test]
    fn long_command_keeps_a_match_at_its_head() {
        let command = format!("cargo build && {}", "x".repeat(COMMAND_CAP * 2));
        let row = shown(&command, &[]).unwrap();
        assert!(row.starts_with("cargo build"), "{row}");
        assert!(row.ends_with("..."), "{row}");
    }

    #[test]
    fn long_command_windows_a_gate_match() {
        let command = format!("{}  &&  bash  scripts/gate.sh", "x".repeat(COMMAND_CAP * 2));
        let row = shown(&command, &["bash scripts/gate.sh"]).unwrap();
        assert!(row.starts_with("..."), "{row}");
        assert!(row.ends_with("&& bash scripts/gate.sh"), "{row}");
    }
}
