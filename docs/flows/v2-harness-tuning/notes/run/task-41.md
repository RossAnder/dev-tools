Status: done — plan emit leaves now print through print_json_line

- tomlctl/src/plan/emit.rs: replaced the `print_json_compact` import and its three calls (launch_args, gate_list, reproduce_units) with `print_json_line`.
- Deviations: none.
- Checks: cargo clippy on tomlctl clean; rustfmt --edition 2024 --check clean; the accept grep check passes. The suggested nextest case (output_options) was not run, as it builds tests under the shared lock; it is left to the gate.

TANGENTIAL: none
