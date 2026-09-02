//! Shared-block parity and skill-gating tests for the dispatch layer.

use crate::blocks::{self, blocks_verify, scan_block_names_warn};
use crate::integrity::hex_lower;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

// ----- blocks verify ---------------------------------------------------

#[test]
fn blocks_verify_detects_drift() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    let good = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
line two
<!-- SHARED-BLOCK:flow-context END -->
";
    fs::write(&a, good).unwrap();
    fs::write(&b, good).unwrap();
    let report = blocks_verify(&[a.clone(), b.clone()], &["flow-context".to_string()]).unwrap();
    assert!(report.ok, "equal content must be ok");

    let drifted = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
DIFFERENT
<!-- SHARED-BLOCK:flow-context END -->
";
    fs::write(&b, drifted).unwrap();
    let report = blocks_verify(&[a, b], &["flow-context".to_string()]).unwrap();
    assert!(!report.ok);
    // drift entries carry per-file hash detail
    let blocks = report
        .report
        .get("blocks")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(blocks.len(), 1);
    let drift_arr = blocks[0].get("drift").and_then(|v| v.as_array()).unwrap();
    assert_eq!(drift_arr.len(), 2);
    let h0 = drift_arr[0].get("hash").and_then(|v| v.as_str()).unwrap();
    let h1 = drift_arr[1].get("hash").and_then(|v| v.as_str()).unwrap();
    assert_ne!(h0, h1, "drift implies distinct hashes");
}

#[test]
fn blocks_verify_missing_marker_reports_per_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    let good = "\
<!-- SHARED-BLOCK:x START -->
body
<!-- SHARED-BLOCK:x END -->
";
    fs::write(&a, good).unwrap();
    fs::write(&b, "nothing here\n").unwrap();
    let report = blocks_verify(&[a, b], &["x".to_string()]).unwrap();
    assert!(!report.ok);
    let blocks = report
        .report
        .get("blocks")
        .and_then(|v| v.as_array())
        .unwrap();
    let missing = blocks[0].get("missing").and_then(|v| v.as_array()).unwrap();
    assert_eq!(missing.len(), 1);
}

#[test]
fn scan_block_names_warn_emits_for_typo_but_keeps_canonical() {
    // A line that looks like a SHARED-BLOCK marker but is malformed
    // (missing hyphen, wrong case, trailing whitespace) must NOT be
    // picked up as a block name, AND must NOT break verification — it's
    // only advisory. We can't easily capture stderr from within a unit
    // test without invasive plumbing; assert on the behavioural
    // guarantees instead: canonical names are still discovered and the
    // typo isn't silently treated as a block.
    let contents = "\
<!-- SHAREDBLOCK:typo START -->
should-be-ignored
<!-- SHAREDBLOCK:typo END -->
<!-- SHARED-BLOCK:real START -->
body
<!-- SHARED-BLOCK:real END -->
";
    let names = scan_block_names_warn(contents, Some("synthetic-fixture"));
    assert_eq!(names, vec!["real".to_string()]);
    // A full verify over two files, each with a typo line, still passes
    // for the canonical block.
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    fs::write(&a, contents).unwrap();
    fs::write(&b, contents).unwrap();
    let report = blocks_verify(&[a, b], &["real".to_string()]).unwrap();
    assert!(
        report.ok,
        "typo lines must not break verification: {:?}",
        report.report
    );
}

/// Every `[[block]]` in `scripts/shared-blocks.toml` as (name, absolute carrier
/// paths), in manifest order. `None` when the manifest is absent.
fn shared_block_manifest(repo_root: &Path) -> Option<Vec<(String, Vec<PathBuf>)>> {
    let manifest_path = repo_root.join("scripts").join("shared-blocks.toml");
    let text = fs::read_to_string(&manifest_path).ok()?;
    let manifest: toml::Table = toml::from_str(&text).expect("parse shared-blocks.toml");
    Some(
        manifest
            .get("block")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|b| b.as_table())
            .filter_map(|t| {
                let name = t.get("name").and_then(|v| v.as_str())?.to_string();
                let files = t
                    .get("files")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|f| repo_root.join(f))
                            .collect()
                    })
                    .unwrap_or_default();
                Some((name, files))
            })
            .collect(),
    )
}

