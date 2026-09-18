//! `tomlctl sweep`: regex hits over the tracked files, keyed by canonical path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use regex::bytes::Regex;
use serde_json::{Value as JsonValue, json};

use crate::io::{join_under, relativise_under};
use crate::query::compile_user_bytes_regex;
use crate::repo_files::tracked_files;

/// A NUL this early marks the file binary, as ripgrep decides it.
const BINARY_SNIFF_BYTES: usize = 8192;

pub(crate) const DEFAULT_MAX_FILE_BYTES: u64 = 4 << 20;
pub(crate) const DEFAULT_MAX_HITS: usize = 5000;

pub(crate) struct SweepOptions {
    pub(crate) max_file_bytes: u64,
    /// Distinct `file:line` sites, not matches.
    pub(crate) max_hits: usize,
    pub(crate) exclude: Vec<String>,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_hits: DEFAULT_MAX_HITS,
            // Ledgers, the backlog and plan documents quote sweep strings
            // verbatim, so every pattern would match its own record there.
            exclude: vec![".claude/**".to_string(), "docs/plans/**".to_string()],
        }
    }
}

/// One matched site. `pattern` indexes the caller's pattern slice; a line
/// several patterns hit carries the lowest index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Hit {
    pub(crate) file: String,
    pub(crate) line: u64,
    pub(crate) pattern: usize,
}

#[derive(Debug, Default)]
pub(crate) struct SweepReport {
    pub(crate) hits: Vec<Hit>,
    /// Canonical keys of the files whose bytes were searched. A key absent
    /// here was skipped or cut by `max_hits`, so its silence is not evidence.
    pub(crate) scanned: BTreeSet<String>,
    /// Canonical keys of the enumerated files the engine opened and refused
    /// to search: binary, oversize or unreadable. A file that never reached
    /// the enumeration, or whose path could not be canonicalised (a staged
    /// delete, a dangling symlink), is in neither set.
    pub(crate) skipped: BTreeSet<String>,
    pub(crate) skipped_binary: usize,
    pub(crate) skipped_oversize: usize,
    /// Read failures, directories, and entries resolving outside the root.
    pub(crate) skipped_unreadable: usize,
    /// Git's exit-0 warnings, each a subtree the listing silently omits.
    pub(crate) skipped_unenumerated: usize,
    pub(crate) truncated: bool,
}

impl SweepReport {
    pub(crate) fn files_scanned(&self) -> usize {
        self.scanned.len()
    }

    /// A skipped file can hide a site, so any skip at all — binary included —
    /// clears this.
    pub(crate) fn coverage_complete(&self) -> bool {
        self.skipped_binary == 0
            && self.skipped_oversize == 0
            && self.skipped_unreadable == 0
            && self.skipped_unenumerated == 0
            && !self.truncated
    }
}

/// Why a listed file contributed no hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skip {
    /// Read failure or a directory.
    Unreadable,
    Oversize,
    Binary,
}

pub(crate) struct ScannedFile {
    pub(crate) bytes: Vec<u8>,
    /// Offset of every `\n`, for `line_of`.
    pub(crate) newlines: Vec<usize>,
}

impl ScannedFile {
    /// Bytes of a one-based line without its `\n`; `None` past the last
    /// line, so a trailing newline does not open an empty extra line.
    pub(crate) fn line(&self, line: u64) -> Option<&[u8]> {
        let index = usize::try_from(line).ok()?.checked_sub(1)?;
        let start = match index.checked_sub(1) {
            None => 0,
            Some(prev) => self.newlines.get(prev)? + 1,
        };
        let end = self
            .newlines
            .get(index)
            .copied()
            .unwrap_or(self.bytes.len());
        (start < self.bytes.len()).then(|| &self.bytes[start..end])
    }
}

