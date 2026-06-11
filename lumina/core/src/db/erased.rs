//! Backend-erased concrete types and their trait impls: the [`Backend`]
//! discriminator, the owned [`Args`] parameter bundle, the [`AnyPool`] /
//! [`AnyRow`] erased wrappers, and the SQLite `DbClient` / `DbTx` impls that
//! front them.
//!
//! See the module-level docs in [`super`] for the seam's design constraints.

use async_trait::async_trait;
use sqlx::sqlite::{SqliteArguments, SqliteRow};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::{DbClient, DbTx};
use crate::error::AppError;

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
///
/// [`args!`]: crate::args
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
                Ok(Box::new(NotifyingTx::new(tx)))
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
///
/// [`connect_in_memory`]: super::connect_in_memory
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
        Ok(Box::new(NotifyingTx::new(tx)))
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

/// A [`DbTx`] wrapper that buffers [`ChangeNotification`]s noted during the
/// transaction and flushes them to the process-wide notify bus
/// ([`crate::notify::bus`]) ONLY AFTER the inner commit succeeds.
///
/// Both `DbClient::begin` arms (the [`AnyPool`] and bare `SqlitePool` impls)
/// return this wrapper, so EVERY seam-routed repo mutation gets post-commit
/// change notifications automatically — zero per-mutator edits. The raw
/// `db::begin_write(&SqlitePool)` path deliberately bypasses it (the PTY
/// subsystem there has its own broadcast).
///
/// Invariants:
/// - **Publish only post-commit.** Publishing inside the transaction would
///   broadcast WAL-isolated pre-commit state a subscriber's re-read could not
///   yet observe; the buffer-until-commit is load-bearing, not stylistic.
/// - **Rollback/drop discards the buffer.** A transaction dropped without
///   `commit()` drops the `Vec` with it — never a phantom signal for a write
///   that did not land.
/// - **Best-effort, non-awaiting.** [`crate::notify::NotifyBus::publish`] is a
///   sync broadcast send whose errors (zero receivers) are swallowed.
///
/// `Send` holds structurally: `Box<dyn DbTx + 'a>` is `Send` via the trait's
/// `Send` supertrait bound, and `Vec<ChangeNotification>` is `Send`.
///
/// [`ChangeNotification`]: crate::notify::ChangeNotification
pub struct NotifyingTx<'a> {
    inner: Box<dyn DbTx + 'a>,
    buffer: Vec<crate::notify::ChangeNotification>,
}

impl<'a> NotifyingTx<'a> {
    /// Wrap an inner [`DbTx`] with an empty notification buffer.
    fn new(inner: impl DbTx + 'a) -> Self {
        NotifyingTx {
            inner: Box::new(inner),
            buffer: Vec::new(),
        }
    }
}

#[async_trait]
impl DbTx for NotifyingTx<'_> {
    async fn execute(&mut self, sql: &'static str, args: Args) -> Result<u64, AppError> {
        self.inner.execute(sql, args).await
    }

    async fn fetch_optional(
        &mut self,
        sql: &'static str,
        args: Args,
    ) -> Result<Option<AnyRow>, AppError> {
        self.inner.fetch_optional(sql, args).await
    }

    async fn fetch_all(&mut self, sql: &'static str, args: Args) -> Result<Vec<AnyRow>, AppError> {
        self.inner.fetch_all(sql, args).await
    }

    fn note_change(&mut self, change: crate::notify::ChangeNotification) {
        self.buffer.push(change);
    }

    async fn commit(self: Box<Self>) -> Result<(), AppError> {
        let NotifyingTx { inner, buffer } = *self;
        // COMMIT FIRST: only a durably-committed write may be announced. On
        // commit failure the buffer is dropped with the early return — no
        // phantom signal.
        inner.commit().await?;
        for n in buffer {
            crate::notify::bus().publish(n);
        }
        Ok(())
    }
}
