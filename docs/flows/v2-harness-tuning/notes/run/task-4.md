Status: done — overlap_findings now skips pairs holding a done row with a commit

- tomlctl/src/tasks/check.rs: overlap_findings filters out pairs where either row is Done with a non-empty commit.
- tomlctl/tests/tasks_graph.rs: added unreachable_claim_skips_done_committed_rows (settled row raises no warning; unsettled pair still warns).
- Checks: narrow nextest test passes (compiled the crate); rustfmt --check clean on the test file; accept grep passes. No separate clippy run.

TANGENTIAL: none
