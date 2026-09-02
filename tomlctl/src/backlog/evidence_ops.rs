//! `backlog evidence dir` and `backlog evidence audit` — the filesystem side
//! of the per-item evidence drop-boxes.
//!
//! `dir` is the only writer in the pair and it writes exactly one file, the
//! marker. It reads the store and never rewrites it, so `backlog.toml` and
//! its sidecar stay byte-identical across any number of calls.
//!
//! `audit` reports and nothing else — no delete, no move, no rename. A
//! drop-box is a human's working area, so a stale finding costs less than a
//! swept-away capture. Only the first five classes are strict-worthy:
//! `tracked` is what a deliberate `git add -f` looks like from the outside,
//! and `empty` is the expected state of every drop-box in a fresh clone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::evidence::{self, EVIDENCE_EXTENSIONS, EVIDENCE_MAX_BYTES, MARKER_NAME};
use super::schema::{self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_ID, FIELD_SUMMARY};
use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::maybe_verify_integrity;
use crate::io::{
    atomic_write, guard_write_path, items_array, read_dir_sorted, read_toml, relativise,
    repo_or_cwd_root,
};
use crate::output::{print_json, print_json_compact};

pub(crate) const CLASS_UNOWNED: &str = "unowned";
pub(crate) const CLASS_NO_MARKER: &str = "no-marker";
pub(crate) const CLASS_OVERSIZE: &str = "oversize";
pub(crate) const CLASS_DISALLOWED_EXTENSION: &str = "disallowed-extension";
pub(crate) const CLASS_REFERENCED_MISSING: &str = "referenced-missing";
pub(crate) const CLASS_TRACKED: &str = "tracked";
pub(crate) const CLASS_EMPTY: &str = "empty";
pub(crate) const CLASS_GIT_UNAVAILABLE: &str = "git-unavailable";

/// Emitted in this order under `counts`, every class present with a zero so
/// a consumer can index the map without a presence check.
const CLASSES: &[&str] = &[
    CLASS_UNOWNED,
    CLASS_NO_MARKER,
    CLASS_OVERSIZE,
    CLASS_DISALLOWED_EXTENSION,
    CLASS_REFERENCED_MISSING,
    CLASS_TRACKED,
    CLASS_EMPTY,
    CLASS_GIT_UNAVAILABLE,
];

/// The classes `--strict` exits non-zero on.
const STRICT_CLASSES: &[&str] = &[
    CLASS_UNOWNED,
    CLASS_NO_MARKER,
    CLASS_OVERSIZE,
    CLASS_DISALLOWED_EXTENSION,
    CLASS_REFERENCED_MISSING,
];

#[derive(Debug)]
pub(crate) struct DirOutcome {
    pub(crate) id: String,
    pub(crate) dir: PathBuf,
    pub(crate) created: bool,
    pub(crate) files: usize,
}

/// Resolve `id` against the store and hand back its drop-box, creating the
/// directory and marker unless `no_create`. `created` tracks the MARKER, not
/// the directory: a drop-box someone made by hand still gets its marker, and
/// an existing marker is never rewritten — `add --on-duplicate bump` leaves
/// `summary` alone, so a rewrite could only ever produce the same bytes with
/// a fresh mtime.
pub(crate) fn ensure_dir(doc: &TomlValue, id: &str, no_create: bool) -> Result<DirOutcome> {
    let id = evidence::resolve_id(doc, id)?;
    let dir = evidence::dir_for(&id)?;

    if no_create {
        let files = evidence::list_dir(&dir)?.ok_or_else(|| {
            tagged_err(
                ErrorKind::NotFound,
                Some(dir.clone()),
                format!(
                    "no evidence directory for \"{id}\" at {}; drop --no-create to create it",
                    dir.display()
                ),
            )
        })?;
        return Ok(DirOutcome {
            id,
            dir,
            created: false,
            files: files.len(),
        });
    }

    let marker = dir.join(MARKER_NAME);
    let created = !marker.exists();
    if created {
        let summary = find_item(doc, &id)
            .and_then(|item| item.get(FIELD_SUMMARY))
            .and_then(TomlValue::as_str)
            .unwrap_or_default();
        // Also the containment-bounded `mkdir -p` of the drop-box itself.
        guard_write_path(&marker, false)?;
        atomic_write(&marker, evidence::marker_text(&id, summary).as_bytes())?;
    }
    let files = evidence::list_dir(&dir)?.map_or(0, |f| f.len());
    Ok(DirOutcome {
        id,
        dir,
        created,
        files,
    })
}

