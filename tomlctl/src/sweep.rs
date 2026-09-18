//! `tomlctl sweep`: regex hits over the tracked files, keyed by canonical path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use regex::bytes::Regex;
use serde_json::{Value as JsonValue, json};

use crate::io::relativise_under;
use crate::query::compile_user_bytes_regex;
use crate::repo_files::tracked_files;

/// A NUL this early marks the file binary, as ripgrep decides it.
const BINARY_SNIFF_BYTES: usize = 8192;

pub(crate) struct SweepOptions {
    pub(crate) max_file_bytes: u64,
    /// Distinct `file:line` sites, not matches.
    pub(crate) max_hits: usize,
    pub(crate) exclude: Vec<String>,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            max_file_bytes: 4 << 20,
            max_hits: 5000,
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
    pub(crate) files_scanned: usize,
    pub(crate) skipped_binary: usize,
    pub(crate) skipped_oversize: usize,
    /// Read failures, directories, and entries resolving outside the root.
    pub(crate) skipped_unreadable: usize,
    /// Git's exit-0 warnings, each a subtree the listing silently omits.
    pub(crate) skipped_unenumerated: usize,
    pub(crate) truncated: bool,
}

impl SweepReport {
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

pub(crate) fn run(root: &Path, patterns: &[String], opts: &SweepOptions) -> Result<SweepReport> {
    let regexes = patterns
        .iter()
        .map(|p| compile_user_bytes_regex(p))
        .collect::<Result<Vec<Regex>>>()?;
    let enumeration = tracked_files(root, &opts.exclude)?;
    let mut report = SweepReport {
        skipped_unreadable: enumeration.skipped_outside,
        skipped_unenumerated: enumeration.warnings.len(),
        ..SweepReport::default()
    };
    let mut scanned: BTreeSet<String> = BTreeSet::new();

    'files: for entry in &enumeration.files {
        // Joined component-wise: a canonical root carries a verbatim
        // `\\?\` prefix on Windows, under which a `/` is not a separator.
        let mut absolute = root.to_path_buf();
        absolute.extend(entry.to_string_lossy().split('/'));
        let Ok(canonical) = fs::canonicalize(&absolute) else {
            report.skipped_unreadable += 1;
            continue;
        };
        let Some(key) = relativise_under(root, &canonical) else {
            report.skipped_unreadable += 1;
            continue;
        };
        if !scanned.insert(key.clone()) {
            continue;
        }
        let Ok(meta) = fs::metadata(&canonical) else {
            report.skipped_unreadable += 1;
            continue;
        };
        if meta.is_dir() {
            report.skipped_unreadable += 1;
            continue;
        }
        if meta.len() > opts.max_file_bytes {
            report.skipped_oversize += 1;
            continue;
        }
        let Ok(buf) = fs::read(&canonical) else {
            report.skipped_unreadable += 1;
            continue;
        };
        if memchr::memchr(0, &buf[..buf.len().min(BINARY_SNIFF_BYTES)]).is_some() {
            report.skipped_binary += 1;
            continue;
        }
        report.files_scanned += 1;

        let newlines: Vec<usize> = memchr::memchr_iter(b'\n', &buf).collect();
        let mut sites: BTreeMap<u64, usize> = BTreeMap::new();
        for (index, re) in regexes.iter().enumerate() {
            for m in re.find_iter(&buf) {
                let line = newlines.partition_point(|&nl| nl < m.start()) as u64 + 1;
                sites.entry(line).or_insert(index);
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
        "files_scanned": report.files_scanned,
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
    use crate::test_support::with_root;
    use std::process::Command;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

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
            assert_eq!(report.files_scanned, 1);
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
            assert_eq!(report.files_scanned, 0);
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
            assert!(!report.coverage_complete());
        });
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
            assert_eq!(report.files_scanned, 1);

            let opts = SweepOptions {
                exclude: Vec::new(),
                ..SweepOptions::default()
            };
            let report = run(root, &patterns(&["foo"]), &opts).unwrap();
            assert_eq!(report.files_scanned, 3);
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
            files_scanned: 2,
            skipped_binary: 1,
            ..SweepReport::default()
        };
        assert_eq!(
            serde_json::to_string(&report_json(&report)).unwrap(),
            r#"{"hits":[{"file":"a.rs","line":3,"pattern":1}],"files_scanned":2,"skipped":{"binary":1,"oversize":0,"unreadable":0,"unenumerated":0},"truncated":false,"coverage_complete":false}"#
        );
    }
}
