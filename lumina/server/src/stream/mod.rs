//! The multiplexed `/api/stream` data-refresh seam (Wave 1 of the
//! read-only sprint/worktree visibility slice).
//!
//! This module is deliberately SOCKET-FREE: it defines the wire-frame
//! contract ([`FrameIn`] / [`FrameOut`]), the per-resource [`TopicResolver`]
//! seam plus its [`TopicRegistry`], and (in [`watcher`]) the per-connection
//! state machine [`ConnState`]. The actual WebSocket handler (T5,
//! `http/stream.rs`) drives these pieces; keeping them socket-free is what
//! makes the state machine unit-testable without a WS upgrade.
//!
//! ## Topic shape
//!
//! A topic string is `"<prefix>:<param>"` — e.g. `"sprint-quiescence:<id>"`.
//! [`TopicRegistry::parse`] splits on the FIRST `:`; the prefix selects a
//! registered resolver and the remainder is the resolver's opaque param.
//!
//! ## Frame contract (NORMATIVE)
//!
//! Every frame is a JSON object tagged with a `type` discriminant
//! (`#[serde(tag = "type", rename_all = "snake_case")]`, mirroring the PTY
//! ws idiom in `http/pty_sessions/ws.rs`). The client's zod schemas (T7) and
//! the e2e (T6) implement EXACTLY this shape — do not drift it.

pub mod topics;
pub mod watcher;

pub use watcher::ConnState;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use lumina_core::db::AnyPool;
use lumina_core::error::AppError;
use lumina_core::notify::ChangeNotification;

/// Inbound stream frames (client → server). A frame body looks like
/// `{"type":"subscribe","topic":"sprint-quiescence:<id>"}`.
///
/// `Serialize` is derived alongside `Deserialize` so test code (and the T6
/// e2e client) can construct wire bytes from the same single source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrameIn {
    /// Subscribe to a topic; the server replies with an `init` full snapshot.
    Subscribe { topic: String },
    /// Unsubscribe from a topic. Idempotent.
    Unsubscribe { topic: String },
    /// Application-layer keepalive; the server replies `pong`.
    Ping,
}

/// Outbound stream frames (server → client). Snapshots, never deltas: a
/// missed `data` frame self-heals on the next push, and reconnect is
/// race-free (init-on-subscribe + resubscribe-all-on-reconnect).
///
/// `Deserialize` is derived alongside `Serialize` so the T6 e2e client can
/// decode frames through the same enum the server encodes with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrameOut {
    /// Full snapshot, sent once in reply to a successful `subscribe`.
    Init {
        topic: String,
        data: serde_json::Value,
    },
    /// Full snapshot on change, deduped-on-equal (an unchanged recompute
    /// emits no frame).
    Data {
        topic: String,
        data: serde_json::Value,
    },
    /// The notify bus lagged; every subscription has been marked dirty and
    /// will recompute. `topic` is `None` for the connection-wide lag case.
    Skipped {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
    },
    /// An error frame.
    ///
    /// CONTRACT NOTE: the client (T7) treats `error.topic` as REQUIRED and
    /// DROPS a topic-less error frame. So every topic-scoped failure (unknown
    /// topic, resolve failure on subscribe or recompute) MUST populate
    /// `topic: Some(..)`. A bare `topic: None` error is reserved for rare
    /// connection-level errors the client may legitimately ignore.
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
        message: String,
    },
    /// Reply to an inbound `ping`.
    Pong,
}

/// One streamable resource family: how to recognise its topics, whether a
/// committed change might affect a given subscription, and how to recompute
/// the full snapshot.
///
/// Implementations live under `stream/topics/` (T6) and are registered in
/// [`TopicRegistry::with_default_topics`].
#[async_trait]
pub trait TopicResolver: Send + Sync {
    /// The topic prefix this resolver owns (the part before the first `:`),
    /// e.g. `"sprint-quiescence"`.
    fn prefix(&self) -> &'static str;

    /// Whether a committed [`ChangeNotification`] MIGHT affect the
    /// subscription identified by `param`. MAY over-approximate (false
    /// positives only cost a cheap recompute that dedupes-on-equal); MUST
    /// NOT under-approximate (a false negative is a missed update).
    fn interested(&self, param: &str, change: &ChangeNotification) -> bool;

    /// The autonomous-vs-interactive mode discriminator (migration 0020) for
    /// the subscription identified by `param`, when the resolver can observe
    /// it from the committed `change`. This is the stream surface's
    /// propagation seam for the [`ChangeNotification::mode`] field that
    /// 1B-F5 observability needs: a resolver whose resource is session-scoped
    /// (e.g. a future PTY-session topic) returns the `change`'s mode so the
    /// discriminator reaches the stream; every existing resolver inherits the
    /// `None` default and is unaffected. Read-only and cheap — it MUST NOT hit
    /// the DB (it runs in the synchronous `note` fold path); derive the value
    /// from the in-hand `change` alone.
    fn mode(&self, _param: &str, change: &ChangeNotification) -> Option<String> {
        let _ = change;
        None
    }

    /// Recompute the full snapshot for `param`. Read-only.
    async fn resolve(&self, pool: &AnyPool, param: &str) -> Result<serde_json::Value, AppError>;
}

/// The set of registered [`TopicResolver`]s; owns topic-string parsing.
pub struct TopicRegistry {
    resolvers: Vec<Arc<dyn TopicResolver>>,
}

impl TopicRegistry {
    /// An empty registry (test/bring-your-own-resolvers construction).
    pub fn new() -> Self {
        Self {
            resolvers: Vec::new(),
        }
    }