pub(crate) fn dispatch_dir(id: String, no_create: bool) -> Result<()> {
    let store = schema::backlog_path()?;
    let doc = read_toml(&store)?;
    let outcome = ensure_dir(&doc, &id, no_create)?;
    let root = repo_or_cwd_root()?;
    print_json_compact(&json!({
        "ok": true,
        "id": outcome.id,
        "dir": relativise(&root, &outcome.dir),
        "created": outcome.created,
        "files": outcome.files,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) class: &'static str,
    pub(crate) dir: String,
    pub(crate) file: Option<String>,
    pub(crate) detail: String,
}

impl Finding {
    fn to_json(&self) -> JsonValue {
        let mut map = serde_json::Map::new();
        map.insert("class".to_string(), json!(self.class));
        map.insert("dir".to_string(), json!(self.dir));
        if let Some(file) = &self.file {
            map.insert("file".to_string(), json!(file));
        }
        map.insert("detail".to_string(), json!(self.detail));
        JsonValue::Object(map)
    }
}

pub(crate) struct AuditReport {
    pub(crate) root: String,
    pub(crate) findings: Vec<Finding>,
}

impl AuditReport {
    pub(crate) fn to_json(&self) -> JsonValue {
        let mut counts = serde_json::Map::new();
        for class in CLASSES {
            let n = self.findings.iter().filter(|f| f.class == *class).count();
            counts.insert((*class).to_string(), json!(n));
        }
        json!({
            "root": self.root,
            "findings": self.findings.iter().map(Finding::to_json).collect::<Vec<_>>(),
            "counts": JsonValue::Object(counts),
        })
    }

    pub(crate) fn strict_failures(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| STRICT_CLASSES.contains(&f.class))
            .count()
    }
}

/// Walk the immediate subdirectories of `root`. `doc` is `None` when the
/// store is absent, which makes every drop-box `unowned` — the honest answer,
/// since nothing then claims any of them.
pub(crate) fn audit(doc: Option<&TomlValue>, root: &Path, max_bytes: u64) -> Result<AuditReport> {
    let repo = repo_or_cwd_root()?;
    let mut findings: Vec<Finding> = Vec::new();
    let mut candidates: Vec<(PathBuf, String, String)> = Vec::new();

    if root.is_dir() {
        for entry in read_dir_sorted(root)? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let dir = relativise(&repo, &path);
            let item = doc.and_then(|d| find_item(d, &name));
            let files = evidence::list_dir(&path)?.unwrap_or_default();
            let has_marker = path.join(MARKER_NAME).exists();

            if item.is_none() {
                findings.push(Finding {
                    class: CLASS_UNOWNED,
                    dir: dir.clone(),
                    file: None,
                    detail: format!("no backlog item with id \"{name}\""),
                });
            }
            if files.is_empty() {
                findings.push(Finding {
                    class: CLASS_EMPTY,
                    dir: dir.clone(),
                    file: None,
                    detail: "no evidence files — the expected state in a fresh clone".to_string(),
                });
            } else if !has_marker {
                findings.push(Finding {
                    class: CLASS_NO_MARKER,
                    dir: dir.clone(),
                    file: None,
                    detail: format!(
                        "{} file(s) present without a `{MARKER_NAME}` marker",
                        files.len()
                    ),
                });
            }

            for (file, size) in &files {
                if *size > max_bytes {
                    findings.push(Finding {
                        class: CLASS_OVERSIZE,
                        dir: dir.clone(),
                        file: Some(file.clone()),
                        detail: format!("{size} bytes exceeds the {max_bytes}-byte threshold"),
                    });
                }
                if !extension_allowed(file) {
                    findings.push(Finding {
                        class: CLASS_DISALLOWED_EXTENSION,
                        dir: dir.clone(),
                        file: Some(file.clone()),
                        detail: format!(
                            "extension is not one of: {}",
                            EVIDENCE_EXTENSIONS.join(", ")
                        ),
                    });
                }
                candidates.push((path.join(file), dir.clone(), file.clone()));
            }

            // An absent drop-box means the bytes are in another clone, which
            // is not a stale reference; only a populated one can be missing
            // the name its item cites.
            if let Some(item) = item
                && !files.is_empty()
            {
                for want in evidence::referenced_names(item) {
                    if !files.iter().any(|(file, _)| *file == want) {
                        findings.push(Finding {
                            class: CLASS_REFERENCED_MISSING,
                            dir: dir.clone(),
                            file: Some(want.clone()),
                            detail: format!("item cites `{want}`, which is not in the directory"),
                        });
                    }
                }
            }
        }
    }

    if !candidates.is_empty() {
        let paths: Vec<PathBuf> = candidates.iter().map(|(p, _, _)| p.clone()).collect();
        match ignored_set(&repo, &paths) {
            Some(ignored) => {
                for (path, dir, file) in &candidates {
                    if !ignored.contains(path) {
                        findings.push(Finding {
                            class: CLASS_TRACKED,
                            dir: dir.clone(),
                            file: Some(file.clone()),
                            detail: "not git-ignored — a deliberate `git add -f`, or a missing ignore rule".to_string(),
                        });
                    }
                }
            }
            None => findings.push(Finding {
                class: CLASS_GIT_UNAVAILABLE,
                dir: relativise(&repo, root),
                file: None,
                detail: "`git check-ignore` did not run; the `tracked` class was skipped"
                    .to_string(),
            }),
        }
    }

    findings.sort_by(|a, b| (&a.dir, a.class, &a.file).cmp(&(&b.dir, b.class, &b.file)));
    Ok(AuditReport {
        root: relativise(&repo, root),
        findings,
    })
}

