//! `review survey` — the vocabulary's deterministic detectors, recorded as a
//! run's signals.
//!
//! Stack detectors run over `git ls-files`, diff detectors over the run's
//! `base..HEAD` patch inside its scope, and health detectors over the tree at
//! HEAD or a `git log` window. A true signal is recorded with `source =
//! "detector"` and its `roots`; a detector that cannot be evaluated is
//! recorded with `source = "unknown"` and the reason as its only `evidence`,
//! never dropped as false. Re-running replaces both and keeps judged signals.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use jiff::SignedDuration;
use regex::bytes::Regex;
use serde_json::{Value as JsonValue, json};
use toml::Table;
use toml::Value as TomlValue;

use super::catalogue::{SignalKind, run_catalogue};
use super::invalid;
use super::store::{self, Run, Signal, Target};
use crate::cli::{ReadIntegrityArgs, ReviewSurveyArgs};
use crate::io::repo_or_cwd_root;
use crate::output::print_json_compact;
use crate::query::compile_user_bytes_regex;

const DETECTOR: &str = "detector";
const UNKNOWN: &str = "unknown";

/// The longest health `window`, in days: a century.
const MAX_WINDOW_DAYS: i64 = 36_500;

pub(crate) fn run(target: &Target, args: ReviewSurveyArgs) -> Result<()> {
    let verb = "review survey";
    let root = repo_or_cwd_root()?;
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let stored = store::load(&target.path, &read)?;
    let run = stored
        .runs
        .iter()
        .find(|r| r.id == args.run)
        .ok_or_else(|| invalid(format!("{verb}: no run {} in this store", args.run)))?;
    if !run.finished.is_empty() {
        return Err(invalid(format!("{verb}: run {} is finished", run.id)));
    }
    let catalogue = run_catalogue(&root, run)?;
    let detected = evaluate(&root, run, &catalogue.detectors());

    let fresh: Vec<Signal> = detected.iter().filter_map(Detected::signal).collect();
    let names: BTreeSet<&str> = detected.iter().map(|d| d.name.as_str()).collect();
    let run_id = args.run.clone();
    store::mutate(&target.path, &args.integrity, |store| {
        let run = store
            .runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .ok_or_else(|| invalid(format!("{verb}: no run {run_id} in this store")))?;
        if !run.finished.is_empty() {
            return Err(invalid(format!("{verb}: run {run_id} is finished")));
        }
        let kept: Vec<Signal> = run
            .signals
            .drain(..)
            .filter(|s| s.source != DETECTOR && s.source != UNKNOWN)
            .filter(|s| !names.contains(s.name.as_str()))
            .collect();
        run.signals = fresh.iter().cloned().chain(kept).collect();
        Ok(())
    })?;

    let mut signals = Vec::new();
    let mut unknown = Vec::new();
    for d in &detected {
        match &d.outcome {
            Outcome::True { roots, evidence } => {
                let mut row = json!({"name": d.name, "kind": d.kind.as_str(), "roots": roots});
                if !evidence.is_empty() {
                    row["evidence"] = json!(evidence);
                }
                signals.push(row);
            }
            Outcome::Unknown(reason) => {
                unknown.push(json!({"name": d.name, "kind": d.kind.as_str(), "reason": reason}))
            }
            Outcome::False => {}
        }
    }
    print_json_compact(&json!({
        "ok": true,
        "run": args.run,
        "signals": signals,
        "unknown": unknown,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    True {
        roots: Vec<String>,
        evidence: Vec<String>,
    },
    False,
    Unknown(String),
}

impl Outcome {
    fn found(roots: BTreeSet<String>) -> Self {
        Self::True {
            roots: roots.into_iter().collect(),
            evidence: Vec::new(),
        }
    }

    /// True when anything passed, else unknown when any input failed, else false.
    fn settle(roots: BTreeSet<String>, errors: &[String]) -> Self {
        if !roots.is_empty() {
            Self::found(roots)
        } else if let Some(first) = errors.first() {
            Self::Unknown(match errors.len() {
                1 => first.clone(),
                n => format!("{first} (and {} more)", n - 1),
            })
        } else {
            Self::False
        }
    }
}

struct Detected {
    name: String,
    kind: SignalKind,
    outcome: Outcome,
}

impl Detected {
    fn signal(&self) -> Option<Signal> {
        let (source, roots, evidence) = match &self.outcome {
            Outcome::True { roots, evidence } => (DETECTOR, roots.clone(), evidence.clone()),
            Outcome::Unknown(reason) => (UNKNOWN, Vec::new(), vec![reason.clone()]),
            Outcome::False => return None,
        };
        Some(Signal {
            name: self.name.clone(),
            source: source.to_string(),
            roots,
            accepted: None,
            evidence,
        })
    }
}

/// Stacks first, since a health `count` entry is evaluated only where its
/// stack signal is true.
fn evaluate(root: &Path, run: &Run, detectors: &[&super::catalogue::Signal]) -> Vec<Detected> {
    let mut repo = Repo {
        root,
        base: &run.base,
        scope: &run.scope,
        tracked: list_tracked(root),
        diff: None,
    };
    let mut out: Vec<Detected> = Vec::new();
    for kind in [SignalKind::Stack, SignalKind::Diff, SignalKind::Health] {
        for signal in detectors.iter().filter(|s| s.kind == kind) {
            let outcome = match signal
                .detect
                .as_ref()
                .ok_or_else(|| "no `detect` table".to_string())
                .and_then(Detect::parse)
            {
                Err(reason) => Outcome::Unknown(reason),
                Ok(detect) => match kind {
                    SignalKind::Stack => repo.stack(&detect),
                    SignalKind::Diff => repo.diff_signal(&detect),
                    _ => repo.health(&detect, &out),
                },
            };
            out.push(Detected {
                name: signal.name.clone(),
                kind,
                outcome,
            });
        }
    }
    out
}

// ------------------------------------------------------------ detect table

#[derive(Debug)]
enum Threshold {
    AtLeast(i64),
    PerKloc(i64),
}

impl Threshold {
    fn reached(&self, measure: i64, lines: i64) -> bool {
        match *self {
            Self::AtLeast(n) => measure >= n,
            Self::PerKloc(n) => {
                lines > 0 && measure.saturating_mul(1000) >= n.saturating_mul(lines)
            }
        }
    }
}

struct CountEntry {
    stack: String,
    files: GlobSet,
    patterns: Vec<Vec<u8>>,
}

struct Detect {
    files: Option<GlobSet>,
    contains: Vec<Regex>,
    manifest: Vec<Vec<String>>,
    count: Option<Vec<CountEntry>>,
    window: Option<i64>,
    threshold: Option<Threshold>,
}

fn strings(value: &TomlValue, key: &str) -> Result<Vec<String>, String> {
    match value {
        TomlValue::String(s) => Ok(vec![s.clone()]),
        TomlValue::Array(items) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("`{key}` must hold strings"))
            })
            .collect(),
        _ => Err(format!("`{key}` must be a string or an array of strings")),
    }
}