/// Read one file under the predicates every listed file passes before it is
/// searched, so a caller re-reading a file sees the bytes the sweep saw.
pub(crate) fn scan_file(path: &Path, max_file_bytes: u64) -> Result<ScannedFile, Skip> {
    let meta = fs::metadata(path).map_err(|_| Skip::Unreadable)?;
    if meta.is_dir() {
        return Err(Skip::Unreadable);
    }
    if meta.len() > max_file_bytes {
        return Err(Skip::Oversize);
    }
    let bytes = fs::read(path).map_err(|_| Skip::Unreadable)?;
    if memchr::memchr(0, &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)]).is_some() {
        return Err(Skip::Binary);
    }
    let newlines = memchr::memchr_iter(b'\n', &bytes).collect();
    Ok(ScannedFile { bytes, newlines })
}

/// One-based line of a byte offset, from a file's newline table.
pub(crate) fn line_of(newlines: &[usize], offset: usize) -> u64 {
    newlines.partition_point(|&nl| nl < offset) as u64 + 1
}

pub(crate) fn compile(patterns: &[String]) -> Result<Vec<Regex>> {
    patterns
        .iter()
        .map(|p| compile_user_bytes_regex(p))
        .collect()
}

pub(crate) fn run(root: &Path, patterns: &[String], opts: &SweepOptions) -> Result<SweepReport> {
    run_compiled(root, &compile(patterns)?, opts)
}