pub(crate) fn dispatch_audit(
    strict: bool,
    max_bytes: Option<u64>,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let store = schema::backlog_path()?;
    let doc = if store.exists() {
        maybe_verify_integrity(&store, read_integrity_opts(&integrity))?;
        Some(read_toml(&store)?)
    } else if integrity.strict_read {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(store.clone()),
            format!("file does not exist: {}", store.display()),
        ));
    } else {
        None
    };
    let root = evidence::evidence_root()?;
    let report = audit(doc.as_ref(), &root, max_bytes.unwrap_or(EVIDENCE_MAX_BYTES))?;
    print_json(&report.to_json())?;
    let failures = report.strict_failures();
    if strict && failures > 0 {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("evidence audit found {failures} finding(s) under --strict"),
        ));
    }
    Ok(())
}

fn find_item<'a>(doc: &'a TomlValue, id: &str) -> Option<&'a TomlValue> {
    for array in [ARRAY_BACKLOG, ARRAY_COMPACTED] {
        for item in items_array(doc, array) {
            if item.get(FIELD_ID).and_then(TomlValue::as_str) == Some(id) {
                return Some(item);
            }
        }
    }
    None
}

/// Extensionless counts as disallowed: a bare `screenshot` tells a later
/// reader nothing about how to open it.
fn extension_allowed(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => {
            let lower = extension.to_ascii_lowercase();
            EVIDENCE_EXTENSIONS.contains(&lower.as_str())
        }
        _ => false,
    }
}