/// A block's digest as `scripts/verify-shared-blocks.sh` computes it: awk emits
/// each line strictly between the two whole-line markers with `\n` as ORS, and
/// sha256 runs over exactly those bytes.
///
/// Re-derived rather than delegating to `blocks::extract_block`, which is the
/// thing under test — sharing the extractor would make the comparison in
/// `blocks_verify_matches_shell_extraction` self-confirming. A trailing `\r` is
/// dropped before the marker comparison because the gawk the hook requires reads
/// in text mode, and some carriers are CRLF on disk. `None` covers the shell's
/// two hard failures — a missing marker, and an empty span between present ones.
fn shell_block_digest(text: &str, name: &str) -> Option<String> {
    let start = format!("<!-- SHARED-BLOCK:{name} START -->");
    let end = format!("<!-- SHARED-BLOCK:{name} END -->");
    let mut inside = false;
    let mut saw_start = false;
    let mut saw_end = false;
    let mut bytes: Vec<u8> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == start {
            inside = true;
            saw_start = true;
            continue;
        }
        if line == end {
            inside = false;
            saw_end = true;
            continue;
        }
        if inside {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
    }
    if !(saw_start && saw_end) || bytes.is_empty() {
        return None;
    }
    Some(hex_lower(&Sha256::digest(&bytes)))
}

/// Parity for every block the manifest names, against a digest re-derived from
/// the shell verifier's own extraction rules. Pinning one block's hash leaves
/// the others enforced solely by the pre-commit hook, so a `--no-verify` commit
/// lands drift in them with `cargo test` still green. Iterating the manifest
/// rather than naming blocks means a block added later is covered from the
/// moment it is listed.
#[test]
fn blocks_verify_matches_shell_extraction() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let Some(manifest) = shared_block_manifest(&repo_root) else {
        eprintln!("blocks_verify_matches_shell_extraction: manifest not found, skipping");
        return;
    };
    if !manifest.iter().flat_map(|(_, f)| f).all(|p| p.exists()) {
        eprintln!("blocks_verify_matches_shell_extraction: carrier files not found, skipping");
        return;
    }
    // The shell verifier exits 2 when the manifest yields no (block, file)
    // pairs; an empty manifest here would otherwise be an unconditional pass.
    assert!(
        !manifest.is_empty(),
        "scripts/shared-blocks.toml declares no `[[block]]` entries — \
         this gate would check nothing"
    );

    let mut failures: Vec<String> = Vec::new();
    for (name, carriers) in &manifest {
        // A single-carrier block is trivially in parity with itself.
        if carriers.len() < 2 {
            failures.push(format!(
                "{name}: {} carrier(s) listed — parity needs at least two",
                carriers.len()
            ));
            continue;
        }

        let mut digests: Vec<(String, String)> = Vec::new();
        for carrier in carriers {
            let rel = repo_relative(carrier, &repo_root);
            let text = fs::read_to_string(carrier).expect("read carrier");
            match shell_block_digest(&text, name) {
                Some(d) => digests.push((rel, d)),
                None => failures.push(format!(
                    "{name}: {rel} has no non-empty span between its markers"
                )),
            }
        }
        if digests.len() != carriers.len() {
            continue;
        }

        let (ref first_file, ref expected) = digests[0];
        if let Some((rel, d)) = digests.iter().find(|(_, d)| d != expected) {
            failures.push(format!(
                "{name}: carriers disagree — {first_file}: {expected} vs {rel}: {d}"
            ));
            continue;
        }

        let report = blocks_verify(carriers, std::slice::from_ref(name)).unwrap();
        if !report.ok {
            failures.push(format!(
                "{name}: blocks_verify reports drift: {:?}",
                report.report
            ));
            continue;
        }
        let hash = report
            .report
            .get("blocks")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|b| b.get("name").and_then(|v| v.as_str()) == Some(name.as_str()))
            })
            .and_then(|b| b.get("hash"))
            .and_then(|v| v.as_str());
        match hash {
            Some(h) if h == expected => {}
            Some(h) => failures.push(format!(
                "{name}: blocks_verify hashed {h}, the shell's extraction hashes {expected}"
            )),
            None => failures.push(format!(
                "{name}: blocks_verify reported no single hash: {:?}",
                report.report
            )),
        }
    }

    if !failures.is_empty() {
        let mut msg = String::from(
            "blocks_verify_matches_shell_extraction: shared block(s) are not in \
             shell-equivalent parity:\n",
        );
        for f in &failures {
            msg.push_str(&format!("  {f}\n"));
        }
        msg.push_str(
            "  fix: restore the block byte-identically across its carriers \
             (`bash scripts/verify-shared-blocks.sh` reports the same set)",
        );
        panic!("{msg}");
    }
}

