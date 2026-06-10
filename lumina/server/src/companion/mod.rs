//! Server-side companion seam (ADR-0006 Step 1b) — the in-process registry
//! through which the record-only control plane talks to the git-executing
//! execution plane (`lumina-companion`).
//!
//! ## Shape
//!
//! [`CompanionRegistry`] owns five pieces of purely in-memory state:
//!
//!   * a SINGLE connection slot (an `mpsc::Sender<ServerToCompanion>` feeding
//!     the WebSocket send task in `http::companion`) — Step 1b is
//!     single-companion by decision: a second concurrent connection is
//!     refused at registration time;
//!   * a monotonically increasing [`RequestId`] counter (never reset across
//!     reconnects, so a stale outcome from a previous connection can never
//!     collide with a live pending entry);
//!   * the pending map `RequestId → oneshot::Sender<Outcome>` pairing each
//!     in-flight [`execute`](CompanionRegistry::execute) with the outcome the
//!     receive task will deliver via [`complete`](CompanionRegistry::complete);
//!   * the merge-lease set keyed by TARGET BRANCH. Leases are IN-MEMORY by
//!     decision (User Decision 3): they are voided wholesale on disconnect
//!     and vanish on server restart — there is deliberately no DB table.
//!   * the merge-lease set keyed by WORKTREE ID (review R8) — same lifecycle
//!     as the target-keyed set. The target lease alone cannot serialise two
//!     concurrent merges of the SAME worktree with DIFFERENT `target_branch`
//!     overrides (disjoint target keys), and the loser of that race would
//!     record-fail AFTER its ref-CAS already advanced a ref; the
//!     `execute_worktree_merge_flow` therefore takes BOTH leases (worktree
//!     first, then target; released in reverse).
//!
//! ## Disconnect cleanup
//!
//! [`disconnect`](CompanionRegistry::disconnect) frees the slot, drains EVERY
//! pending oneshot (dropping the senders, which resolves each waiting
//! `execute` future with [`CompanionError::Disconnected`]), and voids ALL
//! merge leases. An [`Outcome`] whose `RequestId` has no pending entry (e.g.
//! the server restarted mid-merge, or the execute timed out first) is logged
//! and dropped — never a panic or a connection close.
//!
//! Locking: the inner state sits behind a `std::sync::Mutex` held only for
//! short, non-async critical sections — every `.await` in this module happens
//! with the lock released.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};

use lumina_protocol::{Intent, Outcome, RequestId, ServerToCompanion};

/// Default deadline for one [`CompanionRegistry::execute`] round-trip
/// (intent sent → outcome received). Generous because a single coarse intent
/// may cover a whole merge including conflict-detection and abort/restore on
/// a large repo; constructor-tunable via
/// [`CompanionRegistry::with_execute_timeout`] so tests don't wait 120s.
pub const DEFAULT_EXECUTE_TIMEOUT: Duration = Duration::from_secs(120);

/// Errors surfaced by the protocol-client API ([`CompanionRegistry::execute`]).
/// Deliberately coarse — callers branch on "can I retry / is anything
/// connected", never on transport detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanionError {
    /// No companion is connected — the request was never sent. Callers
    /// surface this as "git execution plane unavailable".
    CompanionUnavailable,
    /// The companion disconnected (or the send task died) while the request
    /// was in flight; the outcome will never arrive on this connection.
    Disconnected,
    /// The companion did not answer within the execute timeout. The pending
    /// entry is deregistered, so a late outcome takes the stale-id
    /// logged-and-dropped path rather than waking anything.
    Timeout,
}

impl fmt::Display for CompanionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompanionUnavailable => write!(f, "no companion is connected"),
            Self::Disconnected => write!(f, "companion disconnected while the request was in flight"),
            Self::Timeout => write!(f, "companion did not answer within the execute timeout"),
        }
    }
}

impl std::error::Error for CompanionError {}

/// Opaque proof of a successful [`CompanionRegistry::register`]. Consumed by
/// [`CompanionRegistry::disconnect`]; carries the connection epoch so a late
/// or duplicate cleanup can never tear down a NEWER connection that has since
/// claimed the slot (the epoch guard). Deliberately neither `Clone` nor
/// `Copy` — one registration, one disconnect.
#[derive(Debug)]
pub struct ConnectionToken {
    epoch: u64,
}

