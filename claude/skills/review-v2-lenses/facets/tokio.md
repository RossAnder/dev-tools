+++
id = "tokio"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["runtime:tokio"] }
attaches_to = ["concurrency", "correctness"]
+++

# tokio

## General

Read how the runtime is built before judging anything below: `#[tokio::main]` defaults to the multi-thread flavor with one worker per core, `#[tokio::test]` defaults to `current_thread`, and an explicit `Builder` can set `worker_threads`, `max_blocking_threads` (512 by default) and a `shutdown_timeout`. The flavor changes what a pitfall costs: on `current_thread` one blocking call stalls every task, and `block_in_place` panics.

- **Handles.** Dropping a `JoinHandle` detaches the task, so it keeps running and its panic or error reaches nobody. Dropping a `JoinSet` aborts every task in it. `abort()` stops a task only at its next `.await`.
- **Blocking work.** `spawn_blocking` and `block_in_place` work cannot be aborted, and runtime shutdown waits for blocking tasks to finish unless a `shutdown_timeout` bounds it. `tokio::fs` runs each call on the blocking pool, so many small `tokio::fs` calls each pay a thread hand-off.
- **Locks.** A `std::sync::Mutex` guard held across an `.await` makes the future `!Send`, which `tokio::spawn` refuses at compile time, but `spawn_local`, `block_on` and a `LocalSet` accept it. `tokio::sync::RwLock` is write-preferring: a task that holds a read lock and asks for another read lock deadlocks once a writer is queued between the two.
- **Channels.** `mpsc::unbounded_channel` has no backpressure. `broadcast::Receiver::recv` returns `RecvError::Lagged` when the receiver fell behind, and the channel is still open afterwards; treating `Lagged` as closed ends a consumer early. A `watch::Ref` from `borrow()` holds a read lock, so keeping it across an `.await` blocks every sender.
- **Child processes.** `tokio::process::Command` defaults to `kill_on_drop(false)`, so a cancelled or timed-out task leaves its child running.

## By lens

### For concurrency

These carry the runtime specifics of the v1 async and concurrency agent: blocking work dispatched off the async pool, pool sizing, bounded spawning and backpressure.

- Synchronous calls inside an `async fn`: `std::fs`, `std::process::Command::output`, `std::thread::sleep`, a `std::sync::Mutex` contended by other threads, a blocking HTTP or database client, and a CPU-bound loop whose trip count follows the input. Each holds a worker thread; on a per-request path, name how many workers there are and how long the call holds one. The fix is `spawn_blocking` for bounded blocking work, a dedicated thread or pool for long-running work, and the async equivalent where one exists.
- `spawn_blocking` called per item in a fan-out with no bound queues on a pool of up to 512 threads, each with its own stack; a `Semaphore` or a bounded `JoinSet` drain caps it. A runtime built with a small `max_blocking_threads` turns the same pattern into a stall.
- `tokio::spawn` inside a loop over input with no `Semaphore`, no `JoinSet` and no `buffer_unordered(n)` bound; `futures::future::join_all` over an unbounded list, which polls every future at once.
- A `tokio::sync::Mutex` held across a slow `.await` (I/O, a channel send that can wait, a sleep) serialises every caller behind it. Where nothing awaits inside the critical section, a `std::sync::Mutex` is cheaper, as tokio's own documentation advises; where the section must await, narrow it to the state it protects or move the state behind a channel to one owner task.
- `tokio::time::interval` defaults to `MissedTickBehavior::Burst`: after a stall it fires the missed ticks back to back, so a periodic job meant to run once per period runs several times in a row. `Delay` or `Skip` is the usual intent.
- `mpsc::channel(n)` with a large `n` chosen to make a test pass is unbounded in practice; size it from the producer rate and the consumer's cost.
- Awaits written in sequence that do not depend on each other, such as reads of several rows or files on one request, where `tokio::join!` or `try_join!` would run them together. Check the resource they share first: a pool of `max_connections` connections caps the real parallelism, and SQLite serialises writers whatever the pool size.
- Shutdown: tasks spawned without a tracked handle or a cancellation token (`tokio_util::sync::CancellationToken`) are cut off mid-write when `main` returns, and blocking tasks delay exit with no bound.

### For correctness

- `select!` drops every branch's future that did not finish. A branch whose future is not cancel-safe loses data when another branch wins: `AsyncReadExt::read_exact`, `read_to_end`, `read_line` with partial progress, and any hand-written future that buffers. `mpsc::Receiver::recv` and `broadcast::Receiver::recv` are cancel-safe; check the documentation's "Cancel safety" section for anything else used in a branch.
- A `select!` inside a loop that creates `sleep(d)` in a branch restarts the timer every iteration, so a timeout never fires while the other branch keeps waking; create the `Sleep` once outside the loop and pin it.
- `Notify::notify_waiters` wakes only tasks already waiting and stores no permit, so a task that starts waiting after the call misses it. `notify_one` stores one permit. A check-then-wait sequence that does not call `notified()` before the check has a lost-wakeup window.
- A spawned task's panic surfaces only as a `JoinError` from its handle. Code that never awaits the handle, or that maps a `JoinError` to `Ok`, reports success for work that crashed.
- `Runtime::block_on` or `Handle::block_on` called from inside an async context panics, as does building a second runtime there. Library code that calls `block_on` to bridge sync and async breaks when a caller already runs on tokio.
- `#[tokio::test]` runs on `current_thread`, so a test of concurrent behaviour runs its tasks interleaved on one thread and cannot observe a data race or a lock-order deadlock between threads; `flavor = "multi_thread"` is needed for that.
- A future that holds state across an `.await` while another task can observe it half-updated: two fields of shared state written with an `.await` between them, or a read-check-await-write on state behind a lock released before the await.
- A child process whose task is cancelled keeps running unless `kill_on_drop(true)` is set or the task kills it itself, and a child whose stdout pipe nobody reads can fill the pipe and block forever.