#[test]
fn blocks_verify_reproduces_shell_hashes() {
    // Pin the shell verifier's hash for `forbidden-working-tree-ops`, one of
    // the blocks `scripts/shared-blocks.toml` enumerates, so this Rust mirror
    // is held to the same bytes `scripts/verify-shared-blocks.sh` computes. A
    // drift then surfaces as a named-hash mismatch rather than a confusing
    // "missing" report.
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();

    // Derive each block's carrier file list from the single source of
    // truth — `scripts/shared-blocks.toml` — rather than duplicating the
    // lists here. The manifest's `files` entries are repo-relative; join
    // each onto `repo_root` to recover the absolute paths the verifier
    // (and the graceful-skip guard) expect.
    let Some(manifest) = shared_block_manifest(&repo_root) else {
        eprintln!("blocks_verify_reproduces_shell_hashes: manifest not found, skipping");
        return;
    };
    let carriers_for = |name: &str| -> Vec<PathBuf> {
        manifest
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, files)| files.clone())
            .unwrap_or_default()
    };

    let forbidden_pair = carriers_for("forbidden-working-tree-ops");

    // Only run when every file is present. The test crate is consumable
    // in isolation; degrade gracefully if someone packages it without
    // the command tree.
    if !forbidden_pair.iter().all(|p| p.exists()) {
        eprintln!("blocks_verify_reproduces_shell_hashes: command files not found, skipping");
        return;
    }

    // On hash-drift the bare `assertion_failed` message is hard to
    // act on — the caller sees "block X hash drift" and has to reverse-
    // engineer both the actual hash and which file(s) moved. Emit a
    // structured multi-line message instead:
    //   - expected (the pinned constant that's now stale)
    //   - actual   (the in-parity hash currently produced by the blocks
    //              under test; absent when parity itself is broken)
    //   - per-file hashes (for parity: the single hash each file maps
    //              to; for drift: every (file, hash) pair so the
    //              operator can spot the outlier file without re-running)
    //   - remediation (the literal pinned-hash constant update to make)
    let expect_hash = |report: &blocks::BlocksReport, name: &str, expected: &str| {
        let blocks_arr = report
            .report
            .get("blocks")
            .and_then(|v| v.as_array())
            .expect("blocks array");
        let block = blocks_arr
            .iter()
            .find(|b| b.get("name").and_then(|v| v.as_str()) == Some(name))
            .unwrap_or_else(|| panic!("block `{name}` missing from report: {:?}", report.report));

        // The "happy" shape (`blocks_verify` reports parity): a single
        // `hash` field + a `files` array. Compare the pinned constant
        // against it; on mismatch, print every contributing file so the
        // operator can copy the new hash into the source.
        if let Some(hash) = block.get("hash").and_then(|v| v.as_str()) {
            if hash != expected {
                let files: Vec<String> = block
                    .get("files")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut msg = String::new();
                msg.push_str(&format!("block `{name}` pinned-hash drift\n"));
                msg.push_str(&format!("  expected: {expected}\n"));
                msg.push_str(&format!("  actual:   {hash}\n"));
                msg.push_str("  per-file (all match each other):\n");
                for f in &files {
                    msg.push_str(&format!("    {f}: {hash}\n"));
                }
                msg.push_str(&format!(
                    "  fix: update the pinned hash for `{name}` to {hash}"
                ));
                panic!("{msg}");
            }
            return;
        }

        // The "sad" shape: `blocks_verify` already detected drift across
        // files — there is no single `hash`, only a `drift` array of
        // per-file hashes. Emit all of them so the operator can see
        // both WHICH file moved and whether the pinned constant is
        // stale as well.
        let drift_arr = block
            .get("drift")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| {
                panic!("block `{name}` has neither `hash` nor `drift`: {:?}", block)
            });
        let mut msg = String::new();
        msg.push_str(&format!(
            "block `{name}` parity broken across files (pre-pinned-hash check)\n"
        ));
        msg.push_str(&format!("  expected (pinned): {expected}\n"));
        msg.push_str("  per-file hashes (should be identical, but differ):\n");
        for entry in drift_arr {
            let f = entry
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let h = entry
                .get("hash")
                .and_then(|v| v.as_str())
                .unwrap_or("<no-hash>");
            msg.push_str(&format!("    {f}: {h}\n"));
        }
        msg.push_str(
            "  fix: restore block parity across the listed files first, \
             then re-run this test to see whether the pinned constant \
             also needs updating",
        );
        panic!("{msg}");
    };

    // --- 2-file forbidden-working-tree-ops block (spans the two implement
    //     agent files) ---
    let report =
        blocks_verify(&forbidden_pair, &["forbidden-working-tree-ops".to_string()]).unwrap();
    assert!(
        report.ok,
        "forbidden-working-tree-ops block must be parity: {:?}",
        report.report
    );
    expect_hash(
        &report,
        "forbidden-working-tree-ops",
        "4701d2b8314c997366dfea83b6aa4e0bff5e275d2e1378517b67e5091d7fa026",
    );
}

