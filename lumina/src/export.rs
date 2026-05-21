//! Git-export materialiser — the transactional outbox drain (Task 6).
//!
//! # What this module does
//!
//! Every domain mutation in [`crate::repo`] appends one `events` row in the
//! same transaction as the domain write (the single-source-of-truth discipline)
//! with `exported_at` left NULL. This module is the asynchronous *drain* half of
//! that transactional-outbox pattern: it reads the unexported events, re-renders
//! each affected work-item's CURRENT state to a git-committable per-item TOML
//! snapshot under an export root, then stamps `exported_at` so the event is not
//! re-drained. Export runs entirely OFF the API hot path — a mutation's HTTP /
//! MCP response never waits on it.
//!
//! [`export_pending`] is the synchronous, directly-callable core (so tests and
//! the e2e drive it without `sleep`). [`spawn`] is the background loop that
//! ticks `export_pending` on an interval and selects on a
//! [`CancellationToken`](tokio_util::sync::CancellationToken) for graceful
//! shutdown.
//!
//! # Shutdown / recovery invariant (P12)
//!
//! An ungraceful kill mid-render is SAFE. The drain renders the file FIRST and
//! only stamps `exported_at` AFTER the atomic file write succeeds, and it does
//! so per-event. If the process is killed at any point before the
//! `UPDATE … SET exported_at` commits, the event's `exported_at` stays NULL, so
//! it remains in the outbox and is re-drained on the next start (or the next
//! tick). The file write is itself atomic (tempfile → fsync → rename), so a
//! crash mid-write never leaves a torn snapshot in place — the worst case is a
//! leftover temp file in the export dir, never a half-written `<id>.toml`. The
//! render is idempotent (it re-materialises the work-item's current state), so
//! re-draining a partially-exported batch simply rewrites byte-identical files.
//!
//! No `git add` / `git commit` is performed here — committing the export dir is
//! left to the user / agent, consistent with the apply-flow contracts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::repo;

/// Default export root, used by the background [`spawn`] loop when
/// `LUMINA_EXPORT_ROOT` is unset. Distinct from `.claude/` so the export dir
/// never clashes with live `tomlctl` flow state during coexistence.
const DEFAULT_EXPORT_ROOT: &str = "./.lumina/export";

/// Env var overriding the [`spawn`] loop's export root. `export_pending` itself
/// takes an explicit root for test / e2e isolation; only the background loop
/// reads this.
const EXPORT_ROOT_ENV: &str = "LUMINA_EXPORT_ROOT";

/// How often the background loop drains the outbox.
const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Handle returned by [`spawn`]: owns the [`CancellationToken`] that stops the
/// loop and the [`JoinHandle`](tokio::task::JoinHandle) for the loop task. The
/// composition root (`app.rs`) ignores the return value (`export::spawn(pool)`),
/// so this is purely for callers that want graceful shutdown / join.
///
/// Dropping the handle does NOT cancel the loop — call [`ExportHandle::shutdown`]
/// (or [`cancel`](ExportHandle::cancel)) for that. This keeps the
/// fire-and-forget `app.rs` call site working: the loop outlives the dropped
/// handle and keeps draining until the process exits.
pub struct ExportHandle {
    token: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

impl ExportHandle {
    /// Signal the loop to stop at its next `select!` point without waiting.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Signal the loop to stop and await its completion (graceful shutdown).
    pub async fn shutdown(self) {
        self.token.cancel();
        let _ = self.join.await;
    }