/// The live connection occupying the single slot.
struct Connection {
    /// Feeds the WebSocket send task in `http::companion`.
    tx: mpsc::Sender<ServerToCompanion>,
    /// Generation stamp matching the [`ConnectionToken`] handed to the owner.
    epoch: u64,
    /// The `Hello.repo_root` the companion reported at handshake time — the
    /// absolute path of the repo it executes git against. Surfaced via
    /// [`CompanionRegistry::repo_root`] so the `execute_worktree_merge`
    /// pre-flight can run its split-brain guard (repo_root must match the
    /// project's primary repo-link `local_path` when that column is set).
    repo_root: String,
}

/// Mutex-guarded mutable state. See the module docs for the field semantics.
struct Inner {
    conn: Option<Connection>,
    /// Last allocated request id; global-monotonic (never reset), which is
    /// trivially "monotonic per connection" as the protocol requires.
    next_id: u64,
    pending: HashMap<RequestId, oneshot::Sender<Outcome>>,
    /// Merge leases keyed by target branch (User Decision 3: in-memory only).
    leases: HashSet<String>,
    /// Merge leases keyed by WORKTREE id (review R8) — serialises concurrent
    /// merges of the same worktree under different target overrides. Same
    /// in-memory lifecycle as `leases`.
    worktree_leases: HashSet<String>,
    /// Connection generation counter backing the epoch guard.
    epoch: u64,
}

/// See the module docs. Shared as `Arc<CompanionRegistry>` on `AppState`.
pub struct CompanionRegistry {
    inner: Mutex<Inner>,
    /// "Is a companion connected?" signal. The Task-9 e2e awaits this watch
    /// flipping `true` to know registration completed deterministically.
    connected_tx: watch::Sender<bool>,
    execute_timeout: Duration,
}

impl Default for CompanionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CompanionRegistry {
    /// Registry with the production [`DEFAULT_EXECUTE_TIMEOUT`].
    pub fn new() -> Self {
        Self::with_execute_timeout(DEFAULT_EXECUTE_TIMEOUT)
    }

    /// Registry with an explicit execute timeout — the test seam (a
    /// timeout-path test should not wait two minutes).
    pub fn with_execute_timeout(execute_timeout: Duration) -> Self {
        let (connected_tx, _) = watch::channel(false);
        Self {
            inner: Mutex::new(Inner {
                conn: None,
                next_id: 0,
                pending: HashMap::new(),
                leases: HashSet::new(),
                worktree_leases: HashSet::new(),
                epoch: 0,
            }),
            connected_tx,
            execute_timeout,
        }
    }

    /// Claim the single companion connection slot, recording the validated
    /// `Hello.repo_root` alongside it. Returns `None` when a companion is
    /// already connected (the caller refuses the new connection — loudly); the
    /// slot frees only via [`disconnect`](Self::disconnect) (socket close or
    /// the missed-pong reaper, both of which route there).
    pub fn register(
        &self,
        tx: mpsc::Sender<ServerToCompanion>,
        repo_root: String,
    ) -> Option<ConnectionToken> {
        let epoch = {
            let mut inner = self.inner.lock().expect("companion registry lock poisoned");
            if inner.conn.is_some() {
                return None;
            }
            inner.epoch += 1;
            let epoch = inner.epoch;
            inner.conn = Some(Connection { tx, epoch, repo_root });
            epoch
        };
        self.connected_tx.send_replace(true);
        Some(ConnectionToken { epoch })
    }

    /// Full disconnect cleanup: free the slot, drain ALL pending request
    /// oneshots (their `execute` futures resolve with
    /// [`CompanionError::Disconnected`]), void ALL merge leases, and flip the
    /// connected watch to `false`. A token whose epoch no longer matches the
    /// live connection is a stale cleanup racing a newer registration — it is
    /// ignored, leaving the incumbent untouched.
    pub fn disconnect(&self, token: ConnectionToken) {
        let (drained, voided) = {
            let mut inner = self.inner.lock().expect("companion registry lock poisoned");
            if inner.conn.as_ref().map(|c| c.epoch) != Some(token.epoch) {
                tracing::debug!(
                    epoch = token.epoch,
                    "companion: stale disconnect token ignored (slot re-claimed or already freed)"
                );
                return;
            }
            inner.conn = None;
            let drained = inner.pending.len();
            // Dropping the senders resolves every waiting `execute` future
            // with a RecvError → mapped to `Disconnected`.
            inner.pending.clear();
            // BOTH lease spaces void wholesale (target-keyed + worktree-keyed).
            let voided = inner.leases.len() + inner.worktree_leases.len();
            inner.leases.clear();
            inner.worktree_leases.clear();
            (drained, voided)
        };
        self.connected_tx.send_replace(false);
        tracing::info!(
            pending_drained = drained,
            leases_voided = voided,
            "companion disconnected: pending requests drained, merge leases voided"
        );
    }

