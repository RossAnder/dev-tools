//! The per-connection subscription state machine for `/api/stream`.
//!
//! [`ConnState`] is SOCKET-FREE by design: the WS handler (T5,
//! `http/stream.rs`) owns the socket and the notify-bus receiver, and drives
//! this state machine with plain method calls — which is what lets every
//! subscribe/note/drain behaviour unit-test here without a WebSocket.
//!
//! Lifecycle per connection:
//!   1. `subscribe` frame → [`ConnState::handle_subscribe`] → `init` snapshot.
//!   2. A committed-change notification → [`ConnState::note`] marks the
//!      interested subscriptions dirty (sync, cheap — no recompute yet).
//!   3. After the coalesce window, [`ConnState::drain`] recomputes every
//!      dirty subscription and yields `data` frames — DEDUPED-ON-EQUAL: an
//!      unchanged recompute emits no frame.
//!   4. Bus lag (`RecvError::Lagged`) → [`ConnState::mark_all_dirty`] so the
//!      next drain re-snapshots everything (snapshots, never deltas — a
//!      missed notification self-heals).

use std::collections::BTreeMap;
use std::sync::Arc;

use lumina_core::db::AnyPool;
use lumina_core::notify::ChangeNotification;

use super::{FrameOut, TopicRegistry, TopicResolver};

/// One live subscription: which resolver/param it binds, the last snapshot
/// pushed (for dedupe-on-equal), and whether a recompute is pending.
struct SubEntry {
    resolver: Arc<dyn TopicResolver>,
    param: String,
    /// The last snapshot sent to the client (`init` or `data`). `None` only
    /// in the degenerate window where a subscribe stored no snapshot — in
    /// practice `handle_subscribe` always stores the `init` payload.
    last: Option<serde_json::Value>,
    dirty: bool,
}

/// Per-connection subscription state. One instance per WS connection, owned
/// by the connection's driver task — no interior locking needed.
///
/// Keyed by the full topic string in a `BTreeMap` (deliberate over the
/// suggested `HashMap`): iteration — and therefore the order of `data`
/// frames out of [`ConnState::drain`] — is deterministic, which keeps the
/// T6 e2e assertions and any multi-topic debugging stable.
#[derive(Default)]
pub struct ConnState {
    subs: BTreeMap<String, SubEntry>,
}

impl ConnState {
    /// An empty connection state (no subscriptions).
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle an inbound `subscribe` frame. Returns exactly one frame:
    ///
    /// - unknown/colonless topic → `Error { topic: Some(..) }` (the client
    ///   contract requires a populated `topic` on topic-scoped errors);
    /// - resolve failure → `Error { topic: Some(..) }`, subscription NOT
    ///   stored;
    /// - success → `Init { topic, data }`, subscription stored with the
    ///   snapshot as `last`. Re-subscribing an existing topic re-resolves
    ///   and replaces (the resubscribe-all-on-reconnect path).
    pub async fn handle_subscribe(
        &mut self,
        registry: &TopicRegistry,
        pool: &AnyPool,
        topic: &str,
    ) -> FrameOut {
        let Some((resolver, param)) = registry.parse(topic) else {
            return FrameOut::Error {
                topic: Some(topic.to_string()),
                message: "unknown topic".to_string(),
            };
        };

        match resolver.resolve(pool, &param).await {
            Ok(data) => {
                self.subs.insert(
                    topic.to_string(),
                    SubEntry {
                        resolver,
                        param,
                        last: Some(data.clone()),
                        dirty: false,
                    },
                );
                FrameOut::Init {
                    topic: topic.to_string(),
                    data,
                }
            }
            Err(e) => FrameOut::Error {
                topic: Some(topic.to_string()),
                message: e.to_string(),
            },
        }
    }

    /// Handle an inbound `unsubscribe` frame. Idempotent: unsubscribing a
    /// topic that was never (or is no longer) subscribed is a no-op.
    pub fn handle_unsubscribe(&mut self, topic: &str) {
        self.subs.remove(topic);
    }

    /// Fold one committed-change notification into the dirty flags. Sync and
    /// cheap — `interested` may over-approximate; the recompute in
    /// [`ConnState::drain`] dedupes-on-equal, so a false positive costs only
    /// a cheap read.
    pub fn note(&mut self, change: &ChangeNotification) {
        for entry in self.subs.values_mut() {
            if !entry.dirty && entry.resolver.interested(&entry.param, change) {
                entry.dirty = true;
            }
        }
    }

