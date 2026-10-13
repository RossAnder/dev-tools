+++
id = "rust"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["lang:rust"] }
attaches_to = ["correctness", "type-design", "idioms", "memory-runtime", "algorithm", "hygiene"]
+++

# rust

## General

Read the crate's `Cargo.toml` before judging any pitfall below: `edition` and `rust-version` decide which language rules apply, `[features]` decides which `cfg` branches exist, and `[profile.*]` (plus any `.cargo/config.toml` profile, which overrides the manifest's) decides whether overflow checks and debug assertions run. A pitfall is a defect only when the code in the slice reaches it on an input or state you can name; the lens's own bar still applies.

- **Edition 2024 temporary scopes.** Since edition 2024 an `if let` scrutinee's temporaries drop before the `else` branch, and a block's tail-expression temporaries drop before its locals. Code ported from 2021 that relied on a guard living through `else` changes behaviour; code that re-locks in `else` stops deadlocking. A `match` scrutinee's temporaries still live for the whole `match`, so `match state.lock().unwrap().phase { … }` holds the lock through every arm, and an arm that locks it again deadlocks.
- **`std::env::set_var` and `remove_var` are `unsafe` in edition 2024** because another thread may read the environment at the same time. A test that mutates the environment races every other test in the same process under `cargo test`'s thread-per-test runner, while nextest's process-per-test runner hides the race.
- **Release builds wrap on integer overflow** unless the profile sets `overflow-checks`. Arithmetic on lengths, offsets, counters and durations that panics in a debug test silently wraps in the shipped binary.
- **`as` casts never fail.** `u64 as u32` truncates, `i64 as usize` turns a negative into a huge index, and `f64 as u64` saturates and maps NaN to 0. A cast across a range the value can leave is a defect; `try_from` is the checked form.
- **`unsafe` blocks** need a `// SAFETY:` comment that names the invariant, and the invariant must hold on every path into the block, not only the tested one.
- **CRLF and Windows paths.** `str::lines` strips a trailing `\r`, but `split('\n')` and `BufRead::read_line` keep it, so a comparison or a parsed key can carry a stray `\r` on a CRLF file. `Path::join` with an absolute or drive-prefixed component discards the base. `fs::rename` over a file another process holds open fails on Windows with `os error 5`, which an antivirus or indexer can cause on its own.

## By lens

### For correctness

- `unwrap()`, `expect()` and slice or map indexing on data that came from a file, the command line, the network or another process: each is a panic on a nameable input. Flag the reachable ones, naming the input; an `expect` on a documented invariant of the program's own data is not one.
- `&s[..n]` and other byte-range slicing of a `str` panic when `n` is not on a character boundary. Truncation for display or for a length cap is the common site.
- Errors lost on the way out: `BufWriter` flushes on drop and discards the error, so a write path that returns without calling `flush()` can report success after a failed write. The same holds for a `File` dropped with no `sync_all` where durability is claimed.
- `.ok()`, `.unwrap_or_default()`, `let _ =` and `if let Ok(..)` with no `else` on a `Result` that carries a failure the caller needs. Name the failure and what the caller then does with the default.
- `HashMap` and `HashSet` iteration order differs between runs. Output, a hash, a file written or a test assertion built from that order is non-deterministic; `BTreeMap` or a sort fixes it.
- `partial_cmp(..).unwrap()` in a sort comparator panics on NaN, and a comparator that is not a total order can panic or produce an unspecified order in the standard sorts.
- A write sequence with an early `?` between its steps leaves a partial result on disk. Check for the write-to-temp-then-rename pattern where the code claims atomicity, and for a rename across volumes, which fails.
- `std::process::Command` inherits the parent's environment and current directory unless told otherwise; a bare program name is resolved through `PATH`, and on Windows an empty `PATH` entry or a same-named directory in the working directory can shadow the real binary.

### For type-design

