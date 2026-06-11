//! Post-commit notify-bus integration proof (Wave 1, T2 of
//! `docs/plans/vectorized-brewing-boole.md`).
//!
//! Drives the `NotifyingTx` buffering path end-to-end through a REAL repo
//! mutation: `repo::create_sprint` → `record_event` (which buffers a
//! `ChangeNotification` on the in-flight tx via `DbTx::note_change`) →
//! `NotifyingTx::commit` (which flushes the buffer to the process-wide bus
//! AFTER the commit succeeds). The negative test proves the other half of the
//! contract: a transaction dropped WITHOUT commit publishes nothing.
//!
//! ## Why every assertion filters on its OWN aggregate id
//!
//! `lumina_core::notify::bus()` is a process-wide `OnceLock` singleton. Under
//! nextest (the canonical runner) each test runs in its own process, so the bus
//! is fresh per test. But under plain `cargo test` the tests in this binary
//! share one process — a concurrent test's notification (or an
//! `Err(Lagged(_))` if a receiver falls behind) can appear on this test's
//! receiver. Each test therefore mints a UNIQUE aggregate id and asserts only
//! on notifications carrying that id, skipping everything foreign.

use std::time::Duration;

use lumina_core::db::{connect_in_memory, DbClient};
use lumina_core::domain::NewSprint;
use lumina_core::notify::{bus, ChangeNotification};
use lumina_core::repo;
use tokio::sync::broadcast::error::TryRecvError;

/// A committed repo mutation publishes its change notification to the bus —
/// and only AFTER commit (the receiver is subscribed before the write, and the
/// notification observed here implies the flush ran post-`create_sprint`,
/// whose final act is the tx commit).
#[tokio::test]
async fn notify_bus_publishes_after_commit() {
    let pool = connect_in_memory().await.expect("in-memory pool");

    // Subscribe BEFORE the write: a broadcast receiver only sees
    // notifications published after `subscribe()`.
    let mut rx = bus().subscribe();

    let sprint_id = repo::create_sprint(
        &pool,
        &NewSprint {
            title: Some("notify-bus positive".to_owned()),
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
    .await
    .expect("create_sprint")
    .to_string();

    // Loop until OUR sprint's notification arrives (skipping any foreign
    // notification a sibling test may have published under plain `cargo
    // test`); 1s cap so a regression fails fast instead of hanging.
    let deadline = Duration::from_secs(1);
    loop {
        let n = match tokio::time::timeout(deadline, rx.recv())
            .await
            .expect("timed out waiting for the post-commit notification")
        {
            Ok(n) => n,
            // Lagged: older items were discarded; ours may still be queued
            // (or arrive next) — keep receiving under the same timeout cap.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(e) => panic!("notify bus receiver closed unexpectedly: {e}"),
        };
        if n.aggregate_id != sprint_id {
            continue; // foreign notification from a sibling test — skip.
        }
        assert_eq!(n.aggregate_type, "sprint");
        assert_eq!(n.event_type, "sprint.created");
        break;
    }
}

/// A transaction dropped WITHOUT commit discards its buffered notifications —
/// the bus never sees a phantom signal for a write that did not land.
#[tokio::test]
async fn notify_bus_dropped_tx_publishes_nothing() {
    let pool = connect_in_memory().await.expect("in-memory pool");

    let mut rx = bus().subscribe();

    // A unique aggregate id so this test's assertion is immune to foreign
    // notifications on the shared process-wide bus (see module docs).
    let unique_id = uuid::Uuid::now_v7().to_string();

    {
        // The seam path: `DbClient::begin` returns the `NotifyingTx` wrapper
        // (explicit trait call — the inherent `Pool::begin` would shadow it).
        // `note_change` is the new PUBLIC `DbTx` method; we call it directly
        // because `record_event` is `pub(crate)` and unreachable from this
        // external test crate.
        let mut tx = <sqlx::SqlitePool as DbClient>::begin(&pool)
            .await
            .expect("begin write tx");
        tx.note_change(ChangeNotification::new("sprint", unique_id.as_str(), "created"));
        // Dropped here WITHOUT commit — the buffer must go with it.
    }

    // Drain the receiver. Under plain `cargo test` the process-shared bus can
    // carry a sibling test's notification or report Lagged, so: skip foreign
    // aggregate_ids, treat Lagged as continue (the skipped backlog cannot
    // contain our id any more than the drained items can — our tx never
    // published), and stop on Empty/Closed.
    loop {
        match rx.try_recv() {
            Ok(n) => assert_ne!(
                n.aggregate_id, unique_id,
                "a dropped (uncommitted) tx must publish nothing"
            ),
            Err(TryRecvError::Lagged(_)) => continue,
            Err(TryRecvError::Empty) | Err(TryRecvError::Closed) => break,
        }
    }
}
