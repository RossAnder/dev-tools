+++
id = "dotnet"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["lang:csharp"] }
attaches_to = ["correctness", "type-design", "idioms", "memory-runtime", "concurrency"]
+++

# dotnet

## General

C# on .NET, SDK-style or legacy `.csproj`. Read the project file before judging anything version-dependent: `TargetFramework` (or `TargetFrameworks`), `LangVersion`, `<Nullable>`, `<ImplicitUsings>`, `<PublishAot>`, `<IsTrimmable>` and `<TreatWarningsAsErrors>`, plus `Directory.Build.props` and `Directory.Packages.props` above it. A .NET Framework 4.x project has no `Span<T>`-friendly BCL overloads, no default nullable context and a `SynchronizationContext` under ASP.NET classic and WinForms/WPF that turns sync-over-async into a deadlock; a modern .NET project under ASP.NET Core has no request context, so the same code only starves the thread pool.

- **Nullable context.** With `<Nullable>enable</Nullable>`, a `!` suppression, a `default!` initialiser or a `#nullable disable` region is an unchecked claim. Without it, every reference parameter is nullable and the compiler says nothing.
- **Async signatures.** `async void` outside an event handler loses its exception to the synchronisation context or crashes the process; `.Result`, `.Wait()` and `.GetAwaiter().GetResult()` on a `Task` block a thread and, under a single-threaded context, deadlock.
- **Disposal.** `IDisposable` and `IAsyncDisposable` owners must dispose on every path; `using var` disposes at the end of the enclosing scope, not at the last use, so it holds a connection or lock across everything after it.
- **Dependency injection lifetimes.** A scoped service (a `DbContext`, a unit of work) captured by a singleton or a hosted service outlives its scope; `IServiceScopeFactory.CreateScope()` is the way in from a singleton.
- **HttpClient.** `new HttpClient()` per call exhausts sockets under load; one constructed once and held for the process life misses DNS changes unless `SocketsHttpHandler.PooledConnectionLifetime` is set. `IHttpClientFactory` handles both.
- **Equality and hashing.** A `class` overriding `Equals` without `GetHashCode`, a mutable field in a `record`'s or struct's equality used as a dictionary key, and `==` on two `object`-typed boxed values (reference comparison) are silent wrong-answer bugs.
- **Culture and comparison.** `ToUpper()`, `ToLower()`, `string.Compare`, `StartsWith(string)` and `double.Parse` without a `CultureInfo` or `StringComparison` follow the thread's culture; the Turkish `i` and a decimal comma both change results on a production box.

## By lens

### For correctness

- `DateTime.Now` and `DateTime.Today` stored, compared or sent across a boundary: local time with `Kind = Local` round-trips wrongly through serialisers and databases. `DateTime` arithmetic across a DST change, and `DateTimeOffset` compared by `.DateTime` instead of by instant.
- `decimal` versus `double` for money; integer arithmetic outside a `checked` context overflowing silently in release (overflow checking is off by default); `int` division truncating where a ratio was meant.
- `First()` and `Single()` throwing on an empty or multi-row sequence that the input can produce, and `FirstOrDefault()` on a value-type sequence returning `0` or `default` that is indistinguishable from a real match.
- A LINQ query (`IEnumerable<T>` with deferred execution) enumerated twice, once per `Count()` and once per `foreach`, so a generator, a database query or a random source runs twice and may disagree with itself; and a collection modified while a deferred query over it is still pending.
- `catch (Exception)` that swallows `OperationCanceledException`, so a cancelled request is reported as a failure or retried; `throw ex;` resetting the stack trace where `throw;` was meant.
- An exception from `Task.WhenAll` surfaces only the first inner exception when awaited; the rest sit on `task.Exception.InnerExceptions` and are lost if nobody reads them.
- A `CancellationToken` accepted by a method and not passed to the awaited calls inside it, so cancellation stops nothing.
- `Dispose` on a field-owned resource missing from a class that is itself `IDisposable`, or a finaliser in a class with no unmanaged state.
- Static mutable state (a `static Dictionary`, a lazily filled cache) read and written from request threads without a lock or a concurrent collection.

### For type-design

