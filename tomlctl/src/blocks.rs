//! Shared-markdown-block parity verification, backing the `blocks verify` verb
//! the git pre-commit hook runs.
//!
//! A shared block is a `<!-- SHARED-BLOCK:NAME START -->` … `END` span
//! duplicated across several files that must stay byte-identical.
//! `blocks_verify` hashes each named span in each named file and reports the
//! blocks whose carriers disagree. Which blocks exist and which files carry
//! them is owned by `scripts/shared-blocks.toml`; both arrive as arguments, so
//! this module holds no list of either and does not rot when the manifest
//! changes.
//!
//! `scripts/verify-shared-blocks.sh` is canonical, not this module: it gates
//! commits, so a tree it rejects must never verify here. A line is the bytes
//! between newlines with at most ONE trailing `\r` removed — `str::lines()`,
//! equally the text-mode read of the gawk that script requires — and a marker
//! matches only by whole-line equality after that strip, so a line ending
//! `\r\r\n` keeps a CR and fails on both sides. An empty span fails too: equal
//! empty digests would report parity having compared nothing. `SpanDefect`
//! carries the shell's diagnoses; its marker guard alone tolerates one further
//! CR, which buys a better message for the same verdict.

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::integrity::hex_lower;
use crate::io::advise;

#[derive(Debug)]
pub(crate) struct BlocksReport {
    pub(crate) ok: bool,
    /// The rendered JSON payload (top-level object containing `ok` +
    /// `blocks`) that the dispatcher prints to stdout.
    pub(crate) report: JsonValue,
}

/// Why a file yields no usable span for a block — one per hard failure
/// `scripts/verify-shared-blocks.sh` reports for the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanDefect {
    MissingMarker,
    /// Both markers are there once a further trailing `\r` is stripped: the
    /// shell's marker guard passes such a file and its extractor then finds
    /// nothing between markers it never matched.
    MarkerTrailingCr,
    ExtractedEmpty,
}

impl SpanDefect {
    /// Self-describing enough to act on from the JSON alone.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            SpanDefect::MissingMarker => "missing-marker",
            SpanDefect::MarkerTrailingCr => "marker-trailing-cr",
            SpanDefect::ExtractedEmpty => "extracted-empty",
        }
    }
}