/// The date-key names enumerated in the prose of the tomlctl skill's `write`
/// reference: the run of backtick-quoted names between the two em-dashes that
/// follow the `` `DATE_KEYS` set `` anchor.
///
/// The search is clamped to the paragraph carrying the anchor, so a re-wrapped
/// sentence still parses while a missing dash cannot reach forward and collect
/// unrelated backticked prose. A rewrite that drops the anchor, either dash, or
/// the backticks yields an empty set — which the caller must reject rather than
/// read as agreement.
fn enumerated_date_keys(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(anchor) = text.find("`DATE_KEYS` set") else {
        return out;
    };
    let tail = &text[anchor..];
    let paragraph_end = tail
        .find("\n\n")
        .into_iter()
        .chain(tail.find("\n\r\n"))
        .min()
        .unwrap_or(tail.len());
    let rest = &tail[..paragraph_end];
    let Some(open) = rest.find('—') else {
        return out;
    };
    let after = &rest[open + '—'.len_utf8()..];
    let Some(close) = after.find('—') else {
        return out;
    };

    let segments: Vec<&str> = after[..close].split('`').collect();
    let mut i = 1;
    while i + 1 < segments.len() {
        out.insert(segments[i].to_string());
        i += 2;
    }
    out
}

/// `DATE_KEYS` is enumerated verbatim in the tomlctl skill's `write` reference,
/// with nothing but a doc comment asking the next editor to widen both. That
/// instruction has already been missed once. The crate gates the slice against
/// its `is_date_key` jump table the same way.
#[test]
fn write_reference_enumerates_every_date_key() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let reference = repo_root
        .join("claude")
        .join("skills")
        .join("tomlctl")
        .join("references")
        .join("write.md");
    let Ok(text) = fs::read_to_string(&reference) else {
        eprintln!("write_reference_enumerates_every_date_key: write.md not found, skipping");
        return;
    };
    let rel = repo_relative(&reference, &repo_root);

    let documented = enumerated_date_keys(&text);
    assert!(
        !documented.is_empty(),
        "write_reference_enumerates_every_date_key: parsed zero names out of \
         {rel} — the enumeration must stay a run of backtick-quoted names \
         between the two em-dashes following the `DATE_KEYS` set anchor, or \
         this gate compares an empty set against itself"
    );

    let declared: BTreeSet<String> = crate::convert::DATE_KEYS
        .iter()
        .map(|k| (*k).to_string())
        .collect();
    if documented != declared {
        let mut msg = format!(
            "write_reference_enumerates_every_date_key: {rel} and `DATE_KEYS` in \
             tomlctl/src/convert.rs disagree:\n"
        );
        let undocumented: Vec<&String> = declared.difference(&documented).collect();
        let stale: Vec<&String> = documented.difference(&declared).collect();
        if !undocumented.is_empty() {
            msg.push_str(&format!(
                "  in DATE_KEYS, absent from the reference: {undocumented:?}\n"
            ));
        }
        if !stale.is_empty() {
            msg.push_str(&format!(
                "  in the reference, absent from DATE_KEYS: {stale:?}\n"
            ));
        }
        msg.push_str("  fix: widen the constant and the reference in lockstep");
        panic!("{msg}");
    }
}

/// The extractor behind `write_reference_enumerates_every_date_key`, over
/// synthetic prose so the live reference cannot make it look right by accident.
/// Every rejected shape returns the empty set, which is exactly the input the
/// caller's non-empty assertion exists to catch.
#[test]
fn enumerated_date_keys_needs_the_documented_shape() {
    let sentence = "Date-shaped strings (`YYYY-MM-DD`) in the `DATE_KEYS` set — \
                    `created`, `updated`, `last_seen` — are promoted. The \
                    `DATE_KEYS` constant in `tomlctl/src/convert.rs` owns the set.";
    let keys = enumerated_date_keys(sentence);
    assert_eq!(
        keys,
        ["created", "last_seen", "updated"]
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "names outside the em-dash span must not leak in"
    );

    for empty in [
        // No anchor: the sentence was rewritten around the constant.
        "the promoted keys are `created`, `updated`",
        // Anchor, but the enumeration lost its dashes.
        "keys in the `DATE_KEYS` set: `created`, `updated` are promoted",
        // Only an opening dash — an unterminated span names nothing.
        "the `DATE_KEYS` set — `created`, `updated` are promoted",
        // Dashes, but the names lost their backticks.
        "the `DATE_KEYS` set — created, updated — are promoted",
    ] {
        assert!(
            enumerated_date_keys(empty).is_empty(),
            "must parse to nothing rather than a partial set: {empty}"
        );
    }
}

