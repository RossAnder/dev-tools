//! The serial writer thread: runs ledger and input-store writes off the UI thread and reports each outcome as an event.
//!
//! A facade call can wait up to 30 s for tomlctl's lock, so it never runs on the UI thread or
//! the poller. One thread takes requests in submission order and finishes each before the
//! next, so two writes to the same rows land in the order the user made them. A facade error,
//! a `root mismatch` included, comes back as [`WriteOutcome::error`]; it never ends the thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value};
use tomlctl::{BacklogTriage, LedgerRef, RestoreRow};

use crate::source::{Event, with_reinstall_hint};

/// Pairs a [`WriteOutcome`] with the request that produced it.
pub(crate) type RequestId = u64;

/// One control edit, carrying the compare-and-set values glimpse displayed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WriteRequest {
    Transition {
        request: RequestId,
        ledger: LedgerRef,
        ids: Vec<String>,
        to: String,
        fields: Map<String, Value>,
        expect_status: String,
    },
    Classify {
        request: RequestId,
        ledger: LedgerRef,
        ids: Vec<String>,
        fields: Map<String, Value>,
        expect: Map<String, Value>,
    },
    /// Puts back every row of one undo entry in a single guarded write.
    Restore {
        request: RequestId,
        ledger: LedgerRef,
        rows: Vec<RestoreRow>,
    },
    BacklogTriage {
        request: RequestId,
        ids: Vec<String>,
        triage: BacklogTriage,
        expect_status: String,
    },
    /// Appends `record` to the input store as a `new` record.
    InputAdd { request: RequestId, record: Value },
    InputAnswer {
        request: RequestId,
        question: String,
        picked: Vec<String>,
        text: Option<String>,
    },
    InputWithdraw {
        request: RequestId,
        ids: Vec<String>,
    },
}

impl WriteRequest {
    pub(crate) fn request(&self) -> RequestId {
        match self {
            WriteRequest::Transition { request, .. }
            | WriteRequest::Classify { request, .. }
            | WriteRequest::Restore { request, .. }
            | WriteRequest::BacklogTriage { request, .. }
            | WriteRequest::InputAdd { request, .. }
            | WriteRequest::InputAnswer { request, .. }
            | WriteRequest::InputWithdraw { request, .. } => *request,
        }
    }
}

/// A row the facade left alone because a guarded field no longer held what glimpse showed.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Stale {
    pub(crate) id: String,
    pub(crate) field: String,
    pub(crate) expected: Value,
    pub(crate) found: Value,
}

/// What became of one request. With `error` set, nothing was written.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct WriteOutcome {
    pub(crate) request: RequestId,
    pub(crate) applied: Vec<String>,
    pub(crate) skipped_stale: Vec<Stale>,
    pub(crate) error: Option<String>,
}

impl WriteOutcome {
    fn from_result(request: RequestId, result: Result<Value, String>) -> WriteOutcome {
        match result {
            Ok(value) => WriteOutcome {
                request,
                applied: value["applied"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| id.as_str().map(str::to_string))
                    .collect(),
                skipped_stale: value["skipped_stale"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|stale| Stale::deserialize(stale).ok())
                    .collect(),
                error: None,
            },
            Err(error) => WriteOutcome {
                request,
                error: Some(error),
                ..WriteOutcome::default()
            },
        }
    }
}

/// The handle to the writer thread. [`Writer::finish`] closes the queue and waits, up to a
/// bound, for the thread to perform every queued write, so quitting never strands a write
/// between its sidecar and TOML renames yet never waits out a ledger lock.
pub(crate) struct Writer {
    requests: Sender<WriteRequest>,
    /// Signalled, or disconnected, once the thread has drained the closed queue.
    done: Receiver<()>,
}

impl Writer {
    /// Starts the thread writing under `root`. Moves the process into `root` first, so the
    /// process root tomlctl resolves lazily agrees with it and the facade's root check passes;
    /// spawn it before anything else calls into tomlctl. Also returns, for the header, why
    /// every write would be refused, when that is already known.
    pub(crate) fn spawn(root: PathBuf, events: Sender<Event>) -> (Writer, Option<String>) {
        let warning = match std::env::set_current_dir(&root) {
            Err(e) => Some(format!(
                "writes will fail: cannot enter {}: {e}",
                root.display()
            )),
            Ok(()) => root_conflict(&root, std::env::var_os("TOMLCTL_ROOT")),
        };
        (Writer::spawn_with(root, events, perform), warning)
    }

