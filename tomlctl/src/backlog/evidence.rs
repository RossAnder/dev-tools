//! Evidence-directory paths, marker text, and the advisory policy constants.
//!
//! Pure: the filesystem verbs live in `evidence_ops`.
//!
//! Every answer here is derived, because the store holds no evidence field:
//! the path comes from `schema::backlog_path`, the file set from a directory
//! read. `list_dir` keeps an absent directory distinct from an empty one —
//! the only distinction that changes what a reader does, between "nothing was
//! ever captured" and "the bytes are in another clone".

// A policy module: its consumers are the sibling leaves, so most of what is
// defined here has no call site in this file.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::ErrorKind as IoErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use toml::Value as TomlValue;

use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_CONTEXT, FIELD_EVIDENCE, FIELD_ID,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::items_array;

/// Directory holding one drop-box per item, a sibling of the store itself.
pub(crate) const EVIDENCE_ROOT_NAME: &str = "backlog-evidence";

/// The one tracked file in a drop-box. `.gitignore` negates it out of the
/// contents rule, so it is what makes a directory survive into a fresh clone
/// once its files have been left behind.
pub(crate) const MARKER_NAME: &str = ".evidence";

/// Extensions `audit` expects, compared lowercased. Advisory only — the
/// endorsed capture path is a plain `cp`, so no write path consults this;
/// it exists so `audit` can flag a `.pem` or an extensionless file.
pub(crate) const EVIDENCE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "txt", "log", "json", "har", "csv", "md", "patch",
    "diff",
];

/// Formats whose bytes routinely carry an Authorization header, a session
/// cookie or a token — a `.har` is a whole request log. Compared lowercased,
/// and a subset of `EVIDENCE_EXTENSIONS`: capturing one is legitimate, so
/// only publishing one is a strict failure.
pub(crate) const SENSITIVE_EXTENSIONS: &[&str] = &["har", "json", "log", "patch", "diff"];

/// Default `audit --max-bytes` threshold. The repository is public and has no
/// LFS configuration, so anything published here lands in every clone forever.
pub(crate) const EVIDENCE_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Longest extension `classify` will read as a filename, and the shortest.
/// The floor rejects the `e.g` / `i.e` shape that prose supplies freely.
const EXTENSION_LEN: std::ops::RangeInclusive<usize> = 2..=8;

/// `<store parent>/backlog-evidence`, so `TOMLCTL_ROOT` moves the store and
/// its evidence together rather than leaving one behind.
pub(crate) fn evidence_root() -> Result<PathBuf> {
    let store = schema::backlog_path()?;
    let parent = store.parent().ok_or_else(|| {
        anyhow!(
            "backlog store path {} has no parent directory",
            store.display()
        )
    })?;
    Ok(parent.join(EVIDENCE_ROOT_NAME))
}

/// Drop-box path for one item. `item_id` must be a single path component:
/// resolving through `resolve_id` already guarantees that for a well-formed
/// store, and this refuses the hand-edited row that would otherwise let a
/// `..` segment walk the join out of `.claude/`. The separator test is
/// spelled out because `file_name` alone reads a backslash as an ordinary
/// filename character off Windows.
pub(crate) fn dir_for(item_id: &str) -> Result<PathBuf> {
    if item_id.contains(['/', '\\']) || Path::new(item_id).file_name() != Some(OsStr::new(item_id))
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("backlog id \"{item_id}\" is not a single path component"),
        ));
    }
    Ok(evidence_root()?.join(item_id))
}

/// Confirm `id` names a stored item, in either array, and hand back its
/// stored spelling. Ids widen from 8 to 10 to 12 hex on collision, so a path
/// derived from an eyeballed prefix is silently owned by nobody while a
/// resolved one cannot be. Compacted rows count: folding an item away does
/// not orphan its directory.
pub(crate) fn resolve_id(doc: &TomlValue, id: &str) -> Result<String> {
    for array in [ARRAY_BACKLOG, ARRAY_COMPACTED] {
        for item in items_array(doc, array) {
            if let Some(stored) = item.get(FIELD_ID).and_then(TomlValue::as_str)
                && stored == id
            {
                return Ok(stored.to_string());
            }
        }
    }
    Err(tagged_err(
        ErrorKind::NotFound,
        schema::backlog_path().ok(),
        format!("no backlog item with id \"{id}\""),
    ))
}

