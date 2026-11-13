Status: done — stdin readers, the stdin claim, advisories and the four environment reads go through the invocation accessors

```toml run-note
files_touched = ["tomlctl/src/io.rs", "tomlctl/src/items.rs", "tomlctl/src/review/catalogue.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/io.rs tomlctl/src/items.rs tomlctl/src/review/catalogue.rs"
result = "pass (after applying rustfmt to catalogue.rs)"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass; only dead-code warnings in tomlctl/src/invocation.rs, whose remaining accessors other tasks wire up"

[[checks]]
command = "grep -q 'invocation::stdin_reader' tomlctl/src/io.rs && grep -q 'invocation::env_var_os' tomlctl/src/io.rs"
result = "pass"

[[checks]]
command = "grep -q 'invocation::env_var_os' tomlctl/src/items.rs && grep -q 'invocation::env_var_os' tomlctl/src/review/catalogue.rs"
result = "pass"
```

## What changed

Both `after:` premises held before editing (`STDIN_CONSUMED` is still a process-wide static; `fn env_var_os` exists in `invocation.rs`).

- `tomlctl/src/io.rs`
  - `read_json_arg`, `read_text_arg` and `read_json_value_from_arg` ask `invocation::stdin_is_terminal()` and read from `invocation::stdin_reader()`.
  - `claim_stdin` takes the context's flag from `invocation::claim_stdin()` and falls back to the `STDIN_CONSUMED` swap when no context is installed.
  - `advise_fmt` returns early when `invocation::is_active()`.
  - `resolve_lock_timeout`, `repo_or_cwd_root` and `ensure_process_root` read `TOMLCTL_LOCK_TIMEOUT` / `TOMLCTL_ROOT` through `invocation::env_var_os`. Each `std::env::var(..).ok()` became `env_var_os(..).and_then(|v| v.into_string().ok())`, so a non-UTF-8 value is still ignored as before. The `TOMLCTL_LOCK_TIMEOUT` read inside `mod tests` is unchanged. The cwd-derived `REPO_ROOT` cache stays process-wide, and so does the `repo_root::discover` probe closure, which reads git variables rather than any of the four.
- `tomlctl/src/items.rs`: `dedup_id_disabled` reads `TOMLCTL_NO_DEDUP_ID` through the accessor. `is_some_and(|v| v.into_string().is_ok())` keeps the old `env::var(..).is_ok()` behaviour exactly.
- `tomlctl/src/review/catalogue.rs`: `claude_dir` resolves through an inner `resolve()` that reads `CLAUDE_CONFIG_DIR` through the accessor. It bypasses the `OnceLock` while a context is installed and keeps it otherwise. `USERPROFILE` and `HOME` stay process reads, since the design lists only four variables.

Without a context, every accessor falls back to the process handle or the process environment, so the binary behaves as before.
