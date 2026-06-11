//! The backend-erased query traits (`DbClient` / `DbTx`), the typed-read free
//! helpers (`tx_query_*`, `scalar_*`, `tx_scalar_*`), the `Scalar<T>` adapter,
//! the row-decode helper, and the `args!` convenience macro.
//!
//! See the module-level docs in [`super`] for the seam's design constraints
//! (object-safety on `DbTx`, static dispatch on `DbClient`).

use async_trait::async_trait;
use sqlx::sqlite::SqliteRow;

use super::{AnyRow, Args, Backend};
use crate::error::AppError;

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
    ///
    /// [`begin_write`]: super::begin_write
    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError>;

    /// Which backend this client fronts.
    fn backend(&self) -> Backend;
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

    /// Buffer a change notification to publish to the process-wide notify bus
    /// AFTER this transaction commits. Default no-op: only the `NotifyingTx`
    /// wrapper (db::erased) overrides this to buffer; a raw `Transaction<Sqlite>`
    /// (the `begin_write` path) ignores it — correct, since the PTY subsystem
    /// has its own broadcast. Publishing pre-commit would broadcast WAL-isolated
    /// pre-commit state, so the buffer-until-commit is load-bearing, not
    /// stylistic.
    ///
    /// Sync (not `async fn`) on purpose: buffering onto a `Vec` needs no await,
    /// and `#[async_trait]` rewrites only `async fn`s, so this stays a plain
    /// non-generic method and the trait remains object-safe.
    fn note_change(&mut self, _change: crate::notify::ChangeNotification) {}

    /// Commit the transaction, consuming it.
    async fn commit(self: Box<Self>) -> Result<(), AppError>;
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

/// Single-column scalar row adapter — `try_get(0)` wrapped in a newtype so any
/// `Decode + Type` scalar reads through A1's generic `query_*<T>` /
/// `tx_query_*<T>` helpers with no new (object-safety-breaking) trait methods.
/// Generic over `R` for the same Part-C-readiness reason as [`ExemplarRow`].
///
/// [`ExemplarRow`]: super::ExemplarRow
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
