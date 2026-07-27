---
name: adopt-flow-registry
description: One-time migration procedure for switching an existing repo onto the tomlctl-backed flow registry (.claude/active-flow.toml). DESTRUCTIVE — it deletes all per-flow history under .claude/flows/. Consult only when adopting the registry in a repo that still uses the legacy single-line .claude/active-flow file.
---

# Adopting the flow registry

When switching an existing repo to the `tomlctl`-backed flow registry (`.claude/active-flow.toml`), perform this one-time migration:

> **WARNING: this procedure permanently destroys flow history.** Step 1 (`rm -rf .claude/flows/`) deletes every per-flow directory including `execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`, and `plan-review-findings.toml`. There is no `tomlctl flow migrate` command yet — the planned migration tool is out of scope for this initial overhaul. If you have in-flight flows whose history matters, back up `.claude/flows/` (e.g. `cp -r .claude/flows/ .claude/flows.bak/`) before running step 1, or skip the migration entirely until a migrate command lands.

1. Clear the old per-flow state directories: `rm -rf .claude/flows/`
2. Delete the legacy single-line active-flow file: `rm -f .claude/active-flow`
3. For each flow that should be recreated, run:
   ```bash
   tomlctl flow init --slug <slug> --plan <path/to/plan.md>
   ```
   This seeds `context.toml` + `execution-record.toml` under `.claude/flows/<slug>/` and registers the flow in `.claude/active-flow.toml`.

After migration, all flow commands (`tomlctl flow list`, `tomlctl flow resolve`, etc.) read from `.claude/active-flow.toml` exclusively; the legacy `.claude/active-flow` file is ignored.
