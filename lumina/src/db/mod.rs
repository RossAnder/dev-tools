//! Database pool construction and migration wiring (Task 2).
//!
//! `init` is the single entry point the composition root calls on startup: it
//! opens a `SqlitePool` (creating the file if missing, with foreign-key
//! enforcement on) and applies the embedded migrations. `connect_in_memory` is
//! a test/e2e helper that stands up a freshly-migrated `sqlite::memory:` pool.
//!
//! This crate uses ONLY the runtime `sqlx::query*` API (`sqlx::query` / `.bind`
//! / `.execute`) behind the `DbClient` / `DbTx` seam. Part A removed the
//! compile-time `query!` / `query_as!` macros and deleted the `.sqlx` offline
//! cache, so there is no offline cache and no `cargo sqlx prepare` step. The
//! `migrate` feature is retained for the compile-time `sqlx::migrate!` in
//! [`init`].

mod client;
mod erased;

pub use client::*;
pub use erased::*;

use std::str::FromStr as _;
use std::time::Duration;

use anyhow::Context as _;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// SQLite busy-wait budget applied to every pooled connection. With WAL + an
/// upfront `BEGIN IMMEDIATE` (see [`begin_write`]) the only remaining source of
/// `SQLITE_BUSY` is a concurrent writer holding the RESERVED lock; the
/// busy-handler retries internally for up to this duration before surfacing the
/// error. Five seconds is generous enough to absorb any realistic burst from
/// the MCP write path + the export drain without masking a true deadlock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Max pooled connections for the ON-DISK database (O5). Sized to absorb
/// `get_work_item_detail`'s concurrent child-read fan-out (O1 issues ~11 reads
/// together via `tokio::try_join!`) plus headroom for the export drain and
/// concurrent SPA/MCP readers. Under WAL, SQLite serialises writes on a single
/// RESERVED lock regardless of pool size, so this bound governs READ concurrency
/// (each reader runs on its own connection); surplus readers queue cheaply on
/// `pool.acquire()` with no `SQLITE_BUSY`. The sqlx default ceiling (10) sits
/// just below a single detail fan-out, so we raise it explicitly. Retune with a
/// pool-occupancy benchmark if read latency under concurrent callers warrants it.
///
/// In-memory pools (tests / e2e) deliberately keep the sqlx default: a
/// `sqlite::memory:` pool shares one backing database and the e2e detail thread
/// already exercises the O1 fan-out at the default size, so there is nothing to
/// raise there.
const MAX_CONNECTIONS: u32 = 16;

/// Open (creating if absent) the SQLite database at `database_url`, enable
/// foreign-key enforcement, and run all embedded migrations.
///
/// On-disk databases additionally opt into WAL journal mode, `synchronous=NORMAL`,
/// and a 5-second busy-timeout: WAL lets the export drain's auto-commit reads run
/// concurrent with the writer (instead of mutually excluding it under the default
/// rollback-journal mode), `synchronous=NORMAL` is the SQLite-recommended WAL
/// pairing that drops the default one-fsync-per-commit (losing at most the last
/// transaction on hard power loss, which the idempotent git-export drain re-renders),
/// and the busy-timeout absorbs short bursts of writer contention before bubbling
/// `SQLITE_BUSY` to the caller. In-memory databases
/// (used by tests and the e2e thread) skip WAL — `:memory:` has no file to
/// spill the WAL sidecar to, so the mode is meaningless there.
///
/// `sqlx::migrate!("./migrations")` embeds the migration directory at compile
/// time (relative to the crate root / `CARGO_MANIFEST_DIR`); it needs only the
/// directory present on disk at build time, NOT a live database, so the crate
/// still compiles offline.
pub async fn init(database_url: &str) -> anyhow::Result<SqlitePool> {
    let in_memory = is_in_memory(database_url);

    let mut connect_opts = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("parsing DATABASE_URL {database_url}"))?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT);

    if !in_memory {
        connect_opts = connect_opts
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
    }

    // On-disk: size the pool explicitly for the read fan-out (see MAX_CONNECTIONS).
    // In-memory: keep the sqlx default — `SqlitePoolOptions::new()` matches the
    // old `SqlitePool::connect_with`, so test/e2e behaviour is unchanged.
    let mut pool_opts = SqlitePoolOptions::new();
    if !in_memory {
        pool_opts = pool_opts.max_connections(MAX_CONNECTIONS);
    }

    let pool = pool_opts
        .connect_with(connect_opts)
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

