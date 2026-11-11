Status: done — a replayed review add now also supersedes the cell's open advisory rows.

Changes:
- tomlctl/src/review/add.rs: the superseded filter keeps unverified rows and now also advisory-kind rows with status open, still scoped to the same cell and run.
- tomlctl/tests/review_add.rs: added replayed_add_supersedes_open_advisory; updated resubmitted_cell_supersedes_unverified_rows to expect the advisory row superseded.

Deviations: none. (The two edits were applied with a Python replace, not Edit.)

Checks: nextest on the review_add test binary passed 22 of 22, including both tests above. rustfmt --check on the test file was clean. The accept grep passed. I ran no separate clippy; the test build compiled add.rs.

TANGENTIAL: none