/// Anthropic's skill-authoring guidance caps a SKILL.md body at 500 lines
/// and nothing upstream enforces it, so a body drifts past the ceiling with
/// no signal at all. Same graceful skip as `command_lint` for a checkout
/// without the harness tree.
#[test]
fn skill_bodies_under_line_ceiling() {
    const CEILING: usize = 500;

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let skills_dir = repo_root.join("claude").join("skills");
    if !skills_dir.exists() {
        eprintln!("skill_bodies_under_line_ceiling: claude/skills/ not found, skipping");
        return;
    }

    let mut offenders: Vec<(String, usize)> = Vec::new();
    for entry in fs::read_dir(&skills_dir)
        .expect("read claude/skills")
        .flatten()
    {
        let body = entry.path().join("SKILL.md");
        let Ok(text) = fs::read_to_string(&body) else {
            continue;
        };
        let lines = text.lines().count();
        if lines > CEILING {
            let rel = body
                .strip_prefix(&repo_root)
                .unwrap_or(&body)
                .to_string_lossy()
                .replace('\\', "/");
            offenders.push((rel, lines));
        }
    }
    offenders.sort();

    if !offenders.is_empty() {
        let mut msg = format!(
            "skill_bodies_under_line_ceiling: {} skill body/bodies over the \
             {CEILING}-line ceiling. Move the overflow into \
             `references/*.md` and leave a navigational body:\n",
            offenders.len()
        );
        for (f, n) in &offenders {
            msg.push_str(&format!("  {f}: {n} lines ({} over)\n", n - CEILING));
        }
        panic!("{msg}");
    }
}

/// A reference file is where an over-long body gets moved to, so leaving it
/// ungated just relocates the drift. The ceiling is looser than a body's:
/// a reference is read on demand rather than loaded with the skill.
#[test]
fn skill_references_under_line_ceiling() {
    const CEILING: usize = 600;

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let skills_dir = repo_root.join("claude").join("skills");
    if !skills_dir.exists() {
        eprintln!("skill_references_under_line_ceiling: claude/skills/ not found, skipping");
        return;
    }

    let mut offenders: Vec<(String, usize)> = Vec::new();
    for entry in fs::read_dir(&skills_dir)
        .expect("read claude/skills")
        .flatten()
    {
        let references = entry.path().join("references");
        let Ok(files) = fs::read_dir(&references) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let lines = text.lines().count();
            if lines > CEILING {
                let rel = path
                    .strip_prefix(&repo_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                offenders.push((rel, lines));
            }
        }
    }
    offenders.sort();

    if !offenders.is_empty() {
        let mut msg = format!(
            "skill_references_under_line_ceiling: {} reference file(s) over the \
             {CEILING}-line ceiling. Split the overflow into a further \
             `references/*.md` and link it from the body:\n",
            offenders.len()
        );
        for (f, n) in &offenders {
            msg.push_str(&format!("  {f}: {n} lines ({} over)\n", n - CEILING));
        }
        panic!("{msg}");
    }
}

