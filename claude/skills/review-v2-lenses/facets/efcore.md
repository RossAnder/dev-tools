+++
id = "efcore"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["db:efcore"] }
attaches_to = ["data-access", "domain-model"]
+++

# efcore

## General

Entity Framework Core. Read the package version from the `.csproj` or `Directory.Packages.props` and the provider (SQL Server, PostgreSQL via Npgsql, SQLite) before judging translation: what a LINQ operator becomes in SQL changes between major versions and providers, so look up the translation for the methods called rather than assuming it. EF Core 10 changed the default for a `Contains` over a parameter collection to one scalar parameter per value (padded), where EF Core 8 and 9 sent one JSON array parameter; a query tuned for either shape behaves differently on the other.

- **The final projection runs on the client.** EF Core 3.0 and later throw when a `Where`, `OrderBy` or join cannot translate, but a top-level `Select` calling a C# method still runs client-side over every row fetched; an `AsEnumerable()` or `ToList()` placed before a filter moves the filter into memory.
- **`ToQueryString()`** and the `Microsoft.EntityFrameworkCore.Database.Command` log category show the SQL; a finding about query shape should cite the SQL, not the LINQ.
- **`DbContext` is not thread-safe.** Two queries on one context at once throw "A second operation was started on this context instance"; a `Task.WhenAll` over queries sharing one injected context is the usual cause.
- **Lifetimes.** A `DbContext` is a unit of work: scoped per request in ASP.NET Core, created per operation through `IDbContextFactory<T>` in a singleton, a Blazor Server circuit or a background service. A long-lived context accumulates tracked entities without bound and serves stale data. Under `AddDbContextPool`, state set on the context (a field, a filter value assigned after construction) leaks into the next lease.
- **Raw SQL.** `FromSql` and `FromSqlInterpolated` turn interpolation holes into parameters; `FromSqlRaw` and `ExecuteSqlRaw` given an interpolated or concatenated string do not. EF Core 10 adds an analyser warning for concatenation into `FromSqlRaw`.

## By lens

### For data-access

- **N+1 through lazy loading.** With `UseLazyLoadingProxies()` or an `ILazyLoader`-injected entity, touching a navigation inside a loop issues one query per item. Without lazy loading, the same loop sees `null` or an empty collection and is a correctness bug instead. Either way the fix is an `Include`, a projection, or one query keyed by the collected ids.
- **Cartesian explosion.** Two or more sibling collection `Include`s in one single query join to a cross product: rows returned are the product of the collection sizes per principal. `AsSplitQuery()` (or `UseQuerySplittingBehavior` globally) sends one query per collection instead, at the cost of extra round trips and no snapshot consistency between them unless inside a transaction. Flag a multi-collection `Include` over a principal set the workload makes large.
- **Over-fetching.** Materialising whole entities (and their `Include`d graphs) to read two columns; a `Select` into a DTO fetches only those columns and skips tracking.
- **Tracking on read paths.** A read-only query without `AsNoTracking()` (or `QueryTrackingBehavior.NoTracking`) pays identity resolution and snapshot copies per row; `AsNoTrackingWithIdentityResolution()` when the graph repeats entities. Do not flag a tracked query whose results are modified and saved.
- **Pagination.** `Skip`/`Take` with no `OrderBy` returns an unstable page (and logs a warning); deep `Skip(n)` offset pagination scans and discards n rows per page, where keyset pagination (`Where(x => x.Id > lastId).OrderBy(x => x.Id).Take(k)`) seeks. Under `AsSplitQuery()`, each split query re-applies the paging, so an ordering that is not unique can page differently between them and attach related rows to the wrong page; read the SQL.
- **Counting and existence.** `Count() > 0` where `Any()` would stop at the first row; `ToList().Count` materialising every row to count them.
- **Bulk work through the change tracker.** Loading entities to modify or delete them one by one, then `SaveChanges`, issues per-row statements; `ExecuteUpdate`/`ExecuteDelete` (EF Core 7 and later) issue one statement. They bypass the change tracker, so tracked instances go stale and `SaveChanges` interceptors and domain events do not fire; each runs in its own implicit transaction unless the caller opens one.
- **`SaveChanges` in a loop.** One round trip and one transaction per item where one call after the loop batches the statements.
- **Compiled queries.** `EF.CompileAsyncQuery` removes the LINQ-to-SQL translation cost per call; it pays only on a hot query executed often with small results, and it is a `perf` claim needing the measure method. Queries whose shape varies per call (a dynamic `Where` chain, a `Contains` over a list whose cardinality varies under the EF Core 10 default) produce a new SQL string per variant and churn the query cache.
- **Sync calls.** `ToList()`, `First()`, `SaveChanges()` instead of their `Async` forms on a request path block a pool thread for the round trip.
- **Missing index for a translated predicate.** A `Where` on a column with no `HasIndex` in the model and no index in the migrations, on a table the workload makes large; a predicate wrapping the column in a function (`x.Name.ToLower() == v`) cannot use an index at all.

### For domain-model

- **Model and migrations disagree.** A change to an entity, its configuration or `OnModelCreating` with no new migration, or a `*ModelSnapshot.cs` changed by hand; an applied migration edited instead of a new one added. `dotnet ef migrations has-pending-model-changes` (EF Core 8 and later) reports the first.
- **Migration data safety.** A renamed property that the scaffolder turned into drop-column plus add-column (the data is lost; `RenameColumn` keeps it); a new non-nullable column with no default on a table holding rows; a column type narrowed without a data step.
- **Persistence in the domain.** Entities carrying `[Column]`, `[Table]` or `[ForeignKey]` attributes, public parameterless constructors and public setters only so EF can materialise them, where the project configures mapping through `IEntityTypeConfiguration<T>` and private setters or backing fields. EF Core can bind constructor parameters and private fields, so the persistence shape need not leak.
- **Navigations crossing aggregates.** A navigation property from one aggregate root into another's internals lets one `SaveChanges` modify both and invites lazy-loaded graphs; reference the other aggregate by id.
- **Owned and complex types.** Value objects mapped as owned types or complex types (EF Core 8 and later) versus separate entities with their own keys; a value object given an identity column, or an entity mapped as owned so it cannot be queried on its own.
- **Cascade delete.** The convention cascades required relationships; a delete of a principal removing dependents the domain expects to keep, or `DeleteBehavior.Restrict` on one side and cascade in the database on the other.
- **Concurrency tokens.** An aggregate edited by concurrent users with no `[Timestamp]`/`IsRowVersion()` or `IsConcurrencyToken()`, so the last write silently wins; a `DbUpdateConcurrencyException` caught and ignored.
- **Global query filters.** A soft-delete or tenant filter (`HasQueryFilter`) bypassed by `IgnoreQueryFilters()`, by raw SQL, or by `ExecuteUpdate`/`ExecuteDelete` paths that assume it applies; a required navigation to a filtered entity yields missing rows.
- **Value converters and enums.** An enum stored as `int` and reordered, or stored as string and renamed, changes the meaning of existing rows; a value converter on a column used in a `Where` that prevents translation or index use.