    /// A child token, for callers wiring the loop into a wider shutdown tree.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }
}

/// Drain the outbox once: render every unexported event's aggregate to a TOML
/// snapshot under `export_root`, then stamp `exported_at`. Returns the number of
/// events drained (stamped).
///
/// # Behaviour
///
/// 1. `SELECT id, aggregate_type, aggregate_id FROM events WHERE exported_at IS
///    NULL ORDER BY created_at` — the outbox, oldest first.
/// 2. Per event, if its aggregate is a work-item that has not yet been rendered
///    in THIS drain, re-materialise the work-item's current state via
///    [`repo::get_work_item_detail`] and write it atomically to
///    `<export_root>/<kind>/<id>.toml`. De-duping per aggregate within one drain
///    keeps the file write idempotent (the same aggregate touched by N events in
///    one batch is rendered once).
/// 3. After a successful render (or for a non-rendering event), stamp
///    `exported_at` for that event.
///
/// Each event is stamped only AFTER its aggregate's render has succeeded, so a
/// failure mid-batch leaves the un-stamped events in the outbox for the next
/// drain (the recovery invariant). A second call with no new events is a no-op
/// returning 0 and leaves every snapshot byte-identical.
pub async fn export_pending(pool: &SqlitePool, export_root: &Path) -> anyhow::Result<usize> {
    let pending = sqlx::query!(
        r#"
        SELECT
            id             AS "id!",
            aggregate_type AS "aggregate_type!",
            aggregate_id   AS "aggregate_id!"
        FROM events
        WHERE exported_at IS NULL
        ORDER BY created_at, id
        "#,
    )
    .fetch_all(pool)
    .await
    .context("selecting unexported events")?;

    // Aggregates already rendered in THIS drain — render-once de-dup so repeated
    // events for one aggregate produce a single (idempotent) file write.
    let mut rendered: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut drained = 0usize;

    for ev in &pending {
        // Render the aggregate's current state once per drain. Only work-item
        // aggregates have a renderable snapshot in the slice; any other
        // aggregate_type is still stamped (drained) so it does not wedge the
        // outbox, but produces no file.
        if ev.aggregate_type == "work_item" && !rendered.contains(&ev.aggregate_id) {
            render_work_item(pool, &ev.aggregate_id, export_root)
                .await
                .with_context(|| format!("rendering work_item '{}'", ev.aggregate_id))?;
            rendered.insert(ev.aggregate_id.clone());
        }

        // Stamp AFTER the render succeeds — an error above leaves this event
        // unexported for the next drain (recovery invariant P12). An RFC3339
        // timestamp via jiff matches the TEXT `exported_at` column.
        let now = jiff::Timestamp::now().to_string();
        sqlx::query!(
            r#"UPDATE events SET exported_at = ?2 WHERE id = ?1"#,
            ev.id,
            now,
        )
        .execute(pool)
        .await
        .with_context(|| format!("stamping exported_at for event '{}'", ev.id))?;

        drained += 1;
    }

    Ok(drained)
}

/// Re-materialise one work-item's current state to
/// `<export_root>/<kind>/<id>.toml` via serde + the `toml` crate, written
/// atomically (tempfile → fsync → rename).
///
/// If the work-item no longer exists (e.g. a future delete path), this is a
/// no-op — the event is still stamped by the caller so it leaves the outbox.
async fn render_work_item(
    pool: &SqlitePool,
    aggregate_id: &str,
    export_root: &Path,
) -> anyhow::Result<()> {
    let detail = match repo::get_work_item_detail(pool, aggregate_id).await {
        Ok(d) => d,
        // A missing aggregate is not an error for the drain: skip the file,
        // let the caller stamp the event so the outbox advances.
        Err(crate::error::AppError::NotFound(_)) => return Ok(()),
        Err(e) => return Err(anyhow::Error::new(e)),
    };

    let kind = detail.item.kind.clone();
    let dir = export_root.join(&kind);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating export dir {}", dir.display()))?;

    let path = dir.join(format!("{aggregate_id}.toml"));
    let body = toml::to_string_pretty(&detail)
        .with_context(|| format!("serialising work_item '{aggregate_id}' to TOML"))?;

    atomic_write(&path, body.as_bytes())
        .with_context(|| format!("atomically writing {}", path.display()))?;

    Ok(())
}

/// Atomic tempfile → fsync → rename write, porting the proven `tomlctl/io.rs`
/// idiom (the *approach*, not the crate). The temp file is created in the SAME
/// directory as the target so `persist` is a same-filesystem rename (no EXDEV);
/// the parent dir is fsynced after the rename so the dirent update is durable.
fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write as _;

