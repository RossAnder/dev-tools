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
            aggregate_id   AS "aggregate_id!",
            event_type     AS "event_type!",
            payload        AS "payload?"
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

        // R2: an open-question resolution transitions its affected tasks
        // (→todo / →cancelled) WITHOUT emitting a per-task event (the
        // one-event-per-resolution invariant). So those tasks' snapshots would go
        // stale. Here, after the story aggregate render above, ALSO re-render each
        // task the resolution touched. The affected tasks are those blocked on the
        // resolved question; `question_id` rides in the event payload (added in
        // repo::resolve_open_question). Each render respects the per-drain
        // `rendered` dedup so a task touched twice in one batch is written once.
        let resolved_question_id = (ev.event_type == "open_question.resolved")
            .then_some(ev.payload.as_deref())
            .flatten()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v.get("question_id").and_then(|q| q.as_str()).map(str::to_owned));
        if let Some(question_id) = resolved_question_id {
            let affected: Vec<String> =
                sqlx::query_scalar("SELECT id FROM work_items WHERE blocked_by_question_id = ?")
                    .bind(&question_id)
                    .fetch_all(pool)
                    .await
                    .with_context(|| {
                        format!("selecting tasks affected by resolved question '{question_id}'")
                    })?;

            for task_id in affected {
                if !rendered.contains(&task_id) {
                    render_work_item(pool, &task_id, export_root)
                        .await
                        .with_context(|| {
                            format!("rendering resolution-affected task '{task_id}'")
                        })?;
                    rendered.insert(task_id);
                }
            }
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
///
/// # Whole-struct serialization (Task 6, part 1)
///
/// The snapshot serialises the ENTIRE [`WorkItemDetail`] — `item` (now carrying
/// `attributes`), `children`, `findings`, `context_blocks`, AND the ordered
/// `activity` — via [`toml::Table::try_from`]. Because the Task-3 attribute /
/// payload setters normalise every stored JSON value to a null-free object root,
/// the `serde_json::Value` fields can never present a `null`/scalar root to the
/// `toml` serializer, so the conversion cannot hit the toml crate's
/// non-table-root failure mode. Serialising the whole struct (rather than a
/// hand-built subset) means `attributes` + `activity` ride along for free.
///
/// # Tombstone (Task 6, part 2)
///
/// `repo::get_work_item_detail` deliberately does NOT filter `deleted_at`, so a
/// soft-deleted item still resolves here (it does not 404). The `deleted_at`
/// instant is NOT a field on `WorkItem`/`WorkItemDetail`, so the whole-struct
/// serialize alone does not surface it; we read it separately and, when present,
/// insert a TOP-LEVEL `deleted_at` key into the rendered table — a TOMBSTONE.
/// The snapshot file is REWRITTEN IN PLACE (same atomic path); it is NEVER
/// file-deleted, preserving the git-export audit trail per the soft-delete
/// decision. The drain treats a `work_item.deleted` event like any other
/// work-item event: it renders (writing the tombstone) and stamps `exported_at`.
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

    // Whole-struct serialize → a TOML table root (never a scalar/null root, so
    // the toml serializer cannot fail on the normalised JSON `Value` fields).
    let mut table = toml::Table::try_from(&detail)
        .with_context(|| format!("serialising work_item '{aggregate_id}' to TOML"))?;

    // Tombstone fold: if the row is soft-deleted, stamp a top-level `deleted_at`.
    // The `deleted_at` column is not carried on the detail struct, so it is read
    // here. This re-uses a query string already present in the committed `.sqlx`
    // offline cache (identical to the repo test helper), so it adds NO new cache
    // entry and forces NO `cargo sqlx prepare` regen — the parallel-safety
    // invariant for this task.
    if let Some(deleted_at) = soft_delete_marker(pool, aggregate_id).await? {
        table.insert("deleted_at".to_owned(), toml::Value::String(deleted_at));
    }

    let path = dir.join(format!("{aggregate_id}.toml"));
    let body = toml::to_string_pretty(&table)
        .with_context(|| format!("rendering work_item '{aggregate_id}' TOML"))?;

    atomic_write(&path, body.as_bytes())
        .with_context(|| format!("atomically writing {}", path.display()))?;

    Ok(())
}

