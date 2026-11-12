//! The poller's on-demand document reads: an authored flow file, a cited source as its author
//! saw it, a commit, or a v2 review finding.
//!
//! No value from a request reaches a shell: git runs with separate arguments from the repo
//! root, a sha is passed only when it is 7 to 64 hex digits, and a path only when it is
//! repo-relative with no `..`, root, drive or UNC component. A working-tree read also
//! canonicalises the path and refuses one that leaves the repo root, while a `git show` read
//! leaves resolution to git, so a file deleted or renamed since it was read still shows.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// The largest file shown; anything bigger is refused with a reason.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// The source was dirty when its author read it, so the working tree is shown.
pub(crate) const READ_UNCOMMITTED: &str = "read uncommitted";
/// The note recorded no commit, so the working tree is shown.
pub(crate) const COMMIT_NOT_RECORDED: &str = "commit not recorded";
/// The working tree's file no longer matches the text shown.
pub(crate) const CHANGED_SINCE: &str = "changed since";

/// One document the runtime asks the poller to read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum DocRequest {
    /// A file under `docs/flows/<slug>/`, as `tomlctl::authored_list` names it.
    Authored {
        slug: String,
        path: String,
    },
    /// A file a research note cites, with the commit its author read at (empty when not
    /// recorded) and whether the file was dirty then.
    Source {
        path: String,
        line: Option<u32>,
        read_at: String,
        dirty: bool,
    },
    Commit {
        sha: String,
    },
    /// Finding `id` of the v2 review store at the repo-relative `store`.
    Finding {
        store: String,
        id: String,
    },
}

/// A document read for the details panel. The notices are kept out of `text` so the view
/// can title them; `markdown` is set only for an authored markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Fetched {
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) notices: Vec<&'static str>,
    pub(crate) line: Option<u32>,
    pub(crate) markdown: bool,
}

/// Reads `request` against the repo at `root`; the error is a one-line reason.
pub(crate) fn fetch(root: &Path, request: &DocRequest) -> Result<Fetched, String> {
    match request {
        DocRequest::Authored { slug, path } => authored(root, slug, path),
        DocRequest::Source {
            path,
            line,
            read_at,
            dirty,
        } => source(root, path, *line, read_at, *dirty),
        DocRequest::Commit { sha } => commit(root, sha),
        DocRequest::Finding { store, id } => finding(root, store, id),
    }
}

fn authored(root: &Path, slug: &str, path: &str) -> Result<Fetched, String> {
    let read = tomlctl::authored_read(root, slug, path).map_err(one_line)?;
    let text = read
        .get("text")
        .and_then(|text| text.as_str())
        .ok_or_else(|| format!("{path}: the read carried no text"))?;
    Ok(Fetched {
        title: format!("docs/flows/{slug}/{path}"),
        text: text.to_string(),
        notices: Vec::new(),
        line: None,
        markdown: path.ends_with(".md"),
    })
}

fn finding(root: &Path, store: &str, id: &str) -> Result<Fetched, String> {
    let row = tomlctl::review_finding(root, store, id).map_err(one_line)?;
    let text = match &row {
        serde_json::Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| match value {
                serde_json::Value::String(s) => format!("{key}: {s}"),
                other => format!("{key}: {other}"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    };
    Ok(Fetched {
        title: format!("{id} in {store}"),
        text,
        notices: Vec::new(),
        line: None,
        markdown: false,
    })
}

fn commit(root: &Path, sha: &str) -> Result<Fetched, String> {
    check_sha(sha)?;
    let out = git(
        root,
        &["show", "--no-color", "--stat", "--format=fuller", sha, "--"],
    )?;
    Ok(Fetched {
        title: format!("commit {sha}"),
        text: lf(&String::from_utf8_lossy(&out)),
        notices: Vec::new(),
        line: None,
        markdown: false,
    })
}

fn source(
    root: &Path,
    path: &str,
    line: Option<u32>,
    read_at: &str,
    dirty: bool,
) -> Result<Fetched, String> {
    check_relative(path)?;
    let fetched = |text: String, notices: Vec<&'static str>| Fetched {
        title: path.to_string(),
        text,
        notices,
        line,
        markdown: false,
    };
    if !read_at.is_empty() && !dirty {
        check_sha(read_at)?;
        let bytes = git(root, &["show", &format!("{read_at}:{path}")])?;
        let text = text_of(path, bytes)?;
        let current = working_tree(root, path).ok();
        let notices = if current.as_deref() == Some(text.as_str()) {
            Vec::new()
        } else {
            vec![CHANGED_SINCE]
        };
        return Ok(fetched(text, notices));
    }
    let mut notices = Vec::new();
    if dirty {
        notices.push(READ_UNCOMMITTED);
    }
    if read_at.is_empty() {
        notices.push(COMMIT_NOT_RECORDED);
    }
    Ok(fetched(working_tree(root, path)?, notices))
}

/// The working tree's text of the checked repo-relative `path`, refused when it resolves
/// outside the repo root.
fn working_tree(root: &Path, path: &str) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve the repo root: {e}"))?;
    let resolved: PathBuf = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    if !resolved.starts_with(&root) {
        return Err(format!("{path} resolves outside the repo"));
    }
    let len = std::fs::metadata(&resolved)
        .map_err(|e| format!("{path}: {e}"))?
        .len();
    if len > MAX_BYTES {
        return Err(too_large(path));
    }
    let bytes = std::fs::read(&resolved).map_err(|e| format!("{path}: {e}"))?;
    text_of(path, bytes)
}