/// Extract the byte-content between `<!-- SHARED-BLOCK:NAME START -->` and
/// `<!-- SHARED-BLOCK:NAME END -->` markers. Markers themselves are NOT
/// included in the hash input. Inner lines are joined by `\n` (matching awk's
/// default ORS), with every content line — including the last — followed by
/// `\n`. See the module header for the line and marker semantics, which are the
/// shell verifier's rather than this module's.
pub(crate) fn extract_block(contents: &str, name: &str) -> Result<Vec<u8>, SpanDefect> {
    let start = format!("<!-- SHARED-BLOCK:{} START -->", name);
    let end = format!("<!-- SHARED-BLOCK:{} END -->", name);
    let mut in_block = false;
    let mut saw_start = false;
    let mut saw_end = false;
    let mut relaxed_start = false;
    let mut relaxed_end = false;
    // The extracted block is a subset of `contents`, so `contents.len()` is a
    // trivially correct upper bound that eliminates reallocations during the
    // per-line `extend_from_slice` + `push(b'\n')` loop below.
    let mut out = Vec::with_capacity(contents.len());
    for line in contents.lines() {
        let relaxed = line.strip_suffix('\r').unwrap_or(line);
        relaxed_start |= relaxed == start;
        relaxed_end |= relaxed == end;
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
    if !(saw_start && saw_end) {
        return Err(if relaxed_start && relaxed_end {
            SpanDefect::MarkerTrailingCr
        } else {
            SpanDefect::MissingMarker
        });
    }
    if out.is_empty() {
        return Err(SpanDefect::ExtractedEmpty);
    }
    Ok(out)
}

pub(crate) fn scan_block_names(contents: &str) -> Vec<String> {
    scan_block_names_warn(contents, None)
}

/// Same as `scan_block_names` but also emits a stderr warning for lines
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
        // END markers are canonical too — skip them without adding to `names`
        // (block names are discovered from START markers only). Without this
        // guard every valid END marker fell through to the fuzzy-match block
        // below and emitted a spurious "probable typo'd" warning.
        if let Some(rest) = trimmed.strip_prefix("<!-- SHARED-BLOCK:")
            && rest.strip_suffix(" END -->").is_some()
        {
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
        let path_prefix = src_label
            .map(|p| format!("file {} ", p))
            .unwrap_or_default();
        advise!(
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
        bail!(
            "blocks verify: no files supplied; pass one or more file paths (e.g. `tomlctl blocks verify a.md b.md`)"
        );
    }
    // Preload every file once, running the typo-aware scan up-front, so a
    // `<!-- SHAREDBLOCK:... START -->` (missing hyphen) in ANY file surfaces
    // as a warning — not just in the first one that feeds `effective_blocks`.
    let mut contents_by_file: HashMap<PathBuf, String> = HashMap::new();
    for f in files {
        let c = fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?;
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
        let mut per_file: Vec<(PathBuf, Result<String, SpanDefect>)> = Vec::new();
        for f in files {
            let contents = &contents_by_file[f];
            let outcome = extract_block(contents, name).map(|b| hex_lower(&Sha256::digest(&b)));
            per_file.push((f.clone(), outcome));
        }

        let mut present: Vec<(&PathBuf, &String)> = per_file
            .iter()
            .filter_map(|(p, h)| h.as_ref().ok().map(|d| (p, d)))
            .collect();
        let missing: Vec<JsonValue> = per_file
            .iter()
            .filter(|(_, h)| h.is_err())
            .map(|(f, _)| JsonValue::String(path_to_string(f)))
            .collect();
        // Why each of those files has no usable span. Omitted entirely when
        // none has, so a passing report keeps the shape its readers know.
        let defects: Vec<JsonValue> = per_file
            .iter()
            .filter_map(|(f, h)| h.as_ref().err().map(|d| (f, d)))
            .map(|(f, d)| {
                let mut o = serde_json::Map::new();
                o.insert("file".into(), JsonValue::String(path_to_string(f)));
                o.insert("reason".into(), JsonValue::String(d.reason().to_string()));
                JsonValue::Object(o)
            })
            .collect();

        let mut block_obj = serde_json::Map::new();
        block_obj.insert("name".into(), JsonValue::String(name.clone()));
        if !defects.is_empty() {
            block_obj.insert("defects".into(), JsonValue::Array(defects));
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::hex_lower;
    use sha2::{Digest, Sha256};

    /// Fix A: `extract_block` must return `Some(...)` for CRLF input and the
    /// digest must equal the digest produced from the LF equivalent.
    #[test]
    fn extract_block_crlf_matches_lf_digest() {
        let lf =
            "<!-- SHARED-BLOCK:foo START -->\nline one\nline two\n<!-- SHARED-BLOCK:foo END -->\n";
        let crlf = "<!-- SHARED-BLOCK:foo START -->\r\nline one\r\nline two\r\n<!-- SHARED-BLOCK:foo END -->\r\n";

        let lf_bytes = extract_block(lf, "foo").expect("LF: extract_block returned None");
        let crlf_bytes = extract_block(crlf, "foo").expect("CRLF: extract_block returned None");

        let lf_digest = hex_lower(&Sha256::digest(&lf_bytes));
        let crlf_digest = hex_lower(&Sha256::digest(&crlf_bytes));
        assert_eq!(
            lf_digest, crlf_digest,
            "CRLF and LF content must produce the same digest"
        );
    }

    /// Fix B: `scan_block_names_warn` must NOT emit a typo warning for a
    /// valid END marker. We verify the behavioural contract: the returned
    /// `names` list must equal `["foo"]` (START-only discovery, END is
    /// canonical and silent) and must NOT contain a spurious empty entry or
    /// panic.
    #[test]
    fn scan_block_names_warn_end_marker_not_flagged() {
        let contents = "\
<!-- SHARED-BLOCK:foo START -->
body
<!-- SHARED-BLOCK:foo END -->
";
        let names = scan_block_names_warn(contents, Some("test-fixture"));
        assert_eq!(
            names,
            vec!["foo".to_string()],
            "END marker must be recognised as canonical and not alter the names list"
        );
    }

    /// A marker line that still ends in `\r` after the one strip a line gets
    /// matches nothing — the verdict the shell verifier reaches by clearing its
    /// relaxed marker guard and then extracting no content.
    #[test]
    fn extract_block_marker_keeping_a_cr_is_a_line_endings_defect() {
        let carrier =
            "<!-- SHARED-BLOCK:foo START -->\r\r\nline one\n<!-- SHARED-BLOCK:foo END -->\n";
        assert_eq!(
            extract_block(carrier, "foo"),
            Err(SpanDefect::MarkerTrailingCr)
        );
        assert_eq!(
            extract_block("nothing here\n", "foo"),
            Err(SpanDefect::MissingMarker)
        );
    }

    /// Both carriers hash the empty input and compare equal, so without this
    /// the block reports parity having compared nothing.
    #[test]
    fn blocks_verify_empty_span_fails_on_every_carrier() {
        let dir = tempfile::tempdir().unwrap();
        let empty = "<!-- SHARED-BLOCK:foo START -->\n<!-- SHARED-BLOCK:foo END -->\n";
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, empty).unwrap();
        fs::write(&b, empty).unwrap();

        let report = blocks_verify(&[a, b], &["foo".to_string()]).unwrap();
        assert!(!report.ok, "empty span must fail: {:?}", report.report);
        let block = &report.report["blocks"][0];
        let reasons: Vec<&str> = block["defects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["reason"].as_str().unwrap())
            .collect();
        assert_eq!(reasons, vec!["extracted-empty", "extracted-empty"]);
    }

    /// `blocks_verify`'s no-files error must embed an example invocation, so
    /// an agent reading it sees the expected argument shape directly.
    #[test]
    fn error_message_path_shape_blocks_verify_quotes_expected_invocation() {
        let err = blocks_verify(&[], &[]).unwrap_err().to_string();
        assert!(
            err.contains("no files supplied") && err.contains("tomlctl blocks verify"),
            "path-shape error must quote an example invocation; got: {err}"
        );
    }
}
