//! Git-export materialiser (transactional outbox).
//!
//! STUB for the slice: `spawn` is a no-op. Task 6 implements a synchronous
//! `export_pending(&pool) -> anyhow::Result<usize>` core (select
//! `events WHERE exported_at IS NULL`, render each affected work-item to
//! `./.lumina/export/<kind>/<id>.toml` via serde+toml with atomic
//! tempfile→rename, then stamp `exported_at`) and turns `spawn` into the
//! background loop that ticks `export_pending` and selects on a
//! `tokio_util::sync::CancellationToken`. The `exported_at IS NULL` invariant
//! makes an ungraceful mid-render kill safe — the event re-drains on restart.
//! `spawn` keeps this signature so `app.rs` need not change.

use std::sync::Arc;

use sqlx::SqlitePool;

/// Spawn the background export loop. No-op stub; Task 6 fills the loop body.
pub fn spawn(_pool: Arc<SqlitePool>) {}
