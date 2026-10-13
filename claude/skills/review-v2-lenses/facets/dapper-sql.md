+++
id = "dapper-sql"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["db:dapper"] }
attaches_to = ["security", "data-access", "domain-model"]
+++

# dapper-sql

## General

Dapper over ADO.NET, or raw ADO.NET (`SqlConnection`, `SqlCommand`, `SqlDataReader`), with SQL text in C# strings or in `.sql` files: scripts, stored procedures, views and hand-written migrations. Read the package versions first. `System.Data.SqlClient` is deprecated in favour of `Microsoft.Data.SqlClient`, whose 4.0 release made `Encrypt=true` the default, so a connection string that worked against an untrusted certificate starts failing on upgrade. The fix is a trusted certificate, not `TrustServerCertificate=True` in production.

- **SQL is a string the compiler never sees.** A renamed column, a dropped table or a changed parameter name fails only at runtime; trace each SQL string to the schema the migrations or `.sql` files define.
- **Connection lifetime.** A `SqlConnection` is cheap to open from the pool and must be disposed (`using`) on every path; a leaked one holds a pooled connection until the finaliser runs, and the pool (100 per distinct connection string by default) times out on the next opener. Pools are keyed by the exact connection string, so building it per call with varying text makes a pool per variant.
- **Transactions.** A Dapper call inside a `BeginTransaction()` block must pass that `transaction:` argument, or SQL Server throws because the command is not enlisted. A `TransactionScope` around async code needs `TransactionScopeAsyncFlowOption.Enabled`, or the ambient transaction does not flow across the `await`.
- **Async and cancellation.** `Query`/`Execute` instead of `QueryAsync`/`ExecuteAsync` on a request path block a pool thread for the round trip; a `CancellationToken` reaches Dapper only through a `CommandDefinition`, so a token accepted by the method and not wrapped is ignored.

## By lens

### For security

- **Concatenation and interpolation into SQL.** `$"... WHERE Name = '{name}'"`, `"... " + id` or `string.Format` passed as Dapper's `sql` argument or `SqlCommand.CommandText` is injection, whatever the value's apparent type. Dapper parameterises only what arrives in the `param` object (`new { name }`, `DynamicParameters`). Trace each SQL string back to its literals and flag any hole fed by a request, a file, a message or a column read from elsewhere.
- **Identifiers cannot be parameters.** A column, table or sort direction chosen at runtime must come from a fixed allowlist mapped in code; `QUOTENAME` in T-SQL or a regex check is the second line, not the first. Flag a user-supplied `orderBy` or `sortColumn` concatenated into `ORDER BY`.
- **Dynamic SQL inside the database.** A stored procedure or `.sql` script building a statement with `EXEC('...' + @param)` or `EXEC(@sql)` reintroduces injection behind a parameterised call; `sp_executesql` with a parameter list is the safe form.
- **`LIKE` patterns.** A parameterised `LIKE @term` where `term` comes from the user still lets `%`, `_` and `[` widen the match, which matters when the query enforces scope (a tenant or owner prefix).
- **Least privilege and secrets.** A connection string with `sa` or a `db_owner` login on a read path, a password in `appsettings.json` committed to the repository, and connection strings or SQL with parameter values written to logs.
- **Row-level scope.** A query keyed by an id from the request with no `AND OwnerId = @userId` (or tenant) predicate; the parameterisation is correct and the access check is missing.

### For data-access

- **N+1.** A Dapper query inside a `foreach` over the results of another; one query with `WHERE Id IN @ids` (Dapper expands an `IEnumerable` into one parameter per value), a join with multi-mapping, or `QueryMultiple` with several result sets in one round trip serves.
- **List expansion limits.** SQL Server accepts at most 2100 parameters per request, so `IN @ids` over an unbounded list fails at size; and each distinct list length is a distinct SQL text and query plan. A table-valued parameter (`.AsTableValuedParameter(...)`) or `STRING_SPLIT`/`OPENJSON` over one parameter keeps one plan.
- **Buffered by default.** `Query<T>` reads every row into a list before returning; `buffered: false` streams rows but holds the connection and reader open until enumeration ends, so the caller must not issue another command on that connection or leave the enumeration half-done.
- **Over-fetching.** `SELECT *` mapped to a type that reads three properties, and fetching a whole table to filter, page or count in C#. Paging with `OFFSET ... FETCH` needs a deterministic `ORDER BY`; deep offsets scan and discard rows where a keyset predicate seeks.
- **Implicit conversions defeat indexes.** C# `string` maps to `nvarchar`; compared with a `varchar` column, SQL Server converts the column side and can scan instead of seek. Pass `DbString { IsAnsi = true, Length = n }` or `DbType.AnsiString` for `varchar` columns. A string parameter whose inferred length varies per value (raw ADO.NET `AddWithValue`) produces a plan per length.
- **Round trips.** `Execute` called in a loop with one row per call; Dapper's `Execute(sql, IEnumerable<T>)` still runs one command per element, so for large batches use a table-valued parameter or `SqlBulkCopy`.
- **Plan quality.** A query in a `.sql` file or stored procedure with a non-sargable predicate (`WHERE YEAR(Created) = @y`, `WHERE ISNULL(Col, '') = @v`, a leading-wildcard `LIKE`), or parameter sniffing on a skewed column; read the indexes the migrations define before claiming a missing one.
- **Literal replacement.** `{=name}` in the SQL text inlines a bool or numeric value as a literal rather than a parameter, so each distinct value compiles its own plan; it suits a fixed value such as a status code, not live data that varies per call.
- **Command timeout.** The default command timeout is 30 seconds; a report query that legitimately runs longer fails intermittently, and a blanket large `commandTimeout` hides a regression. Prefer the measure method's numbers to either.

### For domain-model

- **Mapping drift.** Dapper maps columns to properties by name, case-insensitively, and silently leaves an unmatched property at its default; a renamed column or a typo in an alias reads as `null` or `0`, not as an error. Compare each `SELECT` list against the target type's properties and the schema in the migrations or `.sql` files.
- **Multi-mapping `splitOn`.** `Query<A, B, R>` splits columns at `Id` by default; a joined table whose key column has another name, or a `SELECT` whose column order moves the split, maps fields to the wrong object without an error.
- **Nullability.** A nullable column mapped to a non-nullable value type throws on the first `NULL`; mapped to a non-nullable reference type under `<Nullable>enable</Nullable>`, it puts a `null` where the compiler promised none.
- **Type handlers.** Enums map to their underlying integer; a reordered enum changes the meaning of stored rows. A `DateTime` read back has `Kind = Unspecified`, so a value written as UTC compares as local; a `DateOnly`/`TimeOnly` property needs a registered `SqlMapper.TypeHandler` on Dapper versions without built-in support.
- **Schema owned in two places.** Hand-written `.sql` migrations and the C# types both describe the shape; a column added to one without the other, a stored procedure's result set changed under a caller's type, or two scripts altering the same table out of order. Check whether the project's migration runner (DbUp, Flyway, Fluent Migrator or a numbered folder) records applied scripts, and flag an edit to one already applied.
- **Persistence in domain types.** A domain type shaped around a query result (flattened joins, column-name properties, a public setter for every column) used as the aggregate; a separate row type mapped into the domain keeps the two free to change.