    /// Deliver an [`Outcome`] from the wire to the waiting `execute` future.
    /// A stale id (no pending entry — e.g. a pre-restart request, or the
    /// execute already timed out) is logged and DROPPED; never a panic, never
    /// a connection close.
    pub fn complete(&self, id: RequestId, outcome: Outcome) {
        let sender = self
            .inner
            .lock()
            .expect("companion registry lock poisoned")
            .pending
            .remove(&id);
        match sender {
            Some(tx) => {
                // The receiver may have been dropped between our `remove` and
                // this send (execute future cancelled) — benign race.
                if tx.send(outcome).is_err() {
                    tracing::debug!(id = id.0, "companion: outcome arrived after the execute future was dropped");
                }
            }
            None => {
                tracing::warn!(
                    id = id.0,
                    outcome = ?outcome,
                    "companion: outcome carries an unknown request id (stale — dropped)"
                );
            }
        }
    }

    /// Send one [`Intent`] to the connected companion and await its single
    /// [`Outcome`] — the protocol-client API the MCP/HTTP layers call.
    ///
    /// Errors: [`CompanionError::CompanionUnavailable`] when no companion is
    /// connected (nothing was sent); [`CompanionError::Disconnected`] when
    /// the connection died before the outcome arrived;
    /// [`CompanionError::Timeout`] after [`Self::with_execute_timeout`]'s
    /// deadline (default [`DEFAULT_EXECUTE_TIMEOUT`]), in which case the
    /// pending entry is deregistered so a late outcome is treated as stale.
    pub async fn execute(&self, intent: Intent) -> Result<Outcome, CompanionError> {
        let (id, conn_tx, outcome_rx) = {
            let mut inner = self.inner.lock().expect("companion registry lock poisoned");
            let conn_tx = match inner.conn.as_ref() {
                Some(conn) => conn.tx.clone(),
                None => return Err(CompanionError::CompanionUnavailable),
            };
            inner.next_id += 1;
            let id = RequestId(inner.next_id);
            let (tx, rx) = oneshot::channel();
            inner.pending.insert(id, tx);
            (id, conn_tx, rx)
        };

        if conn_tx
            .send(ServerToCompanion::IntentRequest { id, intent })
            .await
            .is_err()
        {
            // The send task is gone but the disconnect sweep may not have run
            // yet — deregister our own entry rather than leaking it.
            self.inner
                .lock()
                .expect("companion registry lock poisoned")
                .pending
                .remove(&id);
            return Err(CompanionError::Disconnected);
        }

        match tokio::time::timeout(self.execute_timeout, outcome_rx).await {
            Ok(Ok(outcome)) => Ok(outcome),
            // Sender dropped without a value: the disconnect sweep drained us.
            Ok(Err(_)) => Err(CompanionError::Disconnected),
            Err(_) => {
                // Deregister so a late outcome takes the stale-id path.
                self.inner
                    .lock()
                    .expect("companion registry lock poisoned")
                    .pending
                    .remove(&id);
                Err(CompanionError::Timeout)
            }
        }
    }

    /// Try to take the merge lease on `target_branch`. Returns `false` when
    /// the lease is already held — leases are NOT re-entrant; the caller maps
    /// a refusal to an "operation already in flight" error. In-memory only:
    /// voided on disconnect, gone on restart (User Decision 3).
    pub fn acquire_lease(&self, target_branch: &str) -> bool {
        self.inner
            .lock()
            .expect("companion registry lock poisoned")
            .leases
            .insert(target_branch.to_string())
    }

    /// Release the merge lease on `target_branch`. Idempotent — releasing an
    /// unheld lease is a no-op.
    pub fn release_lease(&self, target_branch: &str) {
        self.inner
            .lock()
            .expect("companion registry lock poisoned")
            .leases
            .remove(target_branch);
    }

    /// Try to take the WORKTREE-keyed merge lease on `worktree_id` (review R8).
    /// Returns `false` when the lease is already held — not re-entrant; the
    /// caller maps a refusal to the same "merge already in flight" Validation
    /// the target lease produces. Same lifecycle as the target-keyed lease:
    /// released on completion (drop-guard), voided wholesale on disconnect,
    /// gone on restart.
    pub fn acquire_worktree_lease(&self, worktree_id: &str) -> bool {
        self.inner
            .lock()
            .expect("companion registry lock poisoned")
            .worktree_leases
            .insert(worktree_id.to_string())
    }