/// Which of `paths` git considers ignored, or `None` when git could not
/// answer. The index is deliberately consulted (no `--no-index`): that is
/// what makes a force-added file report as NOT ignored, which is the whole
/// signal behind the `tracked` class.
///
/// One child process for the whole walk, fed NUL-separated repo-relative
/// paths. Exit 1 means "none matched an ignore rule" rather than a failure;
/// 128 (and a spawn error) means git could not answer at all.
fn ignored_set(root: &Path, paths: &[PathBuf]) -> Option<BTreeSet<PathBuf>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let mut by_rel: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut payload: Vec<u8> = Vec::new();
    for path in paths {
        let rel = relativise(root, path);
        payload.extend_from_slice(rel.as_bytes());
        payload.push(0);
        by_rel.insert(rel, path.clone());
    }

    let mut child = Command::new("git")
        .args(["check-ignore", "-z", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut sink = child.stdin.take()?;
    // git streams its answers, so a large path list can fill the stdout pipe
    // while we are still writing stdin — feed it from a second thread.
    let writer = std::thread::spawn(move || {
        let _ = sink.write_all(&payload);
    });
    let out = child.wait_with_output().ok();
    let _ = writer.join();
    let out = out?;

    match out.status.code() {
        Some(0) => {}
        Some(1) => return Some(BTreeSet::new()),
        _ => return None,
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .split('\0')
            .filter_map(|rel| by_rel.get(rel).cloned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Holds the env lock for the whole test and drops `TOMLCTL_ROOT` in
    /// `Drop`, so a failed assertion cannot leak the override into whatever
    /// test runs next on this thread.
    struct Sandbox {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // SAFETY: remove_var is unsafe in edition 2024; acceptable here
            // because the env lock is still held by `_lock`, which is dropped
            // after this body runs.
            unsafe {
                std::env::remove_var("TOMLCTL_ROOT");
            }
        }
    }

    impl Sandbox {
        fn new(git: bool) -> Self {
            let lock = crate::test_support::env_lock();
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            // SAFETY: set_var is unsafe in edition 2024; acceptable while the
            // env lock above is held.
            unsafe {
                std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
            }
            fs::create_dir_all(root.join(".claude")).unwrap();
            if git {
                fs::write(
                    root.join(".gitignore"),
                    "/.claude/backlog-evidence/*/*\n!/.claude/backlog-evidence/*/.evidence\n",
                )
                .unwrap();
                let _ = Command::new("git")
                    .args(["init", "-q", "."])
                    .current_dir(&root)
                    .output();
            }
            Self {
                _lock: lock,
                _tmp: tmp,
                root,
            }
        }

        fn seed(&self, body: &str) -> TomlValue {
            let path = self.store();
            fs::write(&path, body).unwrap();
            crate::io::write_sidecar_for(&path, body.as_bytes()).unwrap();
            toml::from_str(body).unwrap()
        }

        fn store(&self) -> PathBuf {
            self.root.join(".claude").join("backlog.toml")
        }

        fn store_bytes(&self) -> (Vec<u8>, Vec<u8>) {
            let store = self.store();
            let sidecar = crate::integrity::sidecar_path(&store);
            (fs::read(&store).unwrap(), fs::read(&sidecar).unwrap())
        }

        fn evidence(&self, id: &str) -> PathBuf {
            self.root.join(".claude").join("backlog-evidence").join(id)
        }

        fn root_dir(&self) -> PathBuf {
            self.root.join(".claude").join("backlog-evidence")
        }

        fn populate(&self, id: &str, marker: bool, files: &[(&str, usize)]) -> PathBuf {
            let dir = self.evidence(id);
            fs::create_dir_all(&dir).unwrap();
            if marker {
                fs::write(dir.join(MARKER_NAME), evidence::marker_text(id, "seeded")).unwrap();
            }
            for (name, size) in files {
                fs::write(dir.join(name), vec![b'x'; *size]).unwrap();
            }
            dir
        }

        fn git(&self, args: &[&str]) {
            Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap();
        }
    }

    const STORE: &str = r#"schema_version = 1

[[backlog]]
id = "B-a1b2c3d4"
summary = "checkout total overlaps the confirm button"
status = "open"

[[compacted]]
id = "B-7f0e2d91"
summary = "aged-out row"
status = "resolved"
"#;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    fn classes<'a>(report: &'a AuditReport, class: &str) -> Vec<&'a Finding> {
        report
            .findings
            .iter()
            .filter(|f| f.class == class)
            .collect()
    }

    #[test]
    fn dir_creates_the_drop_box_and_leaves_the_store_byte_identical() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let before = sb.store_bytes();

        let out = ensure_dir(&doc, "B-a1b2c3d4", false).unwrap();
        assert!(out.created);
        assert_eq!(out.files, 0);
        assert_eq!(out.id, "B-a1b2c3d4");
        assert_eq!(out.dir, sb.evidence("B-a1b2c3d4"));

        let marker = fs::read_to_string(out.dir.join(MARKER_NAME)).unwrap();
        assert!(marker.starts_with("B-a1b2c3d4  checkout total"), "{marker}");
        assert_eq!(sb.store_bytes(), before);
    }

    #[test]
    fn a_second_dir_call_never_rewrites_the_marker() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let dir = ensure_dir(&doc, "B-a1b2c3d4", false).unwrap().dir;
        let marker = dir.join(MARKER_NAME);
        fs::write(&marker, b"sentinel\n").unwrap();
        let stamp = fs::metadata(&marker).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let out = ensure_dir(&doc, "B-a1b2c3d4", false).unwrap();
        assert!(!out.created);
        assert_eq!(fs::read(&marker).unwrap(), b"sentinel\n");
        assert_eq!(fs::metadata(&marker).unwrap().modified().unwrap(), stamp);
    }

    #[test]
    fn dir_resolves_a_compacted_row_and_counts_non_marker_files() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-7f0e2d91", false, &[("a.log", 1), ("b.log", 1)]);
        let out = ensure_dir(&doc, "B-7f0e2d91", false).unwrap();
        assert!(out.created, "a hand-made drop-box still earns its marker");
        assert_eq!(out.files, 2);
    }

    #[test]
    fn an_unknown_id_is_not_found_and_creates_nothing() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let err = ensure_dir(&doc, "B-deadbeef", false).unwrap_err();
        assert_eq!(kind_of(&err), "not_found");
        assert!(!sb.evidence("B-deadbeef").exists());
        assert!(!sb.root_dir().exists());
    }

    #[test]
    fn no_create_errors_on_an_absent_directory_and_creates_nothing() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let err = ensure_dir(&doc, "B-a1b2c3d4", true).unwrap_err();
        assert_eq!(kind_of(&err), "not_found");
        assert!(!sb.evidence("B-a1b2c3d4").exists());

        ensure_dir(&doc, "B-a1b2c3d4", false).unwrap();
        let out = ensure_dir(&doc, "B-a1b2c3d4", true).unwrap();
        assert!(!out.created);
        assert_eq!(out.files, 0);
    }

    #[test]
    fn an_unowned_directory_fails_strict_until_it_is_removed() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let stray = sb.populate("B-deadbeef", true, &[]);

        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let unowned = classes(&report, CLASS_UNOWNED);
        assert_eq!(unowned.len(), 1);
        assert!(unowned[0].dir.ends_with("B-deadbeef"), "{:?}", unowned[0]);
        assert!(report.strict_failures() > 0);
        // A marker-only drop-box is informational, never strict.
        assert_eq!(classes(&report, CLASS_EMPTY).len(), 1);

        fs::remove_dir_all(&stray).unwrap();
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert_eq!(report.strict_failures(), 0);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn a_missing_store_makes_every_directory_unowned() {
        let sb = Sandbox::new(true);
        sb.populate("B-a1b2c3d4", true, &[]);
        let report = audit(None, &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert_eq!(classes(&report, CLASS_UNOWNED).len(), 1);
    }

    #[test]
    fn an_absent_root_reports_nothing() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert!(report.findings.is_empty());
        assert_eq!(report.root, ".claude/backlog-evidence");
        assert_eq!(report.to_json()["counts"][CLASS_UNOWNED], json!(0));
    }

    #[test]
    fn no_marker_needs_files_and_a_bare_directory_is_only_empty() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-a1b2c3d4", false, &[("a.log", 1)]);
        sb.populate("B-7f0e2d91", false, &[]);
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();

        let no_marker = classes(&report, CLASS_NO_MARKER);
        assert_eq!(no_marker.len(), 1, "{:?}", report.findings);
        assert!(no_marker[0].dir.ends_with("B-a1b2c3d4"), "{no_marker:?}");
        let empty = classes(&report, CLASS_EMPTY);
        assert_eq!(empty.len(), 1, "{:?}", report.findings);
        assert!(empty[0].dir.ends_with("B-7f0e2d91"), "{empty:?}");
        assert!(report.strict_failures() > 0);
    }

    #[test]
    fn oversize_fires_one_byte_over_the_threshold_only() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-a1b2c3d4", true, &[("big.log", 65)]);

        let over = audit(Some(&doc), &sb.root_dir(), 64).unwrap();
        let hits = classes(&over, CLASS_OVERSIZE);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file.as_deref(), Some("big.log"));
        assert!(hits[0].detail.contains("65 bytes"), "{:?}", hits[0]);

        let at = audit(Some(&doc), &sb.root_dir(), 65).unwrap();
        assert!(classes(&at, CLASS_OVERSIZE).is_empty());
    }

    #[test]
    fn a_disallowed_extension_is_flagged_and_case_does_not_matter() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate(
            "B-a1b2c3d4",
            true,
            &[("key.pem", 1), ("shot.PNG", 1), ("screenshot", 1)],
        );
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let flagged: Vec<_> = classes(&report, CLASS_DISALLOWED_EXTENSION)
            .iter()
            .map(|f| f.file.clone().unwrap())
            .collect();
        assert_eq!(
            flagged,
            vec!["key.pem".to_string(), "screenshot".to_string()]
        );
    }

    const CITING_STORE: &str = r#"schema_version = 1

