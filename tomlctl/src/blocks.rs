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
/// `\n`. CRLF line endings in the source are normalised to `\n` in the hash
/// input (via `str::lines()`, which strips both `\n` and `\r\n`). Returns
/// None if either marker is missing.
pub(crate) fn extract_block(contents: &str, name: &str) -> Option<Vec<u8>> {
    let start = format!("<!-- SHARED-BLOCK:{} START -->", name);
    let end = format!("<!-- SHARED-BLOCK:{} END -->", name);
    let mut in_block = false;
    let mut saw_start = false;
    let mut saw_end = false;
    // The extracted block is a subset of `contents`, so `contents.len()` is a
    // trivially correct upper bound that eliminates reallocations during the
    // per-line `extend_from_slice` + `push(b'\n')` loop below.
    let mut out = Vec::with_capacity(contents.len());
    for line in contents.lines() {
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
        bail!("blocks verify: no files supplied; pass one or more file paths (e.g. `tomlctl blocks verify a.md b.md`)");
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

// ---------------------------------------------------------------------------
// verify-skills drift engine
//
// Once a shared block is externalised into a `claude/skills/flow-contract-*/
// SKILL.md`, the skill body and any still-embedded carrier copies must remain
// semantically identical. They are NOT byte-identical: the skill carries a
// YAML frontmatter header and cross-references sibling skills by *skill name*
// (e.g. `` `flow-contract-ledger-schema` ``), whereas the carrier copy
// cross-references the same contract by its embedded marker name (e.g.
// `SHARED-BLOCK:ledger-schema`). The drift check normalises away exactly those
// contract cross-reference lines (and the frontmatter) before comparing, so a
// substantive divergence still trips while the expected mechanical differences
// do not. This complements `blocks_verify` (byte-identical carrier↔carrier
// parity) for the as-yet-unexternalised blocks.
// ---------------------------------------------------------------------------

/// Return shape for `verify_skills`, mirroring [`BlocksReport`].
///
/// The `verify-skills` engine (this struct, `verify_skills`, and the private
/// `strip_frontmatter` / `normalise_block` / `first_difference` helpers) is the
/// implementation half of a two-task split: the CLI subcommand + dispatch route
/// that calls `verify_skills` lands in a separate task (T4). Until that wiring
/// exists the engine is reachable only from the in-crate unit tests, so the
/// `#[allow(dead_code)]` annotations below match the reserved-for-future-wiring
/// pattern in `items.rs` (`DispositionError`) and `errors.rs` (`ErrorKind`).
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct SkillDriftReport {
    pub(crate) ok: bool,
    /// The rendered JSON payload (top-level object containing `ok` + `blocks`)
    /// that the dispatcher prints to stdout.
    pub(crate) report: JsonValue,
}

/// Strip a leading YAML frontmatter header from a skill body.
///
/// If `body` begins with a line that is exactly `---`, drop everything from
/// that line through the NEXT line that is exactly `---`, plus one
/// immediately-following blank line if present. If there is no leading `---`,
/// the content is returned unchanged.
///
/// Operates on the raw text (CRLF tolerated: a `\r`-terminated `---` line is
/// recognised because the check trims a trailing `\r`).
#[allow(dead_code)] // reserved for T4 dispatch wiring; used by unit tests.
fn strip_frontmatter(body: &str) -> &str {
    // Peek the first line without consuming the iterator's byte offsets.
    let mut rest = body;
    let first_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let first = rest[..first_end].trim_end_matches(['\n', '\r']);
    if first != "---" {
        return body;
    }
    // Advance past the opening fence.
    rest = &rest[first_end..];
    // Scan for the closing `---` line.
    loop {
        if rest.is_empty() {
            // Unterminated frontmatter — be conservative and return the
            // original body unchanged rather than swallowing the whole file.
            return body;
        }
        let end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        let line = rest[..end].trim_end_matches(['\n', '\r']);
        let after_fence = &rest[end..];
        if line == "---" {
            // Drop one immediately-following blank line if present.
            let blank_end = after_fence
                .find('\n')
                .map(|i| i + 1)
                .unwrap_or(after_fence.len());
            let maybe_blank = after_fence[..blank_end].trim_end_matches(['\n', '\r']);
            if maybe_blank.is_empty() && !after_fence.is_empty() {
                return &after_fence[blank_end..];
            }
            return after_fence;
        }
        rest = after_fence;
    }
}

/// Normalise a shared-block body to a vector of comparable lines.
///
/// This is a PURE function. It:
/// - Splits on `str::lines()` (which normalises CRLF→LF, matching
///   [`extract_block`]).
/// - DROPS any contract cross-reference line. A line is dropped if it matches
///   ANY of: (a) contains the substring `SHARED-BLOCK:` (carrier-style marker
///   ref); (b) contains the substring `` `flow-contract- `` (a backtick
///   immediately followed by `flow-contract-` — skill-style sibling ref);
///   (c) is an embedder-list sentence: lowercased line contains all of
///   `embedded`, `into`, and `carrier`.
///   Note: lines mentioning `flow-research` / `flow-research-deep` are
///   substantive procedure references, NOT contract cross-refs, and are kept
///   (pattern (b) requires the literal `` `flow-contract- ``).
/// - Trims TRAILING whitespace from every surviving line.
/// - Drops trailing blank lines from the result.
#[allow(dead_code)] // reserved for T4 dispatch wiring; used by unit tests.
fn normalise_block(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.contains("SHARED-BLOCK:") {
            continue;
        }
        if line.contains("`flow-contract-") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("embedded") && lower.contains("into") && lower.contains("carrier") {
            continue;
        }
        out.push(line.trim_end().to_string());
    }
    // Drop trailing blank lines.
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// Verify that every externalised shared block (those carrying a `skill` field
/// in the manifest) is in semantic sync with each still-embedded carrier copy.
///
/// `manifest_path` is the path to `scripts/shared-blocks.toml`. Skill and
/// carrier paths recorded in the manifest are repo-relative; they are resolved
/// against the repo root, computed as `manifest_path.parent().parent()`
/// (the manifest lives at `scripts/shared-blocks.toml`, so `scripts/` is
/// directly under the repo root).
///
/// Blocks WITHOUT a `skill` field, or with an empty `files` array, are SKIPPED
/// — there is no externalised skill, or no embedded copy, to compare against.
///
/// `first_differing_line` indices in the report are **1-based** line numbers
/// into the normalised line vectors.
///
/// `ok` is `true` iff no drift (and no missing skill/block) was recorded.
#[allow(dead_code)] // reserved for T4 dispatch wiring; used by unit tests.
pub(crate) fn verify_skills(manifest_path: &Path) -> Result<SkillDriftReport> {
    let manifest_text = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest {}", manifest_path.display()))?;
    let manifest: toml::Table = toml::from_str(&manifest_text)
        .with_context(|| format!("parsing manifest {}", manifest_path.display()))?;

    // repo root = scripts/.. ; manifest lives at scripts/shared-blocks.toml.
    let repo_root: PathBuf = manifest_path
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let blocks = manifest
        .get("block")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut all_ok = true;
    let mut blocks_out: Vec<JsonValue> = Vec::new();

    for block in &blocks {
        let Some(tbl) = block.as_table() else { continue };
        let name = tbl.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let skill = tbl.get("skill").and_then(|v| v.as_str()).unwrap_or_default();
        let files: Vec<String> = tbl
            .get("files")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // Skip blocks with no externalised skill or no embedded carrier copies.
        if skill.is_empty() || files.is_empty() {
            continue;
        }

        let mut block_obj = serde_json::Map::new();
        block_obj.insert("name".into(), JsonValue::String(name.to_string()));
        block_obj.insert("skill".into(), JsonValue::String(skill.to_string()));
        let mut drift: Vec<JsonValue> = Vec::new();

        // Read + normalise the skill body. If the skill file is missing, record
        // a drift entry against every carrier and move on (do not panic).
        let skill_path = repo_root.join(skill);
        let skill_norm = match fs::read_to_string(&skill_path) {
            Ok(contents) => Some(normalise_block(strip_frontmatter(&contents))),
            Err(_) => {
                all_ok = false;
                let mut o = serde_json::Map::new();
                o.insert("error".into(), JsonValue::String("skill file missing".into()));
                o.insert("path".into(), JsonValue::String(path_to_string(&skill_path)));
                drift.push(JsonValue::Object(o));
                None
            }
        };

        for carrier in &files {
            let carrier_path = repo_root.join(carrier);
            let carrier_contents = match fs::read_to_string(&carrier_path) {
                Ok(c) => c,
                Err(_) => {
                    all_ok = false;
                    let mut o = serde_json::Map::new();
                    o.insert("carrier".into(), JsonValue::String(carrier.clone()));
                    o.insert(
                        "error".into(),
                        JsonValue::String("carrier file missing".into()),
                    );
                    drift.push(JsonValue::Object(o));
                    continue;
                }
            };
            let carrier_block = match extract_block(&carrier_contents, name) {
                Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                None => {
                    all_ok = false;
                    let mut o = serde_json::Map::new();
                    o.insert("carrier".into(), JsonValue::String(carrier.clone()));
                    o.insert(
                        "error".into(),
                        JsonValue::String("block markers missing in carrier".into()),
                    );
                    drift.push(JsonValue::Object(o));
                    continue;
                }
            };

            let Some(skill_norm) = skill_norm.as_ref() else {
                // Skill missing — already recorded above; still note the carrier
                // so the operator sees the full pairing set.
                let mut o = serde_json::Map::new();
                o.insert("carrier".into(), JsonValue::String(carrier.clone()));
                o.insert(
                    "error".into(),
                    JsonValue::String("skill unavailable for comparison".into()),
                );
                drift.push(JsonValue::Object(o));
                continue;
            };

            let carrier_norm = normalise_block(&carrier_block);
            if let Some(line_no) = first_difference(skill_norm, &carrier_norm) {
                all_ok = false;
                let mut o = serde_json::Map::new();
                o.insert("carrier".into(), JsonValue::String(carrier.clone()));
                o.insert(
                    "line".into(),
                    JsonValue::Number(serde_json::Number::from(line_no)),
                );
                drift.push(JsonValue::Object(o));
            }
        }

        let block_ok = drift.is_empty();
        block_obj.insert("ok".into(), JsonValue::Bool(block_ok));
        block_obj.insert("drift".into(), JsonValue::Array(drift));
        blocks_out.push(JsonValue::Object(block_obj));
    }

    let mut top = serde_json::Map::new();
    top.insert("ok".into(), JsonValue::Bool(all_ok));
    top.insert("blocks".into(), JsonValue::Array(blocks_out));
    Ok(SkillDriftReport {
        ok: all_ok,
        report: JsonValue::Object(top),
    })
}

/// Return the 1-based index of the first differing normalised line between two
/// line vectors, or `None` if they are equal. A length mismatch is reported at
/// the index of the first absent/extra line (1-based).
#[allow(dead_code)] // reserved for T4 dispatch wiring; used by unit tests.
fn first_difference(a: &[String], b: &[String]) -> Option<u64> {
    let max = a.len().max(b.len());
    for i in 0..max {
        if a.get(i) != b.get(i) {
            return Some((i as u64) + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use crate::integrity::hex_lower;

    /// Fix A: `extract_block` must return `Some(...)` for CRLF input and the
    /// digest must equal the digest produced from the LF equivalent.
    #[test]
    fn extract_block_crlf_matches_lf_digest() {
        let lf = "<!-- SHARED-BLOCK:foo START -->\nline one\nline two\n<!-- SHARED-BLOCK:foo END -->\n";
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

    /// Phase 6 / T6: path-shape rewrites in `blocks_verify` must quote the
    /// expected invocation form when no files are supplied. The rewritten
    /// message embeds an example invocation so an agent sees the shape
    /// directly.
    #[test]
    fn error_message_path_shape_blocks_verify_quotes_expected_invocation() {
        let err = blocks_verify(&[], &[]).unwrap_err().to_string();
        assert!(
            err.contains("no files supplied") && err.contains("tomlctl blocks verify"),
            "path-shape error must quote an example invocation; got: {err}"
        );
    }

    // ---- verify-skills normalisation helpers -----------------------------

    /// Identical bodies must normalise to equal line vectors.
    #[test]
    fn normalise_block_identical_bodies_equal() {
        let body = "line one\nline two\nline three\n";
        assert_eq!(normalise_block(body), normalise_block(body));
        assert_eq!(
            normalise_block(body),
            vec![
                "line one".to_string(),
                "line two".to_string(),
                "line three".to_string()
            ]
        );
    }

    /// Bodies that differ ONLY on a contract cross-reference line — the skill
    /// form (`` `flow-contract-ledger-schema` ``) vs the carrier form
    /// (`SHARED-BLOCK:ledger-schema`) — must normalise equal, because that line
    /// is dropped on both sides.
    #[test]
    fn normalise_block_drops_divergent_cross_reference() {
        let skill = "intro line\n\
                     See the `flow-contract-ledger-schema` skill -> Vet event log section.\n\
                     trailing line\n";
        let carrier = "intro line\n\
                       See `SHARED-BLOCK:ledger-schema` -> `Vet event log` for the full field set.\n\
                       trailing line\n";
        assert_eq!(
            normalise_block(skill),
            normalise_block(carrier),
            "cross-reference lines must be dropped, leaving equal vectors"
        );
        assert_eq!(
            normalise_block(skill),
            vec!["intro line".to_string(), "trailing line".to_string()]
        );
    }

    /// A line mentioning `flow-research-deep` (no `` `flow-contract- ``) is a
    /// substantive procedure reference and MUST be retained.
    #[test]
    fn normalise_block_keeps_flow_research_reference() {
        let body = "re-dispatch that lens to `flow-research-deep` with the reason.\n";
        assert_eq!(
            normalise_block(body),
            vec!["re-dispatch that lens to `flow-research-deep` with the reason.".to_string()]
        );
    }

    /// Bodies differing on a SUBSTANTIVE line must normalise to different
    /// vectors.
    #[test]
    fn normalise_block_substantive_difference_differs() {
        let a = "step one: do the thing\nstep two: verify\n";
        let b = "step one: do a different thing\nstep two: verify\n";
        assert_ne!(normalise_block(a), normalise_block(b));
    }

    /// Trailing whitespace is trimmed and trailing blank lines are dropped.
    #[test]
    fn normalise_block_trims_trailing_ws_and_blanks() {
        let body = "alpha   \nbeta\t\n\n\n";
        assert_eq!(
            normalise_block(body),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    /// Frontmatter strip removes a leading `---\n...\n---\n` block plus the
    /// immediately-following blank line.
    #[test]
    fn strip_frontmatter_removes_leading_block() {
        let body = "---\nname: x\ndescription: y\n---\n\nFirst real line\nSecond line\n";
        assert_eq!(strip_frontmatter(body), "First real line\nSecond line\n");
    }

    /// No leading `---` → content returned unchanged.
    #[test]
    fn strip_frontmatter_no_header_unchanged() {
        let body = "First real line\nSecond line\n";
        assert_eq!(strip_frontmatter(body), body);
    }

    /// Frontmatter strip composed with normalise_block: a skill body (with
    /// frontmatter) and its embedded carrier copy (no frontmatter) that differ
    /// only on the cross-reference line normalise equal.
    #[test]
    fn strip_then_normalise_skill_matches_carrier() {
        let skill = "---\nname: flow-contract-x\ndescription: d\n---\n\
                     **Header.**\n\
                     See the `flow-contract-ledger-schema` skill for fields.\n";
        let carrier = "**Header.**\n\
                       See `SHARED-BLOCK:ledger-schema` for fields.\n";
        assert_eq!(
            normalise_block(strip_frontmatter(skill)),
            normalise_block(strip_frontmatter(carrier))
        );
    }

    /// `first_difference` reports a 1-based index and detects length mismatch.
    #[test]
    fn first_difference_is_one_based() {
        let a = vec!["x".to_string(), "y".to_string()];
        let b = vec!["x".to_string(), "z".to_string()];
        assert_eq!(first_difference(&a, &b), Some(2));

        let c = vec!["x".to_string()];
        let d = vec!["x".to_string(), "extra".to_string()];
        assert_eq!(first_difference(&c, &d), Some(2));

        assert_eq!(first_difference(&a, &a), None);
    }
}