/// Caption line plus the publication policy that holds in THIS clone:
/// `ignored` is git's answer for a file dropped in the directory, `None` when
/// git could not be asked. `tomlctl` is installed globally, so the ignore
/// rules are a fact of the checkout, not of this repository.
///
/// Written once at directory creation and never rewritten, which is safe
/// because `add --on-duplicate bump` leaves `summary` untouched by contract.
/// Whitespace in the summary is folded so a multi-line one cannot push the
/// prose out of the caption line.
pub(crate) fn marker_text(id: &str, summary: &str, ignored: Option<bool>) -> String {
    let caption = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let policy = match ignored {
        Some(true) => {
            "Files here are git-ignored; publish one deliberately with `git add -f\n\
             <file>` after checking it for credentials, personal data and session\n\
             tokens."
        }
        Some(false) => {
            "Files here are NOT git-ignored — add the backlog-evidence rules to\n\
             .gitignore before copying anything in."
        }
        None => {
            "Whether files here are git-ignored could not be determined: `git\n\
             check-ignore` did not run. Verify .gitignore by hand before copying\n\
             anything in."
        }
    };
    format!("{id}  {caption}\nEvidence for this backlog item. {policy}\n")
}

/// `None` when the directory is absent, `Some(files)` otherwise, name-sorted
/// and excluding the marker. Regular files only: a drop-box is populated by
/// `cp`, so a subdirectory or a symlink is not evidence and is not sized.
pub(crate) fn list_dir(dir: &Path) -> Result<Option<Vec<(String, u64)>>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == IoErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::Error::new(e)
                .context(format!("reading evidence directory {}", dir.display())));
        }
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("reading evidence directory {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == MARKER_NAME {
            continue;
        }
        let meta = entry
            .metadata()
            .with_context(|| format!("reading evidence file {}", entry.path().display()))?;
        if !meta.is_file() {
            continue;
        }
        files.push((name, meta.len()));
    }
    files.sort();
    Ok(Some(files))
}

/// Bare evidence filenames an item names inline, from `context` prose and
/// from `evidence[]`. Conservative by design: `audit` turns every name in
/// here into a `referenced-missing` finding when the file is absent, so a
/// prose word read as a filename is a false alarm on a real item.
pub(crate) fn referenced_names(item: &TomlValue) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(context) = item.get(FIELD_CONTEXT).and_then(TomlValue::as_str) {
        collect_names(context, &mut names);
    }
    if let Some(evidence) = item.get(FIELD_EVIDENCE).and_then(TomlValue::as_array) {
        for entry in evidence {
            if let Some(text) = entry.as_str() {
                collect_names(text, &mut names);
            }
        }
    }
    names
}

fn collect_names(text: &str, names: &mut BTreeSet<String>) {
    for token in text.split_whitespace() {
        if let Ok(name) = classify(token) {
            names.insert(name.to_string());
        }
    }
}

/// Why a token is not an evidence reference. Carried rather than collapsed to
/// a bool so each exclusion is independently testable: `src/a.rs:88` and
/// `lumina/web/x.vue` are both rejected, for different reasons, and a single
/// rule covering both would drop one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reject {
    /// `path:line` pointer into tracked source.
    SourcePointer,
    /// Repo path rather than a name inside the item's own directory.
    RepoPath,
    /// Ordinary prose, a bare number, or a version.
    NotAFilename,
}

const LEADING_DECORATION: &[char] = &['`', '"', '\'', '(', '[', '{', '<', '*'];
const TRAILING_DECORATION: &[char] = &[
    '`', '"', '\'', ')', ']', '}', '>', '*', ',', ';', ':', '.', '!', '?',
];

