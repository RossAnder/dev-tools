Status: done — review cover now reads the ruled-out reason cap from fields.rs

- tomlctl/src/review/fields.rs: `RULED_OUT_REASON_CAP` made `pub(crate)`.
- tomlctl/src/review/cover.rs: removed `const REASON_CAP` and its doc comment; call site uses `fields::RULED_OUT_REASON_CAP`.

Deviations: none.

Checks: `cargo clippy --manifest-path tomlctl/Cargo.toml` clean; accept grep check passes.

TANGENTIAL: none