    fn spawn_with<F>(root: PathBuf, events: Sender<Event>, mut perform: F) -> Writer
    where
        F: FnMut(&Path, WriteRequest) -> Result<Value, String> + Send + 'static,
    {
        let (requests, queue) = mpsc::channel::<WriteRequest>();
        let (finished, done) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            for request in queue {
                let id = request.request();
                let outcome = WriteOutcome::from_result(id, perform(&root, request));
                // A closed event channel loses only the report; the queued writes still run.
                let _ = events.send(Event::Written(outcome));
            }
            let _ = finished.send(());
        });
        Writer { requests, done }
    }

    pub(crate) fn submit(&self, request: WriteRequest) {
        let _ = self.requests.send(request);
    }

    /// Closes the queue and waits up to `within` for every queued write to finish. False
    /// means a write was still running at the bound; past it a facade call is waiting for
    /// the ledger lock, holding nothing and having written nothing.
    pub(crate) fn finish(self, within: Duration) -> bool {
        drop(self.requests);
        !matches!(
            self.done.recv_timeout(within),
            Err(RecvTimeoutError::Timeout)
        )
    }
}

/// Why tomlctl's process root cannot be `root`: once the process sits in `root`, only a
/// non-empty `TOMLCTL_ROOT` (`pinned`) naming another directory can move it elsewhere.
fn root_conflict(root: &Path, pinned: Option<OsString>) -> Option<String> {
    let pinned = PathBuf::from(pinned.filter(|value| !value.is_empty())?);
    let same = match (pinned.canonicalize(), root.canonicalize()) {
        (Ok(pinned), Ok(root)) => pinned == root,
        _ => false,
    };
    (!same).then(|| {
        format!(
            "writes will fail: TOMLCTL_ROOT is {}, not glimpse's root {}",
            pinned.display(),
            root.display()
        )
    })
}

/// Runs one request through the tomlctl facade.
fn perform(root: &Path, request: WriteRequest) -> Result<Value, String> {
    let result = match request {
        WriteRequest::Transition {
            ledger,
            ids,
            to,
            fields,
            expect_status,
            ..
        } => tomlctl::ledger_transition(root, &ledger, &ids, &to, fields, &expect_status),
        WriteRequest::Classify {
            ledger,
            ids,
            fields,
            expect,
            ..
        } => tomlctl::ledger_classify(root, &ledger, &ids, fields, expect),
        WriteRequest::Restore { ledger, rows, .. } => {
            tomlctl::ledger_restore_many(root, &ledger, rows)
        }
        WriteRequest::BacklogTriage {
            ids,
            triage,
            expect_status,
            ..
        } => tomlctl::backlog_triage(root, &ids, triage, &expect_status),
        WriteRequest::InputAdd { record, .. } => tomlctl::inputs_add(root, &record).map(created),
        WriteRequest::InputAnswer {
            question,
            picked,
            text,
            ..
        } => tomlctl::inputs_answer(root, &question, &picked, text.as_deref()).map(created),
        WriteRequest::InputWithdraw { ids, .. } => tomlctl::inputs_withdraw(root, &ids),
    };
    result.map_err(|e| with_reinstall_hint(format!("{e:#}")))
}

/// Reports the record an input write created as its one applied id, the id undo withdraws.
fn created(value: Value) -> Value {
    serde_json::json!({"applied": [value["id"].clone()]})
}

/// Records what the loop submitted, for tests that drive the runtime without a thread.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeWriter {
    pub(crate) submitted: Vec<WriteRequest>,
}