fn globset(globs: &[String], key: &str) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob).map_err(|e| format!("`{key}` glob {glob:?}: {e}"))?);
    }
    builder.build().map_err(|e| format!("`{key}`: {e}"))
}

impl Detect {
    fn parse(table: &Table) -> Result<Self, String> {
        let files = match table.get("files") {
            Some(v) => Some(globset(&strings(v, "files")?, "files")?),
            None => None,
        };
        let contains = match table.get("contains") {
            Some(v) => strings(v, "contains")?
                .iter()
                .map(|p| {
                    compile_user_bytes_regex(p)
                        .map_err(|e| format!("`contains` pattern {p:?} does not compile: {e}"))
                })
                .collect::<Result<_, _>>()?,
            None => Vec::new(),
        };
        let manifest = match table.get("manifest") {
            Some(v) => strings(v, "manifest")?
                .iter()
                .map(|p| p.split('.').map(str::to_string).collect())
                .collect(),
            None => Vec::new(),
        };
        let count = match table.get("count") {
            None => None,
            Some(TomlValue::Table(entries)) => Some(
                entries
                    .iter()
                    .map(|(stack, value)| CountEntry::parse(stack, value))
                    .collect::<Result<_, _>>()?,
            ),
            Some(_) => return Err("`count` must be a table keyed by stack signal".into()),
        };
        let window = match table.get("window") {
            None => None,
            Some(TomlValue::Integer(n)) if *n > MAX_WINDOW_DAYS => {
                return Err(format!(
                    "`window` must be at most {MAX_WINDOW_DAYS} days, got {n}"
                ));
            }
            Some(TomlValue::Integer(n)) if *n > 0 => Some(*n),
            Some(_) => return Err("`window` must be a positive number of days".into()),
        };
        let threshold = match table.get("threshold") {
            None => None,
            Some(TomlValue::Integer(n)) => Some(Threshold::AtLeast(*n)),
            Some(TomlValue::Table(t)) => match (t.len(), t.get("per_kloc")) {
                (1, Some(TomlValue::Integer(n))) => Some(Threshold::PerKloc(*n)),
                _ => return Err("`threshold` table must be `{ per_kloc = <integer> }`".into()),
            },
            Some(_) => return Err("`threshold` must be an integer or `{ per_kloc = N }`".into()),
        };
        Ok(Self {
            files,
            contains,
            manifest,
            count,
            window,
            threshold,
        })
    }

