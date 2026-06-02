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
use async_trait::async_trait;
use sqlx::sqlite::{
    SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow,
    SqliteSynchronous,
};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::AppError;

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

// ===========================================================================
// Backend-abstraction seam (Part A, Wave A0 — Task A1)
// ===========================================================================
//
// This seam is the foundation the macro-eradication waves (A4+) build on: it
// lets every repo/http/mcp call site speak to the database through a small,
// backend-erased surface (`DbClient` for auto-commit work, `DbTx` for in-flight
// transactions) instead of naming `SqlitePool` / `Transaction<'_, Sqlite>`
// directly. Only the SQLite arm is live today; the `Pg` arms are reserved for
// the future Part C and are NOT implemented here.
//
// Object-safety is the load-bearing constraint (see the `DbTx` doc-comment):
// `DbTx` is consumed as `&mut dyn DbTx` / `Box<dyn DbTx>`, so it carries ONLY
// non-generic methods. Typed reads inside a transaction go through the free
// generic helpers `tx_query_*` below, which call the object-safe `fetch_*`
// primitives on `DbTx`. `DbClient`, by contrast, is consumed by static dispatch
// (`&impl DbClient`), so it MAY keep generic `query_*<T>` methods.

/// Which concrete database backend an [`AnyPool`] / [`DbClient`] is fronting.
/// `Pg` is reserved for the future Part C (live Postgres); only `Sqlite` is
/// constructed today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    /// Reserved for Part C. Never constructed in the SQLite-only build.
    #[allow(dead_code)]
    Pg,
}

/// Owned, backend-erased bound-parameter bundle for a single statement.
///
/// Values are `add`ed in `$1, $2, …` order and must be `'static` + owned
/// (`String`, `i64`, `bool`, `Option<String>`, …) so the args outlive the
/// transient borrow inside `execute`/`fetch_*`. This is a deliberately concrete
/// (non-`dyn`) type: that is what keeps the `DbTx` primitives object-safe (no
/// generic `args` parameter leaks onto the trait) while still letting any
/// `Encode + Type` value be bound. A future Pg arm generalises this by adding a
/// `Pg(PgArguments)` variant and dispatching in the `execute`/`fetch_*` bodies;
/// call sites keep using [`args!`] / [`Args::add`] unchanged.
///
/// In sqlx 0.9 `SqliteArguments` is itself lifetime-free (it owns its encoded
/// values), so the wrapper needs no lifetime parameter either.
pub struct Args(SqliteArguments);

impl Args {
    /// An empty argument list (for statements with no bound parameters).
    pub fn new() -> Self {
        Args(SqliteArguments::default())
    }

    /// Bind the next positional parameter. Chains, so call sites read
    /// `Args::new().add(a).add(b)`. Panics only on an internal sqlx encode-time
    /// failure (e.g. a value that cannot be encoded), which is a programmer
    /// error at the bind site, not a runtime DB error.
    ///
    /// The `add` name is deliberate (it mirrors sqlx's own `Arguments::add` and
    /// reads naturally in the builder chain); it is not the `std::ops::Add::add`
    /// operator, so the `should_implement_trait` lint is suppressed.
    #[allow(clippy::should_implement_trait)]
    pub fn add<T>(mut self, value: T) -> Self
    where
        T: 'static + sqlx::Encode<'static, Sqlite> + sqlx::Type<Sqlite>,
    {
        use sqlx::Arguments as _;
        self.0
            .add(value)
            .expect("binding a parameter value should not fail to encode");
        self
    }

    /// Consume into the raw `SqliteArguments` for handing to a `query_*_with`.
    fn into_sqlite(self) -> SqliteArguments {
        self.0
    }
}

impl Default for Args {
    fn default() -> Self {
        Args::new()
    }
}

/// Convenience constructor: `args![a, b, c]` == `Args::new().add(a).add(b).add(c)`.
#[macro_export]
macro_rules! args {
    () => { $crate::db::Args::new() };
    ($($v:expr),+ $(,)?) => {{
        let a = $crate::db::Args::new();
        $(let a = a.add($v);)+
        a
    }};
}

/// A backend-erased connection pool. Today only the `Sqlite` arm exists; the
/// `Pg(PgPool)` arm is added in Part C. Construct via [`AnyPool::Sqlite`] or
/// [`AnyPool::from`] (a `SqlitePool`).
pub enum AnyPool {
    Sqlite(SqlitePool),
}

