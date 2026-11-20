# Records

The store shapes milestone A adds. Every new field is optional on read, so a store written by an older binary still loads; a write stamps the schema version named below only when the store holds one of the new shapes, so an older binary keeps reading stores that use none of them.

## Plan store schema 7

`.claude/flows/<slug>/plan.toml` gains schema version 7, a new rung above `VISUAL_ACCEPT_SCHEMA_VERSION` (6) in `tomlctl/src/plan/schema.rs`. A write stamps 7 when the store holds any of: a `[[gate_accept]]` row, a gate command with `base_checked = true`, a gate round with a non-empty `tree`, a handover with non-empty `blocks`, `ready_check` or `note`, an attempt with non-empty `sources`, or an approval with a non-empty `delta_waiver`. `plan-store.md` documents each shape below.

### Gate acceptance

```toml
[[gate_accept]]
milestone = "A"
round = 2
at = 2026-11-21T10:00:00Z
kind = "accept"            # "accept", "extra-round" or "base"
why = "two browser cases flake under load; solo reruns pass"
backlog = "B-1234abcd"     # required for "accept" and "base", empty for "extra-round"

[[gate_accept.commands]]
command = "bun run test:ui"
failed_ids = ["|browser| src/a.test.ts > renders"]
```

Keyed on `(milestone, round)`: a second record for the same pair replaces the first. Each `commands` entry must name a command whose outcome is `fail` in the latest gate run of that round, with `failed_ids` a subset of that run's; for `kind = "accept"` every failing command of that run must be named. An `extra-round` record raises the milestone's round cap by one for each such record. Any record is refused while the round's latest run holds a `timeout` command: the drain's timeout rule (one rerun with a larger budget) settles it first. A `base` record waives only failures that predate the milestone: each `commands` entry names a command that is a base or partial base failure in that run, with `failed_ids` a subset of its `base_failed_ids` (or the command alone when `base_whole_failed` is true); it need not name every failing command and does not by itself satisfy the commit condition. A `base` record is keyed on `(milestone, round, kind)` beside the others, and in later rounds of the milestone the drain treats its waived ids as passing when they fail again.

### Base rerun on a gate command

`[[gate_round.commands]]` gains three fields: the two below, and `base_whole_failed` (bool, written only when true), set by a whole-command base rerun as Base rerun mapping says:

```toml
base_checked = true
base_failed_ids = ["agents_audit::audit_reports_a_bun_suite_and_cuts_long_commands"]
```

`base_checked` is true once the command's failed tests were rerun at the milestone's base. `base_failed_ids` is the subset of `failed_ids` that also failed there. A command is a base failure when `base_checked` is true and either `failed_ids` is non-empty and `base_failed_ids` equals it as sets, or `base_whole_failed` is true and the command is one of the milestone's `gates`; it is a partial base failure when `base_failed_ids` is a non-empty proper subset.

### Tree digest on a gate round

`[[gate_round]]` gains `tree`, written when the recorded run's outcome is `fail`:

```toml
[[gate_round.tree]]
path = "tomlctl/tests/agents_audit.rs"
raw = "<sha256 of the file's bytes>"
stripped = "<sha256 of the file's bytes with every ASCII whitespace byte removed>"
```

It holds one entry per path in the `files` of every row of the milestone at record time, whether or not it changed, read from disk with no git call. A path that does not exist is recorded with `raw = "deleted"` and `stripped = "deleted"`.

### Base rerun mapping