    /// Mark EVERY subscription dirty. Called on bus lag
    /// (`broadcast::error::RecvError::Lagged`): notifications were dropped,
    /// so the only safe move is to re-snapshot everything.
    pub fn mark_all_dirty(&mut self) {
        for entry in self.subs.values_mut() {
            entry.dirty = true;
        }
    }

    /// Recompute every dirty subscription and return the frames to send.
    ///
    /// Per dirty subscription:
    /// - resolve succeeds and the snapshot CHANGED → one
    ///   `Data { topic, data }` frame, `last` updated;
    /// - resolve succeeds and the snapshot is UNCHANGED → no frame
    ///   (dedupe-on-equal);
    /// - resolve fails → one `Error { topic: Some(..), message }` frame
    ///   (chosen over silent skip: the client made `error.topic` required
    ///   precisely so per-topic failures are routable, and swallowing a
    ///   recompute failure would silently freeze the topic at a stale
    ///   snapshot); `last` is left untouched so a later successful recompute
    ///   still dedupes against the last value the client actually saw.
    ///
    /// The dirty flag clears regardless of outcome, so a persistent failure
    /// emits at most one error frame per change-burst rather than looping.
    pub async fn drain(&mut self, pool: &AnyPool) -> Vec<FrameOut> {
        let mut frames = Vec::new();
        for (topic, entry) in self.subs.iter_mut() {
            if !entry.dirty {
                continue;
            }
            entry.dirty = false;
            match entry.resolver.resolve(pool, &entry.param).await {
                Ok(data) => {
                    if entry.last.as_ref() != Some(&data) {
                        entry.last = Some(data.clone());
                        frames.push(FrameOut::Data {
                            topic: topic.clone(),
                            data,
                        });
                    }
                }
                Err(e) => {
                    frames.push(FrameOut::Error {
                        topic: Some(topic.clone()),
                        message: e.to_string(),
                    });
                }
            }
        }
        frames
    }

    /// The number of live subscriptions (test/diagnostic affordance).
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{FailingResolver, TestResolver};
    use super::*;

    use lumina_core::db::connect_in_memory;

    /// Registry with one `TestResolver` (prefix `"test"`); returns the
    /// resolver handle so tests can bump the underlying value.
    fn test_registry() -> (TopicRegistry, Arc<TestResolver>) {
        let resolver = Arc::new(TestResolver::new());
        let mut reg = TopicRegistry::new();
        reg.register(resolver.clone());
        (reg, resolver)
    }

    async fn test_pool() -> AnyPool {
        let pool = connect_in_memory()
            .await
            .expect("in-memory pool should connect");
        AnyPool::from(pool)
    }

    fn thing_changed() -> ChangeNotification {
        ChangeNotification::new("thing", "x", "created")
    }

