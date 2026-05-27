//! `SessionRegistry` — keyed lookup of active [`Session`] instances.
//!
//! The supervisor (T8) owns one shared registry behind an `Arc`; WS handlers
//! consult it to resolve an incoming `session_id` to the right broadcast
//! fan-out, and the supervisor inserts on spawn / removes on terminal exit.
//! Backed by an [`RwLock`] over a `HashMap` so subscriber lookups (reads)
//! don't serialise against each other.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::pty::protocol::SessionId;
use crate::pty::session::Session;

/// Active-session lookup. `Default`-constructible so callers can build an
/// empty registry on startup; `new()` returns the conventional `Arc`-wrapped
/// form the supervisor and HTTP layer share.
#[derive(Default)]
pub struct SessionRegistry {
    inner: RwLock<HashMap<SessionId, Arc<Session>>>,
}

impl SessionRegistry {
    /// Build a fresh empty registry wrapped in `Arc` for sharing across tasks.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Insert (or overwrite) a session under its own [`Session::id`].
    pub async fn insert(&self, session: Arc<Session>) {
        let id = session.id;
        self.inner.write().await.insert(id, session);
    }

    /// Resolve a session by id. Returns a cloned `Arc` so the caller can
    /// drop the read lock immediately.
    pub async fn get(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.inner.read().await.get(id).cloned()
    }

    /// Remove and return a session by id, or `None` if not present.
    pub async fn remove(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.inner.write().await.remove(id)
    }

    /// Membership test without cloning the `Arc`.
    pub async fn contains(&self, id: &SessionId) -> bool {
        self.inner.read().await.contains_key(id)
    }

    /// Snapshot of every currently-registered session (cloned `Arc`s). The
    /// returned `Vec` is in arbitrary (HashMap) order.
    pub async fn list(&self) -> Vec<Arc<Session>> {
        self.inner.read().await.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::sync::{broadcast, mpsc};

    /// Build a throwaway `Session` with dummy channels for registry tests.
    /// The receivers are kept alive in-scope by the caller via the `_guards`
    /// pattern below — dropping them is fine since the tests never feed the
    /// channels, but keeping them silences any "channel closed" surprises if
    /// a future test does poke at the broadcast/input ends.
    fn make_session() -> Arc<Session> {
        let (bcast, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(4);
        Session::new(SessionId::new(), bcast, input_tx)
    }

    #[tokio::test]
    async fn insert_get_remove_round_trip() {
        let registry = SessionRegistry::new();
        let session = make_session();
        let id = session.id;

        registry.insert(session.clone()).await;

        let got = registry.get(&id).await;
        assert!(got.is_some(), "expected get() to return the inserted session");
        assert_eq!(got.unwrap().id, id);

        assert!(registry.contains(&id).await, "contains() should be true after insert");

        let removed = registry.remove(&id).await;
        assert!(removed.is_some(), "remove() should yield the previously-inserted session");
        assert_eq!(removed.unwrap().id, id);

        assert!(
            registry.get(&id).await.is_none(),
            "get() should return None after remove"
        );
        assert!(
            !registry.contains(&id).await,
            "contains() should be false after remove"
        );
    }

    #[tokio::test]
    async fn list_returns_all_sessions() {
        let registry = SessionRegistry::new();
        let s1 = make_session();
        let s2 = make_session();
        let s3 = make_session();

        registry.insert(s1.clone()).await;
        registry.insert(s2.clone()).await;
        registry.insert(s3.clone()).await;

        let all = registry.list().await;
        assert_eq!(all.len(), 3, "list() should return every inserted session");

        // Order is HashMap-arbitrary; check membership by id.
        let mut ids: Vec<SessionId> = all.iter().map(|s| s.id).collect();
        ids.sort();
        let mut expected = vec![s1.id, s2.id, s3.id];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let registry = SessionRegistry::new();
        let unknown = SessionId::new();
        assert!(registry.get(&unknown).await.is_none());
        assert!(!registry.contains(&unknown).await);
        assert!(registry.remove(&unknown).await.is_none());
    }
}