    fn selects(&self, path: &str) -> bool {
        self.files.as_ref().is_none_or(|g| g.is_match(path))
    }

    fn contains_passes(&self, haystack: &[u8]) -> bool {
        self.contains.is_empty() || self.contains.iter().any(|re| re.is_match(haystack))
    }
}

impl CountEntry {
    fn parse(stack: &str, value: &TomlValue) -> Result<Self, String> {
        let at = format!("count.{stack}");
        let table = value
            .as_table()
            .ok_or_else(|| format!("`{at}` must be `{{ files = [...], patterns = [...] }}`"))?;
        if let Some(key) = table.keys().find(|k| *k != "files" && *k != "patterns") {
            return Err(format!(
                "`{at}.{key}` is not a count key; expected files, patterns"
            ));
        }
        let files = table
            .get("files")
            .ok_or_else(|| format!("`{at}.files` is required"))?;
        let patterns = table
            .get("patterns")
            .ok_or_else(|| format!("`{at}.patterns` is required"))?;
        let patterns: Vec<Vec<u8>> = strings(patterns, &format!("{at}.patterns"))?
            .into_iter()
            .filter(|p| !p.is_empty())
            .map(String::into_bytes)
            .collect();
        Ok(Self {
            stack: stack.to_string(),
            files: globset(&strings(files, &format!("{at}.files"))?, &at)?,
            patterns,
        })
    }

    /// Non-overlapping literal occurrences in one line, matched with `\n`
    /// prepended so a pattern starting `\n` anchors at the line start.
    fn occurrences(&self, line: &[u8]) -> i64 {
        let mut haystack = Vec::with_capacity(line.len() + 1);
        haystack.push(b'\n');
        haystack.extend_from_slice(line.strip_suffix(b"\r").unwrap_or(line));
        let n: usize = self
            .patterns
            .iter()
            .map(|p| memchr::memmem::find_iter(&haystack, p).count())
            .sum();
        i64::try_from(n).unwrap_or(i64::MAX)
    }
}

// ------------------------------------------------------------ git inputs

fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(["-c", "core.quotePath=false"]);
    cmd
}

/// The `git log` filter for a window of `days` ending now, or none when the
/// window reaches the epoch, since a negative `--max-age` makes git list no
/// commits at all.
fn window_filter(days: i64) -> Option<String> {
    let cutoff = days
        .checked_mul(24)
        .and_then(SignedDuration::try_from_hours)
        .and_then(|span| crate::time::now().checked_sub(span).ok())?
        .as_second();
    (cutoff > 0).then(|| format!("--max-age={cutoff}"))
}