#[cfg(test)]
impl FakeWriter {
    pub(crate) fn submit(&mut self, request: WriteRequest) {
        self.submitted.push(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tomlctl::LedgerKind;

    use crate::actions::{self, Selection};
    use crate::ledger::{ItemRow, Kind, Ledger};
    use crate::surface::Surface;

    fn transition(request: RequestId) -> WriteRequest {
        WriteRequest::Transition {
            request,
            ledger: LedgerRef::Flow {
                slug: "demo".to_string(),
                kind: LedgerKind::Review,
            },
            ids: vec![format!("R{request}")],
            to: "wontfix".to_string(),
            fields: Map::new(),
            expect_status: "open".to_string(),
        }
    }

    fn outcome(rx: &Receiver<Event>) -> WriteOutcome {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Written(outcome)) => outcome,
            other => panic!("expected a write outcome, got {other:?}"),
        }
    }

    #[test]
    fn writes_run_in_submission_order() {
        let (tx, rx) = mpsc::channel();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&ran);
        let writer = Writer::spawn_with(PathBuf::new(), tx, move |_, request| {
            let id = request.request();
            // Earlier requests take longer, so any overlap would finish them last.
            std::thread::sleep(Duration::from_millis(60 - 20 * id));
            log.lock().expect("log").push(id);
            Ok(serde_json::json!({"applied": [format!("R{id}")], "skipped_stale": []}))
        });
        for id in 0..3 {
            writer.submit(transition(id));
        }
        let order: Vec<RequestId> = (0..3).map(|_| outcome(&rx).request).collect();
        assert_eq!(order, vec![0, 1, 2]);
        assert_eq!(*ran.lock().expect("log"), vec![0, 1, 2]);
    }

    #[test]
    fn finishing_performs_every_queued_write_even_with_no_one_listening() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let ran = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&ran);
        let writer = Writer::spawn_with(PathBuf::new(), tx, move |_, request| {
            std::thread::sleep(Duration::from_millis(20));
            log.lock().expect("log").push(request.request());
            Ok(serde_json::json!({"applied": []}))
        });
        for id in 0..3 {
            writer.submit(transition(id));
        }
        assert!(writer.finish(Duration::from_secs(10)), "the queue drains");
        assert_eq!(*ran.lock().expect("log"), vec![0, 1, 2]);
    }

    #[test]
    fn finishing_gives_up_on_a_write_still_running_at_the_bound() {
        let (tx, _rx) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let writer = Writer::spawn_with(PathBuf::new(), tx, move |_, _| {
            let _ = held.recv();
            Ok(serde_json::json!({"applied": []}))
        });
        writer.submit(transition(0));
        let start = Instant::now();
        assert!(!writer.finish(Duration::from_millis(50)));
        assert!(start.elapsed() < Duration::from_secs(5), "bounded");
        drop(release);
    }

    #[test]
    fn finishing_an_idle_writer_is_prompt() {
        let (tx, _rx) = mpsc::channel();
        let writer = Writer::spawn_with(PathBuf::new(), tx, |_, _| Ok(Value::Null));
        assert!(writer.finish(Duration::from_secs(10)));
    }

    #[test]
    fn a_facade_error_is_reported_not_panicked() {
        let (tx, rx) = mpsc::channel();
        let writer = Writer::spawn_with(PathBuf::new(), tx, |_, request| {
            if request.request() == 0 {
                Err("root mismatch: elsewhere".to_string())
            } else {
                Ok(serde_json::json!({
                    "applied": [],
                    "skipped_stale": [
                        {"id": "R1", "field": "status", "expected": "open", "found": "fixed"}
                    ],
                }))
            }
        });
        writer.submit(transition(0));
        writer.submit(transition(1));
        let failed = outcome(&rx);
        assert_eq!(failed.error.as_deref(), Some("root mismatch: elsewhere"));
        assert!(failed.applied.is_empty());
        let next = outcome(&rx);
        assert_eq!(next.error, None, "the thread survives the error");
        assert_eq!(
            next.skipped_stale,
            vec![Stale {
                id: "R1".to_string(),
                field: "status".to_string(),
                expected: Value::from("open"),
                found: Value::from("fixed"),
            }]
        );
    }

    #[test]
    fn an_input_write_reports_the_record_it_created() {
        let answered = serde_json::json!({"id": "I5", "question": "I2"});
        let outcome = WriteOutcome::from_result(3, Ok(created(answered)));
        assert_eq!(outcome.applied, vec!["I5".to_string()]);
    }

    /// Serialises every test that reads or sets `TOMLCTL_ROOT`, which tomlctl reads live on
    /// each facade call.
    static ROOT_ENV: Mutex<()> = Mutex::new(());

    fn root_env() -> std::sync::MutexGuard<'static, ()> {
        ROOT_ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A throwaway repo root exported as `TOMLCTL_ROOT` until dropped, holding a review
    /// ledger for flow `demo` and a backlog seeded from the test fixtures.
    struct Sandbox {
        root: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Sandbox {
        fn new(name: &str) -> Sandbox {
            let lock = root_env();
            let root =
                std::env::temp_dir().join(format!("glimpse-writer-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let flow = root.join(".claude").join("flows").join("demo");
            std::fs::create_dir_all(&flow).expect("flow dir");
            let review = include_str!("../tests/fixtures/review-ledger.toml");
            std::fs::write(flow.join("review-ledger.toml"), review).expect("review ledger");
            let backlog = include_str!("../tests/fixtures/backlog.toml");
            std::fs::write(root.join(".claude").join("backlog.toml"), backlog).expect("backlog");
            // SAFETY: every test here that reads or sets TOMLCTL_ROOT holds `ROOT_ENV`, and
            // no other test in this binary touches it.
            unsafe { std::env::set_var("TOMLCTL_ROOT", &root) };
            Sandbox { root, _lock: lock }
        }

        fn rows(&self, ledger: &LedgerRef) -> Vec<ItemRow> {
            let value = tomlctl::ledger_read(&self.root, ledger).expect("ledger read");
            Ledger::from_value(value, ledger.clone())
                .expect("ledger loads")
                .rows
        }

        fn row(&self, ledger: &LedgerRef, id: &str) -> ItemRow {
            self.rows(ledger)
                .into_iter()
                .find(|row| row.id == id)
                .expect("the row is present")
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // SAFETY: `_lock` is still held; fields drop after this body.
            unsafe { std::env::remove_var("TOMLCTL_ROOT") };
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Moves `id` to `to` through the real plan builder and facade, returning the row as it
    /// was read and the plan's undo of it.
    fn moved(
        sandbox: &Sandbox,
        kind: Kind,
        ledger: &LedgerRef,
        id: &str,
        to: &str,
        next: &mut RequestId,
    ) -> (ItemRow, actions::RowUndo) {
        let before = sandbox.row(ledger, id);
        let transition = *actions::offered(kind, &before.status)
            .iter()
            .find(|t| t.to == to)
            .expect("the move is offered");
        let fields: Map<String, Value> = transition
            .fields
            .iter()
            .map(|&(field, _)| (field.to_owned(), Value::from(format!("{field} text"))))
            .collect();
        let selection = Selection {
            surface: Surface::of_kind(kind).expect("an item surface"),
            kind,
            ledger: ledger.clone(),
            rows: vec![before.clone()],
        };
        let plan = actions::transition_plan(&selection, transition, &fields, next);
        let [(request, _)] = plan.requests.as_slice() else {
            panic!("expected one request, got {:?}", plan.requests);
        };
        let outcome =
            WriteOutcome::from_result(request.request(), perform(&sandbox.root, request.clone()));
        assert_eq!(
            (outcome.applied, outcome.error),
            (vec![id.to_owned()], None)
        );
        assert_eq!(sandbox.row(ledger, id).status, to);
        let [undo] = plan.undo.as_slice() else {
            panic!("expected one undo, got {:?}", plan.undo);
        };
        (before, undo.clone())
    }

    /// Makes each move, then undoes them all through one restore, and checks every row came
    /// back exactly as it was read.
    fn round_trip(sandbox: &Sandbox, kind: Kind, ledger: LedgerRef, moves: &[(&str, &str)]) {
        let mut next = 0;
        let (before, undos): (Vec<ItemRow>, Vec<actions::RowUndo>) = moves
            .iter()
            .map(|(id, to)| moved(sandbox, kind, &ledger, id, to, &mut next))
            .unzip();
        let restore = actions::restore(&undos, &ledger, &mut next);
        let restored =
            WriteOutcome::from_result(restore.request(), perform(&sandbox.root, restore));
        let ids: Vec<String> = moves.iter().map(|(id, _)| (*id).to_owned()).collect();
        assert_eq!(
            (restored.applied, restored.skipped_stale, restored.error),
            (ids, Vec::new(), None)
        );
        for row in before {
            assert_eq!(
                sandbox.row(&ledger, &row.id).raw,
                row.raw,
                "undo puts {} back",
                row.id
            );
        }
    }

    #[test]
    fn a_review_deferral_and_its_undo_leave_the_row_as_it_was() {
        let sandbox = Sandbox::new("defer");
        let ledger = LedgerRef::Flow {
            slug: "demo".to_string(),
            kind: LedgerKind::Review,
        };
        round_trip(&sandbox, Kind::Review, ledger, &[("R1", "deferred")]);
    }

    #[test]
    fn one_restore_puts_back_rows_moved_from_different_statuses() {
        let sandbox = Sandbox::new("restore-many");
        let ledger = LedgerRef::Flow {
            slug: "demo".to_string(),
            kind: LedgerKind::Review,
        };
        round_trip(
            &sandbox,
            Kind::Review,
            ledger,
            &[("R1", "wontfix"), ("R2", "open")],
        );
    }

    #[test]
    fn a_stale_row_in_a_restore_is_skipped_and_the_rest_put_back() {
        let sandbox = Sandbox::new("restore-stale");
        let ledger = LedgerRef::Flow {
            slug: "demo".to_string(),
            kind: LedgerKind::Review,
        };
        let mut next = 0;
        let (r1, undo1) = moved(&sandbox, Kind::Review, &ledger, "R1", "deferred", &mut next);
        let (_, undo2) = moved(&sandbox, Kind::Review, &ledger, "R2", "open", &mut next);
        moved(&sandbox, Kind::Review, &ledger, "R2", "wontfix", &mut next);
        let restore = actions::restore([&undo1, &undo2], &ledger, &mut next);
        let restored =
            WriteOutcome::from_result(restore.request(), perform(&sandbox.root, restore));
        assert_eq!(restored.error, None);
        assert_eq!(restored.applied, vec!["R1".to_string()]);
        let stale: Vec<&str> = restored
            .skipped_stale
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(stale, vec!["R2"], "never retried");
        assert_eq!(sandbox.row(&ledger, "R1").raw, r1.raw);
        assert_eq!(sandbox.row(&ledger, "R2").status, "wontfix");
    }

    #[test]
    fn a_backlog_dismissal_and_its_undo_leave_the_row_as_it_was() {
        let sandbox = Sandbox::new("dismiss");
        round_trip(
            &sandbox,
            Kind::Backlog,
            LedgerRef::Backlog,
            &[("B-0a1b2c3d", "dismissed")],
        );
    }

    #[test]
    fn a_tomlctl_root_naming_another_directory_is_a_conflict() {
        let root = std::env::temp_dir();
        assert_eq!(root_conflict(&root, None), None);
        assert_eq!(
            root_conflict(&root, Some(OsString::new())),
            None,
            "empty is unset"
        );
        assert_eq!(root_conflict(&root, Some(root.clone().into())), None);
        let elsewhere = root.join("glimpse-no-such-root");
        let conflict = root_conflict(&root, Some(elsewhere.into())).expect("a conflict");
        assert!(conflict.contains("TOMLCTL_ROOT"), "{conflict}");
    }

    #[test]
    fn a_root_other_than_the_process_root_is_refused_as_an_outcome() {
        let _env = root_env();
        let elsewhere = std::env::temp_dir();
        let (tx, rx) = mpsc::channel();
        let writer = Writer::spawn_with(elsewhere, tx, perform);
        writer.submit(transition(7));
        let refused = outcome(&rx);
        assert_eq!(refused.request, 7);
        let error = refused.error.expect("the facade refuses");
        assert!(error.contains("root mismatch"), "{error}");
    }
}
