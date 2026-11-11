Status: done — Store::holds_v2_keys now counts only the entries to_toml writes, and to_toml calls it.

Changes:
- tomlctl/src/tasks/schema.rs: `Store::holds_v2_keys` counts specs and changes with a non-empty ref, milestones with a non-empty id, and non-empty finding_links (alongside audit and id_high_water). `to_toml` drops its local `holds_v2_keys` expression and matches on `store.holds_v2_keys()`.
- tomlctl/src/tasks/remove.rs: no edit. It already calls `store.holds_v2_keys()` and holds no inline copy.

Deviations: none.

Checks:
- `cargo clippy --manifest-path tomlctl/Cargo.toml`: clean.
- `rustfmt --edition 2024 --check tomlctl/src/tasks/schema.rs`: clean.
- The accept check (`review reproduce-status`, Writes build: yes) is left to the gate.

TANGENTIAL: none
