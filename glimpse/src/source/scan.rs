//! Stats a repo's flow files and ledgers, and fetches snapshots, ledgers and flow lists through
//! the tomlctl library.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tomlctl::{LedgerKind, LedgerRef};

use super::Feed;
use crate::flows::{self, FlowEntry};
use crate::ledger::{Inputs, Ledger};
use crate::model::Snapshot;

/// `(mtime, len)` of one file; `None` when it is absent or cannot be statted.
pub(crate) type FileStat = Option<(SystemTime, u64)>;

/// A [`FileStat`] per file a snapshot reads, in [`tomlctl::SNAPSHOT_INPUTS`] order.
pub(crate) type Fingerprint = [FileStat; tomlctl::SNAPSHOT_INPUTS.len()];

pub(super) fn flow_dir(root: &Path, slug: &str) -> PathBuf {
    flows::flows_root(root).join(slug)
}

pub(super) fn stat(path: &Path) -> FileStat {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(crate) fn fingerprint(root: &Path, slug: &str) -> Fingerprint {
    let dir = flow_dir(root, slug);
    tomlctl::SNAPSHOT_INPUTS.map(|name| stat(&dir.join(name)))
}

/// Whether the fingerprinted flow has a task store, which `SNAPSHOT_INPUTS` names first.
pub(super) fn has_task_store(fingerprint: &Fingerprint) -> bool {
    fingerprint[0].is_some()
}

/// The file a feed reads, at the path tomlctl reads it from. `None` for a flow slug or scope
/// name tomlctl refuses: the feed then stats as unreadable, and its read reports the refusal.
pub(super) fn feed_path(root: &Path, feed: &Feed) -> Option<PathBuf> {
    match feed {
        Feed::Inputs => Some(tomlctl::inputs_path(root)),
        Feed::Ledger(ledger) => ledger.path(root).ok(),
    }
}

/// What a flow listing shows of one flow: its `tasks.toml` and `context.toml` mtimes, and
/// which ledgers it holds, in [`LedgerKind::ALL`] order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FlowTimes {
    pub(super) tasks: Option<SystemTime>,
    pub(super) context: Option<SystemTime>,
    pub(super) ledgers: [bool; 3],
}

impl FlowTimes {
    fn has_artifact(&self) -> bool {
        self.tasks.is_some() || self.ledgers.contains(&true)
    }

    /// Whether going from `self` to `next` changes the flow list or the ledger scopes, rather
    /// than only a task store's mtime.
    pub(super) fn relists(&self, next: &FlowTimes) -> bool {
        self.tasks.is_some() != next.tasks.is_some()
            || self.context != next.context
            || self.ledgers != next.ledgers
    }
}

/// What the last scan saw of one flow directory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FlowStat {
    dir: Option<SystemTime>,
    times: FlowTimes,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// [`modified`], but `None` unless `path` is a file, as tomlctl requires of a task store.
fn file_modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    meta.is_file().then(|| meta.modified().ok()).flatten()
}

/// The [`FlowTimes`] of every `<root>/.claude/flows/*` that has a `tasks.toml` or a ledger,
/// keyed by directory name. A flow cached with a task store has its `tasks.toml` and
/// `context.toml` re-statted on every scan: an in-place write never moves the directory's
/// mtime, and on Windows the mtime the enumeration reports can lag even a file created in the
/// directory. Ledger presence, and every flow without a task store, is re-statted only when
/// that mtime moved, which a file arriving by rename does at once, or when a wake evicted it.
pub(super) fn flows_fingerprint(
    root: &Path,
    cache: &mut BTreeMap<String, FlowStat>,
) -> BTreeMap<String, FlowTimes> {
    let Ok(entries) = std::fs::read_dir(flows::flows_root(root)) else {
        cache.clear();
        return BTreeMap::new();
    };
    let mut next = BTreeMap::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.metadata().and_then(|m| m.modified()).ok();
        let stat = match cache.get(&name) {
            Some(old) if old.times.tasks.is_none() && dir.is_some() && old.dir == dir => *old,
            old => {
                let path = entry.path();
                let tasks = file_modified(&path.join("tasks.toml"));
                let ledgers = match old {
                    Some(old) if dir.is_some() && old.dir == dir => old.times.ledgers,
                    _ => LedgerKind::ALL.map(|kind| path.join(kind.flow_file()).is_file()),
                };
                FlowStat {
                    dir,
                    times: FlowTimes {
                        tasks,
                        context: tasks.and_then(|_| modified(&path.join("context.toml"))),
                        ledgers,
                    },
                }
            }
        };
        next.insert(name, stat);
    }
    *cache = next;
    cache
        .iter()
        .filter(|(_, stat)| stat.times.has_artifact())
        .map(|(slug, stat)| (slug.clone(), stat.times))
        .collect()
}