    /// The registry carrying every production topic family.
    pub fn with_default_topics() -> Self {
        let mut reg = Self::new();
        reg.register(Arc::new(topics::sprint_quiescence::SprintQuiescenceTopic));
        reg
    }

    /// Register a resolver. Later registrations do not shadow earlier ones:
    /// [`TopicRegistry::parse`] returns the FIRST resolver whose prefix
    /// matches.
    pub fn register(&mut self, r: Arc<dyn TopicResolver>) {
        self.resolvers.push(r);
    }

    /// Split a topic string on the FIRST `:` into `(prefix, param)` and
    /// select the resolver owning that prefix. Returns `None` when the topic
    /// carries no `:` or no registered resolver matches the prefix.
    pub fn parse(&self, topic: &str) -> Option<(Arc<dyn TopicResolver>, String)> {
        let (prefix, param) = topic.split_once(':')?;
        self.resolvers
            .iter()
            .find(|r| r.prefix() == prefix)
            .map(|r| (Arc::clone(r), param.to_string()))
    }
}

impl Default for TopicRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A deterministic test resolver shared by the `mod.rs` and `watcher.rs`
    //! unit tests. The snapshot derives from an [`AtomicI64`] the test bumps
    //! to simulate "the underlying data changed" — no DB read needed (the
    //! `AnyPool` parameter is accepted and ignored).

    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    pub(crate) struct TestResolver {
        pub(crate) value: AtomicI64,
    }

    impl TestResolver {
        pub(crate) fn new() -> Self {
            Self {
                value: AtomicI64::new(0),
            }
        }

        pub(crate) fn bump(&self) {
            self.value.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl TopicResolver for TestResolver {
        fn prefix(&self) -> &'static str {
            "test"
        }

        fn interested(&self, _param: &str, change: &ChangeNotification) -> bool {
            change.aggregate_type == "thing"
        }

        async fn resolve(
            &self,
            _pool: &AnyPool,
            param: &str,
        ) -> Result<serde_json::Value, AppError> {
            Ok(serde_json::json!({
                "param": param,
                "v": self.value.load(Ordering::SeqCst),
            }))
        }
    }

    /// A resolver whose `resolve` always fails — exercises the error paths.
    pub(crate) struct FailingResolver;

    #[async_trait]
    impl TopicResolver for FailingResolver {
        fn prefix(&self) -> &'static str {
            "fail"
        }

        fn interested(&self, _param: &str, change: &ChangeNotification) -> bool {
            change.aggregate_type == "thing"
        }

        async fn resolve(
            &self,
            _pool: &AnyPool,
            _param: &str,
        ) -> Result<serde_json::Value, AppError> {
            Err(AppError::Validation("resolver deliberately failed".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TestResolver;
    use super::*;

    fn registry_with_test_resolver() -> TopicRegistry {
        let mut reg = TopicRegistry::new();
        reg.register(Arc::new(TestResolver::new()));
        reg
    }

    #[test]
    fn parse_known_prefix_yields_resolver_and_param() {
        let reg = registry_with_test_resolver();
        let (resolver, param) = reg.parse("test:abc").expect("known prefix should parse");
        assert_eq!(resolver.prefix(), "test");
        assert_eq!(param, "abc");
    }

    #[test]
    fn parse_splits_on_first_colon_only() {
        let reg = registry_with_test_resolver();
        let (_, param) = reg.parse("test:a:b").expect("should parse");
        assert_eq!(param, "a:b", "param keeps everything after the FIRST colon");
    }

    #[test]
    fn parse_unknown_prefix_is_none() {
        let reg = registry_with_test_resolver();
        assert!(reg.parse("nope:abc").is_none());
    }

    #[test]
    fn parse_colonless_topic_is_none() {
        let reg = registry_with_test_resolver();
        assert!(reg.parse("nocolon").is_none());
    }

    #[test]
    fn with_default_topics_registers_sprint_quiescence() {
        let reg = TopicRegistry::with_default_topics();
        let (resolver, param) = reg
            .parse("sprint-quiescence:abc")
            .expect("the sprint-quiescence resolver is registered by default");
        assert_eq!(resolver.prefix(), "sprint-quiescence");
        assert_eq!(param, "abc");
        // A prefix nothing registered still parses to None.
        assert!(reg.parse("bogus:abc").is_none());
    }

    #[test]
    fn frame_contract_wire_shapes() {
        // Inbound.
        let sub: FrameIn =
            serde_json::from_str(r#"{"type":"subscribe","topic":"test:abc"}"#).unwrap();
        assert_eq!(
            sub,
            FrameIn::Subscribe {
                topic: "test:abc".into()
            }
        );
        let ping: FrameIn = serde_json::from_str(r#"{"type":"ping"}"#).unwrap();
        assert_eq!(ping, FrameIn::Ping);

        // Outbound: tag + snake_case.
        let init = FrameOut::Init {
            topic: "test:abc".into(),
            data: serde_json::json!({"v": 1}),
        };
        assert_eq!(
            serde_json::to_value(&init).unwrap(),
            serde_json::json!({"type":"init","topic":"test:abc","data":{"v":1}})
        );

        // Skipped/Error omit a None topic entirely (skip_serializing_if).
        let skipped = serde_json::to_value(FrameOut::Skipped { topic: None }).unwrap();
        assert_eq!(skipped, serde_json::json!({"type":"skipped"}));

        let err = serde_json::to_value(FrameOut::Error {
            topic: Some("test:abc".into()),
            message: "boom".into(),
        })
        .unwrap();
        assert_eq!(
            err,
            serde_json::json!({"type":"error","topic":"test:abc","message":"boom"})
        );

        let pong = serde_json::to_value(FrameOut::Pong).unwrap();
        assert_eq!(pong, serde_json::json!({"type":"pong"}));
    }
}
