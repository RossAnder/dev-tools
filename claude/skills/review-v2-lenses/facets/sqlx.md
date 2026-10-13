+++
id = "sqlx"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["db:sqlx"] }
attaches_to = ["data-access", "domain-model"]
+++

# sqlx

## General

Read the crate's pool and connection setup before judging a query: the backend, `max_connections`, the connect options and how write transactions begin. Two query forms behave differently. The macros (`query!`, `query_as!`, `query_scalar!`) check SQL, column names and types at compile time against a live database or a committed `.sqlx` offline cache. The runtime functions (`query`, `query_as`, `query_scalar`, `QueryBuilder`) check nothing until the statement runs, so their mistakes surface only in a test that executes them. A crate that has settled on one form has usually done so deliberately; read its `CLAUDE.md` or module docs before flagging the other.

Defaults in sqlx 0.9 that the code may be leaning on without saying so:

- **Pool.** `max_connections` is 10 and `acquire_timeout` is 30 seconds. A caller that cannot get a connection waits, then fails with `PoolTimedOut`.
- **SQLite connect options.** `foreign_keys` is on, `busy_timeout` is 5 seconds, `create_if_missing` is off, and `journal_mode` is left as the database file has it, which is the rollback journal unless something set WAL. Each connection caches up to 100 prepared statements.
- **Transactions.** `pool.begin()` issues a plain `BEGIN`, which SQLite treats as `DEFERRED`. Dropping a `Transaction` without `commit()` queues a rollback with no error, so a function that returns early through `?` rolls back silently, which is correct, and a function that forgets `commit()` on its success path loses its writes just as silently.
- **Migrations.** `sqlx::migrate!` embeds each migration's SQL at compile time and records a checksum of the raw bytes for each applied version. Startup then refuses with `VersionMismatch` when an applied file's bytes changed and with `VersionMissing` when an applied file is gone. On stable Rust the macro does not track the directory itself, so a new migration file is not picked up until the crate rebuilds for another reason, unless a `build.rs` prints `cargo:rerun-if-changed=migrations`.

## By lens

### For data-access

- A query inside a loop over rows another query returned: `for id in ids { query(..).bind(id).fetch_one(&pool).await? }`. One query with `IN (…)` built through `QueryBuilder::separated`, a join, or a `push_values` batch serves it. sqlx does not expand a bound `Vec` into an `IN` list on SQLite, so check the batch is built with `QueryBuilder`, not `format!`, and that it chunks below SQLite's bound-parameter limit.
- Inserts or updates in a loop with no surrounding transaction: on SQLite each auto-commit statement is its own transaction with its own journal sync, so a batch of N rows pays N syncs where one transaction pays one.
- `fetch_all` on a result that can grow without bound, where `fetch` as a stream or a `LIMIT` with keyset pagination holds one page; a `SELECT *` mapped with `FromRow` that reads columns the caller never uses; a `COUNT(*)` followed by a `fetch_all` of the same rows.
- SQL text assembled with `format!` per call produces a new statement each time and defeats the per-connection statement cache; bind the varying values instead.
- Indexes: SQLite does not index a foreign-key child column automatically, so a lookup or an `ON DELETE` check by that column scans the table unless a migration creates the index. A leading-wildcard `LIKE '%x'` and a function applied to an indexed column also bypass the index.
- Connection holding: a `Transaction`, or a connection from `acquire()`, kept across a slow `.await` that is not database work (an HTTP call, a process, a channel wait) holds one of `max_connections` for that whole time. A `tokio::try_join!` fan-out wider than `max_connections` queues on `acquire()`; for SQLite under WAL, writes serialise on one lock whatever the pool size, so a larger pool buys read concurrency only.
- A long-running read transaction under WAL stops checkpoints from completing, so the `-wal` file grows for as long as the reader holds its snapshot. A loop of short auto-commit reads avoids it; turning such a loop into one transaction is the wrong fix.

### For domain-model

- **Never edit a migration that has been applied or committed.** The changed checksum makes every existing database refuse to start with `VersionMismatch`, and the recovery is a database wipe; renaming or deleting an applied file fails with `VersionMissing`. A schema change is always a new migration with the next free number. The same refusal follows a line-ending flip on a migration file, from a renormalise or a fresh clone with different `core.autocrlf`, because the checksum covers the raw bytes.
- A migration that changes stored shape — a new `NOT NULL` column, a split or merged column, a type change — with no data step for existing rows, or with a default that gives existing rows a value the domain forbids.
- SQLite table rebuilds (create new, copy, drop old, rename), which SQLite needs for most column changes: sqlx runs each migration inside a transaction unless its first line is `-- no-transaction`, and `PRAGMA foreign_keys = OFF` is a no-op inside a transaction. With `foreign_keys` on, as sqlx sets it by default, dropping the old parent table fires its children's `ON DELETE CASCADE` and deletes their rows. Check the migration either is `-- no-transaction` and turns foreign keys off around the rebuild, or rebuilds no table that cascade-deleting children reference.
- Mapping drift with the runtime API: `FromRow` and `try_get` match columns by name at run time, so a renamed or dropped column fails with `ColumnNotFound` only when the query runs, and a nullable column decoded into a non-`Option` field fails with `UnexpectedNull` only on a row holding NULL. A new column on a domain type needs its `SELECT` lists, row decoders and every construction site changed in the same edit.
- SQLite type affinity: an `INTEGER` column decoded into `i32` overflows past its range where `i64` is the column's real width; booleans, timestamps and UUIDs are stored as integers or text by convention, and two code paths that pick different conventions for one column read each other's rows wrongly.
- A domain type that derives `sqlx::FromRow` and also serves as the wire or request type ties the storage shape to the API; a column rename then becomes a breaking API change.
- Mixing `&pool` and `&mut *tx` in one logical operation: the pool call runs outside the transaction, so it does not see the transaction's uncommitted writes, is not rolled back with it, and on SQLite blocks on the write lock the transaction holds until `busy_timeout`. With a deferred `BEGIN` under WAL, a transaction that reads and then writes fails with `SQLITE_BUSY` when another writer committed in between, and the busy timeout does not retry that case; a crate that issues `BEGIN IMMEDIATE` for write transactions has closed it.