fn classify(token: &str) -> std::result::Result<&str, Reject> {
    let name = token
        .trim_start_matches(LEADING_DECORATION)
        .trim_end_matches(TRAILING_DECORATION);
    if name.is_empty() {
        return Err(Reject::NotAFilename);
    }
    if let Some((head, tail)) = name.rsplit_once(':')
        && !head.is_empty()
        && !tail.is_empty()
        && tail.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Reject::SourcePointer);
    }
    if name.contains(['/', '\\']) {
        return Err(Reject::RepoPath);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | '~' | '@'))
    {
        return Err(Reject::NotAFilename);
    }
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return Err(Reject::NotAFilename);
    };
    if stem.is_empty()
        || !EXTENSION_LEN.contains(&extension.len())
        || !extension.chars().any(|c| c.is_ascii_alphabetic())
    {
        return Err(Reject::NotAFilename);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Resolve paths under a throwaway root, then drop the override before
    /// any assertion runs — a panic inside would otherwise leak
    /// `TOMLCTL_ROOT` into every later test in the process.
    fn under_root<T>(f: impl FnOnce(&Path) -> T) -> (PathBuf, T) {
        let _guard = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        let out = f(&root);
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        (root, out)
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn dir_for_resolves_beside_the_store_with_forward_slashes() {
        let (root, (evidence, dir)) =
            under_root(|_| (evidence_root().unwrap(), dir_for("B-a1b2c3d4").unwrap()));
        assert_eq!(evidence, root.join(".claude").join(EVIDENCE_ROOT_NAME));
        assert!(
            dir.ends_with(Path::new(".claude/backlog-evidence/B-a1b2c3d4")),
            "{}",
            dir.display()
        );
        // Asserting the whole forward-slash literal is the falsifiable form
        // of "no backslash": the absolute path necessarily carries them on
        // Windows, so only the repo-relative rendering the verbs emit can be
        // held to the rule.
        assert_eq!(
            crate::io::relativise(&root, &dir),
            ".claude/backlog-evidence/B-a1b2c3d4"
        );
    }

    #[test]
    fn dir_for_refuses_an_id_that_is_not_one_component() {
        let (_root, errs) = under_root(|_| {
            [
                dir_for("../../etc").unwrap_err(),
                dir_for("B-a1b2/c3d4").unwrap_err(),
                dir_for("B-a1b2\\c3d4").unwrap_err(),
                dir_for("").unwrap_err(),
            ]
        });
        for err in &errs {
            assert_eq!(kind_of(err), "validation", "{err:#}");
        }
    }

    const STORE: &str = r#"
[[backlog]]
id = "B-a1b2c3d4"
summary = "live row"
status = "open"

[[compacted]]
id = "B-7f0e2d91"
summary = "aged-out row"
status = "resolved"
"#;

    #[test]
    fn resolve_id_reads_both_arrays_and_errors_not_found() {
        let doc: TomlValue = toml::from_str(STORE).unwrap();
        assert_eq!(resolve_id(&doc, "B-a1b2c3d4").unwrap(), "B-a1b2c3d4");
        assert_eq!(resolve_id(&doc, "B-7f0e2d91").unwrap(), "B-7f0e2d91");
        let err = resolve_id(&doc, "B-deadbeef").unwrap_err();
        assert_eq!(kind_of(&err), "not_found");
        assert!(format!("{err:#}").contains("B-deadbeef"));
    }

    #[test]
    fn list_dir_separates_absent_from_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("B-a1b2c3d4");
        assert_eq!(list_dir(&dir).unwrap(), None);

        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join(MARKER_NAME),
            marker_text("B-a1b2c3d4", "live row", Some(true)),
        )
        .unwrap();
        assert_eq!(list_dir(&dir).unwrap(), Some(vec![]));

        fs::write(dir.join("shot.png"), b"1234").unwrap();
        assert_eq!(
            list_dir(&dir).unwrap(),
            Some(vec![("shot.png".to_string(), 4)])
        );
    }

    #[test]
    fn list_dir_sorts_by_name_and_skips_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("B-a1b2c3d4");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(MARKER_NAME), "x").unwrap();
        fs::write(dir.join("z.log"), b"12").unwrap();
        fs::write(dir.join("a.log"), b"1").unwrap();
        fs::create_dir(dir.join("nested")).unwrap();
        assert_eq!(
            list_dir(&dir).unwrap(),
            Some(vec![("a.log".to_string(), 1), ("z.log".to_string(), 2)])
        );
    }

    #[test]
    fn marker_text_leads_with_the_id_and_names_the_force_add() {
        let text = marker_text(
            "B-a1b2c3d4",
            "checkout total overlaps the confirm button below 1400px",
            Some(true),
        );
        assert!(
            text.starts_with("B-a1b2c3d4  checkout total overlaps"),
            "{text}"
        );
        assert!(text.contains("git add -f"));
        assert!(text.contains("credentials, personal data and session"), "{text}");
        assert!(text.ends_with('\n'));
    }

    /// The marker may only claim what git actually answered for this clone,
    /// and may never claim the repository it sits in is public.
    #[test]
    fn marker_text_states_only_the_ignore_status_git_confirmed() {
        let ignored = marker_text("B-a1b2c3d4", "s", Some(true));
        let exposed = marker_text("B-a1b2c3d4", "s", Some(false));
        let unknown = marker_text("B-a1b2c3d4", "s", None);

        assert!(ignored.contains("are git-ignored;"), "{ignored}");
        assert!(!ignored.contains("NOT git-ignored"), "{ignored}");

        assert!(exposed.contains("NOT git-ignored"), "{exposed}");
        assert!(exposed.contains(".gitignore before copying"), "{exposed}");
        assert!(!exposed.contains("git add -f"), "{exposed}");

        assert!(unknown.contains("could not be determined"), "{unknown}");
        assert!(unknown.contains("Verify .gitignore by hand"), "{unknown}");

        for text in [&ignored, &exposed, &unknown] {
            assert!(!text.contains("public"), "{text}");
            assert!(text.starts_with("B-a1b2c3d4  s\n"), "{text}");
            assert!(text.ends_with('\n'), "{text}");
        }
    }

    #[test]
    fn marker_text_folds_a_multi_line_summary_into_the_caption() {
        let text = marker_text("B-a1b2c3d4", "two\nlines   here", Some(true));
        assert!(text.starts_with("B-a1b2c3d4  two lines here\n"), "{text}");
    }

    fn item(body: &str) -> TomlValue {
        toml::from_str(body).unwrap()
    }

    #[test]
    fn referenced_names_reads_prose_and_evidence_entries() {
        let it = item(
            r#"
context = "The overlap is visible in `shot.png` at 1280px."
evidence = ["src/a.rs:88", "lumina/web/x.vue", "trace.har"]
"#,
        );
        let expected: BTreeSet<String> = ["shot.png", "trace.har"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(referenced_names(&it), expected);
    }

    #[test]
    fn referenced_names_is_empty_without_context_or_evidence() {
        assert!(referenced_names(&item("id = \"B-a1b2c3d4\"\n")).is_empty());
    }

    #[test]
    fn a_source_pointer_is_rejected_for_its_line_suffix() {
        assert_eq!(classify("src/a.rs:88"), Err(Reject::SourcePointer));
        assert_eq!(classify("a.rs:88"), Err(Reject::SourcePointer));
        assert_eq!(classify("shot.png:12"), Err(Reject::SourcePointer));
    }

    #[test]
    fn a_repo_path_is_rejected_for_its_separator() {
        assert_eq!(classify("lumina/web/x.vue"), Err(Reject::RepoPath));
        assert_eq!(classify("lumina/web/"), Err(Reject::RepoPath));
    }

    #[test]
    fn prose_and_versions_are_not_filenames() {
        for token in ["visible", "1280px.", "v0.5.0", "e.g.", "i.e.", ".evidence"] {
            assert_eq!(classify(token), Err(Reject::NotAFilename), "{token}");
        }
    }

    #[test]
    fn decoration_around_a_filename_is_stripped() {
        assert_eq!(classify("`shot.png`"), Ok("shot.png"));
        assert_eq!(classify("(shot.png),"), Ok("shot.png"));
        assert_eq!(
            classify("checkout-total-overlap-1280.png."),
            Ok("checkout-total-overlap-1280.png")
        );
    }
}
