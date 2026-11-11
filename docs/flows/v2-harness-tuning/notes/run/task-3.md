Status: done — own_work now counts rows with a set agent and no commit, whatever their status.

- tomlctl/src/plan/drift.rs: `own_work` keeps the status rule and adds `|| agent non-empty`, both still requiring an empty commit. Added unit test `started_row_reset_to_pending_is_own_work`.
- Deviations: none.
- Checks: rustfmt --edition 2024 --check clean; narrow nextest run of the new test passed; accept grep passed. Compile happened via the nextest build.

TANGENTIAL: none