/// The `*.toml` files in every flow-less ledger directory, a symlink counting when it leads to
/// a file. Names are read fresh on every scan: only the directory mtime lags on Windows, not
/// the entries.
pub(super) fn scope_files(root: &Path) -> BTreeSet<(LedgerKind, String)> {
    let claude = root.join(".claude");
    let mut files = BTreeSet::new();
    for dir in LedgerKind::ALL {
        let Ok(entries) = std::fs::read_dir(claude.join(dir.scope_dir())) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_file = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => entry.path().is_file(),
                Ok(kind) => kind.is_file(),
                Err(_) => false,
            };
            if is_file && name.ends_with(".toml") {
                files.insert((dir, name));
            }
        }
    }
    files
}

/// The `tomlctl::ledger_scopes` document rebuilt from a scan's results, with no second walk:
/// `flows` as [`flows_fingerprint`] returns them and `files` as [`scope_files`] does. Keeps
/// the same flows and scopes, in the same order, under the same slug and scope-name rules.
pub(super) fn scopes_document(
    flows: &BTreeMap<String, FlowTimes>,
    files: &BTreeSet<(LedgerKind, String)>,
) -> serde_json::Value {
    let flows: Vec<serde_json::Value> = flows
        .iter()
        .filter(|(slug, times)| times.has_artifact() && tomlctl::validate_slug(slug).is_ok())
        .map(|(slug, times)| {
            let ledgers: Vec<&str> = LedgerKind::ALL
                .iter()
                .zip(times.ledgers)
                .filter(|(_, held)| *held)
                .map(|(kind, _)| kind.as_str())
                .collect();
            serde_json::json!({
                "slug": slug,
                "has_tasks": times.tasks.is_some(),
                "ledgers": ledgers,
            })
        })
        .collect();
    let mut scopes = Vec::new();
    for kind in LedgerKind::ALL {
        for (_, name) in files.iter().filter(|(dir, _)| *dir == kind) {
            let path = Path::new(name);
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(scope) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if tomlctl::validate_slug(scope).is_ok() {
                scopes.push(serde_json::json!({"kind": kind.as_str(), "scope": scope}));
            }
        }
    }
    serde_json::json!({"flows": flows, "scopes": scopes})
}

/// What a flow scan asks the poller to send.
pub(super) enum FlowsChange {
    Relist,
    Mtimes(BTreeMap<String, SystemTime>),
}

/// The `tasks.toml` mtime of every flow among `flows` that has one.
pub(super) fn task_stores(flows: &BTreeMap<String, FlowTimes>) -> BTreeMap<String, SystemTime> {
    flows
        .iter()
        .filter_map(|(slug, times)| Some((slug.clone(), times.tasks?)))
        .collect()
}

/// The `tasks.toml` mtime of every flow under `root` that has one, from one fresh scan.
pub(crate) fn task_store_mtimes(root: &Path) -> BTreeMap<String, SystemTime> {
    task_stores(&flows_fingerprint(root, &mut BTreeMap::new()))
}

