//! Shared-block parity and skill-gating tests for the dispatch layer.

use crate::blocks::{self, blocks_verify, scan_block_names_warn};
use std::fs;
use std::path::Path;

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
    // R53: a line that looks like a SHARED-BLOCK marker but is malformed
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

#[test]
fn blocks_verify_reproduces_shell_hashes() {
    // R87: pin the hash for every block enumerated in
    // `scripts/shared-blocks.toml`. After the progressive-disclosure
    // wave-2 migration (2026-05-20) every command-carried block was
    // externalised to a flow-contract / apply-* skill and deleted from
    // the manifest, leaving exactly one block: `forbidden-working-tree-ops`,
    // which spans the two implement agent files. Pinning its hash
    // here keeps the test guarding a real surviving block; a drift surfaces
    // independently with a named hash rather than a confusing "missing"
    // report.
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();

    // R87: derive each block's carrier file list from the single source of
    // truth — `scripts/shared-blocks.toml` — rather than duplicating the
    // lists here. The manifest's `files` entries are repo-relative; join
    // each onto `repo_root` to recover the absolute paths the verifier
    // (and the graceful-skip guard) expect.
    let manifest_path = repo_root.join("scripts").join("shared-blocks.toml");
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("blocks_verify_reproduces_shell_hashes: manifest not found, skipping");
            return;
        }
    };
    let manifest: toml::Table = toml::from_str(&manifest_text).expect("parse shared-blocks.toml");
    let carriers_for = |name: &str| -> Vec<std::path::PathBuf> {
        manifest
            .get("block")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|b| b.as_table())
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(name))
            .map(|t| {
                t.get("files")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|f| repo_root.join(f))
                            .collect()
                    })
                    .unwrap_or_default()
            })
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

    // R53: on hash-drift the bare `assertion_failed` message is hard to
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

    // --- 2-file forbidden-working-tree-ops block (the sole surviving
    //     block after the wave-2 migration; spans the two implement
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

/// T4: skill-body↔carrier drift guard for any *hybrid* shared block — one
/// that both carries a `skill` field in `scripts/shared-blocks.toml` AND
/// still has a non-empty `files` list of embedded carrier copies. For every
/// such block `verify_skills` must report `ok == true`; the engine
/// normalises away the expected mechanical differences (frontmatter,
/// contract cross-references), so any drift there is a genuine finding.
///
/// POST-WAVE-2 STATE: this guard is currently DORMANT. After the
/// progressive-disclosure migration the manifest's sole surviving block
/// (`forbidden-working-tree-ops`) has no `skill` field, and every
/// externalised block was deleted from the manifest — so no block satisfies
/// the `skill && files` predicate and `verify_skills` iterates ZERO blocks,
/// returning `ok == true` unconditionally. The test therefore passes
/// vacuously today and exists as a regression latch: it reactivates the
/// moment a future wave reintroduces a hybrid block (a skill body whose
/// contract is still embedded in some agent/carrier that lacks the Skill
/// tool). The assertion logic is deliberately unchanged. Uses the same
/// repo-root resolution + graceful-skip-on-absent-files pattern as
/// `blocks_verify_reproduces_shell_hashes`.
#[test]
fn verify_skills_clean() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let manifest_path = repo_root.join("scripts").join("shared-blocks.toml");
    if !manifest_path.exists() {
        eprintln!("verify_skills_clean: manifest not found, skipping");
        return;
    }
    let report = blocks::verify_skills(&manifest_path).unwrap();
    assert!(
        report.ok,
        "externalised skills must be in semantic sync with their carriers: {:?}",
        report.report
    );
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

/// T6: progressive-disclosure invocation guard. A skeletonised carrier must
/// still INVOKE each `flow-contract-*` skill it delegates to — `command_lint`
/// only catches CLI-flag drift, and `verify_skills_clean` is dormant
/// post-wave-2, so without this test a carrier could silently drop an
/// "Invoke the `flow-contract-X` skill" line and no check would fail. For
/// each migrated carrier we assert the file text mentions every expected
/// skill name. Same repo-root resolution + graceful-skip-on-absent-files
/// pattern as `command_lint`.
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
            if !text.contains(skill) {
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
                if !text.contains(skill) {
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
    // skill silently drops out of linting rather than failing, and
    // `verify_skills_clean` is dormant post-wave-2.
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