/// Every markdown file the link check scans: each skill's body plus one level
/// of its `references/`, sorted.
fn skill_markdown_files(skills_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }
        let body = skill_dir.join("SKILL.md");
        if body.is_file() {
            files.push(body);
        }
        if let Ok(refs) = fs::read_dir(skill_dir.join("references")) {
            for r in refs.flatten() {
                let p = r.path();
                if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("md") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    files
}

/// The inline-link targets in `text`, in source order, with any `"title"`
/// suffix dropped. Reference-style links (`[a][b]`) carry no target and an
/// escaped `\](` is not a link, so neither appears.
fn markdown_link_targets(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("](") {
        let open = cursor + rel + 2;
        cursor = open;
        if open >= 3 && bytes[open - 3] == b'\\' {
            continue;
        }
        let Some(close) = text[open..].find(')') else {
            break;
        };
        let target = text[open..open + close].trim();
        cursor = open + close + 1;
        if let Some(target) = target.split_whitespace().next() {
            out.push(target);
        }
    }
    out
}

/// Collapse `.` and `..` without touching the filesystem, so a resolved path
/// is reportable as the location a reader would go and create.
fn lexical_normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn repo_relative(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Link-existence outcome over one file set.
#[derive(Default)]
struct MarkdownLinkReport {
    /// Relative path targets resolved against their linking file's directory.
    /// Zero means the walk found nothing and the check proved nothing.
    checked: usize,
    /// (linking file, raw target, resolved path that is absent).
    broken: Vec<(String, String, String)>,
}

/// Resolve every relative path link in `files` against its own linking file's
/// directory. Absolute URLs and pure-anchor links name no path; a `#fragment`
/// on a path link is dropped, since anchor resolution needs GitHub's slug
/// algorithm and is out of scope.
fn check_markdown_links(files: &[PathBuf], repo_root: &Path) -> MarkdownLinkReport {
    let mut report = MarkdownLinkReport::default();

    for file in files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let rel = repo_relative(file, repo_root);
        let dir = file.parent().unwrap_or(Path::new("."));

        for target in markdown_link_targets(&text) {
            if target.starts_with('#') || target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let path_part = target.split('#').next().unwrap_or("");
            if path_part.is_empty() {
                continue;
            }
            report.checked += 1;
            let resolved = dir.join(path_part);
            if !resolved.exists() {
                report.broken.push((
                    rel.clone(),
                    target.to_string(),
                    repo_relative(&lexical_normalise(&resolved), repo_root),
                ));
            }
        }
    }

    report
}

/// A skill's cross-references are its navigation, and a body trimmed under the
/// line ceiling pushes ever more of itself behind them — so a link to a file
/// that was renamed or never split out strands the content silently. Same
/// graceful skip as `command_lint` for a checkout without the harness tree.
#[test]
fn skill_markdown_links_resolve() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let skills_dir = repo_root.join("claude").join("skills");
    if !skills_dir.exists() {
        eprintln!("skill_markdown_links_resolve: claude/skills/ not found, skipping");
        return;
    }

    let files = skill_markdown_files(&skills_dir);
    let report = check_markdown_links(&files, &repo_root);

    // A walk rooted at the wrong directory, or an extractor that never fires,
    // yields an empty broken list and a green test that gated nothing.
    assert!(
        report.checked > 0,
        "skill_markdown_links_resolve: scanned {} file(s) and found zero relative \
         links to check — the walk or the link extractor is broken, not the corpus",
        files.len()
    );

    if !report.broken.is_empty() {
        let mut msg = format!(
            "skill_markdown_links_resolve: {} of {} relative link(s) point at a \
             path that does not exist:\n",
            report.broken.len(),
            report.checked
        );
        for (file, target, resolved) in &report.broken {
            msg.push_str(&format!(
                "  {file}\n    link:     {target}\n    resolved: {resolved}\n"
            ));
        }
        panic!("{msg}");
    }
}

/// The link checker over a temp tree, so the live corpus cannot make it pass
/// by accident: a missing sibling is named, traversal resolves against the
/// linking file rather than the root, and anchors/URLs are not paths.
#[test]
fn skill_markdown_links_flags_a_missing_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let skills = root.join("claude").join("skills");
    let skill = skills.join("x");
    fs::create_dir_all(skill.join("references")).unwrap();

    fs::write(skill.join("references").join("present.md"), "# present\n").unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "See [present](references/present.md) and [gone](references/gone.md).\n\
         Not paths: [anchor](#a-section), [url](https://example.com/nope.md).\n\
         A fragment on a path checks the path only: [p](references/present.md#verbs).\n",
    )
    .unwrap();
    fs::write(
        skill.join("references").join("nested.md"),
        "Traversal is relative to this file: [body](../SKILL.md), [up](../../../nope.md).\n",
    )
    .unwrap();

    let files = skill_markdown_files(&skills);
    assert_eq!(
        files.len(),
        3,
        "scan set must reach body + references: {files:?}"
    );

    let report = check_markdown_links(&files, root);
    assert_eq!(
        report.checked, 5,
        "anchors and URLs must not be counted as paths: {:?}",
        report.broken
    );

    let broken: Vec<&str> = report.broken.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(
        broken,
        ["references/gone.md", "../../../nope.md"],
        "every broken link must be reported, not just the first: {:?}",
        report.broken
    );
    assert_eq!(
        report.broken[0].2, "claude/skills/x/references/gone.md",
        "the report must name the resolved path: {:?}",
        report.broken
    );
    assert_eq!(
        report.broken[1].2, "claude/nope.md",
        "`../` must resolve against the linking file's directory: {:?}",
        report.broken
    );
}