pub(crate) fn run_compiled(
    root: &Path,
    regexes: &[Regex],
    opts: &SweepOptions,
) -> Result<SweepReport> {
    let enumeration = tracked_files(root, &opts.exclude)?;
    let mut report = SweepReport {
        skipped_unreadable: enumeration.skipped_outside,
        skipped_unenumerated: enumeration.warnings.len(),
        ..SweepReport::default()
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();

    'files: for entry in &enumeration.files {
        let Some(absolute) = join_under(root, entry) else {
            report.skipped_unreadable += 1;
            continue;
        };
        let Ok(canonical) = fs::canonicalize(&absolute) else {
            report.skipped_unreadable += 1;
            continue;
        };
        let Some(key) = relativise_under(root, &canonical) else {
            report.skipped_unreadable += 1;
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        let file = match scan_file(&canonical, opts.max_file_bytes) {
            Ok(file) => file,
            Err(skip) => {
                match skip {
                    Skip::Unreadable => report.skipped_unreadable += 1,
                    Skip::Oversize => report.skipped_oversize += 1,
                    Skip::Binary => report.skipped_binary += 1,
                }
                report.skipped.insert(key);
                continue;
            }
        };
        report.scanned.insert(key.clone());

        let mut sites: BTreeMap<u64, usize> = BTreeMap::new();
        for (index, re) in regexes.iter().enumerate() {
            for m in re.find_iter(&file.bytes) {
                sites
                    .entry(line_of(&file.newlines, m.start()))
                    .or_insert(index);
            }
        }
        for (line, pattern) in sites {
            if report.hits.len() >= opts.max_hits {
                report.truncated = true;
                break 'files;
            }
            report.hits.push(Hit {
                file: key.clone(),
                line,
                pattern,
            });
        }
    }

    report.hits.sort();
    Ok(report)
}

pub(crate) fn report_json(report: &SweepReport) -> JsonValue {
    let hits: Vec<JsonValue> = report
        .hits
        .iter()
        .map(|h| json!({ "file": h.file, "line": h.line, "pattern": h.pattern }))
        .collect();
    json!({
        "hits": hits,
        "files_scanned": report.files_scanned(),
        "skipped": {
            "binary": report.skipped_binary,
            "oversize": report.skipped_oversize,
            "unreadable": report.skipped_unreadable,
            "unenumerated": report.skipped_unenumerated,
        },
        "truncated": report.truncated,
        "coverage_complete": report.coverage_complete(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, git_available, with_root, write};

    fn patterns(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn hit(file: &str, line: u64, pattern: usize) -> Hit {
        Hit {
            file: file.to_string(),
            line,
            pattern,
        }
    }

    fn sweep(root: &Path, pats: &[&str], opts: &SweepOptions) -> SweepReport {
        git(root, &["init", "-q"]);
        run(root, &patterns(pats), opts).unwrap()
    }

    #[test]
    fn an_lf_file_reports_one_based_lines() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "a.rs", b"fn a() {}\nfn foo() {}\nfoo();\n");
            let report = sweep(root, &["(?-u:\\bfoo\\b)"], &SweepOptions::default());
            assert_eq!(report.hits, [hit("a.rs", 2, 0), hit("a.rs", 3, 0)]);
            assert_eq!(report.files_scanned(), 1);
            assert!(report.coverage_complete());
        });
    }

    #[test]
    fn a_crlf_line_still_ends_where_dollar_expects() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "a.txt", b"x\r\nfoo\r\n");
            let report = sweep(root, &["(?-u:foo$)"], &SweepOptions::default());
            assert_eq!(report.hits, [hit("a.txt", 2, 0)]);
        });
    }

    #[test]
    fn a_nul_bearing_file_is_skipped_and_clears_coverage() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "blob.bin", b"foo\0foo\n");
            let report = sweep(root, &["foo"], &SweepOptions::default());
            assert!(report.hits.is_empty(), "{:?}", report.hits);
            assert_eq!(report.skipped_binary, 1);
            assert_eq!(report.skipped, ["blob.bin".to_string()].into());
            assert_eq!(report.files_scanned(), 0);
            assert!(!report.coverage_complete());
        });
    }

    #[test]
    fn an_oversize_file_is_skipped_and_counted() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "big.txt", b"foo foo foo\n");
            write(root, "small.txt", b"foo\n");
            let opts = SweepOptions {
                max_file_bytes: 8,
                ..SweepOptions::default()
            };
            let report = sweep(root, &["foo"], &opts);
            assert_eq!(report.hits, [hit("small.txt", 1, 0)]);
            assert_eq!(report.skipped_oversize, 1);
            assert_eq!(report.skipped, ["big.txt".to_string()].into());
            assert!(!report.coverage_complete());
        });
    }

    #[test]
    fn a_line_is_addressed_one_based_without_its_newline() {
        let bytes = b"a\n\nbc\n".to_vec();
        let newlines = memchr::memchr_iter(b'\n', &bytes).collect();
        let file = ScannedFile { bytes, newlines };
        assert_eq!(file.line(0), None);
        assert_eq!(file.line(1), Some(&b"a"[..]));
        assert_eq!(file.line(2), Some(&b""[..]));
        assert_eq!(file.line(3), Some(&b"bc"[..]));
        assert_eq!(file.line(4), None);

        let unterminated = ScannedFile {
            bytes: b"a\nb".to_vec(),
            newlines: vec![1],
        };
        assert_eq!(unterminated.line(2), Some(&b"b"[..]));
        assert_eq!(unterminated.line(3), None);
    }

    #[test]
    fn the_hit_cap_truncates_over_distinct_sites() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "a.txt", b"foo\nfoo\nfoo\n");
            let opts = SweepOptions {
                max_hits: 2,
                ..SweepOptions::default()
            };
            let report = sweep(root, &["foo"], &opts);
            assert_eq!(report.hits, [hit("a.txt", 1, 0), hit("a.txt", 2, 0)]);
            assert!(report.truncated);
            assert!(!report.coverage_complete());
        });
    }

    #[test]
    fn a_run_landing_exactly_on_the_cap_is_not_truncated() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "a.txt", b"foo\nfoo\n");
            let opts = SweepOptions {
                max_hits: 2,
                ..SweepOptions::default()
            };
            let report = sweep(root, &["foo"], &opts);
            assert_eq!(report.hits.len(), 2);
            assert!(!report.truncated);
        });
    }

    #[test]
    fn one_line_is_one_hit_under_the_lowest_pattern_index() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "a.txt", b"bar foo foo\nbar\n");
            let report = sweep(root, &["foo", "bar"], &SweepOptions::default());
            assert_eq!(report.hits, [hit("a.txt", 1, 0), hit("a.txt", 2, 1)]);
        });
    }

    #[test]
    fn hits_sort_by_file_then_line() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "z.txt", b"foo\n");
            write(root, "dir/a.txt", b"x\nfoo\n");
            let report = sweep(root, &["foo"], &SweepOptions::default());
            assert_eq!(report.hits, [hit("dir/a.txt", 2, 0), hit("z.txt", 1, 0)]);
        });
    }

    #[test]
    fn an_excluded_glob_keeps_its_files_out_of_the_hits() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            write(root, "docs/plans/plan.md", b"foo\n");
            write(root, ".claude/ledger.toml", b"foo\n");
            write(root, "src/a.rs", b"foo\n");
            let report = sweep(root, &["foo"], &SweepOptions::default());
            assert_eq!(report.hits, [hit("src/a.rs", 1, 0)]);
            assert_eq!(report.files_scanned(), 1);

            let opts = SweepOptions {
                exclude: Vec::new(),
                ..SweepOptions::default()
            };
            let report = run(root, &patterns(&["foo"]), &opts).unwrap();
            assert_eq!(report.files_scanned(), 3);
        });
    }

    #[test]
    fn a_staged_delete_counts_as_unreadable() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "gone.txt", b"foo\n");
            git(root, &["add", "gone.txt"]);
            fs::remove_file(root.join("gone.txt")).unwrap();
            let report = run(root, &patterns(&["foo"]), &SweepOptions::default()).unwrap();
            assert!(report.hits.is_empty());
            assert_eq!(report.skipped_unreadable, 1);
            assert!(!report.coverage_complete());
        });
    }

    /// The `.github/` mirror of `claude/` is a symlink, so a site reached
    /// through two listings must count once. A link out of the tree or to
    /// nothing is never read, and git lists both without a warning.
    #[cfg(unix)]
    #[test]
    fn symlinks_dedupe_by_target_and_never_reach_outside_the_root() {
        use std::os::unix::fs::symlink;
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "a.txt", b"foo\n");
            symlink("a.txt", root.join("link.txt")).unwrap();
            git(root, &["add", "-A"]);
            let report = run(root, &patterns(&["foo"]), &SweepOptions::default()).unwrap();
            assert_eq!(report.hits, [hit("a.txt", 1, 0)]);
            assert_eq!(report.files_scanned(), 1);
            assert!(report.coverage_complete());
        });
        with_root(|root| {
            git(root, &["init", "-q"]);
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("secret.txt");
            fs::write(&target, b"foo\n").unwrap();
            symlink(&target, root.join("escape.txt")).unwrap();
            git(root, &["add", "-A"]);
            let report = run(root, &patterns(&["foo"]), &SweepOptions::default()).unwrap();
            assert!(report.hits.is_empty(), "{:?}", report.hits);
            assert_eq!(report.files_scanned(), 0);
            assert_eq!(report.skipped_unreadable, 1);
            assert!(!report.coverage_complete());
        });
        with_root(|root| {
            git(root, &["init", "-q"]);
            symlink("nowhere", root.join("dangling")).unwrap();
            git(root, &["add", "-A"]);
            let report = run(root, &patterns(&["foo"]), &SweepOptions::default()).unwrap();
            assert!(report.hits.is_empty(), "{:?}", report.hits);
            assert_eq!(report.skipped_unreadable, 1);
            assert_eq!(report.skipped_unenumerated, 0);
            assert!(!report.coverage_complete());
        });
    }

    #[test]
    fn a_bad_pattern_fails_before_any_enumeration() {
        with_root(|root| {
            let err = run(root, &patterns(&["("]), &SweepOptions::default()).unwrap_err();
            assert!(err.to_string().starts_with("invalid regex"), "{err}");
        });
    }

    #[test]
    fn report_json_keeps_the_documented_key_order() {
        let report = SweepReport {
            hits: vec![hit("a.rs", 3, 1)],
            scanned: ["a.rs", "b.rs"].map(String::from).into(),
            skipped_binary: 1,
            ..SweepReport::default()
        };
        assert_eq!(
            serde_json::to_string(&report_json(&report)).unwrap(),
            r#"{"hits":[{"file":"a.rs","line":3,"pattern":1}],"files_scanned":2,"skipped":{"binary":1,"oversize":0,"unreadable":0,"unenumerated":0},"truncated":false,"coverage_complete":false}"#
        );
    }
}
