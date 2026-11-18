Status: done — added the partial-rule/change-item test and the later-run repost test

```toml run-note
files_touched = ["tomlctl/tests/plan_request.rs"]

[[checks]]
command = "cargo nextest run -E 'test(/^plan_request::(a_partial|a_task_may)/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_request.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_request.rs`: added `a_partial_rule_leaves_the_request_open_and_rules_a_change_item_by_index` and `a_task_may_post_again_in_a_later_run`. Both pass.