/// Produces a snapshot for one flow, reads ledgers and lists the repo's flows. The production
/// implementation reads in-process through the tomlctl library; tests substitute a fake.
pub(crate) trait Fetcher: Send {
    /// The flow's snapshot, or `Ok(None)` when its inputs still hash to `known`.
    fn fetch(
        &mut self,
        root: &Path,
        slug: &str,
        known: Option<&str>,
    ) -> Result<Option<Snapshot>, String>;
    /// The flows among `task_stores` (slug to `tasks.toml` mtime), freshest first.
    fn list_flows(
        &mut self,
        root: &Path,
        task_stores: &BTreeMap<String, SystemTime>,
    ) -> Result<Vec<FlowEntry>, String>;
    /// One ledger, or `Ok(None)` when it still hashes to `known`; a missing file reads as an
    /// empty ledger with no revision.
    fn fetch_ledger(
        &mut self,
        root: &Path,
        ledger: &LedgerRef,
        known: Option<&str>,
    ) -> Result<Option<Ledger>, String>;
    /// The input store, or `Ok(None)` when it still hashes to `known`; a missing file reads as
    /// no records with no revision.
    fn fetch_inputs(&mut self, root: &Path, known: Option<&str>) -> Result<Option<Inputs>, String>;
}

/// Builds the `tasks snapshot` document in-process from `<root>/.claude/flows/<slug>`.
pub(crate) struct InProcessFetcher;

impl Fetcher for InProcessFetcher {
    fn fetch(
        &mut self,
        root: &Path,
        slug: &str,
        known: Option<&str>,
    ) -> Result<Option<Snapshot>, String> {
        let Some(value) = tomlctl::snapshot_if_changed(root, slug, known)
            .map_err(|e| with_reinstall_hint(format!("{e:#}")))?
        else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|e| format!("bad `tasks snapshot` document: {e}"))
    }

    fn list_flows(
        &mut self,
        root: &Path,
        task_stores: &BTreeMap<String, SystemTime>,
    ) -> Result<Vec<FlowEntry>, String> {
        flows::list(root, task_stores)
    }

    fn fetch_ledger(
        &mut self,
        root: &Path,
        ledger: &LedgerRef,
        known: Option<&str>,
    ) -> Result<Option<Ledger>, String> {
        tomlctl::ledger_read_if_changed(root, ledger, known)
            .map_err(|e| with_reinstall_hint(format!("{e:#}")))?
            .map(Ledger::from_value)
            .transpose()
    }

    fn fetch_inputs(&mut self, root: &Path, known: Option<&str>) -> Result<Option<Inputs>, String> {
        tomlctl::inputs_read_if_changed(root, known)
            .map_err(|e| with_reinstall_hint(format!("{e:#}")))?
            .map(Inputs::from_value)
            .transpose()
    }
}