/// Read a work item's `deleted_at` (the soft-delete tombstone instant), `None`
/// if the row is live. Returns `Ok(None)` for a missing row too (the caller has
/// already resolved the detail; this is purely the deletion marker).
///
/// The query string is BYTE-IDENTICAL to the `repo` module's test-helper read,
/// whose hash is already in the committed `.sqlx/` cache, so this `query!`
/// resolves offline against the existing entry and triggers NO cache regen.
async fn soft_delete_marker(
    pool: &SqlitePool,
    aggregate_id: &str,
) -> anyhow::Result<Option<String>> {
    let row = sqlx::query!(
        r#"SELECT deleted_at AS "deleted_at?" FROM work_items WHERE id = ?1"#,
        aggregate_id,
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("reading deleted_at for work_item '{aggregate_id}'"))?;

    Ok(row.and_then(|r| r.deleted_at))
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
        // migration-0010 valid chain: epic outcome, focus shape, epic close-criterion.
        let epic = repo::create_work_item_full(
            &pool, "epic", Some(&project), "E", None, None, Some("the epic outcome"), None,
        )
        .await
        .unwrap()
        .to_string();
        repo::add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .unwrap();
        let feature = repo::create_work_item_full(
            &pool, "focus", Some(&epic), "FO", None, None, None, Some("vertical-slice"),
        )
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
        // 6 events: project/epic/focus/story/task creates + the epic close-criterion.
        assert_eq!(unexported_count(&pool).await, 6);
        let drained = export_pending(&pool, dir.path()).await.expect("recovery drain");
        assert_eq!(drained, 6);

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

    /// (Task 6, part 1) After setting NESTED-object attributes + appending an
    /// activity entry, the drained snapshot round-trips the nested `attributes`
    /// object (proving TOML-serialization safety over a `serde_json::Value`
    /// object root) and contains the activity entry.
    #[tokio::test]
    async fn export_folds_attributes_and_activity() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        // Build a legal chain to a `task` — the `task` kind accepts a NESTED
        // object attribute (`dispatch`), which exercises the nested-object path.
        let project = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .unwrap()
            .to_string();
        // migration-0010 valid chain: epic outcome, focus shape, epic close-criterion.
        let epic = repo::create_work_item_full(
            &pool, "epic", Some(&project), "E", None, None, Some("the epic outcome"), None,
        )
        .await
        .unwrap()
        .to_string();
        repo::add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .unwrap();
        let feature = repo::create_work_item_full(
            &pool, "focus", Some(&epic), "FO", None, None, None, Some("vertical-slice"),
        )
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

        // Nested-object attribute set (the `set_story_plan`-equivalent path).
        repo::set_work_item_attributes(
            &pool,
            &task,
            &serde_json::json!({ "dispatch": { "agent": "deep", "level": "L3" } }),
        )
        .await
        .expect("set nested attribute");

        // An activity entry.
        repo::append_activity(&pool, &task, "execution", Some("bob"), "ran the task", None, None)
            .await
            .expect("append activity");

        let drained = export_pending(&pool, dir.path()).await.expect("drain");
        assert_eq!(
            drained, 8,
            "5 creates + 1 epic close-criterion + 1 attr-update + 1 activity event"
        );

        let path = dir.path().join("task").join(format!("{task}.toml"));
        let raw = std::fs::read_to_string(&path).expect("read snapshot");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse snapshot TOML");

        // The NESTED attributes object round-trips intact.
        let dispatch = &parsed["item"]["attributes"]["dispatch"];
        assert_eq!(dispatch["agent"].as_str(), Some("deep"), "nested attr round-trips");
        assert_eq!(dispatch["level"].as_str(), Some("L3"));

        // The ordered activity entry is present.
        let activity = parsed["activity"].as_array().expect("activity array");
        assert_eq!(activity.len(), 1, "one activity entry serialised");
        assert_eq!(activity[0]["summary"].as_str(), Some("ran the task"));
        assert_eq!(activity[0]["entry_kind"].as_str(), Some("execution"));
        assert_eq!(activity[0]["seq"].as_integer(), Some(1));

        // A live item carries NO tombstone marker.
        assert!(
            parsed.get("deleted_at").is_none(),
            "a live item snapshot has no deleted_at tombstone"
        );
    }

    /// (Task 6, part 2) After a soft-`delete_work_item` + drain, the snapshot
    /// STILL EXISTS (never file-deleted — audit trail) and carries a TOP-LEVEL
    /// `deleted_at` tombstone. A second drain is a no-op leaving it byte-identical.
    #[tokio::test]
    async fn export_writes_tombstone_on_soft_delete_and_second_drain_is_noop() {
        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        let id = repo::create_work_item(&pool, "project", None, "Doomed", None)
            .await
            .expect("create")
            .to_string();

        // First drain: the live snapshot (no tombstone).
        let drained = export_pending(&pool, dir.path()).await.expect("drain create");
        assert_eq!(drained, 1);
        let path = dir.path().join("project").join(format!("{id}.toml"));
        let live = toml::from_str::<toml::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(live.get("deleted_at").is_none(), "live snapshot has no tombstone");

        // Soft-delete, then drain the `work_item.deleted` event.
        repo::delete_work_item(&pool, &id).await.expect("soft delete");
        let drained = export_pending(&pool, dir.path()).await.expect("drain delete");
        assert_eq!(drained, 1, "the work_item.deleted event drains (not skipped)");

        // The file STILL EXISTS — never file-deleted — and carries the tombstone.
        assert!(path.exists(), "tombstoned snapshot still on disk (audit trail)");
        let raw = std::fs::read_to_string(&path).expect("read tombstone");
        let parsed: toml::Value = toml::from_str(&raw).expect("parse tombstone TOML");
        let tombstone = parsed
            .get("deleted_at")
            .and_then(|v| v.as_str())
            .expect("top-level deleted_at tombstone present");
        assert!(!tombstone.is_empty(), "deleted_at carries a timestamp");
        // The item body is still rendered alongside the tombstone (preserved).
        assert_eq!(parsed["item"]["id"].as_str(), Some(id.as_str()));

        // A SECOND drain is a no-op: nothing left in the outbox, file untouched.
        let bytes_before = std::fs::read(&path).expect("read before second drain");
        let second = export_pending(&pool, dir.path()).await.expect("second drain");
        assert_eq!(second, 0, "no events left to drain after the delete");
        let bytes_after = std::fs::read(&path).expect("read after second drain");
        assert_eq!(bytes_before, bytes_after, "tombstone byte-identical after no-op drain");
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

    /// (Task 7, part 1) After adding a research note + an acceptance criterion +
    /// an open question (with options) to real items via `repo::*`, the drained
    /// snapshot round-trips all three new child collections, and a STORY snapshot
    /// carries the `relevance` column. This proves the new `WorkItem` columns +
    /// child collections ride along for FREE via the whole-struct
    /// `toml::Table::try_from(&detail)` — no export-side reshape needed.
    #[tokio::test]
    async fn export_folds_new_columns_and_child_collections() {
        use crate::domain::{ClosureGate, Relevance};

        let pool = connect_in_memory().await.expect("pool");
        let dir = tempfile::tempdir().expect("tempdir");

        // Legal chain down to a story (relevance + open-question scope) and a task
        // (acceptance-criteria scope).
        let project = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .unwrap()
            .to_string();
        // migration-0010 valid chain: epic outcome, focus shape, epic close-criterion.
        let epic = repo::create_work_item_full(
            &pool, "epic", Some(&project), "E", None, None, Some("the epic outcome"), None,
        )
        .await
        .unwrap()
        .to_string();
        repo::add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .unwrap();
        let feature = repo::create_work_item_full(
            &pool, "focus", Some(&epic), "FO", None, None, None, Some("vertical-slice"),
        )
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

        // Story-scoped: an explicit relevance (overriding the create default) and a
        // closure gate (a story-only column) — both ride the WorkItem scalars.
        repo::set_relevance(&pool, &story, Relevance::Active)
            .await
            .expect("set relevance");
        repo::set_closure_gate(&pool, &story, ClosureGate::Hard)
            .await
            .expect("set closure gate");

        // Story-scoped open question + two answer-option branches (the nested
        // array-of-tables — the highest-risk export path).
        let question = repo::add_open_question(&pool, &story, "DB engine?")
            .await
            .expect("add open question")
            .to_string();
        repo::add_question_option(&pool, &question, "sqlite", Some("embedded"))
            .await
            .expect("add option A");
        repo::add_question_option(&pool, &question, "postgres", None)
            .await
            .expect("add option B");

        // Task-scoped: an acceptance criterion + a research note.
        repo::add_acceptance_criterion(&pool, &task, "It compiles offline")
            .await
            .expect("add criterion");
        repo::add_research_note(
            &pool,
            &task,
            "Hybrid storage holds",
            Some("columns for queryable axes"),
            Some("high"),
            Some("storage"),
            Some("plan"),
        )
        .await
        .expect("add research note");

        let drained = export_pending(&pool, dir.path()).await.expect("drain");
        assert!(drained > 0, "events drained");

        // --- Story snapshot: carries the new `relevance` (and `closure_gate`) column.
        let story_path = dir.path().join("story").join(format!("{story}.toml"));
        let story_raw = std::fs::read_to_string(&story_path).expect("read story snapshot");
        let story_parsed: toml::Value = toml::from_str(&story_raw).expect("parse story TOML");
        assert_eq!(
            story_parsed["item"]["relevance"].as_str(),
            Some("active"),
            "story snapshot carries relevance"
        );
        assert_eq!(
            story_parsed["item"]["closure_gate"].as_str(),
            Some("hard"),
            "story snapshot carries closure_gate"
        );

        // The story's open_questions(+options) round-trip as array-of-tables.
        let questions = story_parsed["open_questions"]
            .as_array()
            .expect("open_questions array");
        assert_eq!(questions.len(), 1, "one open question folded");
        assert_eq!(questions[0]["question"].as_str(), Some("DB engine?"));
        let options = questions[0]["options"].as_array().expect("options array");
        assert_eq!(options.len(), 2, "two option branches folded");
        assert_eq!(options[0]["label"].as_str(), Some("sqlite"));
        assert_eq!(options[1]["label"].as_str(), Some("postgres"));

        // --- Task snapshot: carries acceptance_criteria + research_notes.
        let task_path = dir.path().join("task").join(format!("{task}.toml"));
        let task_raw = std::fs::read_to_string(&task_path).expect("read task snapshot");
        let task_parsed: toml::Value = toml::from_str(&task_raw).expect("parse task TOML");

        let criteria = task_parsed["acceptance_criteria"]
            .as_array()
            .expect("acceptance_criteria array");
        assert_eq!(criteria.len(), 1, "one acceptance criterion folded");
        assert_eq!(criteria[0]["text"].as_str(), Some("It compiles offline"));

        let notes = task_parsed["research_notes"]
            .as_array()
            .expect("research_notes array");
        assert_eq!(notes.len(), 1, "one research note folded");
        assert_eq!(notes[0]["summary"].as_str(), Some("Hybrid storage holds"));
        assert_eq!(notes[0]["confidence"].as_str(), Some("high"));
    }

    /// (Task 7, part 2 — the tables-last RUNTIME gate) A round-trip
    /// `toml::to_string_pretty` over a hand-built `WorkItemDetail` carrying a
    /// POPULATED `open_questions` (each with a nested `options` array-of-tables).
    /// This is the highest-risk path: a scalar declared AFTER a `Vec` on
    /// `WorkItem`/`OpenQuestion`/`WorkItemDetail` would make the serializer fail
    /// at runtime with `ValueAfterTable`. This drives the SAME `toml` call
    /// `render_work_item` uses, so it gates the declaration order in `domain.rs`
    /// without touching the DB. If this fails with `ValueAfterTable`, the fix
    /// belongs in `domain.rs` declaration order (NOT this file) — escalate.
    #[test]
    fn open_questions_round_trip_gates_tables_last_ordering() {
        use crate::domain::{OpenQuestion, QuestionOption, WorkItem, WorkItemDetail};

        let item = WorkItem {
            id: "wi-1".to_owned(),
            kind: "story".to_owned(),
            parent_id: Some("wi-0".to_owned()),
            title: "S".to_owned(),
            body: Some("a body".to_owned()),
            status: "open".to_owned(),
            position: Some(1),
            attributes: Some(serde_json::json!({ "k": "v" })),
            relevance: Some("active".to_owned()),
            effort: Some("m".to_owned()),
            complexity: Some("high".to_owned()),
            origin: Some("plan".to_owned()),
            closure_gate: Some("hard".to_owned()),
            blocked_by_question_id: None,
            enabling_option_id: None,
            task_kind: None,
            tier: None,
            shape: None,
            created_at: "2026-05-22T00:00:00Z".to_owned(),
            updated_at: "2026-05-22T00:00:00Z".to_owned(),
        };

        let question = OpenQuestion {
            id: "q-1".to_owned(),
            story_id: "wi-1".to_owned(),
            seq: 1,
            question: "DB engine?".to_owned(),
            status: Some("open".to_owned()),
            answer: None,
            chosen_option_id: None,
            decided_at: None,
            decided_by: None,
            prompting_finding_id: None,
            prompting_note_id: None,
            created_at: "2026-05-22T00:00:00Z".to_owned(),
            options: vec![
                QuestionOption {
                    id: "opt-1".to_owned(),
                    question_id: "q-1".to_owned(),
                    seq: 1,
                    label: "sqlite".to_owned(),
                    detail: Some("embedded".to_owned()),
                    created_at: "2026-05-22T00:00:00Z".to_owned(),
                },
                QuestionOption {
                    id: "opt-2".to_owned(),
                    question_id: "q-1".to_owned(),
                    seq: 2,
                    label: "postgres".to_owned(),
                    detail: None,
                    created_at: "2026-05-22T00:00:00Z".to_owned(),
                },
            ],
        };

        let detail = WorkItemDetail {
            item,
            children: vec![],
            findings: vec![],
            context_blocks: vec![],
            activity: vec![],
            acceptance_criteria: vec![],
            research_notes: vec![],
            open_questions: vec![question],
            repo_links: vec![],
            risks: vec![],
            rejected_alternatives: vec![],
            task_dependencies: vec![],
        };

        // The SAME conversion render_work_item performs: whole-struct → table →
        // to_string_pretty. A tables-last violation surfaces HERE as an Err.
        let table = toml::Table::try_from(&detail)
            .expect("WorkItemDetail serialises to a TOML table (no scalar/null root)");
        let body = toml::to_string_pretty(&table).unwrap_or_else(|e| {
            panic!(
                "tables-last gate FAILED — toml::to_string_pretty errored ({e}); \
                 a scalar is declared after a Vec on WorkItem/OpenQuestion/WorkItemDetail. \
                 The fix belongs in domain.rs declaration order (cross-file — escalate)."
            )
        });

        // Round-trips back and the nested options array survived.
        let parsed: toml::Value = toml::from_str(&body).expect("re-parse rendered TOML");
        let opts = parsed["open_questions"][0]["options"]
            .as_array()
            .expect("nested options array round-trips");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0]["label"].as_str(), Some("sqlite"));
        // A scalar that follows the nested options in WorkItemDetail's render order
        // is still present (proves no truncation at the tables boundary).
        assert_eq!(parsed["item"]["relevance"].as_str(), Some("active"));
    }
}
