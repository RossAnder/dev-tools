Status: done — formatted tomlctl/src/plan/check.rs

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check.rs"
result = "pass"
```

## What changed

check.rs declares `mod` children, so rustfmt was not applied in place. The one reported diff, the `ui_findings(...)` call, was wrapped by hand with Edit as rustfmt printed it. The check then passed. No code change.