/// Whether `text` invokes `skill`, as opposed to merely naming it.
///
/// An invocation is a backtick-quoted skill name followed by the word "skill"
/// ("Invoke the `x` skill", "run the `x` skill's gate") whose clause carries no
/// negation cue — so a cross-reference, a path, or a sentence warning against
/// the skill does not qualify. One qualifying phrase in the file is enough.
fn invokes_skill(text: &str, skill: &str) -> bool {
    const NEGATIONS: &[&str] = &[
        "do not",
        "don't",
        "never",
        "no longer",
        "must not",
        "cannot",
        "can't",
        "rather than",
        "instead of",
    ];
    // How far back a negation cue may sit and still govern the phrase.
    const CLAUSE_WINDOW: usize = 160;

    let quoted = format!("`{skill}`");
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(&quoted) {
        let start = cursor + rel;
        let after_quote = start + quoted.len();
        cursor = after_quote;

        // Markdown emphasis and a wrapped line break can both sit between the
        // closing backtick and the noun.
        let tail = text[after_quote..]
            .trim_start_matches(|c: char| c.is_whitespace() || c == '*' || c == '_');
        let Some(head) = tail.get(..5) else {
            continue;
        };
        if !head.eq_ignore_ascii_case("skill") {
            continue;
        }
        // Accepts "skill", "skill's", "skill**"; rejects "skillset".
        if tail[5..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }

        let window_start = text[..start]
            .char_indices()
            .rev()
            .take(CLAUSE_WINDOW)
            .last()
            .map_or(0, |(i, _)| i);
        let preceding = text[window_start..start].to_ascii_lowercase();
        let clause = match preceding.rfind(['.', '!', '?', '\n']) {
            Some(i) => &preceding[i + 1..],
            None => &preceding[..],
        };
        if NEGATIONS.iter().any(|n| clause.contains(n)) {
            continue;
        }
        return true;
    }
    false
}

/// Progressive-disclosure invocation guard. A skeletonised carrier must
/// still INVOKE each `flow-contract-*` skill it delegates to — `command_lint`
/// only catches CLI-flag drift, so without this test a carrier could silently
/// drop an "Invoke the `flow-contract-X` skill" line and no check would fail. For
/// each migrated carrier we assert the file text carries an invocation phrase
/// (see `invokes_skill`) for every expected skill. Same repo-root resolution +
/// graceful-skip-on-absent-files pattern as `command_lint`.
#[test]
fn carrier_invokes_required_skills() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let commands_dir = repo_root.join("claude").join("commands");
    if !commands_dir.exists() {
        eprintln!("carrier_invokes_required_skills: claude/commands/ not found, skipping");
        return;
    }

    // Expected carrier → required flow-contract skills mapping (verified
    // during the wave-2 review, extended when `flow-contract-task-visibility`
    // landed across every multi-step carrier). A miss means a skeletonised
    // carrier dropped its delegation to a skill it must still invoke.
    //
    // `flow-contract-task-visibility` is listed for every carrier on
    // purpose. Its whole failure mode is silence: the Task tools sit behind
    // a model-version gate that has caught this project before, so a carrier
    // that quietly loses the invocation produces a run that looks completely
    // normal and simply renders no progress anywhere. There is no runtime
    // signal to catch that, which makes this the main guard against a
    // dropped line — but not a complete one: the test self-skips when
    // `claude/commands/` is absent (a packaged or partial checkout), so it
    // is a guard for this repo's own tree rather than an absolute one.
    let expected: &[(&str, &[&str])] = &[
        (
            "implement.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-execution-record-schema",
                "flow-contract-task-visibility",
                "backlog-capture",
            ],
        ),
        (
            "review.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-ledger-schema",
                "flow-contract-ledger-disposition-sweep",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
                "backlog-capture",
            ],
        ),
        (
            "plan-new.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-plansdirectory-prompt",
                "flow-contract-plan-output-format",
                "flow-contract-execution-record-schema",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "review-plan.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-plansdirectory-prompt",
                "flow-contract-plan-output-format",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "tdd.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-execution-record-schema",
                "flow-contract-task-visibility",
                "backlog-capture",
            ],
        ),
        (
            "optimise.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-ledger-schema",
                "flow-contract-ledger-disposition-sweep",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
                "backlog-capture",
            ],
        ),
        (
            "optimise-apply.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-ledger-schema",
                "flow-contract-apply-pipeline",
                "flow-contract-apply-dependency-sort",
                "flow-contract-apply-vet-implement-lite",
                "flow-contract-apply-rollback-protocol",
                "flow-contract-apply-constraints",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "review-apply.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-ledger-schema",
                "flow-contract-apply-pipeline",
                "flow-contract-apply-dependency-sort",
                "flow-contract-apply-vet-implement-lite",
                "flow-contract-apply-rollback-protocol",
                "flow-contract-apply-constraints",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "plan-update.md",
            &[
                "flow-contract-flow-context",
                "flow-contract-plansdirectory-prompt",
                "flow-contract-execution-record-schema",
                "flow-contract-plan-restructure",
                "flow-contract-reconciler",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "test-bootstrap.md",
            &[
                "flow-contract-showcase-bundle",
                "flow-contract-vet-research",
                "flow-contract-task-visibility",
            ],
        ),
        (
            "backlog.md",
            &["flow-contract-task-visibility", "backlog-capture"],
        ),
    ];

    // Plugin orchestrators live outside `claude/commands/` but are carriers
    // in every sense that matters here: `run-sprint` drives an agent team for
    // hours, `plan-story` drives a six-stage machine with research fan-out.
    // Scanning only the commands tree is what let them sit uncovered.
    let plugin_skills_dir = repo_root
        .join("claude")
        .join("plugins")
        .join("lumina-story-blocks")
        .join("skills");
    let expected_plugins: &[(&str, &[&str])] = &[
        ("run-sprint", &["flow-contract-task-visibility"]),
        ("plan-story", &["flow-contract-task-visibility"]),
    ];

    let mut missing: Vec<String> = Vec::new();
    for (carrier, skills) in expected {
        let path = commands_dir.join(carrier);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                // A carrier we expect but cannot read is itself a miss.
                missing.push(format!("{carrier}: file not readable"));
                continue;
            }
        };
        for skill in *skills {
            if !invokes_skill(&text, skill) {
                missing.push(format!("{carrier}: missing invocation of `{skill}`"));
            }
        }
    }

    // Plugin carriers are scanned only when the plugin tree is present, so a
    // checkout without it degrades the same way the commands scan does.
    if plugin_skills_dir.exists() {
        for (carrier, skills) in expected_plugins {
            let path = plugin_skills_dir.join(carrier).join("SKILL.md");
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => {
                    missing.push(format!("plugins/{carrier}: SKILL.md not readable"));
                    continue;
                }
            };
            for skill in *skills {
                if !invokes_skill(&text, skill) {
                    missing.push(format!(
                        "plugins/{carrier}: missing invocation of `{skill}`"
                    ));
                }
            }
        }
    }

    // Asserting the invocation TEXT is only half the contract: a carrier can
    // faithfully name a skill that no longer exists, and nothing else catches
    // that — `command_lint` gates its scan on `skill.exists()`, so a deleted
    // skill silently drops out of linting rather than failing.
    let mut required: Vec<&str> = expected
        .iter()
        .flat_map(|(_, skills)| skills.iter().copied())
        .chain(
            expected_plugins
                .iter()
                .flat_map(|(_, skills)| skills.iter().copied()),
        )
        .collect();
    required.sort_unstable();
    required.dedup();
    let skills_dir = repo_root.join("claude").join("skills");
    for skill in required {
        if !skills_dir.join(skill).join("SKILL.md").is_file() {
            missing.push(format!(
                "claude/skills/{skill}/SKILL.md: required by a carrier but absent"
            ));
        }
    }

    if !missing.is_empty() {
        let mut msg = String::from(
            "carrier_invokes_required_skills: skeletonised carrier(s) dropped a \
             required flow-contract skill invocation:\n",
        );
        for m in &missing {
            msg.push_str(&format!("  {m}\n"));
        }
        panic!("{msg}");
    }
}

