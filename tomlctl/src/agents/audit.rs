//! `tomlctl agents audit` — the Bash commands a launch's agents ran that match
//! a pattern, read from the `agent-*.jsonl` transcripts in one directory.
//! Read-only: nothing is written and no subdirectory is entered.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value as JsonValue, json};

/// The patterns used when none is given: the build and whole-suite test runs
/// of the common toolchains, which a milestone gate owns. Compile checks such
/// as `cargo clippy` or a `type-check` script are a worker's, so they pass.
pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    r"\bcargo\s+(?:\+\S+\s+)?(?:build|test|nextest)\b",
    r"\b(?:bun|npm|pnpm|yarn)\s+(?:run\s+)?(?:test|build)\b",
    r"\b(?:npx|bunx|pnpm\s+exec)\s+(?:vitest|jest|playwright\s+test)\b",
    r"\bdocker\s+(?:compose\s+|buildx\s+)?build\b",
    r"\bgo\s+(?:build|test)\b",
    r"\bdotnet\s+(?:build|test)\b",
    r"\bpytest\b",
];

/// The longest command a row carries, in characters.
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
            let collapsed = collapse(&command);
            if regexes.iter().any(|re| re.is_match(&command))
                || gates.iter().any(|g| collapsed.contains(g.as_str()))
            {
                out.push(json!({"agent": agent, "command": truncate(&command)}));
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

fn truncate(command: &str) -> String {
    command.chars().take(COMMAND_CAP).collect()
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
        ] {
            assert!(!flagged(command), "{command}");
        }
    }

    #[test]
    fn truncate_counts_characters() {
        let long = "é".repeat(COMMAND_CAP + 5);
        assert_eq!(truncate(&long).chars().count(), COMMAND_CAP);
    }
}