`plan run gate --base` maps base results to the round's commands by order: the drain reruns every `fail` command of the round's latest run at the base, in the order that run lists them, a command with `failed_ids` narrowed to those ids and one without run whole, except a held command (one the milestone's `external_gates` or an `effect = "external"` accept marks, or one the drain held by judgement), which is never run at base: its place in the base call is a `skip` block that leaves it `base_checked = false`. The i-th block or row of the base call belongs to the i-th failing command, and a count that differs is refused. A base result's failed ids that are not among the round command's `failed_ids` are ignored, and a whole-command rerun that fails with no ids sets `base_failed_ids` to the command's own `failed_ids`, or leaves it empty when the command had none, and sets `base_whole_failed = true`, which makes it a base failure only when the command is one of the milestone's `gates`. A forward accept check is never rerun at base, since it fails there by construction, and takes a `skip` block like a held command. The drain provisions the worktree with the project's setup step before the rerun, and reports an id-less base failure it could not provision for as unverified, not pre-existing.

### Handover blocks, ready check and note

`[[handover]]` gains:

```toml
blocks = [6]                       # task ids that may not launch until this handover is done
ready_check = "grep -q '^CLOUDFLARE_API_TOKEN=' .env"   # read-only; exits 0 when the input is in place
note = "verified on the dashboard 2026-11-21"           # set by the orchestrator when marking done
```

`blocks` names task ids. `ready_check` is a read-only command the orchestrator runs, never tomlctl. `note` records what was observed when the handover was marked done. Neither `note` nor `ready_check` ever holds a secret's value, since plan.toml is committed: a note names where an input lives and its kind, and a ready_check tests presence (`grep -q '^NAME=' .env`, `test -n "$NAME"`) without printing it. All three are written through `plan handover set`.

### Run-note sources

An attempt (`[[attempt]]`) gains `sources`, parsed from a `[[sources]]` table in the run note's block:

```toml
[[sources]]
url = "https://developers.cloudflare.com/analytics/graphql-api/"   # url or query, at least one
query = ""
claim = "workersInvocationsAdaptive carries cpuTimeUs per scriptVersion"
how_verified = "queried live with the account token; 3 rows returned"
fetched_at = 2026-11-20
version = ""
```

`claim` is required, and so is one of `url` or `query`. `fetched_at` (a date, required) and `version` (the library, API or platform version the claim holds for, optional) travel with the entry into `docs/references/`.

### Approval delta waiver

`[[approval]]` gains `delta_waiver`, the user's reason for approving with plan records changed since the last finished review run left unreviewed. It is empty otherwise.

## Task store milestone fields

`[[milestones]]` in `.claude/flows/<slug>/tasks.toml` gains:

```toml
baseline_gates = ["bun run parity:static-identity"]   # each must also be in gates or external_gates
ui = true                  # the Design or elaboration call: does this milestone add or reshape a UI surface

[[milestones.baseline_proofs]]
gate = "bun run parity:static-identity"
verdict = "pass"          # "pass", "fail" or "null-control"
at = 2026-11-21T10:00:00Z
```

A milestone holding a baseline gate stamps the next task-store schema version above the one the external-effect mark introduced. A baseline gate whose latest proof is `fail` is unproved: `plan check` reports it until a later proof passes or a decision records the user accepting it, and Prove puts it to the user before approval. A `null-control` proof also carries `task`, the id of the milestone task whose accept runs the gate's tool on the base against itself; `plan/baseline-gate-unproved` treats a `null-control` proof as missing when `task` names no task of the milestone or a task with no accept entry. `ui` is absent until Design or elaboration records the call through `plan milestone add` or `update --ui yes|no`; `plan/ui-capture-missing` warns on a milestone with `ui = true` and no `ui_states`, with or without a capture config. An accept entry with `run_by = "worker"` and `writes_build = true` is now accepted, and changes no stamp.

## Review run plan digest

A flow-mode plan-family review run (a `review run start` on a flow with `plan.toml`) records `plan_digest` on its run row in the review store: a table from each plan target, in the target grammar of `claude/skills/review-v2-method/references/plan-records.md`, to the SHA-256 of that target's current value. A full run's `plan_digest` holds every plan target; a re-check run's holds only the targets it re-checks, which its start derives from the run its `rechecks` names (that run's `review recheck-targets`, or the review delta when approve.md opens the re-check). The review delta is the set of targets whose current digest differs from, or is absent in, the `plan_digest` of the latest finished run whose digest holds that target. It refuses only a first approval; under `plan approve --amend` it is reported in `plan stage` and as a `review-delta` risk in `plan gate`, and never refuses.

## Emit outputs

- `plan emit gate-list --after-round <n>`: it compares every path in the `files` of the milestone's rows now against round n's latest run's `tree`. A path whose current `raw` equals the stored one is unchanged. A path whose `raw` differs but whose `stripped` equals the stored one is whitespace-only, unless it is whitespace-significant (`.py`, `.yaml`, `.yml`, `.md`, `.sh`, `.toml`, `.json`, `Makefile`). When every path is unchanged or whitespace-only, and no path is missing from the tree, it prints `commands` as round n's commands whose outcome was `fail` or `timeout`, adds `carried` (the commands whose outcome was `pass` or `flaky`) and sets `narrowed = true`. Any other case, including a path with no tree entry or a round with no tree, prints the full list with `narrowed = false`. Held commands still owed print under `held` as today. Accepted limits: a spacing change inside a string literal, or one that merges two tokens, reads as whitespace-only, and a formatter edit that adds a token (rustfmt's trailing comma) reads as a change and keeps the full list.
- `plan emit launch-args`: each task entry gains `builds = true` when the task holds an accept entry with `run_by = "worker"` and `writes_build = true`. A task named in the `blocks` of a handover that is not `done` is withheld with the reason `handover:<handover id>`.
- `plan drift` gains `affected_decisions`, one `{id, binds_hit, commits}` entry per decision whose file `binds` intersect `changed_files`. `commits` holds the unowned commits that touched those files.
