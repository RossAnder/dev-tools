//! `tomlctl agents audit` — the Bash commands a launch's agents ran that match
//! a pattern, read from the `agent-*.jsonl` transcripts in one directory.
//! Read-only: nothing is written and no subdirectory is entered.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value as JsonValue, json};

/// The patterns used when none is given: a cargo build, test or nextest run,
/// which the milestone gate owns, and a recursive grep, which walks ignored
/// trees such as `target/` and `.git/`.
pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    r"\bcargo\s+(?:\+\S+\s+)?(?:build|test|nextest)\b",
    r"\b[ef]?grep\b[^|;&\n]*\s(?:-[A-Za-z]*[rR][A-Za-z]*|--recursive|--dereference-recursive)(?:\s|$)",
];

/// The longest command a row carries, in characters.
pub(crate) const COMMAND_CAP: usize = 200;

/// One `{agent, command}` row per matching Bash command, ordered by
/// transcript file name and then by position in the transcript.
pub(crate) fn rows(dir: &Path, patterns: &[String]) -> Result<JsonValue> {
    let regexes = compile(patterns)?;
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
            if regexes.iter().any(|re| re.is_match(&command)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flagged(command: &str) -> bool {
        compile(&[]).unwrap().iter().any(|re| re.is_match(command))
    }

    #[test]
    fn default_patterns_flag_gate_owned_runs_and_recursive_greps() {
        for command in [
            "cargo test --manifest-path tomlctl/Cargo.toml",
            "CARGO_INCREMENTAL=0 cargo build",
            "cargo nextest run -E 'test(x)'",
            "cargo +nightly test",
            "grep -rn foo .",
            "grep -n foo -R src",
            "cd x && grep --recursive foo",
        ] {
            assert!(flagged(command), "{command}");
        }
    }

    #[test]
    fn default_patterns_pass_narrow_checks() {
        for command in [
            "cargo clippy --manifest-path tomlctl/Cargo.toml",
            "rg -n foo src",
            "grep -n foo file.rs",
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
