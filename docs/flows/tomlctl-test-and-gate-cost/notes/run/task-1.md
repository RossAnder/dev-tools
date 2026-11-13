Status: done — sandbox() now copies a cached `git init` template from the cargo target tmp directory

```toml run-note
files_touched = ["tomlctl/tests/common/mod.rs"]

[[tangential]]
kind = "question"
area = "tomlctl/tests/common/mod.rs"
summary = "The template's .git/config carries filesystem probes git makes at init time (core.symlinks, core.ignorecase, core.filemode) for the target tmp directory's volume, not the sandbox's"
why = "If CARGO_TARGET_TMPDIR and the system temp directory ever sit on filesystems with different capabilities, a sandbox's config would describe the wrong one; today both are on the same NTFS volume"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/common/mod.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test it"
result = "pass"

[[checks]]
command = "grep -q 'CARGO_TARGET_TMPDIR' tomlctl/tests/common/mod.rs"
result = "pass"
```

## What changed

`tomlctl/tests/common/mod.rs`:

- `sandbox()` no longer runs `git init`; it copies `<template>/.git` into the new root with `copy_tree`, which creates every directory, the empty `objects/info`, `objects/pack`, `refs/heads` and `refs/tags` included, and copies every file, hook samples included.
- `git_init_template()` caches the template path per process in a `OnceLock`, so in-process threads share one lookup.
- `build_git_init_template()` returns the template under `env!("CARGO_TARGET_TMPDIR")` when it exists; otherwise it runs `git init -q .` through `git_command` in a sibling directory named after the pid and a timestamp, then renames it to the fixed name. When the rename fails and the template now exists, the loser is deleted and the winner used; otherwise it retries up to 20 times, 50 ms apart, then panics naming the error. A git that cannot be started returns `None`, so the sandbox gets no `.git`, as before; a `git init` that runs and fails panics with the same `` `git init` failed in … `` message, now naming the build directory.
- `git_install_key()` names the template: the first 16 hex characters of a SHA-256 over the git executable found on PATH (path, size, modified time) and the modified time of `.gitconfig` in `HOME`, or `USERPROFILE` when `HOME` is unset, all read without starting a process.

The SHARED-BLOCK:shipped-gitignore span is untouched. The gate's nextest guard was left to the gate, as its `Run by:` line says.