/// Canonical compiling exemplar of the multi-column row-mapper recipe above.
///
/// Reads three columns from `work_items`: `id` (NOT NULL TEXT -> `String`),
/// `body` (nullable TEXT -> `Option<String>`), and `position` (nullable INTEGER,
/// read here as a NOT-NULL-after-COALESCE `i64` in the test SELECT). It is wired
/// into the `transparent_row_*` tests to prove a generic-R struct flows through
/// BOTH `query_all` (pool) and `tx_query_all` (`&mut dyn DbTx`) unchanged.
///
/// A4+ copies the `impl` shape, not this struct.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExemplarRow {
    pub id: String,
    /// Nullable column — note the `Option<String>` field + matching bound.
    pub body: Option<String>,
    pub position: i64,
}

#[cfg(test)]
impl<'r, R> sqlx::FromRow<'r, R> for ExemplarRow
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ExemplarRow {
            id: row.try_get("id")?,
            body: row.try_get("body")?,
            position: row.try_get("position")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;
    use crate::error::AppError;

    /// Insert a `work_item` via the runtime query API with bound params.
    /// Returns the `execute` result so callers can assert success / failure.
    async fn insert_item(
        pool: &SqlitePool,
        id: &str,
        kind: &str,
        parent_id: Option<&str>,
        title: &str,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        // migration-0010: a `focus` row must carry a shape (enforced by the
        // focus-shape DB trigger), so seed a representative shape for focus
        // rows; other kinds leave `shape` NULL (allowed).
        let shape: Option<&str> = if kind == "focus" {
            Some("vertical-slice")
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO work_items (id, kind, parent_id, title, status, shape) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(parent_id)
        .bind(title)
        .bind("open")
        .bind(shape)
        .execute(pool)
        .await
    }

    /// Build the full legal chain project→epic→focus→story→task and assert a
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
        insert_item(&pool, "f1", "focus", Some("e1"), "Focus")
            .await
            .expect("legal focus under epic");
        insert_item(&pool, "s1", "story", Some("f1"), "Story")
            .await
            .expect("legal story under focus");

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
        insert_item(&pool, "f1", "focus", Some("e1"), "Focus")
            .await
            .expect("legal focus");
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

    // -----------------------------------------------------------------------
    // Seam tests (Task A1) — exercise DbClient / DbTx / AnyPool against an
    // in-memory pool.
    // -----------------------------------------------------------------------

    /// A minimal hand-written `FromRow` row mapper proving the generic-over-`R`
    /// pattern the A2 wave generalises. Decodes `(id, kind, title)` from a
    /// `work_items` SELECT.
    #[derive(Debug, PartialEq, Eq)]
    struct ItemRow {
        id: String,
        kind: String,
        title: String,
    }

    impl<'r, R> sqlx::FromRow<'r, R> for ItemRow
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(ItemRow {
                id: row.try_get(0)?,
                kind: row.try_get(1)?,
                title: row.try_get(2)?,
            })
        }
    }

    const SELECT_ITEM: &str =
        "SELECT id, kind, title FROM work_items WHERE id = $1";
    const SELECT_TITLES: &str =
        "SELECT id, kind, title FROM work_items WHERE kind = $1 ORDER BY id";
    const INSERT_PROJECT: &str =
        "INSERT INTO work_items (id, kind, parent_id, title, status, shape) \
         VALUES ($1, 'project', NULL, $2, 'open', NULL)";

    /// `DbClient::execute` runs a parameterised INSERT and reports the affected
    /// row count.
    #[tokio::test]
    async fn dbclient_execute_inserts_row() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        assert_eq!(db.backend(), Backend::Sqlite);

        let n = db
            .execute(INSERT_PROJECT, args!["px".to_owned(), "Proj X".to_owned()])
            .await
            .expect("insert");
        assert_eq!(n, 1, "one row affected");
    }

    /// `query_one` / `query_opt` / `query_all` read rows back via a hand-written
    /// `FromRow`. `query_one` on a missing row surfaces `NotFound`.
    #[tokio::test]
    async fn dbclient_query_variants_read_rows() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        db.execute(INSERT_PROJECT, args!["p1".to_owned(), "First".to_owned()])
            .await
            .expect("insert p1");
        db.execute(INSERT_PROJECT, args!["p2".to_owned(), "Second".to_owned()])
            .await
            .expect("insert p2");

        // query_one — exactly one row.
        let one: ItemRow = db
            .query_one(SELECT_ITEM, args!["p1".to_owned()])
            .await
            .expect("query_one p1");
        assert_eq!(
            one,
            ItemRow {
                id: "p1".to_owned(),
                kind: "project".to_owned(),
                title: "First".to_owned(),
            }
        );

        // query_opt — present and absent.
        let some: Option<ItemRow> = db
            .query_opt(SELECT_ITEM, args!["p2".to_owned()])
            .await
            .expect("query_opt p2");
        assert_eq!(some.map(|r| r.title), Some("Second".to_owned()));
        let none: Option<ItemRow> = db
            .query_opt(SELECT_ITEM, args!["nope".to_owned()])
            .await
            .expect("query_opt absent");
        assert!(none.is_none());

        // query_all — both projects, ordered.
        let all: Vec<ItemRow> = db
            .query_all(SELECT_TITLES, args!["project".to_owned()])
            .await
            .expect("query_all projects");
        let titles: Vec<_> = all.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["First", "Second"]);

        // query_one on a missing row → NotFound (not Db 500).
        let missing: Result<ItemRow, _> = db.query_one(SELECT_ITEM, args!["ghost".to_owned()]).await;
        assert!(
            matches!(missing, Err(AppError::NotFound(_))),
            "missing single-row read maps to NotFound, got {missing:?}"
        );
    }

    /// A tuple `FromRow` (sqlx built-in) also decodes through the seam, proving
    /// the bound is not specific to hand-written structs.
    #[tokio::test]
    async fn dbclient_tuple_from_row() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        db.execute(INSERT_PROJECT, args!["pt".to_owned(), "Tup".to_owned()])
            .await
            .expect("insert");
        let row: (String, String) = db
            .query_one(
                "SELECT id, title FROM work_items WHERE id = $1",
                args!["pt".to_owned()],
            )
            .await
            .expect("tuple query_one");
        assert_eq!(row, ("pt".to_owned(), "Tup".to_owned()));
    }

    /// `begin()` issues BEGIN IMMEDIATE; a write through the tx + `commit()`
    /// persists, visible on a subsequent pool read.
    #[tokio::test]
    async fn dbtx_begin_write_commit_persists() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();

        let mut tx = db.begin().await.expect("begin");
        let n = tx
            .execute(INSERT_PROJECT, args!["tx1".to_owned(), "Committed".to_owned()])
            .await
            .expect("tx insert");
        assert_eq!(n, 1);
        // Read-through the SAME tx via the free helper (object-safe path).
        let in_tx: Option<ItemRow> = tx_query_opt(tx.as_mut(), SELECT_ITEM, args!["tx1".to_owned()])
            .await
            .expect("read in tx");
        assert_eq!(in_tx.map(|r| r.title), Some("Committed".to_owned()));
        tx.commit().await.expect("commit");

        // Visible on the pool after commit.
        let after: ItemRow = db
            .query_one(SELECT_ITEM, args!["tx1".to_owned()])
            .await
            .expect("post-commit read");
        assert_eq!(after.title, "Committed");
    }

    /// Dropping a tx WITHOUT commit rolls back — nothing persists.
    #[tokio::test]
    async fn dbtx_rollback_on_drop() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        {
            let mut tx = db.begin().await.expect("begin");
            tx.execute(INSERT_PROJECT, args!["rb".to_owned(), "Rolled".to_owned()])
                .await
                .expect("tx insert");
            // tx dropped here without commit.
        }
        let gone: Option<ItemRow> = db
            .query_opt(SELECT_ITEM, args!["rb".to_owned()])
            .await
            .expect("post-rollback read");
        assert!(gone.is_none(), "uncommitted write must not persist");
    }

    /// The `record_event` coercion contract: a raw `Transaction<'_, Sqlite>`
    /// satisfies `&mut dyn DbTx`. This mirrors how A3 will call
    /// `record_event(&mut tx, …)` with no caller changes. We obtain a real
    /// transaction from `begin_write`, coerce `&mut tx` to `&mut dyn DbTx`, and
    /// write through it — proving the unsizing the A3 edit relies on.
    #[tokio::test]
    async fn transaction_coerces_to_dyn_dbtx() {
        let pool = connect_in_memory().await.expect("pool");
        let mut tx: Transaction<'_, Sqlite> = begin_write(&pool).await.expect("begin_write");

        // The coercion under test: `&mut tx` unsizes to `&mut dyn DbTx`.
        let dyn_tx: &mut dyn DbTx = &mut tx;
        let n = dyn_tx
            .execute(INSERT_PROJECT, args!["co".to_owned(), "Coerced".to_owned()])
            .await
            .expect("write through &mut dyn DbTx");
        assert_eq!(n, 1);
        // Read it back through the same dyn ref.
        let row: ItemRow = tx_query_one(dyn_tx, SELECT_ITEM, args!["co".to_owned()])
            .await
            .expect("read through dyn tx");
        assert_eq!(row.title, "Coerced");

        tx.commit().await.expect("commit raw tx");
        let after: ItemRow = AnyPool::from(pool)
            .query_one(SELECT_ITEM, args!["co".to_owned()])
            .await
            .expect("post-commit");
        assert_eq!(after.title, "Coerced");
    }

    /// Compile-time proof that the seam is object-safe: `Box<dyn DbTx>` and
    /// `&mut dyn DbTx` are well-formed types. (If `DbTx` grew a generic method
    /// this would fail to compile.)
    #[tokio::test]
    async fn dbtx_is_object_safe() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let boxed: Box<dyn DbTx + '_> = db.begin().await.expect("begin");
        let _: Box<dyn DbTx + '_> = boxed;
        // Also exercise &mut dyn through as_mut already covered above.
    }

    // -----------------------------------------------------------------------
    // Recipe tests (Task A2) — the canonical row-mapper + scalar fetch path
    // that A4+ copies to ~142 sites. Reuses A1's in-memory-pool pattern.
    // -----------------------------------------------------------------------

    // Seeds with a NON-NULL and a NULL `body`, plus a `position`, so the
    // exemplar's `Option<String>` column is exercised on both sides of NULL.
    const INSERT_FULL: &str = "INSERT INTO work_items \
         (id, kind, parent_id, title, status, shape, body, position) \
         VALUES ($1, 'project', NULL, $2, 'open', NULL, $3, $4)";
    // The exemplar SELECT: nullable `body` passes through as-is; `position`
    // (nullable INTEGER) is COALESCEd to a NOT-NULL i64 for the `i64` field.
    const SELECT_EXEMPLAR: &str =
        "SELECT id, body, COALESCE(position, 0) AS position FROM work_items \
         WHERE id = $1";
    const SELECT_EXEMPLARS: &str =
        "SELECT id, body, COALESCE(position, 0) AS position FROM work_items \
         WHERE kind = 'project' ORDER BY id";

    /// (a) Single-column scalar reads via the `Scalar<T>` path: a COUNT(*) i64
    /// and a TEXT String, through BOTH the pool helpers and the tx helpers.
    /// Also proves `bool` decodes from a SQLite INTEGER column.
    #[tokio::test]
    async fn scalar_path_reads_i64_string_bool() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        db.execute(INSERT_PROJECT, args!["s1".to_owned(), "Alpha".to_owned()])
            .await
            .expect("insert s1");
        db.execute(INSERT_PROJECT, args!["s2".to_owned(), "Beta".to_owned()])
            .await
            .expect("insert s2");

        // i64 COUNT(*) through the pool.
        let count: i64 = scalar_one(&db, "SELECT COUNT(*) FROM work_items", args![])
            .await
            .expect("count");
        assert_eq!(count, 2);

        // String single column through the pool.
        let title: String = scalar_one(
            &db,
            "SELECT title FROM work_items WHERE id = $1",
            args!["s1".to_owned()],
        )
        .await
        .expect("title");
        assert_eq!(title, "Alpha");

        // scalar_opt: present + absent.
        let some: Option<String> = scalar_opt(
            &db,
            "SELECT title FROM work_items WHERE id = $1",
            args!["s2".to_owned()],
        )
        .await
        .expect("scalar_opt present");
        assert_eq!(some, Some("Beta".to_owned()));
        let none: Option<String> = scalar_opt(
            &db,
            "SELECT title FROM work_items WHERE id = $1",
            args!["ghost".to_owned()],
        )
        .await
        .expect("scalar_opt absent");
        assert!(none.is_none());

        // scalar_all: ordered titles.
        let titles: Vec<String> = scalar_all(
            &db,
            "SELECT title FROM work_items WHERE kind = 'project' ORDER BY id",
            args![],
        )
        .await
        .expect("scalar_all");
        assert_eq!(titles, vec!["Alpha".to_owned(), "Beta".to_owned()]);

        // bool decodes from a SQLite INTEGER literal (1 = true, 0 = false).
        let yes: bool = scalar_one(&db, "SELECT 1", args![]).await.expect("bool 1");
        let no: bool = scalar_one(&db, "SELECT 0", args![]).await.expect("bool 0");
        assert!(yes && !no, "bool round-trips through SQLite INTEGER");

        // scalar_one on a missing row → NotFound (mirrors query_one semantics).
        let missing: Result<String, _> = scalar_one(
            &db,
            "SELECT title FROM work_items WHERE id = $1",
            args!["nope".to_owned()],
        )
        .await;
        assert!(
            matches!(missing, Err(AppError::NotFound(_))),
            "missing scalar maps to NotFound, got {missing:?}"
        );

        // Scalar path through a TRANSACTION (&mut dyn DbTx), proving it rides the
        // object-safe tx helpers too.
        let mut tx = db.begin().await.expect("begin");
        let count_in_tx: i64 = tx_scalar_one(tx.as_mut(), "SELECT COUNT(*) FROM work_items", args![])
            .await
            .expect("tx count");
        assert_eq!(count_in_tx, 2);
        let tx_titles: Vec<String> = tx_scalar_all(
            tx.as_mut(),
            "SELECT title FROM work_items WHERE kind = 'project' ORDER BY id",
            args![],
        )
        .await
        .expect("tx scalar_all");
        assert_eq!(tx_titles, vec!["Alpha".to_owned(), "Beta".to_owned()]);
        tx.commit().await.expect("commit");
    }

    /// (b) The generic-R [`ExemplarRow`] (≥2 columns incl. a nullable
    /// `Option<String>`) flows through BOTH `query_all`/`query_one` (pool) and
    /// `tx_query_all`/`tx_query_one` (`&mut dyn DbTx`) UNCHANGED — proving a
    /// single generic-over-`R` impl satisfies A1's `FromRow<'r, SqliteRow>`
    /// bound on every read path.
    #[tokio::test]
    async fn transparent_row_flows_through_pool_and_tx() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        // Row with a non-NULL body + explicit position.
        db.execute(
            INSERT_FULL,
            args!["r1".to_owned(), "WithBody".to_owned(), "hello".to_owned(), 7_i64],
        )
        .await
        .expect("insert r1");
        // Row with a NULL body (Option<String> -> None) and NULL position
        // (COALESCEd to 0 in the SELECT).
        db.execute(
            INSERT_FULL,
            args![
                "r2".to_owned(),
                "NullBody".to_owned(),
                Option::<String>::None,
                Option::<i64>::None
            ],
        )
        .await
        .expect("insert r2");

        // --- POOL: query_one on the non-NULL row.
        let one: ExemplarRow = db
            .query_one(SELECT_EXEMPLAR, args!["r1".to_owned()])
            .await
            .expect("query_one r1");
        assert_eq!(
            one,
            ExemplarRow {
                id: "r1".to_owned(),
                body: Some("hello".to_owned()),
                position: 7,
            }
        );

        // --- POOL: query_all reads both, proving the nullable column decodes
        // to None for r2.
        let all: Vec<ExemplarRow> = db
            .query_all(SELECT_EXEMPLARS, args![])
            .await
            .expect("query_all");
        assert_eq!(
            all,
            vec![
                ExemplarRow {
                    id: "r1".to_owned(),
                    body: Some("hello".to_owned()),
                    position: 7,
                },
                ExemplarRow {
                    id: "r2".to_owned(),
                    body: None,
                    position: 0,
                },
            ]
        );

        // --- TX: the SAME struct through tx_query_all / tx_query_one, proving
        // the generic-R impl rides the object-safe tx path identically.
        let mut tx = db.begin().await.expect("begin");
        let tx_all: Vec<ExemplarRow> = tx_query_all(tx.as_mut(), SELECT_EXEMPLARS, args![])
            .await
            .expect("tx_query_all");
        assert_eq!(tx_all, all, "tx read matches pool read for the same rows");
        let tx_one: ExemplarRow = tx_query_one(tx.as_mut(), SELECT_EXEMPLAR, args!["r2".to_owned()])
            .await
            .expect("tx_query_one r2");
        assert_eq!(
            tx_one,
            ExemplarRow {
                id: "r2".to_owned(),
                body: None,
                position: 0,
            }
        );
        tx.commit().await.expect("commit");
    }
}