impl From<SqlitePool> for AnyPool {
    fn from(pool: SqlitePool) -> Self {
        AnyPool::Sqlite(pool)
    }
}

impl AnyPool {
    /// Borrow the underlying `SqlitePool`. Panics on a (future) `Pg` arm.
    ///
    /// `AppState.pool` is `Arc<AnyPool>` post-A12, so the code paths that still
    /// run RAW sqlx (`sqlx::query*(…).fetch_*`) against a concrete
    /// `&SqlitePool` — the PTY subsystem, the export drain, and the inline raw
    /// reads in a handful of `http/*` helpers — obtain that `&SqlitePool` from
    /// the erased pool through this accessor. The seam-routed call sites
    /// (`repo::*` taking `&impl DbClient`) never need it.
    pub fn sqlite(&self) -> &SqlitePool {
        match self {
            AnyPool::Sqlite(p) => p,
        }
    }
}

/// Auto-commit query surface, consumed by static dispatch (`&impl DbClient`).
///
/// Because every call site passes a concrete `&AnyPool` as `&impl DbClient`,
/// this trait is NOT required to be object-safe and so MAY carry generic
/// `query_*<T: FromRow>` methods. The SQLite arm is the only live impl.
///
/// Part-C swap point: the typed-read bound `for<'r> FromRow<'r, SqliteRow>`
/// (below, and the `Scalar<T>: FromRow<'r, SqliteRow>` bound on the scalar
/// helpers) names `SqliteRow` concretely, so the Pg backend must widen it to
/// also decode `PgRow` (mirroring the `Pg(PgRow)` plan on [`AnyRow`]).
#[async_trait]
pub trait DbClient: Send + Sync {
    /// Run a non-returning statement; yields the affected-row count.
    async fn execute(&self, sql: &'static str, args: Args) -> Result<u64, AppError>;

    /// Run a SELECT expected to return at most one row.
    async fn query_opt<T>(&self, sql: &'static str, args: Args) -> Result<Option<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin;

    /// Run a SELECT expected to return exactly one row; `RowNotFound` →
    /// [`AppError::NotFound`].
    async fn query_one<T>(&self, sql: &'static str, args: Args) -> Result<T, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin;

    /// Run a SELECT returning zero or more rows.
    async fn query_all<T>(&self, sql: &'static str, args: Args) -> Result<Vec<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin;

    /// Open a write transaction. On SQLite this issues `BEGIN IMMEDIATE` (the
    /// same RESERVED-lock-at-begin semantics as [`begin_write`]), so writer
    /// contention surfaces upfront rather than after the first statement.
    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError>;

    /// Which backend this client fronts.
    fn backend(&self) -> Backend;
}

#[async_trait]
impl DbClient for AnyPool {
    async fn execute(&self, sql: &'static str, args: Args) -> Result<u64, AppError> {
        match self {
            AnyPool::Sqlite(pool) => {
                let res = sqlx::query_with(sql, args.into_sqlite())
                    .execute(pool)
                    .await?;
                Ok(res.rows_affected())
            }
        }
    }

    async fn query_opt<T>(&self, sql: &'static str, args: Args) -> Result<Option<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        match self {
            AnyPool::Sqlite(pool) => {
                let row = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
                    .fetch_optional(pool)
                    .await?;
                Ok(row)
            }
        }
    }

    async fn query_one<T>(&self, sql: &'static str, args: Args) -> Result<T, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        match self {
            AnyPool::Sqlite(pool) => {
                let row = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
                    .fetch_one(pool)
                    .await
                    .map_err(|e| AppError::from_sqlx_not_found(e, ""))?;
                Ok(row)
            }
        }
    }

    async fn query_all<T>(&self, sql: &'static str, args: Args) -> Result<Vec<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        match self {
            AnyPool::Sqlite(pool) => {
                let rows = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
                    .fetch_all(pool)
                    .await?;
                Ok(rows)
            }
        }
    }

    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError> {
        match self {
            AnyPool::Sqlite(pool) => {
                let tx = pool.begin_with("BEGIN IMMEDIATE").await?;
                Ok(Box::new(tx))
            }
        }
    }

    fn backend(&self) -> Backend {
        match self {
            AnyPool::Sqlite(_) => Backend::Sqlite,
        }
    }
}

/// [`DbClient`] impl for a bare `SqlitePool`. KEPT permanently: the repo unit
/// tests and the e2e/migration tests construct a `SqlitePool` directly via
/// [`connect_in_memory`] and pass `&pool` as `&impl DbClient`, and several raw
/// helpers thread a `&SqlitePool` (obtained via [`AnyPool::sqlite`]) into
/// `repo::*` calls that take `&impl DbClient`. It mirrors the [`AnyPool`] SQLite
/// arm method for method (including the `from_sqlx_not_found` mapping in
/// `query_one`).
#[async_trait]
impl DbClient for SqlitePool {
    async fn execute(&self, sql: &'static str, args: Args) -> Result<u64, AppError> {
        let res = sqlx::query_with(sql, args.into_sqlite())
            .execute(self)
            .await?;
        Ok(res.rows_affected())
    }

