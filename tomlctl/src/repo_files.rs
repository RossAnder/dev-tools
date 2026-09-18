//! Tracked-file enumeration through `git ls-files`.

use std::path::Path;
use std::process::Command;

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::errors::{ErrorKind, tagged_err};
use crate::io::{join_under, path_under_root};

/// The files git knows about under a root: tracked plus untracked-but-not-
/// ignored, as repo-relative paths spelled the way git emits them (forward
/// slashes), sorted and deduplicated.
#[derive(Debug)]
pub(crate) struct Enumeration {
    pub(crate) files: Vec<String>,
    /// Entries whose resolved location lies outside the root — a symlink
    /// pointing out of the tree.
    pub(crate) skipped_outside: usize,
    /// Non-empty stderr lines git emitted while still exiting 0. Git warns and
    /// continues past a subtree it cannot open (a dangling directory symlink),
    /// so the listing is silently short whenever this is non-empty.
    pub(crate) warnings: Vec<String>,
}

/// Run `git ls-files` under `root` and return every entry that survives the
/// `exclude` globs and the containment check. Directories and unreadable
/// entries (staged deletes, gitlinks) are not filtered here; the reader
/// decides what to do with them.
///
/// A bad `exclude` glob is a `Validation` error rather than a dropped
/// pattern: the exclusions are what keep a sweep from matching the ledger
/// that records its own search strings, so silently losing one would make
/// every pattern hit its own record.
pub(crate) fn tracked_files(root: &Path, exclude: &[String]) -> Result<Enumeration> {
    let excluded = compile_excludes(exclude)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--full-name",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|e| {
            tagged_err(
                ErrorKind::Other,
                Some(root.to_path_buf()),
                format!("git ls-files failed: {e}"),
            )
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_lines: Vec<String> = stderr
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if !output.status.success() {
        let first = stderr_lines
            .first()
            .map_or_else(|| output.status.to_string(), String::clone);
        return Err(tagged_err(
            ErrorKind::Other,
            Some(root.to_path_buf()),
            format!("git ls-files failed: {first}"),
        ));
    }

    let mut skipped_outside = 0usize;
    let mut files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|entry| !excluded.as_ref().is_some_and(|set| set.is_match(entry)))
        .filter(|entry| {
            let inside =
                join_under(root, entry).is_some_and(|absolute| path_under_root(root, &absolute));
            if !inside {
                skipped_outside += 1;
            }
            inside
        })
        .map(str::to_string)
        .collect();
    files.sort();
    files.dedup();
    Ok(Enumeration {
        files,
        skipped_outside,
        warnings: stderr_lines,
    })
}

fn compile_excludes(exclude: &[String]) -> Result<Option<GlobSet>> {
    if exclude.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pat in exclude {
        let glob = Glob::new(pat).map_err(|e| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("invalid --exclude glob {pat:?}: {e}"),
            )
        })?;
        builder.add(glob);
    }
    let set = builder.build().map_err(|e| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid --exclude globs: {e}"),
        )
    })?;
    Ok(Some(set))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use crate::test_support::{git, git_available, with_root};
    use std::fs;

    /// One committed file, one untracked file, one committed file under a
    /// directory the caller excludes, and one ignored file.
    fn seed_repo(root: &Path) {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("tracked.rs"), "fn a() {}\n").unwrap();
        fs::create_dir_all(root.join("docs/plans")).unwrap();
        fs::write(root.join("docs/plans/plan.md"), "# plan\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "x"]);
        fs::write(root.join("untracked.rs"), "fn b() {}\n").unwrap();
        fs::write(root.join("ignored.txt"), "x\n").unwrap();
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn lists_tracked_and_untracked_but_not_ignored() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            seed_repo(root);
            let e = tracked_files(root, &[]).unwrap();
            assert_eq!(
                e.files,
                [
                    ".gitignore",
                    "docs/plans/plan.md",
                    "tracked.rs",
                    "untracked.rs"
                ]
            );
            assert_eq!(e.skipped_outside, 0);
            assert!(e.warnings.is_empty(), "{:?}", e.warnings);
        });
    }

    #[test]
    fn exclude_globs_drop_matching_paths() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            seed_repo(root);
            let e = tracked_files(root, &["docs/plans/**".to_string()]).unwrap();
            assert_eq!(e.files, [".gitignore", "tracked.rs", "untracked.rs"]);
        });
    }

    #[test]
    fn a_bad_exclude_glob_is_a_validation_error() {
        with_root(|root| {
            let err = tracked_files(root, &["docs/[".to_string()]).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(err.to_string().contains("docs/["), "{err}");
        });
    }

    #[test]
    fn a_non_git_directory_is_an_other_error() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let err = tracked_files(root, &[]).unwrap_err();
            assert_eq!(kind_of(&err), "other");
            assert!(
                err.to_string().starts_with("git ls-files failed: "),
                "{err}"
            );
        });
    }
}
