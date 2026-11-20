Status: done — ten milestone A features advertised and tomlctl bumped to 0.27.0

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md", "tomlctl/Cargo.toml", "Cargo.lock"]

[[checks]]
command = "grep -q gate_accept/review_plan_digest in mod.rs && grep version 0.27.0 in Cargo.toml"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/capabilities.rs"
result = "pass"
```

## What changed
Added the ten feature names to FEATURES, the pinned list in capabilities.rs, the README sample block and feature table. Bumped version to 0.27.0 in Cargo.toml, Cargo.lock and the test pin. The README one-line descriptions were written from the feature names and task intent, so the milestone owners may want to tighten them. The nextest capabilities check is the gate's.