- `string` or `int` ids for different aggregates passed through the same signature; a `readonly record struct` wrapper costs nothing at runtime and makes a swapped argument a compile error.
- A closed set of values as `string` constants where an `enum` would serve, and an `enum` deserialised from external input without `Enum.IsDefined`, so any integer becomes a "valid" member.
- A `bool` pair or a nullable pair (`IsDeleted` + `DeletedAt`, `Error` + `Value`) that admits combinations the domain forbids; a discriminated shape written as a class hierarchy with a `Kind` property duplicating the runtime type.
- `required` members (C# 11) or constructor parameters missing where an object is invalid without the value, leaving the type constructible half-initialised with `new T { }`.
- A `record` used as an entity with identity, so value equality compares every property, and a `with` expression copies an object that should have one identity.
- A public mutable `List<T>` property where `IReadOnlyList<T>` and a method would keep the invariant inside the type.

### For idioms

Decide every idiom question from the project's own history first: the `LangVersion`, analyser set and `.editorconfig` say what the team adopted.

- Pattern matching (`is`, `switch` expressions, property patterns) available but the code chains `as` and null checks; a primary constructor or file-scoped namespace adopted elsewhere in the project and not here.
- `ArgumentNullException.ThrowIfNull` and `ArgumentException.ThrowIfNullOrEmpty` available (.NET 6 and later, .NET 7 and later for the latter) but hand-written guard clauses used.
- `string.Format` or concatenation in a log call where the project uses structured logging with message templates or `LoggerMessage` source generation; interpolated strings passed to `ILogger` lose the named properties.
- `Task.Run` wrapping a synchronous call inside an ASP.NET Core request, which only moves the work to another pool thread; synchronous methods named `...Async`, and async methods missing the `Async` suffix where the codebase uses it.
- Hand-written `Equals`/`GetHashCode` where a `record` or `HashCode.Combine` is the established form; `Newtonsoft.Json` and `System.Text.Json` attributes mixed on one model.
- `DateTime.UtcNow` and `Guid.NewGuid()` called inline in code whose neighbours take `TimeProvider` (.NET 8 and later) or an injected clock.

### For memory-runtime

- **Boxing.** A struct passed as `object` or a non-generic interface, a `List<T>` or `Dictionary<TKey, TValue>` enumerated through `IEnumerable<T>` (its struct enumerator is boxed), `string.Format` and `params object[]` overloads with value-type arguments, an `enum` used as a dictionary key or in `HasFlag` on older runtimes, and a `Dictionary<TStruct, …>` whose struct key does not implement `IEquatable<T>` (falls back to the reflective `ValueType.Equals`).
- **LINQ on hot paths.** Each operator allocates an enumerator and its delegate, and a lambda capturing a local allocates a closure per call; `Any()`, `Count()` and `ToList()` over a `List<T>` re-enumerate. Flag LINQ chains inside a per-item or per-request loop where the workload is large; leave one-shot start-up code alone.
- **Strings.** Concatenation in a loop instead of `StringBuilder`, `Substring` and `Split` where a `ReadOnlySpan<char>` slice would serve, and `ToLower()` for a case-insensitive comparison instead of `StringComparison.OrdinalIgnoreCase`.
- **Large buffers.** Arrays of 85,000 bytes or more land on the large object heap and are collected only with gen 2; a per-request `new byte[…]` of that size is a candidate for `ArrayPool<T>.Shared` or a `RecyclableMemoryStream`. A rented array returned twice or used after return corrupts another caller.
- **Devirtualisation.** A non-`sealed` class whose virtual or interface calls sit in a hot loop blocks devirtualisation and inlining; calls through `IEnumerable<T>` or `IList<T>` on what is always a `List<T>` or array take interface dispatch. Dynamic PGO on .NET 8 and later recovers part of this, so ask for a measurement before claiming a gain.
- **Generic specialisation.** Generic methods over value types get a specialised body per type; over reference types they share one canonical body with dictionary lookups, so a generic hot path over a reference type pays that.
- **AOT and trimming.** Under `<PublishAot>true</PublishAot>` or `<IsTrimmable>true</IsTrimmable>`, reflection, `Activator.CreateInstance`, `Type.GetType(string)`, `System.Text.Json` without a source-generated `JsonSerializerContext` and `dynamic` produce trim or AOT warnings and runtime failures; `MakeGenericType` over value types may have no compiled code. Treat a new `IL2026`/`IL3050` warning suppression as a defect unless it is justified beside the call.
- **Retention.** Static caches with no eviction, event handlers subscribed to a longer-lived publisher and never removed, and a `MemoryCache` without a size limit keep objects alive for the process life.
- **Finalisers.** A type with a finaliser survives one extra collection; `GC.SuppressFinalize` missing from its `Dispose` doubles that cost per instance.

### For concurrency

- **Sync over async.** `.Result`, `.Wait()` or `.GetAwaiter().GetResult()` on a request path blocks a thread-pool thread for the full I/O; under load the pool injects threads slowly and latency climbs before anything fails. Under a single-threaded `SynchronizationContext` (classic ASP.NET, WinForms, WPF) it deadlocks unless every await below used `ConfigureAwait(false)`.
- **Async over sync.** `Task.Run(() => SyncIo())` in a server only moves the blocked thread; a truly async API (`ReadAsync`, `ExecuteReaderAsync`) frees it.
- **`ConfigureAwait`.** Library code that may run under a `SynchronizationContext` should use `ConfigureAwait(false)`; ASP.NET Core application code has no context, so its absence there is not a finding.
- **Locks.** `lock` cannot contain an `await` (compile error), so code works around it with `SemaphoreSlim(1,1)`; check every `WaitAsync` has a `Release` in a `finally`. `Monitor`, `ReaderWriterLockSlim` and `Mutex` held across an `await` are thread-affine and fail on release from another thread. On .NET 9 and later, check whether the project adopted `System.Threading.Lock`.
- **Fan-out.** `Task.WhenAll(items.Select(DoAsync))` over an unbounded input starts every task at once; bound it with `Parallel.ForEachAsync` and `MaxDegreeOfParallelism`, or a `SemaphoreSlim`. `Parallel.For` and `Parallel.ForEach` on async lambdas become `async void` and do not wait.
- **Channels.** `Channel.CreateUnbounded<T>()` with a producer faster than its consumer grows without limit; `CreateBounded` with a `FullMode` makes the backpressure explicit.
- **Fire and forget.** `_ = DoAsync();` or an unawaited task loses its exception and its lifetime; in ASP.NET Core it outlives the request scope and its scoped services. A `BackgroundService` or a queue drained by one is the tracked form.
- **ValueTask.** A `ValueTask` awaited twice, or awaited after `.AsTask()`, is undefined behaviour; it is a single-consumption type.
- **Thread-pool starvation.** A synchronous wait inside a timer callback, a `Parallel` loop or a hosted service on the shared pool; `ThreadPool.SetMinThreads` raised as a workaround is a symptom to name, not a fix.
