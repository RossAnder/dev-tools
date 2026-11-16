Status: done — added the `after` key to the expected milestone JSON in the v2 snapshot test

```toml run-note
files_touched = ["tomlctl/tests/tasks_snapshot.rs"]
```

## What changed

`tomlctl/tests/tasks_snapshot.rs`: the expected milestone object in `a_v2_store_carries_its_milestones_and_change_trail` now has `"after": []` after `gate_probes`. The accept check is marked `Run by: gate`, so I did not run it.
