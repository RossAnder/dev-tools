//! Write bookkeeping: the requests waiting for the runtime, the ones awaiting an outcome,
//! and the undo stack.
//!
//! A submitted request marks its rows saving on the surface it targets. The rows stay saving
//! until a ledger read shows them changed, or until an error or stale-skip outcome clears
//! them. Each method that has something to tell the user returns it as a notice for `App`.

use std::collections::{BTreeSet, HashMap};

use tomlctl::LedgerRef;

use crate::actions::{self, InputForm, Plan, Selection, UndoEntry, UndoKind};
use crate::surface::{InboxState, ItemsState, Surface};
use crate::writer::{RequestId, WriteOutcome, WriteRequest};

/// The saving sets of the item surfaces and the Inbox, borrowed from `App` for one call.
pub(crate) struct Saving<'a> {
    pub(crate) items: &'a mut HashMap<Surface, ItemsState>,
    pub(crate) inbox: &'a mut InboxState,
}

impl Saving<'_> {
    fn get(&mut self, surface: Surface) -> Option<&mut BTreeSet<String>> {
        if surface == Surface::Inbox {
            return Some(&mut self.inbox.saving);
        }
        self.items.get_mut(&surface).map(|state| &mut state.saving)
    }
}

/// A submitted request awaiting its outcome: the rows it marked saving, and whether it is
/// an undo, which pushes no undo entry of its own.
#[derive(Debug, Clone)]
struct InFlight {
    surface: Surface,
    ids: Vec<String>,
    undo: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Writes {
    /// One entry per submitted control edit or created input record, newest last.
    pub(crate) undo: Vec<UndoEntry>,
    /// Requests made and not yet taken by the runtime, in submission order.
    queue: Vec<WriteRequest>,
    in_flight: HashMap<RequestId, InFlight>,
    pub(crate) next_request: RequestId,
}

impl Writes {
    /// The write requests made since the last call, for the runtime to hand to the writer.
    pub(crate) fn take(&mut self) -> Vec<WriteRequest> {
        std::mem::take(&mut self.queue)
    }

    /// Takes a writer outcome. An error clears every row the request marked saving and a
    /// stale skip clears that row, each with a notice; applied rows stay saving until a
    /// ledger read shows them changed.
    pub(crate) fn apply_written(
        &mut self,
        mut saving: Saving<'_>,
        outcome: WriteOutcome,
    ) -> Option<String> {
        let flight = self.in_flight.remove(&outcome.request)?;
        let stale: Vec<&str> = outcome
            .skipped_stale
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        if let Some(saving) = saving.get(flight.surface) {
            if outcome.error.is_some() {
                flight.ids.iter().for_each(|id| {
                    saving.remove(id);
                });
            } else {
                stale.iter().for_each(|id| {
                    saving.remove(*id);
                });
            }
        }
        if !flight.undo
            && let Some(at) = self
                .undo
                .iter()
                .position(|entry| entry.outstanding.contains(&outcome.request))
        {
            let entry = &mut self.undo[at];
            entry.outstanding.remove(&outcome.request);
            entry.applied.extend(outcome.applied.iter().cloned());
            if entry.outstanding.is_empty() && entry.applied.is_empty() {
                self.undo.remove(at);
            }
        }
        let what = if flight.undo { "undo" } else { "write" };
        if let Some(error) = &outcome.error {
            Some(format!("{what} failed: {error}"))
        } else if !stale.is_empty() {
            let ids = stale.join(", ");
            Some(format!("{what} skipped {ids}: changed since shown"))
        } else {
            None
        }
    }

    /// Queues `plan`'s requests, marks their rows saving and pushes its undo entry.
    pub(crate) fn dispatch(&mut self, mut saving: Saving<'_>, selection: &Selection, plan: Plan) {
        let mut outstanding = BTreeSet::new();
        for (request, ids) in plan.requests {
            outstanding.insert(request.request());
            self.track(&mut saving, selection.surface, request, ids, false);
        }
        self.undo.push(UndoEntry {
            surface: selection.surface,
            kind: UndoKind::Rows {
                ledger: selection.ledger.clone(),
                rows: plan.undo,
            },
            outstanding,
            applied: Default::default(),
        });
    }

    /// Queues an input store write from `surface`. A record it creates is undone by
    /// withdrawing it; a withdrawal has no undo. The Inbox rows it changes are marked saving.
    pub(crate) fn dispatch_input(
        &mut self,
        mut saving: Saving<'_>,
        surface: Surface,
        input: &InputForm,
        request: WriteRequest,
    ) {
        let (surface, ids) = match input {
            InputForm::Answer(row) | InputForm::Withdraw(row) => {
                (Surface::Inbox, vec![row.id.clone()])
            }
            InputForm::Capture | InputForm::Request(_) => (surface, Vec::new()),
        };
        if !matches!(input, InputForm::Withdraw(_)) {
            self.undo.push(UndoEntry {
                surface: Surface::Inbox,
                kind: UndoKind::Input,
                outstanding: [request.request()].into(),
                applied: Default::default(),
            });
        }
        self.track(&mut saving, surface, request, ids, false);
    }

    fn track(
        &mut self,
        saving: &mut Saving<'_>,
        surface: Surface,
        request: WriteRequest,
        ids: Vec<String>,
        undo: bool,
    ) {
        if let Some(saving) = saving.get(surface) {
            saving.extend(ids.iter().cloned());
        }
        self.in_flight
            .insert(request.request(), InFlight { surface, ids, undo });
        self.queue.push(request);
    }

    /// Pops the newest undo entry into one restore of the rows its write applied, or one
    /// withdrawal of the records it created, and returns the notice. An entry still awaiting
    /// an outcome stays, since what to put back is not yet known.
    ///
    /// `shown` names the ledger a surface shows now. A restore always goes to the ledger the
    /// write went to, but its rows are marked saving only while that ledger is still shown:
    /// a same-id row of another ledger would never see the restore and stay saving for good.
    pub(crate) fn undo_last(
        &mut self,
        mut saving: Saving<'_>,
        shown: impl Fn(Surface) -> Option<LedgerRef>,
    ) -> String {
        let Some(top) = self.undo.last() else {
            return "nothing to undo".to_owned();
        };
        if !top.outstanding.is_empty() {
            return "the last write is still saving".to_owned();
        }
        let Some(entry) = self.undo.pop() else {
            return "nothing to undo".to_owned();
        };
        let mut elsewhere = false;
        let ids: Vec<String> = match &entry.kind {
            UndoKind::Rows { ledger, rows } => {
                let rows: Vec<_> = rows
                    .iter()
                    .filter(|row| entry.applied.contains(&row.id))
                    .collect();
                elsewhere = shown(entry.surface).as_ref() != Some(ledger);
                let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
                if !rows.is_empty() {
                    let request = actions::restore(rows, ledger, &mut self.next_request);
                    let marked = if elsewhere { Vec::new() } else { ids.clone() };
                    self.track(&mut saving, entry.surface, request, marked, true);
                }
                ids
            }
            UndoKind::Input => {
                let ids: Vec<String> = entry.applied.iter().cloned().collect();
                let request = actions::withdraw(ids.clone(), &mut self.next_request);
                self.track(&mut saving, entry.surface, request, ids.clone(), true);
                ids
            }
        };
        let ids = ids.join(", ");
        if elsewhere {
            format!("undoing {ids} in another ledger")
        } else {
            format!("undoing {ids}")
        }
    }
}