    /// Release the worktree-keyed merge lease on `worktree_id`. Idempotent —
    /// releasing an unheld lease is a no-op.
    pub fn release_worktree_lease(&self, worktree_id: &str) {
        self.inner
            .lock()
            .expect("companion registry lock poisoned")
            .worktree_leases
            .remove(worktree_id);
    }

    /// Subscribe to the "companion connected" signal (`true` while the slot
    /// is occupied). The e2e awaits `wait_for(|c| *c)` on this receiver to
    /// observe registration deterministically.
    pub fn connected(&self) -> watch::Receiver<bool> {
        self.connected_tx.subscribe()
    }

    /// Snapshot of the connected signal.
    pub fn is_connected(&self) -> bool {
        *self.connected_tx.borrow()
    }

    /// The connected companion's `Hello.repo_root` — `None` when no companion
    /// occupies the slot. Doubles as a connected check for callers that need
    /// the root anyway (the `execute_worktree_merge` pre-flight split-brain
    /// guard compares it against the project's primary repo-link `local_path`).
    pub fn repo_root(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("companion registry lock poisoned")
            .conn
            .as_ref()
            .map(|c| c.repo_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// `execute` against an empty registry never sends anything and reports
    /// the companion as unavailable.
    #[tokio::test]
    async fn companion_execute_without_connection_is_unavailable() {
        let reg = CompanionRegistry::new();
        assert_eq!(
            reg.execute(Intent::Reconcile).await,
            Err(CompanionError::CompanionUnavailable)
        );
    }

    /// Happy path: register → execute sends an `IntentRequest` through the
    /// slot's mpsc → `complete` with the matching id resolves the future.
    #[tokio::test]
    async fn companion_execute_round_trips_an_outcome() {
        let reg = Arc::new(CompanionRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);
        let _token = reg.register(tx, "/work/repo".to_string()).expect("slot free");

        let exec = tokio::spawn({
            let reg = reg.clone();
            async move { reg.execute(Intent::Reconcile).await }
        });

        let ServerToCompanion::IntentRequest { id, intent } =
            rx.recv().await.expect("intent on the wire");
        assert_eq!(intent, Intent::Reconcile);
        reg.complete(id, Outcome::WorktreeRemoved);

        assert_eq!(exec.await.unwrap(), Ok(Outcome::WorktreeRemoved));
    }

    /// Disconnect drains the pending map: an in-flight execute resolves with
    /// `Disconnected` instead of hanging until the timeout.
    #[tokio::test]
    async fn companion_pending_drained_on_disconnect() {
        let reg = Arc::new(CompanionRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);
        let token = reg.register(tx, "/work/repo".to_string()).expect("slot free");

        let exec = tokio::spawn({
            let reg = reg.clone();
            async move { reg.execute(Intent::Reconcile).await }
        });

        // Wait until the request is genuinely pending (it reached the wire).
        let _ = rx.recv().await.expect("intent on the wire");
        reg.disconnect(token);

        assert_eq!(exec.await.unwrap(), Err(CompanionError::Disconnected));
    }

    /// Lease semantics: not re-entrant, idempotent release, voided wholesale
    /// on disconnect.
    #[tokio::test]
    async fn companion_leases_void_on_disconnect_and_are_not_reentrant() {
        let reg = CompanionRegistry::new();
        assert!(reg.acquire_lease("main"));
        assert!(!reg.acquire_lease("main"), "leases are not re-entrant");
        reg.release_lease("main");
        reg.release_lease("main"); // idempotent — second release is a no-op
        assert!(reg.acquire_lease("main"), "released lease is acquirable again");

        let (tx, _rx) = mpsc::channel(1);
        let token = reg.register(tx, "/work/repo".to_string()).expect("slot free");
        reg.disconnect(token);
        assert!(reg.acquire_lease("main"), "disconnect voids all leases");
    }

    /// Worktree-keyed lease semantics (R8): not re-entrant, idempotent release,
    /// independent of the target-keyed space, voided wholesale on disconnect.
    #[tokio::test]
    async fn companion_worktree_leases_mirror_target_lease_semantics() {
        let reg = CompanionRegistry::new();
        assert!(reg.acquire_worktree_lease("wt-1"));
        assert!(!reg.acquire_worktree_lease("wt-1"), "worktree leases are not re-entrant");
        // The two lease key spaces are independent: the SAME string is free in
        // the target space while held in the worktree space.
        assert!(reg.acquire_lease("wt-1"), "target space is independent of worktree space");
        reg.release_lease("wt-1");
        reg.release_worktree_lease("wt-1");
        reg.release_worktree_lease("wt-1"); // idempotent — second release is a no-op
        assert!(reg.acquire_worktree_lease("wt-1"), "released worktree lease is acquirable");

        let (tx, _rx) = mpsc::channel(1);
        let token = reg.register(tx, "/work/repo".to_string()).expect("slot free");
        reg.disconnect(token);
        assert!(
            reg.acquire_worktree_lease("wt-1"),
            "disconnect voids the worktree-keyed leases too"
        );
    }

    /// Single-companion slot: a second concurrent registration is refused;
    /// the slot frees once the first disconnects.
    #[tokio::test]
    async fn companion_double_connect_is_refused() {
        let reg = CompanionRegistry::new();
        let (tx1, _rx1) = mpsc::channel(1);
        let token = reg.register(tx1, "/work/repo".to_string()).expect("slot free");

        let (tx2, _rx2) = mpsc::channel(1);
        assert!(
            reg.register(tx2, "/work/repo".to_string()).is_none(),
            "second concurrent connection must be refused"
        );

        reg.disconnect(token);
        let (tx3, _rx3) = mpsc::channel(1);
        assert!(
            reg.register(tx3, "/work/repo".to_string()).is_some(),
            "slot frees after disconnect"
        );
    }

    /// Execute timeout: the future errs with `Timeout`, the pending entry is
    /// deregistered, and a LATE outcome takes the stale-id logged-and-dropped
    /// path (no panic). Paused-clock test so the deadline elapses instantly.
    #[tokio::test(start_paused = true)]
    async fn companion_execute_times_out_and_late_outcome_is_dropped() {
        let reg = Arc::new(CompanionRegistry::with_execute_timeout(Duration::from_millis(50)));
        let (tx, mut rx) = mpsc::channel(8);
        let _token = reg.register(tx, "/work/repo".to_string()).expect("slot free");

        let exec = tokio::spawn({
            let reg = reg.clone();
            async move { reg.execute(Intent::Reconcile).await }
        });

        let ServerToCompanion::IntentRequest { id, .. } =
            rx.recv().await.expect("intent on the wire");
        assert_eq!(exec.await.unwrap(), Err(CompanionError::Timeout));

        // The companion answers after the deadline: stale id, dropped.
        reg.complete(id, Outcome::WorktreeRemoved);
    }

    /// A stale outcome against an EMPTY registry (e.g. server restarted
    /// mid-merge and the old id means nothing) is dropped without panicking.
    #[tokio::test]
    async fn companion_stale_outcome_on_empty_registry_is_dropped() {
        let reg = CompanionRegistry::new();
        reg.complete(RequestId(999), Outcome::WorktreeRemoved);
    }

    /// The connected watch flips true on register and false on disconnect —
    /// the deterministic registration signal the e2e awaits.
    #[tokio::test]
    async fn companion_connected_watch_tracks_registration() {
        let reg = CompanionRegistry::new();
        let watch_rx = reg.connected();
        assert!(!*watch_rx.borrow());
        assert!(!reg.is_connected());
        assert_eq!(reg.repo_root(), None, "no repo_root before registration");

        let (tx, _rx) = mpsc::channel(1);
        let token = reg.register(tx, "/work/repo".to_string()).expect("slot free");
        assert!(*watch_rx.borrow());
        assert!(reg.is_connected());
        assert_eq!(
            reg.repo_root().as_deref(),
            Some("/work/repo"),
            "repo_root surfaces the registered Hello.repo_root"
        );

        reg.disconnect(token);
        assert!(!*watch_rx.borrow());
        assert!(!reg.is_connected());
        assert_eq!(reg.repo_root(), None, "repo_root clears on disconnect");
    }

    /// Epoch guard: a duplicate/late cleanup carrying a PREVIOUS connection's
    /// epoch must not tear down the connection that has since claimed the
    /// slot (nor void its leases).
    #[tokio::test]
    async fn companion_stale_disconnect_token_is_ignored() {
        let reg = CompanionRegistry::new();
        let (tx1, _rx1) = mpsc::channel(1);
        let token_a = reg.register(tx1, "/work/repo".to_string()).expect("slot free"); // epoch 1
        reg.disconnect(token_a);

        let (tx2, _rx2) = mpsc::channel(1);
        let _token_b = reg.register(tx2, "/work/repo".to_string()).expect("slot free"); // epoch 2
        assert!(reg.acquire_lease("main"));

        // Forge a late cleanup from the first connection's generation.
        reg.disconnect(ConnectionToken { epoch: 1 });
        assert!(reg.is_connected(), "stale disconnect must not free the slot");
        assert!(
            !reg.acquire_lease("main"),
            "stale disconnect must not void the incumbent's leases"
        );
    }
}