/// `bytes` as LF-normalised text, refused when too large or not UTF-8.
fn text_of(path: &str, bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_large(path));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8 text"))?;
    Ok(lf(&text))
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn too_large(path: &str) -> String {
    format!("{path} is larger than 2 MiB")
}

/// Refuses anything but a non-empty, `/`-separated path of plain components.
fn check_relative(path: &str) -> Result<(), String> {
    let plain = !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(format!("{path:?} is not a repo-relative path"))
    }
}

fn check_sha(sha: &str) -> Result<(), String> {
    if (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{sha:?} is not a commit id of 7 to 64 hex digits"))
    }
}

/// Runs git in `root` with `args` as separate values, returning its stdout or the first
/// line of its complaint.
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let reason = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    Err(if reason.is_empty() {
        format!("git {} failed", args[0])
    } else {
        reason.trim().to_string()
    })
}

fn one_line(error: impl std::fmt::Display) -> String {
    let text = format!("{error:#}");
    text.lines().next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::super::scan::tests::temp_root;
    use super::*;

    fn run_git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=glimpse",
                "-c",
                "user.email=glimpse@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo with `src/a.txt` committed as `old`, returning the root and that commit.
    fn repo(name: &str) -> (PathBuf, String) {
        let root = temp_root(&format!("doc-{name}"));
        run_git(&root, &["init", "-q"]);
        std::fs::create_dir_all(root.join("src")).expect("src dir");
        std::fs::write(root.join("src").join("a.txt"), "old\n").expect("write");
        run_git(&root, &["add", "src/a.txt"]);
        run_git(&root, &["commit", "-q", "-m", "first"]);
        let sha = run_git(&root, &["rev-parse", "HEAD"]);
        (root, sha)
    }

    fn source_request(path: &str, read_at: &str, dirty: bool) -> DocRequest {
        DocRequest::Source {
            path: path.into(),
            line: Some(1),
            read_at: read_at.into(),
            dirty,
        }
    }

    #[test]
    fn a_clean_source_shows_its_commit_text_and_notes_a_later_change() {
        let (root, sha) = repo("clean");
        std::fs::write(root.join("src").join("a.txt"), "new\n").expect("edit");

        let doc = fetch(&root, &source_request("src/a.txt", &sha, false)).expect("read");
        assert_eq!(doc.text, "old\n");
        assert_eq!(doc.notices, [CHANGED_SINCE]);
        assert_eq!(doc.line, Some(1));
        assert!(!doc.markdown);

        std::fs::write(root.join("src").join("a.txt"), "old\n").expect("restore");
        let doc = fetch(&root, &source_request("src/a.txt", &sha, false)).expect("read");
        assert!(doc.notices.is_empty(), "{:?}", doc.notices);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_deleted_since_its_commit_still_shows_as_it_was() {
        let (root, sha) = repo("deleted");
        std::fs::remove_file(root.join("src").join("a.txt")).expect("delete");
        let doc = fetch(&root, &source_request("src/a.txt", &sha, false)).expect("read");
        assert_eq!(doc.text, "old\n");
        assert_eq!(doc.notices, [CHANGED_SINCE]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dirty_source_shows_the_working_tree() {
        let (root, sha) = repo("dirty");
        std::fs::write(root.join("src").join("a.txt"), "new\n").expect("edit");
        let doc = fetch(&root, &source_request("src/a.txt", &sha, true)).expect("read");
        assert_eq!(doc.text, "new\n");
        assert_eq!(doc.notices, [READ_UNCOMMITTED]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_with_no_commit_shows_the_working_tree() {
        let (root, _) = repo("unrecorded");
        let doc = fetch(&root, &source_request("src/a.txt", "", false)).expect("read");
        assert_eq!(doc.text, "old\n");
        assert_eq!(doc.notices, [COMMIT_NOT_RECORDED]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unsafe_and_unreadable_sources_are_refused_with_one_line() {
        let (root, sha) = repo("refused");
        std::fs::write(root.join("big.txt"), vec![b'x'; 2 * 1024 * 1024 + 1]).expect("big");
        std::fs::write(root.join("bin.txt"), [0xff, 0xfe, 0x00, 0x80]).expect("binary");
        let outside = root.parent().expect("parent").join("outside.txt");
        let absolute = outside.to_string_lossy().into_owned();

        let cases = [
            source_request("../outside.txt", "", false),
            source_request("src/../../outside.txt", &sha, false),
            source_request(&absolute, "", false),
            source_request("src\\a.txt", "", false),
            source_request("", "", false),
            source_request("src/a.txt", "not-hex!", false),
            source_request("src/a.txt", "abc", false),
            source_request("big.txt", "", false),
            source_request("bin.txt", "", false),
            DocRequest::Commit {
                sha: "--output=x".into(),
            },
            DocRequest::Commit {
                sha: "HEAD~1".into(),
            },
        ];
        for request in cases {
            let reason = fetch(&root, &request).expect_err(&format!("{request:?} was read"));
            assert!(!reason.is_empty() && !reason.contains('\n'), "{reason:?}");
        }
        assert!(
            fetch(&root, &source_request("big.txt", "", false))
                .unwrap_err()
                .contains("2 MiB")
        );
        assert!(
            fetch(&root, &source_request("bin.txt", "", false))
                .unwrap_err()
                .contains("not UTF-8")
        );
        assert!(!root.join("x").exists(), "no option reached git");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_commit_shows_its_stat() {
        let (root, sha) = repo("commit");
        let doc = fetch(&root, &DocRequest::Commit { sha: sha.clone() }).expect("show");
        assert!(doc.text.contains("CommitDate:"), "{}", doc.text);
        assert!(doc.text.contains("src/a.txt"), "{}", doc.text);
        assert!(doc.text.contains("1 file changed"), "{}", doc.text);
        assert_eq!(doc.title, format!("commit {sha}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    const STORE: &str = ".claude/review-v2/doc.toml";

    const REVIEW: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"
catalogue = { builtin = "catalogue" }

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 30
summary = "the limit is off by one"
severity = "major"
failure_scenario = "a nine-row input returns eight rows"
fix = "compare with <= instead of <"
status = "open"
"#;

    #[test]
    fn a_finding_renders_its_fields_as_lines() {
        let root = temp_root("doc-finding");
        let path = root.join(STORE);
        std::fs::create_dir_all(path.parent().expect("store dir")).expect("dir");
        std::fs::write(&path, REVIEW).expect("store");

        let request = DocRequest::Finding {
            store: STORE.into(),
            id: "F1".into(),
        };
        let doc = fetch(&root, &request).expect("finding");
        let lines: Vec<&str> = doc.text.lines().collect();
        assert!(
            lines.contains(&"summary: the limit is off by one"),
            "{lines:?}"
        );
        assert!(lines.contains(&"status: open"), "{lines:?}");
        assert!(!doc.markdown);

        let missing = DocRequest::Finding {
            store: STORE.into(),
            id: "F9".into(),
        };
        let reason = fetch(&root, &missing).expect_err("no such finding");
        assert!(reason.contains("F9") && !reason.contains('\n'), "{reason}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_authored_file_reads_through_the_library_as_markdown() {
        let root = temp_root("doc-authored");
        let dir = root.join("docs").join("flows").join("f");
        std::fs::create_dir_all(&dir).expect("flow docs");
        std::fs::write(dir.join("brief.md"), "# Brief\r\n\r\nbody\r\n").expect("brief");

        let request = DocRequest::Authored {
            slug: "f".into(),
            path: "brief.md".into(),
        };
        let doc = fetch(&root, &request).expect("authored");
        assert_eq!(doc.text, "# Brief\n\nbody\n");
        assert!(doc.markdown);
        assert!(doc.notices.is_empty());

        let outside = DocRequest::Authored {
            slug: "f".into(),
            path: "../../../Cargo.toml".into(),
        };
        assert!(fetch(&root, &outside).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
