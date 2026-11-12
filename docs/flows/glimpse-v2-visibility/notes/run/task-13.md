Status: done — five feature names appended to FEATURES, the capabilities test and both README tables.

Changed:
- tomlctl/src/cli/types/mod.rs: appended snapshot_plan_records, plan_run_records, authored_files, agent_usage, review_finding_read.
- tomlctl/tests/capabilities.rs: same five, same order, in the expected list.
- tomlctl/README.md: names added to the sample block and one row each in the feature table. authored_files and review_finding_read are library functions (authored_list, authored_read, review_finding), not CLI verbs, and their rows say so.

Deviations: none.

Checks: the worker-run grep accept checks pass (each name found in mod.rs, capabilities.rs and the README). rustfmt --check on tests/capabilities.rs is clean. The nextest capabilities check is the gate's and was not run.

TANGENTIAL: none