    async fn query_opt<T>(&self, sql: &'static str, args: Args) -> Result<Option<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        let row = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
            .fetch_optional(self)
            .await?;
        Ok(row)
    }

    async fn query_one<T>(&self, sql: &'static str, args: Args) -> Result<T, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        let row = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
            .fetch_one(self)
            .await
            .map_err(|e| AppError::from_sqlx_not_found(e, ""))?;
        Ok(row)
    }

    async fn query_all<T>(&self, sql: &'static str, args: Args) -> Result<Vec<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        let rows = sqlx::query_as_with::<_, T, _>(sql, args.into_sqlite())
            .fetch_all(self)
            .await?;
        Ok(rows)
    }

    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError> {
        let tx = self.begin_with("BEGIN IMMEDIATE").await?;
        Ok(Box::new(tx))
    }

    fn backend(&self) -> Backend {
        Backend::Sqlite
    }
}

/// [`DbClient`] impl for `Arc<AnyPool>` — the post-A12 `AppState.pool` /
/// `LuminaTools.pool` handle type. Callers pass `&state.pool` / `&self.pool` (a
/// `&Arc<AnyPool>`) directly as `&impl DbClient`; trait-bound resolution does
/// NOT auto-deref through `Arc`, so the `Arc` itself must impl `DbClient`. Every
/// method delegates to the inner [`AnyPool`] impl via `&**self`. This is what
/// lets the ~68 `repo::foo(&self.pool, …)` / `repo::foo(state.pool.as_ref(), …)`
/// seam call sites keep compiling UNCHANGED across the A12 handle swap.
#[async_trait]
impl DbClient for std::sync::Arc<AnyPool> {
    async fn execute(&self, sql: &'static str, args: Args) -> Result<u64, AppError> {
        (**self).execute(sql, args).await
    }

    async fn query_opt<T>(&self, sql: &'static str, args: Args) -> Result<Option<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        (**self).query_opt(sql, args).await
    }

    async fn query_one<T>(&self, sql: &'static str, args: Args) -> Result<T, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        (**self).query_one(sql, args).await
    }

