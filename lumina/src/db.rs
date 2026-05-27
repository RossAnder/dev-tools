//! Database pool construction and migration wiring (Task 2).
//!
//! `init` is the single entry point the composition root calls on startup: it
//! opens a `SqlitePool` (creating the file if missing, with foreign-key
//! enforcement on) and applies the embedded migrations. `connect_in_memory` is
//! a test/e2e helper that stands up a freshly-migrated `sqlite::memory:` pool.
//!
//! Only the RUNTIME query API (`sqlx::query` / `.bind` / `.execute`) is used in
//! this crate so far — the compile-checked `query!` / `query_as!` macros need
//! the `.sqlx` offline cache that Task 3 generates, so introducing them here
//! would break the offline build.

use std::str::FromStr as _;
use std::time::Duration;

use anyhow::Context as _;
use sqlx::{Sqlite, SqlitePool, Transaction};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};

/// SQLite busy-wait budget applied to every pooled connection. With WAL + an
/// upfront `BEGIN IMMEDIATE` (see [`begin_write`]) the only remaining source of
/// `SQLITE_BUSY` is a concurrent writer holding the RESERVED lock; the
/// busy-handler retries internally for up to this duration before surfacing the
/// error. Five seconds is generous enough to absorb any realistic burst from
/// the MCP write path + the export drain without masking a true deadlock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Open (creating if absent) the SQLite database at `database_url`, enable
/// foreign-key enforcement, and run all embedded migrations.
///
/// On-disk databases additionally opt into WAL journal mode and a 5-second
/// busy-timeout: WAL lets the export drain's auto-commit reads run concurrent
/// with the writer (instead of mutually excluding it under the default
/// rollback-journal mode), and the busy-timeout absorbs short bursts of writer
/// contention before bubbling `SQLITE_BUSY` to the caller. In-memory databases
/// (used by tests and the e2e thread) skip WAL — `:memory:` has no file to
/// spill the WAL sidecar to, so the mode is meaningless there.
///
/// `sqlx::migrate!("./migrations")` embeds the migration directory at compile
/// time (relative to the crate root / `CARGO_MANIFEST_DIR`); it needs only the
/// directory present on disk at build time, NOT a live database, so the crate
/// still compiles offline.
pub async fn init(database_url: &str) -> anyhow::Result<SqlitePool> {
    let mut connect_opts = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("parsing DATABASE_URL {database_url}"))?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT);

    if !is_in_memory(database_url) {
        connect_opts = connect_opts.journal_mode(SqliteJournalMode::Wal);
    }

    let pool = SqlitePool::connect_with(connect_opts)
        .await
        .with_context(|| format!("connecting to database at {database_url}"))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running embedded migrations")?;

    Ok(pool)
}

/// Open a SQLite write transaction with an upfront RESERVED lock — replaces
/// the default `pool.begin()` (which issues `BEGIN DEFERRED`) so writer
/// contention surfaces at begin-time rather than after the first statement.
///
/// Every mutation path in [`crate::repo`] uses this helper; read paths stay on
/// auto-commit (the pool directly) and do NOT need this, since IMMEDIATE on a
/// read-only call would pessimistically take the writer lock for nothing.
pub async fn begin_write(pool: &SqlitePool) -> Result<Transaction<'_, Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

/// True if `database_url` names an in-memory SQLite database. Accepts the bare
/// `:memory:` form and the URI form (`sqlite::memory:`, with or without query
/// params like `?cache=shared`).
fn is_in_memory(database_url: &str) -> bool {
    database_url == ":memory:" || database_url.starts_with("sqlite::memory:")
}

/// Stand up a freshly-migrated in-memory SQLite pool for tests / e2e.
///
/// Foreign-key enforcement is enabled so trigger / FK behaviour matches the
/// on-disk database. `sqlite::memory:` lives for the lifetime of the pool.
pub async fn connect_in_memory() -> anyhow::Result<SqlitePool> {
    init("sqlite::memory:").await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insert a `work_item` via the runtime query API with bound params.
    /// Returns the `execute` result so callers can assert success / failure.
    async fn insert_item(
        pool: &SqlitePool,
        id: &str,
        kind: &str,
        parent_id: Option<&str>,
        title: &str,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        sqlx::query(
            "INSERT INTO work_items (id, kind, parent_id, title, status) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(parent_id)
        .bind(title)
        .bind("open")
        .execute(pool)
        .await
    }

    /// Build the full legal chain project→epic→feature→story→task and assert a
    /// legal `task` under a `story` succeeds, while an illegal `task` under a
    /// `project` is rejected by the BEFORE INSERT trigger.
    #[tokio::test]
    async fn hierarchy_trigger_enforces_legal_parentage() {
        let pool = connect_in_memory()
            .await
            .expect("migrated in-memory pool");

        // ids are deterministic string literals here (UUIDv7 generation is the
        // repo layer's job in Task 3; the schema only requires TEXT).
        insert_item(&pool, "p1", "project", None, "Project")
            .await
            .expect("legal project under NULL parent");
        insert_item(&pool, "e1", "epic", Some("p1"), "Epic")
            .await
            .expect("legal epic under project");
        insert_item(&pool, "f1", "feature", Some("e1"), "Feature")
            .await
            .expect("legal feature under epic");
        insert_item(&pool, "s1", "story", Some("f1"), "Story")
            .await
            .expect("legal story under feature");

        // Legal: task under story SUCCEEDS.
        insert_item(&pool, "t1", "task", Some("s1"), "Task")
            .await
            .expect("legal task under story");

        // Illegal: task under project is REJECTED by the trigger.
        let illegal = insert_item(&pool, "t2", "task", Some("p1"), "Bad Task").await;
        assert!(
            illegal.is_err(),
            "expected the hierarchy trigger to reject a task under a project, got Ok"
        );

        // Illegal: non-project with NULL parent is REJECTED.
        let orphan = insert_item(&pool, "e2", "epic", None, "Orphan Epic").await;
        assert!(
            orphan.is_err(),
            "expected the hierarchy trigger to reject a non-project with a NULL parent, got Ok"
        );

        // Illegal: project with a non-NULL parent is REJECTED.
        let nested_project =
            insert_item(&pool, "p2", "project", Some("p1"), "Nested Project").await;
        assert!(
            nested_project.is_err(),
            "expected the hierarchy trigger to reject a project with a non-NULL parent, got Ok"
        );
    }

    /// The BEFORE UPDATE trigger guards re-parenting too: moving a `task` to sit
    /// directly under a `project` must be rejected.
    #[tokio::test]
    async fn hierarchy_trigger_enforces_legal_reparent() {
        let pool = connect_in_memory()
            .await
            .expect("migrated in-memory pool");

        insert_item(&pool, "p1", "project", None, "Project")
            .await
            .expect("legal project");
        insert_item(&pool, "e1", "epic", Some("p1"), "Epic")
            .await
            .expect("legal epic");
        insert_item(&pool, "f1", "feature", Some("e1"), "Feature")
            .await
            .expect("legal feature");
        insert_item(&pool, "s1", "story", Some("f1"), "Story")
            .await
            .expect("legal story");
        insert_item(&pool, "t1", "task", Some("s1"), "Task")
            .await
            .expect("legal task");

        // Re-parent the task under the project — illegal, must abort.
        let reparent = sqlx::query("UPDATE work_items SET parent_id = ? WHERE id = ?")
            .bind("p1")
            .bind("t1")
            .execute(&pool)
            .await;
        assert!(
            reparent.is_err(),
            "expected the BEFORE UPDATE trigger to reject re-parenting a task under a project, got Ok"
        );
    }
}
