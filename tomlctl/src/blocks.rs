//! R59: shared-markdown-block parity verification. Used by the git pre-commit
//! hook to ensure `## Flow Context` and `## Ledger Schema` blocks remain
//! byte-identical across `claude/commands/{optimise,review,optimise-apply,review-apply}.md`.
//!
//! Public surface:
//! - `blocks_verify` — the dispatch entrypoint
//! - `BlocksReport` — return shape: `{ok, report: <json>}`
//! - `extract_block` / `scan_block_names` / `scan_block_names_warn` — helpers
//!   reusable by tests and future consumers

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::integrity::hex_lower;

#[derive(Debug)]
pub(crate) struct BlocksReport {
    pub(crate) ok: bool,
    /// R28: the rendered JSON payload (top-level object containing `ok` +
    /// `blocks`) that the dispatcher prints to stdout.
    pub(crate) report: JsonValue,
}

/// Extract the byte-content between `<!-- SHARED-BLOCK:NAME START -->` and
/// `<!-- SHARED-BLOCK:NAME END -->` markers. Markers themselves are NOT
/// included in the hash input. Inner lines are joined by `\n` (matching awk's
/// default ORS), with every content line — including the last — followed by
/// `\n`. Returns None if either marker is missing.
pub(crate) fn extract_block(contents: &str, name: &str) -> Option<Vec<u8>> {
    let start = format!("<!-- SHARED-BLOCK:{} START -->", name);
    let end = format!("<!-- SHARED-BLOCK:{} END -->", name);
    let mut in_block = false;
    let mut saw_start = false;
    let mut saw_end = false;
    let mut out = Vec::new();
    for line in contents.split('\n') {
        if line == start {
            in_block = true;
            saw_start = true;
            continue;
        }
        if line == end {
            in_block = false;
            saw_end = true;
            continue;
        }
        if in_block {
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
    }
    if saw_start && saw_end {
        Some(out)
    } else {
        None
    }
}

pub(crate) fn scan_block_names(contents: &str) -> Vec<String> {
    scan_block_names_warn(contents, None)
}

/// R53: same as `scan_block_names` but also emits a stderr warning for lines
/// that look like SHARED-BLOCK markers but don't match the canonical
/// `<!-- SHARED-BLOCK:<name> START -->` / `... END -->` shape. Typical typos
/// caught: missing hyphen (`SHAREDBLOCK`), lowercase keyword, trailing
/// whitespace, wrong keyword (`STARTS`, `end`). Typo lines do NOT break
/// parity verification — the warning is advisory.
///
/// `src_label` (if supplied) is prefixed into the warning so the operator can
/// locate the offending file quickly.
pub(crate) fn scan_block_names_warn(contents: &str, src_label: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for (i, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- SHARED-BLOCK:")
            && let Some(inner) = rest.strip_suffix(" START -->")
        {
            let n = inner.trim().to_string();
            if !names.contains(&n) {
                names.push(n);
            }
            continue;
        }
        // Fuzzy match: heuristically flag anything that contains
        // "SHARED" (case-insensitive) and "BLOCK" (case-insensitive) on a
        // comment-like line but isn't the canonical form. Cheap substring
        // checks only — no regex dependency.
        if !trimmed.starts_with("<!--") {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        // Require the broken marker to contain at least "SHARED" + "BLOCK"
        // near each other — otherwise a perfectly legitimate HTML comment
        // mentioning the word "block" would trigger a false positive.
        let has_shared = upper.contains("SHARED");
        let has_block = upper.contains("BLOCK");
        if !(has_shared && has_block) {
            continue;
        }
        let path_prefix = src_label.map(|p| format!("file {} ", p)).unwrap_or_default();
        eprintln!(
            "tomlctl: warning: {}line {}: probable typo'd SHARED-BLOCK marker: {}",
            path_prefix,
            i + 1,
            trimmed
        );
    }
    names
}

pub(crate) fn blocks_verify(files: &[PathBuf], blocks: &[String]) -> Result<BlocksReport> {
    if files.is_empty() {
        bail!("blocks verify: no files supplied");
    }
    // Preload every file once. R53: run typo-aware scan on each file's
    // contents up-front, so a `<!-- SHAREDBLOCK:... START -->` (missing
    // hyphen) in ANY file surfaces as a warning — not just the first one
    // that feeds `effective_blocks`.
    let mut contents_by_file: HashMap<PathBuf, String> = HashMap::new();
    for f in files {
        let c = fs::read_to_string(f)
            .with_context(|| format!("reading {}", f.display()))?;
        // Side-effect: emit typo warnings to stderr. Return value discarded
        // here because `effective_blocks` is derived below from the first
        // file only when the user didn't pass `--block`.
        let _ = scan_block_names_warn(&c, Some(&f.display().to_string()));
        contents_by_file.insert(f.clone(), c);
    }

    // If no block names given, infer from the first file's canonical markers.
    let effective_blocks: Vec<String> = if blocks.is_empty() {
        let first = &files[0];
        scan_block_names(&contents_by_file[first])
    } else {
        blocks.to_vec()
    };

    let mut all_ok = true;
    let mut blocks_out = Vec::new();
    for name in &effective_blocks {
        let mut per_file: Vec<(PathBuf, Option<String>)> = Vec::new();
        for f in files {
            let contents = &contents_by_file[f];
            match extract_block(contents, name) {
                Some(bytes) => {
                    let digest = hex_lower(&Sha256::digest(&bytes));
                    per_file.push((f.clone(), Some(digest)));
                }
                None => per_file.push((f.clone(), None)),
            }
        }

        // R22: filter_map collapses the `is_some` filter and the later
        // `.as_ref().unwrap()` calls into a single pass. `present` is now a
        // Vec<(&PathBuf, &String)> — no Option unwraps below.
        let mut present: Vec<(&PathBuf, &String)> = per_file
            .iter()
            .filter_map(|(p, h)| h.as_ref().map(|d| (p, d)))
            .collect();
        let missing: Vec<JsonValue> = per_file
            .iter()
            .filter(|(_, h)| h.is_none())
            .map(|(f, _)| JsonValue::String(path_to_string(f)))
            .collect();

        let mut block_obj = serde_json::Map::new();
        block_obj.insert("name".into(), JsonValue::String(name.clone()));

        if present.is_empty() {
            all_ok = false;
            block_obj.insert("ok".into(), JsonValue::Bool(false));
            block_obj.insert("missing".into(), JsonValue::Array(missing));
            blocks_out.push(JsonValue::Object(block_obj));
            continue;
        }

        // Sort present by file path for deterministic output.
        present.sort_by(|a, b| a.0.cmp(b.0));
        let first_hash = present[0].1.clone();
        let drift = present.iter().any(|(_, h)| *h != &first_hash);

        if drift || !missing.is_empty() {
            all_ok = false;
        }

        if drift {
            block_obj.insert("ok".into(), JsonValue::Bool(false));
            let drift_arr: Vec<JsonValue> = present
                .iter()
                .map(|(f, h)| {
                    let mut o = serde_json::Map::new();
                    o.insert("file".into(), JsonValue::String(path_to_string(f)));
                    o.insert("hash".into(), JsonValue::String((*h).clone()));
                    JsonValue::Object(o)
                })
                .collect();
            block_obj.insert("drift".into(), JsonValue::Array(drift_arr));
            block_obj.insert("missing".into(), JsonValue::Array(missing));
        } else {
            let files_arr: Vec<JsonValue> = present
                .iter()
                .map(|(f, _)| JsonValue::String(path_to_string(f)))
                .collect();
            block_obj.insert("hash".into(), JsonValue::String(first_hash));
            block_obj.insert("files".into(), JsonValue::Array(files_arr));
            block_obj.insert("missing".into(), JsonValue::Array(missing));
        }
        blocks_out.push(JsonValue::Object(block_obj));
    }

    let mut top = serde_json::Map::new();
    top.insert("ok".into(), JsonValue::Bool(all_ok));
    top.insert("blocks".into(), JsonValue::Array(blocks_out));
    Ok(BlocksReport {
        ok: all_ok,
        report: JsonValue::Object(top),
    })
}

fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