    async fn query_all<T>(&self, sql: &'static str, args: Args) -> Result<Vec<T>, AppError>
    where
        T: for<'r> sqlx::FromRow<'r, SqliteRow> + Send + Unpin,
    {
        (**self).query_all(sql, args).await
    }

    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError> {
        // Disambiguate to the `DbClient` trait method on the inner pool — bare
        // `.begin()` would resolve to sqlx's inherent `Pool::begin`.
        <AnyPool as DbClient>::begin(&**self).await
    }

    fn backend(&self) -> Backend {
        <AnyPool as DbClient>::backend(&**self)
    }
}

/// A backend-erased row, returned by the object-safe [`DbTx`] `fetch_*`
/// primitives. The free `tx_query_*` helpers decode it via `FromRow`. A future
/// Pg arm adds a `Pg(PgRow)` variant; the helpers stay generic over the wrapped
/// row type.
pub enum AnyRow {
    Sqlite(SqliteRow),
}

/// An in-flight write transaction, backend-erased and **object-safe**.
///
/// ## Object-safety contract (load-bearing — do not add generic methods)
/// `DbTx` is consumed as `&mut dyn DbTx` and `Box<dyn DbTx>`, so it MUST stay
/// object-safe: every method here is non-generic. Typed SELECTs inside a
/// transaction are NOT methods on this trait — they live in the free generic
/// helpers [`tx_query_one`] / [`tx_query_opt`] / [`tx_query_all`], which decode
/// the erased [`AnyRow`] returned by the `fetch_*` primitives below.
///
/// ## `record_event` coercion contract (Task A3 depends on this)
/// `sqlx::Transaction<'_, Sqlite>` implements `DbTx`, so an in-flight
/// `tx: Transaction<'_, Sqlite>` coerces at a call site via `&mut tx as
/// &mut dyn DbTx` — i.e. `&mut tx` unsizes to `&mut dyn DbTx`. This is what lets
/// A3 change `record_event(tx: &mut Transaction<'_, Sqlite>, …)` to
/// `record_event(tx: &mut dyn DbTx, …)` WITHOUT editing record_event's ~100
/// callers: they all pass `&mut tx` already, which now satisfies the new
/// parameter type by unsizing. `execute` runs a bound INSERT/UPDATE (e.g.
/// record_event's 5-column `events` insert) inside the transaction.
#[async_trait]
pub trait DbTx: Send {
    /// Run a non-returning statement inside the transaction; affected-row count.
    async fn execute(&mut self, sql: &'static str, args: Args) -> Result<u64, AppError>;

    /// Fetch at most one row (object-safe primitive; decode via [`tx_query_opt`]).
    async fn fetch_optional(
        &mut self,
        sql: &'static str,
        args: Args,
    ) -> Result<Option<AnyRow>, AppError>;

    /// Fetch zero or more rows (object-safe primitive; decode via [`tx_query_all`]).
    async fn fetch_all(&mut self, sql: &'static str, args: Args) -> Result<Vec<AnyRow>, AppError>;

    /// Commit the transaction, consuming it.
    async fn commit(self: Box<Self>) -> Result<(), AppError>;
}

#[async_trait]
impl DbTx for Transaction<'_, Sqlite> {
    async fn execute(&mut self, sql: &'static str, args: Args) -> Result<u64, AppError> {
        let res = sqlx::query_with(sql, args.into_sqlite())
            .execute(&mut **self)
            .await?;
        Ok(res.rows_affected())
    }

    async fn fetch_optional(
        &mut self,
        sql: &'static str,
        args: Args,
    ) -> Result<Option<AnyRow>, AppError> {
        let row = sqlx::query_with(sql, args.into_sqlite())
            .fetch_optional(&mut **self)
            .await?;
        Ok(row.map(AnyRow::Sqlite))
    }

    async fn fetch_all(&mut self, sql: &'static str, args: Args) -> Result<Vec<AnyRow>, AppError> {
        let rows = sqlx::query_with(sql, args.into_sqlite())
            .fetch_all(&mut **self)
            .await?;
        Ok(rows.into_iter().map(AnyRow::Sqlite).collect())
    }

    async fn commit(self: Box<Self>) -> Result<(), AppError> {
        Transaction::commit(*self).await?;
        Ok(())
    }
}

/// Decode an [`AnyRow`] into a `FromRow` type. Free function (not a trait
/// method) so the `FromRow` generic stays off the object-safe [`DbTx`] surface.
fn decode_row<T>(row: AnyRow) -> Result<T, AppError>
where
    T: for<'r> sqlx::FromRow<'r, SqliteRow>,
{
    match row {
        AnyRow::Sqlite(r) => T::from_row(&r).map_err(AppError::Db),
    }
}

/// Typed SELECT inside a transaction, expecting exactly one row.
/// `RowNotFound` → [`AppError::NotFound`]. Calls the object-safe
/// [`DbTx::fetch_optional`] primitive, so it works through `&mut dyn DbTx`.
#[allow(dead_code)]
pub async fn tx_query_one<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<T, AppError>
where
    T: for<'r> sqlx::FromRow<'r, SqliteRow>,
{
    match tx.fetch_optional(sql, args).await? {
        Some(row) => decode_row(row),
        None => Err(AppError::NotFound("row not found".to_owned())),
    }
}

/// Typed SELECT inside a transaction, expecting at most one row.
#[allow(dead_code)]
pub async fn tx_query_opt<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<Option<T>, AppError>
where
    T: for<'r> sqlx::FromRow<'r, SqliteRow>,
{
    match tx.fetch_optional(sql, args).await? {
        Some(row) => Ok(Some(decode_row(row)?)),
        None => Ok(None),
    }
}

/// Typed SELECT inside a transaction, returning zero or more rows.
#[allow(dead_code)]
pub async fn tx_query_all<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<Vec<T>, AppError>
where
    T: for<'r> sqlx::FromRow<'r, SqliteRow>,
{
    let rows = tx.fetch_all(sql, args).await?;
    rows.into_iter().map(decode_row).collect()
}

// ===========================================================================
// Canonical row-mapper recipe + scalar fetch path (Part A, Wave A0 — Task A2)
// ===========================================================================
//
// This block is the *recipe* the macro-eradication waves (A4+) copy mechanically
// to ~142 conversion sites. It pins two things the A1 seam left open:
//
//   1. How to hand-write a multi-column `FromRow` so it flows through A1's
//      `query_*<T>` / `tx_query_*<T>` helpers (which bound `T: FromRow<'r,
//      SqliteRow>`) with zero churn AND stays Part-C-ready.
//   2. How to read a single-column scalar (the `query_scalar!` target) through
//      the same helpers, for SQLite's `i64` / `String` / `bool` columns.

// --- 1. CANONICAL MULTI-COLUMN ROW-STRUCT MAPPER --------------------------
//
// RECIPE (copy this shape verbatim per conversion site):
//
//   #[derive(Debug)]                       // + PartialEq/Eq in tests if useful
//   struct FooRow {
//       id: String,                        // NOT NULL TEXT  -> String
//       label: Option<String>,             // nullable TEXT  -> Option<String>
//       n: i64,                            // NOT NULL INT   -> i64
//   }
//
//   impl<'r, R> sqlx::FromRow<'r, R> for FooRow
//   where
//       R: sqlx::Row,
//       usize: sqlx::ColumnIndex<R>,        // index-by-position support
//       &'r str: sqlx::ColumnIndex<R>,      // index-by-NAME support (try_get("x"))
//       String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
//       Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
//       i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
//   {
//       fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
//           Ok(FooRow {
//               id: row.try_get("id")?,
//               label: row.try_get("label")?,   // Option<T> tolerates SQL NULL
//               n: row.try_get("n")?,
//           })
//       }
//   }
//
// Rules:
//   * GENERIC over `R: sqlx::Row` (NOT bound to `SqliteRow`). A generic-R impl
//     automatically satisfies A1's `for<'r> FromRow<'r, SqliteRow>` bound, so it
//     drops straight into `query_all::<FooRow>` / `tx_query_all::<FooRow>` today
//     AND needs zero edits when Part C adds a Pg arm — the `transparent_row_*`
//     tests below prove the SQLite flow-through.
//   * try_get by COLUMN NAME (`row.try_get("col")`), not by position. This is
//     robust to column reordering in the SELECT and matches what `query_as!`
//     produced. Add the `&'r str: sqlx::ColumnIndex<R>` bound to enable it.
//   * Add ONE `T: Decode<'r, R::Database> + Type<R::Database>` bound per DISTINCT
//     Rust column type the struct reads (String, i64, Option<String>, bool, …).
//     Repeating a type is harmless but unnecessary.
//   * Nullable SQL column -> `Option<T>` field. `try_get::<Option<T>>` returns
//     `Ok(None)` on SQL NULL; a bare `try_get::<T>` on a NULL column errors.
//   * AS-HINT REMOVAL: the `query_as!`/`query!` macros sometimes carried inline
//     type-override hints in the SQL, e.g. `SELECT count AS "count: i64"`. Those
//     hints are a COMPILE-TIME macro affordance only — DROP them when moving the
//     SQL to a runtime `&'static str`; the hand-written `FromRow` carries the
//     type decision in Rust now, and `AS "x: T"` is not valid runtime SQL.

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

// --- 2. SINGLE-COLUMN SCALAR FETCH PATH -----------------------------------
//
// CHOSEN IDIOM: a generic `Scalar<T>(pub T)` newtype with a generic-R `FromRow`
// impl that `try_get(0)`s column 0. Read it through the EXISTING A1 helpers:
//
//   let n: i64       = db.query_one::<Scalar<i64>>(SQL, args![…]).await?.0;
//   let s: String    = tx_query_one::<Scalar<String>>(tx, SQL, args![]).await?.0;
//   let b: bool      = db.query_one::<Scalar<bool>>(SQL, args![]).await?.0;
//   let opt: Option<String> =
//       db.query_opt::<Scalar<Option<String>>>(SQL, args![]).await?.map(|s| s.0);
//
// Why a newtype rather than `query_scalar_*` helper methods: the newtype is just
// another `FromRow`, so it rides A1's `query_*<T>` / `tx_query_*<T>` verbatim —
// it needs NO new methods on `DbClient`, and crucially NONE on the object-safe
// `DbTx` (a generic `query_scalar` method there would break object-safety). One
// fetch path, pool and tx alike. Convenience wrappers `scalar_one`/`opt`/`all`
// below unwrap the newtype so call sites read clean.
//
// bool/SQLite: SQLite has no native boolean — `bool` stores as INTEGER 0/1, and
// sqlx's `Type<Sqlite>`/`Decode<Sqlite> for bool` round-trips it. So
// `Scalar<bool>` over an INTEGER column decodes correctly (see the
// `scalar_bool_*` test).

/// Single-column scalar row adapter — `try_get(0)` wrapped in a newtype so any
/// `Decode + Type` scalar reads through A1's generic `query_*<T>` /
/// `tx_query_*<T>` helpers with no new (object-safety-breaking) trait methods.
/// Generic over `R` for the same Part-C-readiness reason as [`ExemplarRow`].
#[allow(dead_code)]
pub struct Scalar<T>(pub T);

impl<'r, R, T> sqlx::FromRow<'r, R> for Scalar<T>
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    T: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(Scalar(row.try_get(0)?))
    }
}

/// Fetch exactly one single-column scalar through the auto-commit pool.
/// `RowNotFound` → [`AppError::NotFound`] (via [`DbClient::query_one`]).
#[allow(dead_code)]
pub async fn scalar_one<T>(
    db: &impl DbClient,
    sql: &'static str,
    args: Args,
) -> Result<T, AppError>
where
    T: Send + Unpin,
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(db.query_one::<Scalar<T>>(sql, args).await?.0)
}

