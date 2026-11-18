//! Turning a lens's slice rule into file slices, and merging or splitting them.

use super::*;

// ------------------------------------------------------------ slicing

/// The deepest directory holding every path; empty at the repository root.
pub(super) fn common_dir<S: AsRef<str>>(paths: &[S]) -> String {
    let mut common: Option<Vec<&str>> = None;
    for path in paths {
        let dirs: Vec<&str> = match path.as_ref().rsplit_once('/') {
            Some((dir, _)) => dir.split('/').collect(),
            None => Vec::new(),
        };
        common = Some(match common {
            None => dirs,
            Some(prev) => prev
                .iter()
                .zip(&dirs)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    common.unwrap_or_default().join("/")
}

/// `dir/**` when every tracked file under the group's deepest common
/// directory is in the group, else the files themselves.
pub(super) fn compact_tree(files: &[String], tracked: &[String]) -> Vec<String> {
    let lcd = common_dir(files);
    let group: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    if !lcd.is_empty()
        && tracked
            .iter()
            .filter(|t| within(t, &lcd))
            .all(|t| group.contains(t.as_str()))
    {
        vec![format!("{lcd}/**")]
    } else {
        files.to_vec()
    }
}

/// `dir/*` when every tracked file directly in `dir` is in the group, else
/// the files themselves.
fn compact_dir(dir: &str, files: &[String], tracked: &[String]) -> Vec<String> {
    let group: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    if dir != "."
        && tracked
            .iter()
            .filter(|t| parent_dir(t) == dir)
            .all(|t| group.contains(t.as_str()))
    {
        vec![format!("{dir}/*")]
    } else {
        files.to_vec()
    }
}

/// The subsystem a path belongs to: its deepest stack root, else its top
/// directory, else `.`.
fn subsystem(file: &str, signals: &Signals) -> String {
    signals
        .stack_roots
        .iter()
        .filter(|root| within(file, root))
        .max_by_key(|root| root.len())
        .cloned()
        .unwrap_or_else(|| match file.split_once('/') {
            Some((top, _)) => top.to_string(),
            None => ".".to_string(),
        })
}

/// A lens's slices by its `slice` rule; empty when the scope holds no file
/// the lens reads. On a kinded scope a judgement lens reads only its `kinds`
/// (code when it declares none) and its slices keep kinds apart; lockfiles
/// reach it only through `signal-evidence`.
pub(super) fn slices_for(lens: &Entry, scope: &ScopeView, signals: &Signals) -> Vec<Vec<String>> {
    if scope.files.is_empty() {
        return vec![scope.whole.clone()];
    }
    let rule = match lens.slice {
        Some(SliceRule::SignalEvidence | SliceRule::ChangedWithTests) if !scope.kinded => {
            SliceRule::PerSubsystem
        }
        Some(rule) => rule,
        None => return vec![scope.whole.clone()],
    };
    match rule {
        SliceRule::PerSubsystem | SliceRule::PerFile => by_kind(rule, lens, scope, signals),
        SliceRule::SignalEvidence => {
            let matched: Vec<&ActiveSignal> = signals
                .located
                .iter()
                .filter(|s| lens.apply_when.signals.contains(&s.name))
                .collect();
            let pointed: Vec<String> = scope
                .files
                .iter()
                .filter(|f| {
                    matched.iter().any(|s| {
                        s.evidence.iter().any(|e| e == *f)
                            || s.roots.iter().any(|root| within(f, root))
                    })
                })
                .cloned()
                .collect();
            if pointed.is_empty() {
                return by_kind(SliceRule::PerSubsystem, lens, scope, signals);
            }
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for file in pointed {
                groups
                    .entry(subsystem(&file, signals))
                    .or_default()
                    .push(file);
            }
            groups
                .into_values()
                .map(|files| compact_tree(&files, &scope.tracked))
                .collect()
        }
        SliceRule::ChangedWithTests => {
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for file in scope
                .files
                .iter()
                .filter(|f| scope.kind(f) == FileKind::Code)
            {
                groups
                    .entry(subsystem(file, signals))
                    .or_default()
                    .push(file.clone());
            }
            for test in scope
                .tracked
                .iter()
                .filter(|t| scope.kind(t) == FileKind::Test)
            {
                if let Some(files) = groups.get_mut(&subsystem(test, signals)) {
                    files.push(test.clone());
                }
            }
            groups
                .into_values()
                .map(|mut files| {
                    files.sort();
                    compact_tree(&files, &scope.tracked)
                })
                .collect()
        }
        _ => vec![scope.whole.clone()],
    }
}

/// `per-subsystem` and `per-file` slices, each churn hotspot in scope a slice
/// of its own. A judgement lens on a kinded scope groups by subsystem or
/// directory and kind and keeps only the kinds it reads.
fn by_kind(
    rule: SliceRule,
    lens: &Entry,
    scope: &ScopeView,
    signals: &Signals,
) -> Vec<Vec<String>> {
    let kinded = scope.kinded && lens.mode == Some(Mode::Judgement);
    let reads = |file: &str| {
        !kinded
            || match lens.kinds.as_slice() {
                [] => scope.kind(file) == FileKind::Code,
                kinds => kinds.contains(&scope.kind(file)),
            }
    };
    let files: Vec<&String> = scope.files.iter().filter(|f| reads(f)).collect();
    let hot: Vec<&String> = signals
        .hotspots
        .iter()
        .filter(|h| files.contains(h))
        .collect();
    let mut groups: BTreeMap<(String, Option<FileKind>), Vec<String>> = BTreeMap::new();
    for file in files.into_iter().filter(|f| !hot.contains(f)) {
        let key = match rule {
            SliceRule::PerSubsystem => subsystem(file, signals),
            _ => parent_dir(file).to_string(),
        };
        let kind = kinded.then(|| scope.kind(file));
        groups.entry((key, kind)).or_default().push(file.clone());
    }
    let mut slices: Vec<Vec<String>> = hot.into_iter().map(|h| vec![h.clone()]).collect();
    for ((key, _), files) in groups {
        slices.push(match rule {
            SliceRule::PerSubsystem => compact_tree(&files, &scope.tracked),
            _ => compact_dir(&key, &files, &scope.tracked),
        });
    }
    slices
}

// ------------------------------------------------------------ shaping

/// The most scope files one sweeper cell's slice holds while the budget allows.
pub(super) const SWEEPER_SLICE_CAP: usize = 40;

#[derive(Debug, Clone)]
pub(super) struct Slice {
    pub(super) entries: Vec<String>,
    /// The scope files the entries match, in path order.
    pub(super) files: Vec<String>,
    /// The one kind of every file on a kinded scope; `None` when the files
    /// mix kinds, or for a sweeper slice.
    pub(super) kind: Option<FileKind>,
}

impl Slice {
    pub(super) fn of(entries: Vec<String>, scope: &ScopeView) -> Self {
        let set = PathSet::new(&entries);
        let files: Vec<String> = scope
            .files
            .iter()
            .filter(|f| set.matches(f))
            .cloned()
            .collect();
        let mut kinds = files.iter().map(|f| scope.kind(f));
        let kind = match kinds.next() {
            Some(first) if scope.kinded && kinds.all(|k| k == first) => Some(first),
            _ => None,
        };
        Self {
            entries,
            files,
            kind,
        }
    }

    /// The parent of the slice's deepest common directory; two slices sharing
    /// it are adjacent.
    fn parent(&self) -> String {
        let home = if self.files.is_empty() {
            common_dir(&self.entries)
        } else {
            common_dir(&self.files)
        };
        if home.is_empty() {
            ".".to_string()
        } else {
            parent_dir(&home).to_string()
        }
    }

    fn joined(&self, other: &Slice) -> Slice {
        let mut entries = self.entries.clone();
        entries.extend(
            other
                .entries
                .iter()
                .filter(|e| !self.entries.contains(e))
                .cloned(),
        );
        let files: BTreeSet<&String> = self.files.iter().chain(&other.files).collect();
        Slice {
            entries,
            files: files.into_iter().cloned().collect(),
            kind: self.kind.filter(|k| other.kind == Some(*k)),
        }
    }
}

pub(super) fn entries_of(slices: &[Slice]) -> Vec<Vec<String>> {
    slices.iter().map(|s| s.entries.clone()).collect()
}

/// Splits every slice holding more than the sweeper cap into chunks of its
/// files in path order. A sweeper reads every kind, so its slices carry none.
pub(super) fn split_to_cap(slices: Vec<Slice>) -> Vec<Slice> {
    slices
        .into_iter()
        .flat_map(|slice| {
            if slice.files.len() <= SWEEPER_SLICE_CAP {
                return vec![Slice {
                    kind: None,
                    ..slice
                }];
            }
            slice
                .files
                .chunks(SWEEPER_SLICE_CAP)
                .map(|chunk| Slice {
                    entries: chunk.to_vec(),
                    files: chunk.to_vec(),
                    kind: None,
                })
                .collect()
        })
        .collect()
}

/// One merge rule: whether the pair must be adjacent, and the file cap the
/// merged slice must stay within.
pub(super) type MergeRule = (bool, Option<usize>);

/// Merges the pair the first rule admits that binds, the smallest merged
/// slice first and the earliest on a tie: a consecutive pair sharing a parent
/// under an adjacent rule, any pair otherwise, so slices of one kind still
/// merge past a slice of another. Two slices of different kinds never merge.
pub(super) fn merge_step(
    slices: &[Slice],
    rules: &[MergeRule],
    binds: &dyn Fn(&[String]) -> bool,
) -> Option<Vec<Slice>> {
    rules.iter().find_map(|&(adjacent, cap)| {
        let candidates: Vec<(usize, usize)> = if adjacent {
            (1..slices.len()).map(|j| (j - 1, j)).collect()
        } else {
            (0..slices.len())
                .flat_map(|i| (i + 1..slices.len()).map(move |j| (i, j)))
                .collect()
        };
        let mut pairs: Vec<(usize, usize, Slice)> = candidates
            .into_iter()
            .map(|(i, j)| (i, j, &slices[i], &slices[j]))
            .filter(|(_, _, a, b)| !adjacent || a.parent() == b.parent())
            .filter(|(_, _, a, b)| match (a.kind, b.kind) {
                (Some(x), Some(y)) => x == y,
                _ => true,
            })
            .map(|(i, j, a, b)| (i, j, a.joined(b)))
            .filter(|(_, _, merged)| cap.is_none_or(|cap| merged.files.len() <= cap))
            .collect();
        pairs.sort_by_key(|(i, j, merged)| (merged.files.len(), *i, *j));
        pairs
            .into_iter()
            .find(|(_, _, merged)| binds(&merged.entries))
            .map(|(i, j, merged)| {
                let mut next = slices.to_vec();
                next.remove(j);
                next[i] = merged;
                next
            })
    })
}

/// Merges until `done` holds or no rule admits another merge.
pub(super) fn shrink(
    slices: &mut Vec<Slice>,
    rules: &[MergeRule],
    binds: &dyn Fn(&[String]) -> bool,
    done: &dyn Fn(&[Slice]) -> bool,
) {
    while !done(slices) {
        match merge_step(slices, rules, binds) {
            Some(next) => *slices = next,
            None => return,
        }
    }
}