[[backlog]]
id = "B-a1b2c3d4"
summary = "checkout total overlaps the confirm button"
status = "open"
context = "The overlap is visible in `shot.png` at 1280px."
"#;

    #[test]
    fn referenced_missing_clears_once_the_named_file_lands() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(CITING_STORE);
        let dir = sb.populate("B-a1b2c3d4", true, &[("other.png", 1)]);

        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let hits = classes(&report, CLASS_REFERENCED_MISSING);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file.as_deref(), Some("shot.png"));

        fs::write(dir.join("shot.png"), b"x").unwrap();
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert!(classes(&report, CLASS_REFERENCED_MISSING).is_empty());
    }

    #[test]
    fn referenced_missing_stays_quiet_for_a_marker_only_directory() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(CITING_STORE);
        sb.populate("B-a1b2c3d4", true, &[]);
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert!(classes(&report, CLASS_REFERENCED_MISSING).is_empty());
        assert_eq!(classes(&report, CLASS_EMPTY).len(), 1);
        assert_eq!(report.strict_failures(), 0);
    }

    #[test]
    fn a_force_added_file_is_tracked_and_strict_still_passes() {
        if !git_available() {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-a1b2c3d4", true, &[("shot.png", 1), ("quiet.log", 1)]);
        sb.git(&["add", "-f", ".claude/backlog-evidence/B-a1b2c3d4/shot.png"]);

        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let hits = classes(&report, CLASS_TRACKED);
        assert_eq!(hits.len(), 1, "{:?}", report.findings);
        assert_eq!(hits[0].file.as_deref(), Some("shot.png"));
        assert!(classes(&report, CLASS_GIT_UNAVAILABLE).is_empty());
        assert_eq!(report.strict_failures(), 0);
    }

    #[test]
    fn the_marker_is_never_tracked_even_though_git_does_not_ignore_it() {
        if !git_available() {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-a1b2c3d4", true, &[("shot.png", 1)]);
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert!(
            classes(&report, CLASS_TRACKED).is_empty(),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn without_a_repo_the_tracked_class_degrades_to_one_note() {
        let sb = Sandbox::new(false);
        let doc = sb.seed(STORE);
        sb.populate("B-a1b2c3d4", true, &[("a.log", 1), ("b.log", 1)]);
        assert_eq!(ignored_set(&sb.root, &[sb.evidence("B-a1b2c3d4")]), None);

        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        assert_eq!(classes(&report, CLASS_GIT_UNAVAILABLE).len(), 1);
        assert!(classes(&report, CLASS_TRACKED).is_empty());
        assert_eq!(report.strict_failures(), 0);
    }

    #[test]
    fn findings_sort_by_dir_then_class_then_file() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-zzzzzzzz", true, &[]);
        sb.populate("B-a1b2c3d4", true, &[("z.pem", 1), ("a.pem", 1)]);

        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let keys: Vec<_> = report
            .findings
            .iter()
            .map(|f| (f.dir.clone(), f.class, f.file.clone()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(
            keys[0].0.ends_with("B-a1b2c3d4"),
            "{keys:?} must lead with the lexicographically first directory"
        );
    }

    #[test]
    fn the_report_envelope_carries_every_class_count() {
        let sb = Sandbox::new(true);
        let doc = sb.seed(STORE);
        sb.populate("B-deadbeef", true, &[]);
        let report = audit(Some(&doc), &sb.root_dir(), EVIDENCE_MAX_BYTES).unwrap();
        let envelope = report.to_json();
        let counts = envelope["counts"].as_object().unwrap();
        assert_eq!(counts.len(), CLASSES.len());
        for class in CLASSES {
            assert!(counts.contains_key(*class), "{class} missing from counts");
        }
        assert_eq!(counts[CLASS_UNOWNED], json!(1));
        assert_eq!(counts[CLASS_EMPTY], json!(1));
        assert_eq!(envelope["root"], json!(".claude/backlog-evidence"));
        let first = &envelope["findings"][0];
        assert_eq!(first["class"], json!(CLASS_EMPTY));
        assert!(first.get("file").is_none(), "{first}");
    }

    #[test]
    fn a_finding_with_a_file_renders_it_between_dir_and_detail() {
        let finding = Finding {
            class: CLASS_OVERSIZE,
            dir: ".claude/backlog-evidence/B-a1b2c3d4".to_string(),
            file: Some("big.log".to_string()),
            detail: "too big".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&finding.to_json()).unwrap(),
            r#"{"class":"oversize","dir":".claude/backlog-evidence/B-a1b2c3d4","file":"big.log","detail":"too big"}"#
        );
    }
}