fn git_output(cmd: &mut Command, what: &str) -> Result<Vec<u8>, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{what} failed: {}", stderr.trim()));
    }
    Ok(out.stdout)
}

fn list_tracked(root: &Path) -> Result<Vec<String>, String> {
    let out = git_output(
        git(root).args(["ls-files", "-z", "--full-name"]),
        "git ls-files",
    )?;
    Ok(String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map_or_else(|| ".".to_string(), |(dir, _)| dir.to_string())
}

fn line_count(bytes: &[u8]) -> i64 {
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let unterminated = bytes.last().is_some_and(|&b| b != b'\n');
    i64::try_from(newlines + usize::from(unterminated)).unwrap_or(i64::MAX)
}

struct Changed {
    path: String,
    old: Option<String>,
    /// The file's patch without its `---` / `+++` lines.
    patch: Vec<u8>,
}

/// Adds and removals of one file within a `git log -p` or `git diff` patch.
enum PatchLine<'a> {
    Added(&'a [u8]),
    Removed(&'a [u8]),
}

/// Walks a `-p` patch, handing each hunk's added and removed lines to `f`
/// with the file they belong to.
fn walk_patch(patch: &[u8], mut f: impl FnMut(&str, PatchLine<'_>)) {
    let mut file = String::new();
    let mut in_hunk = false;
    for line in patch.split(|&b| b == b'\n') {
        if line.starts_with(b"diff --git ") {
            file.clear();
            in_hunk = false;
        } else if line.starts_with(b"@@") {
            in_hunk = true;
        } else if !in_hunk {
            if let Some(path) = line.strip_prefix(b"--- a/") {
                file = header_path(path);
            } else if let Some(path) = line.strip_prefix(b"+++ b/") {
                file = header_path(path);
            }
        } else if let Some(rest) = line.strip_prefix(b"+") {
            f(&file, PatchLine::Added(rest));
        } else if let Some(rest) = line.strip_prefix(b"-") {
            f(&file, PatchLine::Removed(rest));
        }
    }
}

/// Git ends a header path that holds a space with a tab.
fn header_path(raw: &[u8]) -> String {
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    String::from_utf8_lossy(raw)
        .trim_end_matches('\t')
        .to_string()
}

/// A `count` entry whose stack is true, with that stack's roots (`None` for `"*"`).
type LiveEntry<'e> = (&'e CountEntry, Option<&'e [String]>);

struct Repo<'a> {
    root: &'a Path,
    base: &'a str,
    scope: &'a [String],
    tracked: Result<Vec<String>, String>,
    diff: Option<Result<Vec<Changed>, String>>,
}

impl Repo<'_> {
    fn tracked(&self) -> Result<&[String], String> {
        self.tracked.as_deref().map_err(Clone::clone)
    }

    // -------------------------------------------------------- stack

    fn stack(&self, detect: &Detect) -> Outcome {
        let tracked = match self.tracked() {
            Ok(t) => t,
            Err(e) => return Outcome::Unknown(e),
        };
        let mut roots = BTreeSet::new();
        let mut errors = Vec::new();
        for path in tracked.iter().filter(|p| detect.selects(p)) {
            if detect.manifest.is_empty() && detect.contains.is_empty() {
                roots.insert(parent_dir(path));
                continue;
            }
            let bytes = match fs::read(self.root.join(path)) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    errors.push(format!("{path}: {e}"));
                    continue;
                }
            };
            match manifest_passes(path, &bytes, &detect.manifest) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            }
            if detect.contains_passes(&bytes) {
                roots.insert(parent_dir(path));
            }
        }
        Outcome::settle(roots, &errors)
    }

    // -------------------------------------------------------- diff

    fn changed(&mut self) -> Result<&[Changed], String> {
        if self.diff.is_none() {
            self.diff = Some(self.load_diff());
        }
        match self.diff.as_ref().expect("filled above") {
            Ok(files) => Ok(files),
            Err(e) => Err(e.clone()),
        }
    }

    /// Pairs `--name-status` entries with the patch's `diff --git` sections;
    /// both come from the same diff queue in the same order.
    fn load_diff(&self) -> Result<Vec<Changed>, String> {
        if self.base.is_empty() {
            return Err("the run records no base".into());
        }
        let range = format!("{}..HEAD", self.base);
        let specs: Vec<String> = self.scope.iter().map(|s| format!(":(glob){s}")).collect();
        let common = ["--no-ext-diff", "--no-textconv", "--no-color", "-M"];
        let names = git_output(
            git(self.root)
                .args(["diff", "--name-status", "-z"])
                .args(common)
                .arg(&range)
                .arg("--")
                .args(&specs),
            "git diff --name-status",
        )?;
        let patch = git_output(
            git(self.root)
                .args(["diff", "-p"])
                .args(common)
                .arg(&range)
                .arg("--")
                .args(&specs),
            "git diff",
        )?;

        let mut entries: Vec<(String, Option<String>)> = Vec::new();
        let mut fields = names.split(|&b| b == 0).filter(|f| !f.is_empty());
        while let Some(status) = fields.next() {
            let text = |f: Option<&[u8]>| f.map(|b| String::from_utf8_lossy(b).into_owned());
            if matches!(status.first(), Some(b'R' | b'C')) {
                let old = text(fields.next());
                let new = text(fields.next()).ok_or("truncated `git diff --name-status`")?;
                entries.push((new, old));
            } else {
                let path = text(fields.next()).ok_or("truncated `git diff --name-status`")?;
                entries.push((path, None));
            }
        }

        let mut sections: Vec<Vec<u8>> = Vec::new();
        let mut in_hunk = false;
        for line in patch.split_inclusive(|&b| b == b'\n') {
            if line.starts_with(b"diff --git ") {
                sections.push(Vec::new());
                in_hunk = false;
            } else if line.starts_with(b"@@") {
                in_hunk = true;
            }
            let Some(current) = sections.last_mut() else {
                continue;
            };
            let file_header = line.starts_with(b"--- ") || line.starts_with(b"+++ ");
            if in_hunk || !file_header {
                current.extend_from_slice(line);
            }
        }
        if sections.len() != entries.len() {
            return Err(format!(
                "git diff listed {} changed files but its patch has {} sections",
                entries.len(),
                sections.len()
            ));
        }
        Ok(entries
            .into_iter()
            .zip(sections)
            .map(|((path, old), patch)| Changed { path, old, patch })
            .collect())
    }

    fn diff_signal(&mut self, detect: &Detect) -> Outcome {
        let root = self.root;
        let changed = match self.changed() {
            Ok(c) => c,
            Err(e) => return Outcome::Unknown(e),
        };
        let mut roots = BTreeSet::new();
        let mut errors = Vec::new();
        for file in changed {
            let selected = detect.selects(&file.path)
                || file.old.as_deref().is_some_and(|old| detect.selects(old));
            if !selected || !detect.contains_passes(&file.patch) {
                continue;
            }
            if !detect.manifest.is_empty() {
                let bytes = fs::read(root.join(&file.path)).unwrap_or_default();
                match manifest_passes(&file.path, &bytes, &detect.manifest) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            }
            roots.insert(parent_dir(&file.path));
        }
        Outcome::settle(roots, &errors)
    }

    // -------------------------------------------------------- health

    fn health(&self, detect: &Detect, earlier: &[Detected]) -> Outcome {
        let Some(threshold) = &detect.threshold else {
            return Outcome::Unknown("a health detector needs a `threshold`".into());
        };
        match (&detect.count, detect.window) {
            (Some(entries), None) => self.count_at_head(entries, threshold, earlier),
            (Some(entries), Some(days)) => self.count_in_window(entries, days, threshold, earlier),
            (None, Some(days)) if detect.files.is_some() => {
                self.commits_per_file(detect, days, threshold)
            }
            _ => {
                Outcome::Unknown("a health detector needs `count`, or `files` with `window`".into())
            }
        }
    }

    /// The entries whose stack is true, plus the reasons any entry could not
    /// be evaluated.
    fn live_entries<'e>(
        entries: &'e [CountEntry],
        earlier: &'e [Detected],
    ) -> (Vec<LiveEntry<'e>>, Vec<String>) {
        let mut live = Vec::new();
        let mut errors = Vec::new();
        for entry in entries {
            if entry.stack == "*" {
                live.push((entry, None));
                continue;
            }
            match earlier
                .iter()
                .find(|d| d.name == entry.stack && d.kind == SignalKind::Stack)
                .map(|d| &d.outcome)
            {
                Some(Outcome::True { roots, .. }) => live.push((entry, Some(roots.as_slice()))),
                Some(Outcome::False) => {}
                Some(Outcome::Unknown(reason)) => {
                    errors.push(format!("stack `{}` is unknown: {reason}", entry.stack));
                }
                None => errors.push(format!("`count.{}` names no stack signal", entry.stack)),
            }
        }
        (live, errors)
    }

    fn count_at_head(
        &self,
        entries: &[CountEntry],
        threshold: &Threshold,
        earlier: &[Detected],
    ) -> Outcome {
        let tracked = match self.tracked() {
            Ok(t) => t,
            Err(e) => return Outcome::Unknown(e),
        };
        let (live, mut errors) = Self::live_entries(entries, earlier);
        let mut roots = BTreeSet::new();
        for (entry, stack_roots) in live {
            let mut measure = 0i64;
            let mut lines = 0i64;
            let mut contributed = BTreeSet::new();
            for path in tracked.iter().filter(|p| entry.files.is_match(p.as_str())) {
                let bytes = match fs::read(self.root.join(path)) {
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        errors.push(format!("{path}: {e}"));
                        continue;
                    }
                };
                lines += line_count(&bytes);
                let found: i64 = bytes
                    .split(|&b| b == b'\n')
                    .map(|line| entry.occurrences(line))
                    .sum();
                if found > 0 {
                    contributed.insert(parent_dir(path));
                }
                measure += found;
            }
            if threshold.reached(measure, lines) {
                match stack_roots {
                    Some(r) => roots.extend(r.iter().cloned()),
                    None => roots.extend(contributed),
                }
            }
        }
        Outcome::settle(roots, &errors)
    }

    fn count_in_window(
        &self,
        entries: &[CountEntry],
        days: i64,
        threshold: &Threshold,
        earlier: &[Detected],
    ) -> Outcome {
        let (live, errors) = Self::live_entries(entries, earlier);
        if live.is_empty() {
            return Outcome::settle(BTreeSet::new(), &errors);
        }
        let patch = match git_output(
            git(self.root)
                .args([
                    "log",
                    "--no-merges",
                    "--format=",
                    "-p",
                    "-U0",
                    "--no-renames",
                ])
                .args(["--no-ext-diff", "--no-textconv", "--no-color"])
                .args(window_filter(days)),
            "git log -p",
        ) {
            Ok(p) => p,
            Err(e) => return Outcome::Unknown(e),
        };
        // Per live entry: net occurrences, lines added, contributing dirs.
        let mut tallies: Vec<(i64, i64, BTreeSet<String>)> =
            vec![(0, 0, BTreeSet::new()); live.len()];
        let mut selected: HashMap<String, Vec<usize>> = HashMap::new();
        walk_patch(&patch, |file, line| {
            let picks = selected.entry(file.to_string()).or_insert_with(|| {
                live.iter()
                    .enumerate()
                    .filter(|(_, (e, _))| e.files.is_match(file))
                    .map(|(i, _)| i)
                    .collect()
            });
            for &i in picks.iter() {
                let (entry, _) = live[i];
                let tally = &mut tallies[i];
                match line {
                    PatchLine::Added(text) => {
                        let found = entry.occurrences(text);
                        tally.0 += found;
                        tally.1 += 1;
                        if found > 0 {
                            tally.2.insert(parent_dir(file));
                        }
                    }
                    PatchLine::Removed(text) => tally.0 -= entry.occurrences(text),
                }
            }
        });
        let mut roots = BTreeSet::new();
        for ((_, stack_roots), (measure, added, contributed)) in live.iter().zip(tallies) {
            if threshold.reached(measure, added) {
                match stack_roots {
                    Some(r) => roots.extend(r.iter().cloned()),
                    None => roots.extend(contributed),
                }
            }
        }
        Outcome::settle(roots, &errors)
    }

    /// Commits in the window touching each tracked file the `files` globs
    /// select; `contains` filters the commits by subject line. The files that
    /// reach the threshold are the evidence, most commits first.
    fn commits_per_file(&self, detect: &Detect, days: i64, threshold: &Threshold) -> Outcome {
        let Threshold::AtLeast(min) = *threshold else {
            return Outcome::Unknown(
                "a commits-per-file detector takes an integer threshold".into(),
            );
        };
        let tracked: BTreeSet<&str> = match self.tracked() {
            Ok(t) => t.iter().map(String::as_str).collect(),
            Err(e) => return Outcome::Unknown(e),
        };
        let log = match git_output(
            git(self.root)
                .args([
                    "log",
                    "--no-merges",
                    "--name-only",
                    "--no-renames",
                    "--format=%x1e%s",
                ])
                .args(window_filter(days)),
            "git log --name-only",
        ) {
            Ok(l) => l,
            Err(e) => return Outcome::Unknown(e),
        };
        let mut commits: BTreeMap<String, i64> = BTreeMap::new();
        for record in log.split(|&b| b == 0x1e).filter(|r| !r.is_empty()) {
            let mut lines = record
                .split(|&b| b == b'\n')
                .map(|l| l.strip_suffix(b"\r").unwrap_or(l));
            let subject = lines.next().unwrap_or_default();
            if !detect.contains_passes(subject) {
                continue;
            }
            let files: BTreeSet<String> = lines
                .filter(|l| !l.is_empty())
                .map(|l| String::from_utf8_lossy(l).into_owned())
                .filter(|p| tracked.contains(p.as_str()) && detect.selects(p))
                .collect();
            for path in files {
                *commits.entry(path).or_default() += 1;
            }
        }
        let mut hot: Vec<(String, i64)> = commits.into_iter().filter(|(_, n)| *n >= min).collect();
        if hot.is_empty() {
            return Outcome::False;
        }
        hot.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Outcome::True {
            roots: hot
                .iter()
                .map(|(p, _)| parent_dir(p))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            evidence: hot.into_iter().map(|(p, _)| p).collect(),
        }
    }
}

/// Whether any `manifest` key path exists in the file, read as TOML or JSON
/// by extension. A file of any other type never passes; one that does not
/// parse is an error.
fn manifest_passes(path: &str, bytes: &[u8], keys: &[Vec<String>]) -> Result<bool, String> {
    if keys.is_empty() {
        return Ok(true);
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".toml") {
        let doc: Table =
            toml::from_str(text).map_err(|e| format!("{path} does not parse as TOML: {e}"))?;
        Ok(keys.iter().any(|key| {
            let mut node = doc.get(&key[0]);
            for seg in &key[1..] {
                node = node.and_then(|n| n.as_table()).and_then(|t| t.get(seg));
            }
            node.is_some()
        }))
    } else if lower.ends_with(".json") {
        let doc: JsonValue = serde_json::from_str(text)
            .map_err(|e| format!("{path} does not parse as JSON: {e}"))?;
        Ok(keys.iter().any(|key| {
            key.iter()
                .try_fold(&doc, |node, seg| node.get(seg.as_str()))
                .is_some()
        }))
    } else {
        Ok(false)
    }
}
