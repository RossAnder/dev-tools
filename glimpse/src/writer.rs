//! The serial writer thread: runs ledger and input-store writes off the UI thread and reports each outcome as an event.
//!
//! A facade call can wait up to 30 s for tomlctl's lock, so it never runs on the UI thread or
//! the poller. One thread takes requests in submission order and finishes each before the
//! next, so two writes to the same rows land in the order the user made them. A facade error,
//! a `root mismatch` included, comes back as [`WriteOutcome::error`]; it never ends the thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use serde::Deserialize;
use serde_json::{Map, Value};
use tomlctl::{BacklogTriage, LedgerRef};

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
    Restore {
        request: RequestId,
        ledger: LedgerRef,
        id: String,
        set: Map<String, Value>,
        unset: Vec<String>,
        expect: Map<String, Value>,
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

/// The handle to the writer thread. Dropping it lets the thread finish what is queued and
/// end; it is never joined, so exit never waits out a lock.
pub(crate) struct Writer {
    requests: Sender<WriteRequest>,
}

impl Writer {
    /// Starts the thread writing under `root`. Moves the process into `root` first, so the
    /// process root tomlctl resolves lazily agrees with it and the facade's root check passes;
    /// spawn it before anything else calls into tomlctl.
    pub(crate) fn spawn(root: PathBuf, events: Sender<Event>) -> Writer {
        let _ = std::env::set_current_dir(&root);
        Writer::spawn_with(root, events, perform)
    }

    fn spawn_with<F>(root: PathBuf, events: Sender<Event>, mut perform: F) -> Writer
    where
        F: FnMut(&Path, WriteRequest) -> Result<Value, String> + Send + 'static,
    {
        let (requests, queue) = mpsc::channel::<WriteRequest>();
        std::thread::spawn(move || {
            for request in queue {
                let id = request.request();
                let outcome = WriteOutcome::from_result(id, perform(&root, request));
                if events.send(Event::Written(outcome)).is_err() {
                    return;
                }
            }
        });
        Writer { requests }
    }

    pub(crate) fn submit(&self, request: WriteRequest) {
        let _ = self.requests.send(request);
    }
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
        WriteRequest::Restore {
            ledger,
            id,
            set,
            unset,
            expect,
            ..
        } => tomlctl::ledger_restore(root, &ledger, &id, set, unset, expect),
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
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tomlctl::LedgerKind;

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

    #[test]
    fn a_root_other_than_the_process_root_is_refused_as_an_outcome() {
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