/// Fetch at most one single-column scalar through the auto-commit pool.
#[allow(dead_code)]
pub async fn scalar_opt<T>(
    db: &impl DbClient,
    sql: &'static str,
    args: Args,
) -> Result<Option<T>, AppError>
where
    T: Send + Unpin,
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(db.query_opt::<Scalar<T>>(sql, args).await?.map(|s| s.0))
}

/// Fetch zero or more single-column scalars through the auto-commit pool.
#[allow(dead_code)]
pub async fn scalar_all<T>(
    db: &impl DbClient,
    sql: &'static str,
    args: Args,
) -> Result<Vec<T>, AppError>
where
    T: Send + Unpin,
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(db
        .query_all::<Scalar<T>>(sql, args)
        .await?
        .into_iter()
        .map(|s| s.0)
        .collect())
}

/// Fetch exactly one single-column scalar inside a transaction
/// (`&mut dyn DbTx`). `RowNotFound` → [`AppError::NotFound`].
#[allow(dead_code)]
pub async fn tx_scalar_one<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<T, AppError>
where
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(tx_query_one::<Scalar<T>>(tx, sql, args).await?.0)
}

/// Fetch at most one single-column scalar inside a transaction.
#[allow(dead_code)]
pub async fn tx_scalar_opt<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<Option<T>, AppError>
where
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(tx_query_opt::<Scalar<T>>(tx, sql, args).await?.map(|s| s.0))
}

/// Fetch zero or more single-column scalars inside a transaction.
#[allow(dead_code)]
pub async fn tx_scalar_all<T>(
    tx: &mut dyn DbTx,
    sql: &'static str,
    args: Args,
) -> Result<Vec<T>, AppError>
where
    for<'r> Scalar<T>: sqlx::FromRow<'r, SqliteRow>,
{
    Ok(tx_query_all::<Scalar<T>>(tx, sql, args)
        .await?
        .into_iter()
        .map(|s| s.0)
        .collect())
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