- A `pub` struct with all-`pub` fields lets any caller build a state the constructor would refuse. Where the type has an invariant, private fields plus a validating constructor or `TryFrom` is the form to ask for.
- A `pub` enum or struct crossing a crate boundary without `#[non_exhaustive]` makes every added variant or field a breaking change for downstream crates; inside one crate that cost does not apply.
- serde attributes that widen what deserialises: `#[serde(default)]` turns a missing required field into a silent default; a config or ledger struct without `#[serde(deny_unknown_fields)]` accepts a misspelt key and ignores it; `#[serde(untagged)]` tries variants in order and reports only "did not match any variant", so a typo silently selects the wrong variant or fails with no useful message.
- A newtype that implements `Deref` to its inner type, or `From` in both directions, gives back every operation the newtype existed to withhold.
- `Option<bool>`, two `bool` parameters side by side, and `(String, String)` tuples crossing a module boundary are the illegal-state and untyped-primitive shapes this lens names; a library-facing function returning `anyhow::Error` hides variants a caller needs to match on, where an application binary using it is normal.

### For idioms

- Before flagging a language idiom, read the `LINT BASELINE`: clippy already reports needless clones, manual `map`, `&String` and `&Vec<T>` parameters, redundant closures and most iterator rewrites. Do not restate a lint it reports or a lint the crate's `[lints]` table or an `#[allow]` turns off deliberately.
- What clippy cannot see is the crate's own converged pattern: its error type and context style (`anyhow::Context` versus `map_err`), its path and IO helpers, its test-fixture builders, its CLI argument structs. Decide the pattern from history with the history-idiom method, then flag a new site that rolls its own.
- `clone()` added to satisfy the borrow checker where a borrow, a `mem::take` or a restructured scope serves; `collect::<Vec<_>>()` only to iterate once; `match` on an `Option` or `Result` that `?`, `let else` or a combinator expresses; a manual `entry`-style lookup-then-insert on a map.
- `Box<dyn Trait>` where every call site knows the concrete type, and generics where one concrete type ever exists, are both idiom questions only when the codebase has settled the other way.

### For memory-runtime

- Perf claims need an optimised build: a measurement or a `PROFILE BASELINE` taken from a debug or `opt-level = 1` profile does not support an allocation or dispatch argument. Note which profile produced it.
- `Regex::new`, `format!`, `to_string()`, `to_owned()` and `clone()` of a `String`, `Vec` or `PathBuf` inside a per-item loop; a regex compiled per call where a `std::sync::LazyLock` or a compiled value passed in would compile it once.
- `fs::read_to_string` or `read` of an input that can be large, where a `BufReader` stream would hold one line or block; a `Vec` grown by `push` in a loop of known length without `with_capacity`.
- `Arc<Mutex<T>>` or `Rc<RefCell<T>>` around data with one owner; `Box<dyn Fn>` or `dyn Trait` dispatch inside a hot inner loop where the concrete type is known.
- Memory held past its use: a large `Vec` or buffer kept alive in a long-lived struct after it is consumed, a cache in a `static` or `LazyLock` with no bound, `String::with_capacity` sized for the worst case and kept.

### For algorithm

- `Vec::contains`, `iter().position` or `iter().any` inside a loop over another collection is quadratic; a `HashSet` or a sorted `binary_search` is the usual fix.
- `Vec::remove(0)` or `insert(0, ..)` in a loop is quadratic; `VecDeque` is the queue form.
- `s.chars().nth(i)` inside a loop over `i` rescans from the start each time; so does repeated `str::find` from position 0 on a growing buffer.
- `sort()` followed by `dedup()` is right for adjacent duplicates only; a dedup that needs the first occurrence kept in input order needs a seen-set.
- Recursion over a tree or graph whose depth follows the input has no stack bound in Rust; a deep input overflows the stack and aborts the process rather than returning an error.

### For hygiene

- rustc already warns on unused private items, imports and variables, so a row here must be something the compiler is silent about: a `pub` item in a library crate with no caller anywhere in the tree, an item kept alive only by `#[allow(dead_code)]` or `#[allow(unused)]`, or code reachable only from a `#[cfg(test)]` module that no test calls.
- A false positive costs a reverted edit. Before calling an item dead, search for uses the compiler resolves without a plain name: `#[test]` functions, `build.rs`, every `[[bin]]` and example target, items named in a macro body, `include_str!` and `include_bytes!` paths, a `#[no_mangle]` export, serde `with` and `default` attribute strings, and other crates in the same workspace.
- A `[features]` entry no `cfg(feature = "…")` checks, and a `cfg(feature = "…")` naming a feature the manifest does not declare (the `unexpected_cfgs` lint warns on the second since Rust 1.80).
- A dependency in `[dependencies]` with no `use`, path or macro reference in its package. Check `[dev-dependencies]` and `[build-dependencies]` separately, since a crate used only by tests belongs in the former.