    #[tokio::test]
    async fn subscribe_known_topic_returns_init_and_stores_sub() {
        let (reg, _resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();

        let frame = conn.handle_subscribe(&reg, &pool, "test:abc").await;
        assert_eq!(
            frame,
            FrameOut::Init {
                topic: "test:abc".into(),
                data: serde_json::json!({"param": "abc", "v": 0}),
            }
        );
        assert_eq!(conn.sub_count(), 1);
    }

    #[tokio::test]
    async fn subscribe_unknown_topic_returns_error_with_topic() {
        let (reg, _resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();

        for bad in ["nope:abc", "nocolon"] {
            let frame = conn.handle_subscribe(&reg, &pool, bad).await;
            assert_eq!(
                frame,
                FrameOut::Error {
                    topic: Some(bad.into()),
                    message: "unknown topic".into(),
                }
            );
        }
        assert_eq!(conn.sub_count(), 0, "failed subscribes store nothing");
    }

    #[tokio::test]
    async fn subscribe_resolve_failure_returns_error_and_stores_nothing() {
        let mut reg = TopicRegistry::new();
        reg.register(Arc::new(FailingResolver));
        let pool = test_pool().await;
        let mut conn = ConnState::new();

        let frame = conn.handle_subscribe(&reg, &pool, "fail:x").await;
        match frame {
            FrameOut::Error { topic, message } => {
                assert_eq!(topic.as_deref(), Some("fail:x"));
                assert!(message.contains("deliberately failed"));
            }
            other => panic!("expected Error frame, got {other:?}"),
        }
        assert_eq!(conn.sub_count(), 0);
    }

    #[tokio::test]
    async fn matching_note_then_drain_pushes_exactly_one_data_frame() {
        let (reg, resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();
        conn.handle_subscribe(&reg, &pool, "test:abc").await;

        resolver.bump(); // the underlying data changes...
        conn.note(&thing_changed()); // ...and a matching notification lands.

        let frames = conn.drain(&pool).await;
        assert_eq!(
            frames,
            vec![FrameOut::Data {
                topic: "test:abc".into(),
                data: serde_json::json!({"param": "abc", "v": 1}),
            }]
        );

        // The dirty flag cleared: a second drain with no new note is empty.
        assert!(conn.drain(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn non_matching_note_then_drain_pushes_nothing() {
        let (reg, resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();
        conn.handle_subscribe(&reg, &pool, "test:abc").await;

        resolver.bump(); // data DID change, but no interested notification...
        conn.note(&ChangeNotification::new("other", "x", "created"));

        assert!(
            conn.drain(&pool).await.is_empty(),
            "a non-matching aggregate_type must not dirty the sub"
        );
    }

    #[tokio::test]
    async fn unchanged_recompute_is_deduped() {
        let (reg, _resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();
        conn.handle_subscribe(&reg, &pool, "test:abc").await;

        // Matching note, but the resolver returns the SAME snapshot.
        conn.note(&thing_changed());

        assert!(
            conn.drain(&pool).await.is_empty(),
            "dedupe-on-equal: an unchanged recompute emits NO frame"
        );
    }

    #[tokio::test]
    async fn mark_all_dirty_recomputes_every_sub() {
        let (reg, resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();
        conn.handle_subscribe(&reg, &pool, "test:a").await;
        conn.handle_subscribe(&reg, &pool, "test:b").await;

        resolver.bump();
        conn.mark_all_dirty(); // the bus-Lagged path

        let frames = conn.drain(&pool).await;
        // BTreeMap keying makes the order deterministic: "test:a" first.
        assert_eq!(
            frames,
            vec![
                FrameOut::Data {
                    topic: "test:a".into(),
                    data: serde_json::json!({"param": "a", "v": 1}),
                },
                FrameOut::Data {
                    topic: "test:b".into(),
                    data: serde_json::json!({"param": "b", "v": 1}),
                },
            ]
        );
    }

    /// Succeeds on the FIRST resolve (so the subscribe lands), then fails on
    /// every later resolve — exercises the recompute-failure branch of
    /// `drain`, which is unreachable through `FailingResolver` (its
    /// subscribe already errors, so no sub is ever stored).
    struct FlakyResolver {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl super::super::TopicResolver for FlakyResolver {
        fn prefix(&self) -> &'static str {
            "flaky"
        }

        fn interested(&self, _param: &str, change: &ChangeNotification) -> bool {
            change.aggregate_type == "thing"
        }

        async fn resolve(
            &self,
            _pool: &AnyPool,
            _param: &str,
        ) -> Result<serde_json::Value, lumina_core::error::AppError> {
            let n = self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Ok(serde_json::json!({"ok": true}))
            } else {
                Err(lumina_core::error::AppError::Validation(
                    "recompute failed".into(),
                ))
            }
        }
    }

    #[tokio::test]
    async fn drain_resolve_failure_emits_topic_scoped_error_and_clears_dirty() {
        let mut reg = TopicRegistry::new();
        reg.register(Arc::new(FlakyResolver {
            calls: std::sync::atomic::AtomicUsize::new(0),
        }));
        let pool = test_pool().await;
        let mut conn = ConnState::new();

        // First resolve succeeds → sub stored with the init snapshot.
        let init = conn.handle_subscribe(&reg, &pool, "flaky:x").await;
        assert!(matches!(init, FrameOut::Init { .. }));

        // Recompute now fails → exactly ONE topic-scoped Error frame.
        conn.note(&thing_changed());
        let frames = conn.drain(&pool).await;
        match frames.as_slice() {
            [FrameOut::Error { topic, message }] => {
                assert_eq!(topic.as_deref(), Some("flaky:x"));
                assert!(message.contains("recompute failed"));
            }
            other => panic!("expected exactly one Error frame, got {other:?}"),
        }

        // Dirty cleared despite the failure: no error loop without a new note.
        assert!(conn.drain(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn unsubscribe_is_idempotent() {
        let (reg, resolver) = test_registry();
        let pool = test_pool().await;
        let mut conn = ConnState::new();
        conn.handle_subscribe(&reg, &pool, "test:abc").await;
        assert_eq!(conn.sub_count(), 1);

        conn.handle_unsubscribe("test:abc");
        assert_eq!(conn.sub_count(), 0);
        conn.handle_unsubscribe("test:abc"); // second remove: no panic, no-op
        conn.handle_unsubscribe("never-subscribed");
        assert_eq!(conn.sub_count(), 0);

        // A removed sub no longer recomputes.
        resolver.bump();
        conn.note(&thing_changed());
        assert!(conn.drain(&pool).await.is_empty());
    }
}