    let parent: PathBuf = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut tmp = tempfile::NamedTempFile::new_in(&parent)
        .with_context(|| format!("creating temp file in {}", parent.display()))?;
    tmp.as_file_mut()
        .write_all(bytes)
        .with_context(|| format!("writing temp file for {}", path.display()))?;
    tmp.as_file()
        .sync_data()
        .with_context(|| format!("fsync temp file for {}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("atomic rename to {} failed: {}", path.display(), e.error))?;

    #[cfg(unix)]
    {
        let dir = std::fs::File::open(&parent)
            .with_context(|| format!("opening parent {} for fsync", parent.display()))?;
        dir.sync_all()
            .with_context(|| format!("fsync parent directory {}", parent.display()))?;
    }

    Ok(())
}

/// Resolve the background loop's export root from `LUMINA_EXPORT_ROOT`, falling
/// back to [`DEFAULT_EXPORT_ROOT`].
fn loop_export_root() -> PathBuf {
    std::env::var_os(EXPORT_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_EXPORT_ROOT))
}

/// Spawn the background export loop. Drains the outbox on a [`TICK_INTERVAL`]
/// tick and `select!`s the tick against a [`CancellationToken`] for graceful
/// shutdown.
///
/// `app.rs` calls `export::spawn(pool.clone())` and discards the return value
/// (fire-and-forget); the returned [`ExportHandle`] is for callers that want to
/// drive a clean shutdown (e.g. the e2e / a future signal handler). A drain
/// error is logged to stderr and the loop continues — a transient render or DB
/// hiccup must not silently kill the materialiser, and the failing events stay
/// in the outbox for the next tick (recovery invariant).
pub fn spawn(pool: Arc<SqlitePool>) -> ExportHandle {
    let token = CancellationToken::new();
    let loop_token = token.clone();
    let root = loop_export_root();

    let join = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        loop {
            tokio::select! {
                // Stop promptly on shutdown; do NOT run a final drain — the
                // outbox invariant means anything unexported re-drains on the
                // next start, so a torn shutdown loses no events.
                () = loop_token.cancelled() => break,
                _ = ticker.tick() => {
                    if let Err(e) = export_pending(&pool, &root).await {
                        eprintln!("lumina export: drain failed (events stay in outbox): {e:#}");
                    }
                }
            }
        }
    });

    ExportHandle { token, join }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;

    /// Read an event's `exported_at` (NULL → None).
    async fn exported_at(pool: &SqlitePool, aggregate_id: &str) -> Option<String> {
        sqlx::query!(
            r#"SELECT exported_at AS "exported_at?" FROM events WHERE aggregate_id = ?1"#,
            aggregate_id,
        )
        .fetch_one(pool)
        .await
        .unwrap()
        .exported_at
    }

    /// Count unexported events.
    async fn unexported_count(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM events WHERE exported_at IS NULL"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// (a) After a `create_work_item`, `export_pending` writes a matching
    /// `<root>/<kind>/<id>.toml` AND stamps the event's `exported_at`.
    #[tokio::test]
    async fn export_writes_snapshot_and_stamps() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        let id = repo::create_work_item(&pool, "project", None, "Root", Some("a body"))
            .await
            .expect("create project")
            .to_string();

        assert!(exported_at(&pool, &id).await.is_none(), "event starts unexported");

        let drained = export_pending(&pool, dir.path()).await.expect("drain");
        assert_eq!(drained, 1, "one event drained");

        let path = dir.path().join("project").join(format!("{id}.toml"));
        assert!(path.exists(), "snapshot file written at {}", path.display());

        // The snapshot parses and round-trips the work-item fields.
        let raw = std::fs::read_to_string(&path).expect("read snapshot");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse snapshot TOML");
        assert_eq!(
            parsed["item"]["id"].as_str(),
            Some(id.as_str()),
            "snapshot item.id matches"
        );
        assert_eq!(parsed["item"]["kind"].as_str(), Some("project"));
        assert_eq!(parsed["item"]["title"].as_str(), Some("Root"));

        // The event is now stamped (exported_at non-NULL).
        assert!(
            exported_at(&pool, &id).await.is_some(),
            "event stamped exported_at after drain"
        );
        assert_eq!(unexported_count(&pool).await, 0, "outbox empty");
    }

    /// (b) A SECOND `export_pending` is a no-op: returns 0 and leaves the file
    /// byte-identical (idempotent — nothing to drain, so nothing rewritten).
    #[tokio::test]
    async fn second_export_is_idempotent_noop() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        let id = repo::create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("create project")
            .to_string();

        let first = export_pending(&pool, dir.path()).await.expect("first drain");
        assert_eq!(first, 1);

        let path = dir.path().join("project").join(format!("{id}.toml"));
        let bytes_after_first = std::fs::read(&path).expect("read after first");

        let second = export_pending(&pool, dir.path()).await.expect("second drain");
        assert_eq!(second, 0, "no events left to drain");

        let bytes_after_second = std::fs::read(&path).expect("read after second");
        assert_eq!(
            bytes_after_first, bytes_after_second,
            "snapshot byte-identical after a no-op second drain"
        );
    }

    /// De-dup: multiple events for the SAME aggregate within one drain produce a
    /// single file write but stamp ALL of that aggregate's events.
    #[tokio::test]
    async fn multiple_events_one_aggregate_render_once_stamp_all() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        let id = repo::create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("create")
            .to_string();
        // Two more status mutations → three events total for one aggregate.
        repo::update_work_item_status(&pool, &id, "in-progress")
            .await
            .expect("status 1");
        repo::update_work_item_status(&pool, &id, "review")
            .await
            .expect("status 2");

        assert_eq!(unexported_count(&pool).await, 3, "three unexported events");

        let drained = export_pending(&pool, dir.path()).await.expect("drain");
        assert_eq!(drained, 3, "all three events stamped");
        assert_eq!(unexported_count(&pool).await, 0, "outbox empty");

        // The snapshot reflects the CURRENT (latest) status, not the create.
        let path = dir.path().join("project").join(format!("{id}.toml"));
        let raw = std::fs::read_to_string(&path).expect("read snapshot");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse");
        assert_eq!(parsed["item"]["status"].as_str(), Some("review"));
    }

    /// (c) Recovery: a mutation whose event was NOT exported (simulating a kill
    /// before export) re-drains on the next `export_pending` and produces the
    /// file. We simply do not call `export_pending` until after the mutation,
    /// mirroring "killed before the materialiser ran".
    #[tokio::test]
    async fn unexported_event_redrains_after_simulated_kill() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        // Build a legal chain so the leaf is a `task` (exercises a non-root kind
        // path for the export dir layout).
        let project = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .unwrap()
            .to_string();
        let epic = repo::create_work_item(&pool, "epic", Some(&project), "E", None)
            .await
            .unwrap()
            .to_string();
        let feature = repo::create_work_item(&pool, "feature", Some(&epic), "F", None)
            .await
            .unwrap()
            .to_string();
        let story = repo::create_work_item(&pool, "story", Some(&feature), "S", None)
            .await
            .unwrap()
            .to_string();
        let task = repo::create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .unwrap()
            .to_string();

        // "Killed before export": the events all sit unexported. Now restart →
        // export_pending drains them and produces every file, including the task.
        assert_eq!(unexported_count(&pool).await, 5);
        let drained = export_pending(&pool, dir.path()).await.expect("recovery drain");
        assert_eq!(drained, 5);

        let task_path = dir.path().join("task").join(format!("{task}.toml"));
        assert!(
            task_path.exists(),
            "the unexported task event re-drained to {}",
            task_path.display()
        );
        let raw = std::fs::read_to_string(&task_path).expect("read task snapshot");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse");
        assert_eq!(parsed["item"]["id"].as_str(), Some(task.as_str()));
        assert_eq!(parsed["item"]["kind"].as_str(), Some("task"));
    }

    /// `spawn` returns a handle whose `shutdown` stops the loop cleanly. We
    /// cannot use the paused test clock (the `tokio` `test-util` feature is not
    /// enabled and the dependency set is out of this task's cluster), so this is
    /// a structural smoke test: the loop's FIRST `interval` tick fires
    /// immediately (tokio `interval` yields its first tick without delay), so a
    /// brief real wait lets the loop drain once before we shut it down.
    #[tokio::test]
    async fn spawn_loop_drains_then_shuts_down() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
        let dir = tempfile::tempdir().expect("tempdir");
        // Point the loop at an isolated root via the env var.
        // SAFETY: single-threaded test runtime, no concurrent env access.
        unsafe { std::env::set_var(EXPORT_ROOT_ENV, dir.path()) };

        let id = repo::create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("create")
            .to_string();

        let handle = spawn(pool.clone());

        // `interval` fires its first tick immediately, so a short real sleep is
        // enough for the loop to drain the outbox once.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        handle.shutdown().await;

        unsafe { std::env::remove_var(EXPORT_ROOT_ENV) };

        let path = dir.path().join("project").join(format!("{id}.toml"));
        assert!(path.exists(), "loop drained the outbox at {}", path.display());
    }
}
