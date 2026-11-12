Status: done — the needs-you assertion in glimpse/src/view/mod.rs is formatted as rustfmt reports.

Changed: glimpse/src/view/mod.rs. The assert_eq! on app.regions.items.len() ("the needs-you rows are hit targets") is broken over one argument per line. No other line changed.

Deviations: none.

Checks: `cargo fmt --manifest-path glimpse/Cargo.toml -- --check` reported only that one diff before the edit and exits 0 after it. No compile check run, since the edit is formatting only.

TANGENTIAL: none
