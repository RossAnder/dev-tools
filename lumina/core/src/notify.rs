//! Process-wide, best-effort change-notification broadcast.
//!
//! The single global [`NotifyBus`] (via [`bus`]) carries a [`ChangeNotification`]
//! for every committed domain write. Notifications are published **POST-COMMIT**
//! by the `DbTx` commit path (the `NotifyingTx` wrapper buffers them during the
//! transaction and flushes only after `commit()` succeeds — see `db/erased.rs`),
//! so a subscriber never observes a phantom signal for a rolled-back write.
//!
//! Delivery semantics are deliberately loose:
//! - channel capacity is 1024; a slow subscriber that lags is the SUBSCRIBER's
//!   problem (`RecvError::Lagged` on its receiver — re-snapshot and move on);
//! - [`NotifyBus::publish`] NEVER fails: with zero live receivers the send is a
//!   silent no-op (the normal idle case).
//!
//! Consumers (the server's `/api/stream` watcher) treat each notification as a
//! "something changed" hint and re-read state — never as a delta.

use std::sync::OnceLock;

use tokio::sync::broadcast;

/// Channel capacity of the process-wide bus. A receiver that falls more than
/// this many notifications behind observes `RecvError::Lagged` and must
/// re-snapshot.
const BUS_CAPACITY: usize = 1024;

/// One committed domain change: which aggregate changed and how.
///
/// Fields are public so out-of-crate tests and the `record_event` buffering
/// path can both construct it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeNotification {
    /// The aggregate family, e.g. `"work_item"`, `"sprint"`, `"worktree"`.
    pub aggregate_type: String,
    /// The id of the changed aggregate row.
    pub aggregate_id: String,
    /// The event vocabulary entry, e.g. `"created"`, `"status_changed"`.
    pub event_type: String,
}

impl ChangeNotification {
    /// Construct a notification from any string-ish parts.
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        event_type: impl Into<String>,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            event_type: event_type.into(),
        }
    }
}

/// A handle onto one shared broadcast channel of [`ChangeNotification`]s.
///
/// `Clone` is LOAD-BEARING: cloning a `NotifyBus` clones the inner
/// `broadcast::Sender`, and a cloned `Sender` shares the SAME underlying
/// channel — so a clone of the global [`bus`] stored on server `AppState`
/// receives exactly what the commit path publishes via `bus()`. Subscribing
/// from any clone is equivalent to subscribing from the global.
#[derive(Clone)]
pub struct NotifyBus {
    tx: broadcast::Sender<ChangeNotification>,
}

impl NotifyBus {
    /// Create a fresh, independent bus (capacity [`BUS_CAPACITY`]).
    ///
    /// The initial `Receiver` returned by `broadcast::channel` is dropped —
    /// subscribers come and go via [`NotifyBus::subscribe`].
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }

    /// Open a new receiver on the shared channel. The receiver sees only
    /// notifications published AFTER this call.
    pub fn subscribe(&self) -> broadcast::Receiver<ChangeNotification> {
        self.tx.subscribe()
    }

    /// Publish a notification, best-effort. NEVER fails: a send with zero
    /// live receivers returns `Err(SendError)` from the channel, which is the
    /// normal idle case and is deliberately swallowed here.
    pub fn publish(&self, n: ChangeNotification) {
        let _ = self.tx.send(n);
    }
}

impl Default for NotifyBus {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide bus. Lazily initialised on first access; every caller in
/// the process (the commit path's publisher, the server's stream watcher)
/// shares this one channel.
pub fn bus() -> &'static NotifyBus {
    static BUS: OnceLock<NotifyBus> = OnceLock::new();
    BUS.get_or_init(NotifyBus::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_publish_with_no_receivers_is_ok() {
        let bus = NotifyBus::new();
        // Zero receivers: the underlying send returns Err(SendError), which
        // publish swallows — this must be a silent no-op, not a panic.
        bus.publish(ChangeNotification::new("sprint", "s1", "created"));
    }

    #[tokio::test]
    async fn notify_subscribe_publish_recv_roundtrip() {
        let bus = NotifyBus::new();
        let mut rx = bus.subscribe();

        let sent = ChangeNotification::new("sprint", "s1", "created");
        bus.publish(sent.clone());

        let got = rx.recv().await.expect("receiver should yield the published notification");
        assert_eq!(got, sent);
    }

    #[tokio::test]
    async fn notify_clone_shares_the_same_channel() {
        // The load-bearing Clone semantic: a clone's subscriber receives what
        // the original publishes (AppState stores bus().clone()).
        let original = NotifyBus::new();
        let clone = original.clone();
        let mut rx = clone.subscribe();

        let sent = ChangeNotification::new("work_item", "w1", "status_changed");
        original.publish(sent.clone());

        assert_eq!(rx.try_recv().expect("clone's receiver should see it"), sent);
    }
}
