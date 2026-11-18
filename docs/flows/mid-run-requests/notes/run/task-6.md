Status: done — tomlctl version set to 0.25.0 in its manifest and the root lockfile

```toml run-note
files_touched = ["tomlctl/Cargo.toml", "Cargo.lock"]

[[checks]]
command = "grep -q '^version = \"0.25.0\"' tomlctl/Cargo.toml"
result = "pass"
```

## What changed

- tomlctl/Cargo.toml: version 0.24.0 to 0.25.0.
- Cargo.lock: the `tomlctl` package entry's version 0.24.0 to 0.25.0; nothing else changed.