/// Adds the fix to a tomlctl "newer than the supported" schema error. tomlctl's own advice is
/// to upgrade tomlctl, but glimpse reads through the copy linked into it, so the binary to
/// rebuild is glimpse.
pub(crate) fn with_reinstall_hint(message: String) -> String {
    if message.contains("newer than the supported") {
        format!(
            "{message} (glimpse embeds its own tomlctl: reinstall it with `cargo install --path glimpse`)"
        )
    } else {
        message
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    pub(in crate::source) fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("glimpse-source-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude").join("flows")).expect("temp root");
        dir
    }

    /// Returns each queued result in turn, then repeats the last, as `None` when its revision
    /// is the known one; counts its calls. Lists no flows, counting those calls apart. Ledger
    /// reads step through `ledgers` and input reads through `inputs` the same way, an empty
    /// queue reading as a missing file.
    pub(in crate::source) struct FakeFetcher {
        results: Vec<Result<Snapshot, String>>,
        calls: Arc<AtomicUsize>,
        lists: Arc<AtomicUsize>,
        ledgers: Vec<Result<Ledger, String>>,
        ledger_reads: Arc<AtomicUsize>,
        inputs: Vec<Result<Inputs, String>>,
        input_reads: Arc<AtomicUsize>,
    }

    impl Fetcher for FakeFetcher {
        fn fetch(
            &mut self,
            _root: &Path,
            _slug: &str,
            known: Option<&str>,
        ) -> Result<Option<Snapshot>, String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let result = self.results[n.min(self.results.len() - 1)].clone()?;
            Ok((known != Some(result.revision.as_str())).then_some(result))
        }

        fn list_flows(
            &mut self,
            _root: &Path,
            _task_stores: &BTreeMap<String, SystemTime>,
        ) -> Result<Vec<FlowEntry>, String> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        fn fetch_ledger(
            &mut self,
            _root: &Path,
            _ledger: &LedgerRef,
            known: Option<&str>,
        ) -> Result<Option<Ledger>, String> {
            let n = self.ledger_reads.fetch_add(1, Ordering::SeqCst);
            let ledger = match self.ledgers.len() {
                0 => ledger_with_revision(None),
                len => self.ledgers[n.min(len - 1)].clone()?,
            };
            Ok((known.is_none() || known != ledger.revision.as_deref()).then_some(ledger))
        }

        fn fetch_inputs(
            &mut self,
            _root: &Path,
            known: Option<&str>,
        ) -> Result<Option<Inputs>, String> {
            let n = self.input_reads.fetch_add(1, Ordering::SeqCst);
            let inputs = match self.inputs.len() {
                0 => inputs_with_revision(None),
                len => self.inputs[n.min(len - 1)].clone()?,
            };
            Ok((known.is_none() || known != inputs.revision.as_deref()).then_some(inputs))
        }
    }

    pub(in crate::source) fn with_revision(revision: &str) -> Snapshot {
        Snapshot {
            revision: revision.to_string(),
            ..crate::model::fixture()
        }
    }

    pub(in crate::source) fn ledger_with_revision(revision: Option<&str>) -> Ledger {
        Ledger::from_value(serde_json::json!({
            "path": ".claude/backlog.toml",
            "kind": "backlog",
            "revision": revision,
            "items": [],
        }))
        .expect("ledger")
    }

    pub(in crate::source) fn inputs_with_revision(revision: Option<&str>) -> Inputs {
        Inputs::from_value(serde_json::json!({
            "path": ".claude/inputs.toml",
            "revision": revision,
            "inputs": [],
        }))
        .expect("inputs")
    }

    /// A fake whose ledger reads step through `ledgers`, and the count of those reads.
    pub(in crate::source) fn fake_ledgers(
        ledgers: Vec<Result<Ledger, String>>,
    ) -> (Box<dyn Fetcher>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let ledger_reads = Arc::new(AtomicUsize::new(0));
        let fetcher = FakeFetcher {
            ledgers,
            ledger_reads: Arc::clone(&ledger_reads),
            ..empty_fake(Arc::clone(&calls), vec![Ok(with_revision("r1"))])
        };
        (Box::new(fetcher), calls, ledger_reads)
    }

    /// A fake whose input store reads step through `inputs`, and the count of those reads.
    pub(in crate::source) fn fake_inputs(
        inputs: Vec<Result<Inputs, String>>,
    ) -> (Box<dyn Fetcher>, Arc<AtomicUsize>) {
        let input_reads = Arc::new(AtomicUsize::new(0));
        let fetcher = FakeFetcher {
            inputs,
            input_reads: Arc::clone(&input_reads),
            ..empty_fake(Arc::new(AtomicUsize::new(0)), vec![Ok(with_revision("r1"))])
        };
        (Box::new(fetcher), input_reads)
    }

    fn empty_fake(calls: Arc<AtomicUsize>, results: Vec<Result<Snapshot, String>>) -> FakeFetcher {
        FakeFetcher {
            results,
            calls,
            lists: Arc::new(AtomicUsize::new(0)),
            ledgers: Vec::new(),
            ledger_reads: Arc::new(AtomicUsize::new(0)),
            inputs: Vec::new(),
            input_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(in crate::source) fn fake(
        results: Vec<Result<Snapshot, String>>,
    ) -> (Box<dyn Fetcher>, Arc<AtomicUsize>) {
        let (fetcher, calls, _lists) = fake_counting_lists(results);
        (fetcher, calls)
    }

    pub(in crate::source) fn fake_counting_lists(
        results: Vec<Result<Snapshot, String>>,
    ) -> (Box<dyn Fetcher>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let lists = Arc::new(AtomicUsize::new(0));
        let fetcher = FakeFetcher {
            lists: Arc::clone(&lists),
            ..empty_fake(Arc::clone(&calls), results)
        };
        (Box::new(fetcher), calls, lists)
    }

    /// Writes `name` the way tomlctl does, through a temporary file renamed into place,
    /// stamped with `secs` past the epoch.
    pub(in crate::source) fn write_renamed(dir: &Path, name: &str, secs: u64) {
        let tmp = dir.join(format!("{name}.tmp"));
        let file = std::fs::File::create(&tmp).expect("create");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .expect("stamp");
        drop(file);
        std::fs::rename(&tmp, dir.join(name)).expect("rename");
    }

    /// Writes `name` in place, creating it if absent, stamped with `secs` past the epoch, and
    /// puts the directory's mtime back if the write moved it.
    pub(in crate::source) fn write_in_place(dir: &Path, name: &str, secs: u64) {
        let before = modified(dir).expect("dir mtime");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dir.join(name))
            .expect("open");
        std::io::Write::write_all(&mut file, b"status = \"done\"\n").expect("write");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .expect("stamp");
        drop(file);
        if modified(dir) != Some(before) {
            open_dir_for_stamp(dir)
                .set_modified(before)
                .expect("restore");
        }
    }

    #[cfg(windows)]
    fn open_dir_for_stamp(dir: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
            .expect("open dir")
    }

    #[cfg(not(windows))]
    fn open_dir_for_stamp(dir: &Path) -> std::fs::File {
        std::fs::File::open(dir).expect("open dir")
    }

    #[test]
    fn fingerprint_changes_when_a_flow_file_changes() {
        let root = temp_root("fingerprint");
        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");

        let absent = fingerprint(&root, "f");
        assert!(
            absent.iter().all(Option::is_none),
            "absent files read as None"
        );

        // Every file the snapshot reads moves its own slot.
        let mut last = absent;
        for (slot, name) in tomlctl::SNAPSHOT_INPUTS.iter().enumerate() {
            std::fs::write(dir.join(name), "x").expect("write");
            let next = fingerprint(&root, "f");
            assert!(next[slot].is_some(), "{name} is fingerprinted");
            assert_ne!(next, last, "{name} moves the fingerprint");
            last = next;
        }
        assert_eq!(fingerprint(&root, "f"), last, "stable while untouched");

        std::fs::write(dir.join("tasks.toml"), "a = 12345\n").expect("rewrite");
        assert_ne!(fingerprint(&root, "f"), last);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_ledger_only_flow_with_an_unmoved_directory_is_served_from_the_cache() {
        let root = temp_root("ledger-only-cache");
        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");

        let cached = FlowStat {
            dir: modified(&dir),
            times: FlowTimes {
                tasks: None,
                context: None,
                ledgers: [true, false, false],
            },
        };
        let mut cache = BTreeMap::from([("f".to_string(), cached)]);

        let flows = flows_fingerprint(&root, &mut cache);
        assert_eq!(
            flows.get("f").map(|t| t.ledgers),
            Some([true, false, false])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_schema_too_new_error_gains_the_reinstall_hint() {
        let too_new = "tasks store schema_version 9 is newer than the supported 1 — upgrade";
        let hinted = with_reinstall_hint(too_new.to_string());
        assert!(hinted.starts_with(too_new), "{hinted}");
        assert!(hinted.contains("cargo install --path glimpse"), "{hinted}");
        assert_eq!(with_reinstall_hint("boom".into()), "boom");
    }
}
