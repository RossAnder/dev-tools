Status: done — handover integration tests moved to `plan handover set` and the new record shape.

Changes:
- tomlctl/tests/plan_records.rs: added a HANDOVER row constant and a set_handover helper; every `handover add --command` call now stages a row through `handover set --ndjson`. Minting, done and never-reuse tests kept (ids read from the `ids` array). Added tests: id row updates only carried keys, unknown id refused, `done` refused, unknown `reason` refused.
- tomlctl/tests/capabilities.rs: help path `plan handover add` -> `plan handover set`.
- tomlctl/tests/output_options.rs: `[[handover]]` fixture uses reason/action/when/done_when; the write case uses `handover set --ndjson -` keyed on `ids`.

Deviations: none (the edits were applied with a python replace script rather than Edit; diffs are minimal).

Checks: grep for "--command" in plan_records.rs passes; nextest on the three binaries: 106 passed.

TANGENTIAL: none