/// The predicate behind `carrier_invokes_required_skills`. A bare-substring
/// check passes every one of the rejected fixtures below, which is what made
/// the guard satisfiable by prose.
#[test]
fn invokes_skill_requires_an_invocation_phrase() {
    let accepted = [
        "Invoke the `flow-contract-task-visibility` skill to load the surface.",
        "5. **Task surface** — invoke the `flow-contract-task-visibility` skill for the run.",
        "run the `backlog-capture` skill's check-then-add gate before minting",
        "honour the contract — **invoke the `flow-contract-plan-restructure` skill** to load it",
        "see the\n`flow-contract-showcase-bundle` skill.",
    ];
    for text in accepted {
        assert!(
            invokes_skill(text, text.split('`').nth(1).unwrap()),
            "should count as an invocation: {text}"
        );
    }

    // A carrier whose only mention is a warning against the skill.
    let negative_only = "\
# /demo

Do NOT invoke the `flow-contract-task-visibility` skill here; this carrier is
single-step and mints no task entries.
";
    assert!(
        !invokes_skill(negative_only, "flow-contract-task-visibility"),
        "a negative sentence must not satisfy the invocation guard"
    );

    let rejected = [
        // Bare name, no noun — a cross-reference, not a delegation.
        (
            "per the `flow-contract-ledger-schema` contract",
            "flow-contract-ledger-schema",
        ),
        // Unquoted prose mention.
        (
            "the flow-contract-vet-research skill is documented elsewhere",
            "flow-contract-vet-research",
        ),
        // A frontmatter/path occurrence with no phrase at all.
        ("claude/skills/backlog-capture/SKILL.md", "backlog-capture"),
        // Word-boundary: the noun must be "skill".
        (
            "the `flow-contract-flow-context` skillset overview",
            "flow-contract-flow-context",
        ),
    ];
    for (text, skill) in rejected {
        assert!(
            !invokes_skill(text, skill),
            "should not count as an invocation: {text}"
        );
    }
}
