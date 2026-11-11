# Shapes

The data shapes and verb surfaces that more than one task of this plan reads or writes. A task cites one section; the task's own row says what it does with it. Every field here is additive: a store written before it reads as it did.

## Accept owner

An `accept` entry gains two optional keys.

| Key | Values | Absent means |
|---|---|---|
| `run_by` | `worker`, `gate`, `covered` | `worker` when `writes_build = false`, otherwise `gate` |
| `covered_by` | the exact command of one gate of the row's own milestone | empty; required when `run_by = "covered"` and refused otherwise |

The effective owner is `run_by` when set, else the value derived from `writes_build`. `writes_build` keeps one meaning: whether running the check builds, tests or writes.

- A check is probed only when its effective owner is `worker` and `writes_build = false`. `plan/probe-absent` follows the same rule.
- `run_by = "worker"` with `writes_build` true or absent is refused at write and reported by `plan check` as `plan/accept-owner-invalid`, an error: a worker never runs a check that builds.
- `plan check` reports `plan/accept-cover-unknown`, an error, when `covered_by` equals no entry of the `gates` of the row's `checkpoint`.
- The worker view keeps its `Writes build:` line and adds `Run by: worker`, `Run by: gate` or `Run by: gate (covered by <command>)` under each check. A worker runs the `worker` checks and no other.
- `tomlctl capabilities` advertises the feature `accept_run_by`. `plan_view_writes_build` stays.

## Fix files

A finding gains `fix_files`: a list of at most 8 repo-relative paths, allowed on defect and cleanup rows, naming every file the fix edits or creates beyond the finding's own anchor, test files included. It is visible to the finder, verifier and implementer roles.

`review schedule` adds a finding's `fix_files` to the files of the row it mints, and groups findings by the widened set. `Finding::edited_paths` does not read the field, so the test-removal hold-back and the cleanup route are unchanged. A `fix_files` path absent from the working tree is written to the row's `file_notes` as `(new)`. The lite rule counts the widened set.

## Reproduce status by milestone

`tomlctl review reproduce-status` takes, in place of its positional ids, `--flow <slug> --milestone <id>`: the findings are the `fixes` of every `[[finding_links]]` entry on a row of that flow whose `checkpoint` is the milestone and whose `store` is the store the call names. With no such finding the call prints an empty `findings` and exits 0, or 1 under `--expect`, since a fix row carries the check only when its milestone has findings to prove. The positional form is unchanged, and the two forms exclude each other.

`review schedule` gives every fix row that is not a `remove` cleanup this one accept entry, the same string on every row of the milestone:

```toml
behaviour = "every finding this milestone fixes no longer reproduces"
check = "tomlctl review reproduce-status --flow <slug> --milestone <id> <store-flag> --expect gone"
kind = "forward"
run_by = "gate"
writes_build = false
```

A `remove` cleanup keeps its `review anchor <F> --gone` entry, which the worker runs.

## Emit verbs

Three read-only verbs under a new `tomlctl plan emit` group, in one module, `tomlctl/src/plan/emit.rs`. Each prints a single JSON object and writes nothing. The feature is `plan_emit`.

### launch-args

`tomlctl plan emit launch-args --slug <slug> --milestone <id> [--max-parallel <n>]`

```json
{"args": {"slug": "", "milestone": "", "root": "", "max_parallel": 4,
          "tasks": [{"id": 1, "executor": "lite", "needs": [], "after": [], "files": []}]},
 "args_sha256": "",
 "after_checks": [{"task": 2, "premise": 0, "after": 1, "check": ""}],
 "dropped": [{"id": 3, "reason": "withheld:3:0:fails"}]}
```

- `args` is the object the `implement-v2-milestone` Workflow takes. `args_sha256` is the SHA-256 of `args` printed as compact JSON with keys in the order above, and is what `plan run start --args-hash` records.
- The dispatch set is the six rules of run-milestone.md, section 4, "Build the args", applied to the stored rows. `dropped[].reason` is one of `not-pending`, `withheld:<task>:<premise>:<verdict>`, `needs-withheld:<task>`, `executor:<value>`, `no-files`, `need-not-done:<task>:<status>`.
- A `base` premise, or an `after:<m>` premise whose task `m` is `done`, must carry a `probe` whose `at` is the current `HEAD`. One that does not makes the call fail, naming `plan probes --emit --premises --milestone <id>`.
- An `after:<m>` premise whose task `m` is in the dispatch set keeps its task in, with `m` in that task's `after`, and puts the premise's command in `after_checks` for the orchestrator to read before launch. One whose task `m` is neither `done` nor in the set drops the task as `withheld:<task>:<premise>:unchecked`.
- The started-row exception for anchor premises on an apply row is applied as findings.md states it.

### reproduce-units

`tomlctl plan emit reproduce-units --slug <slug> --milestone <id> [--since-launch] [--relaunch <n>]`

```json
{"launches": [{"mode": "reproduce", "store": {"slug": ""}, "flow": "", "milestone": "", "round": 1, "run_dir": "",
               "units": [{"task": "<task ref>", "files": [], "findings": ["F1"], "route": "checker"}]}]}
```

- One `launches` entry per review store the milestone's `[[finding_links]]` name. A finding several rows fix belongs to the highest-id row's unit.
- `round` is the highest `round` among the store's `[[reproductions]]` for this flow and milestone, plus one. Under `--relaunch <n>` it is the `--round <r>` the call names, the round being relaunched, and `--task <ref>[,<ref>...]` keeps only the named units, so a relaunch holds the failed units alone, in their own round. `--round` and `--task` are refused without `--relaunch`.
- `run_dir` is `.review-v2-runs/apply-<slug>-<milestone>-<round>`, then `-<s>` or `-<scope>` naming the store when the milestone links more than one, then `-r<n>` under `--relaunch <n>`, so neither a second store nor a relaunch of a round shares a directory with another launch.
- `route` is `checker` when every finding of the unit has the stored route `checker` and none has `direction = "add"`, `instances`, `fix_files` or `target_file`; otherwise `verifier`.
- `store` is the object `review-v2-pipeline` takes, `{"slug": "<slug>"}` or `{"scope": "<scope>"}`, and a unit's `task` is the row's `ref`, as a reproduction's `task` is. A `launches` entry passes the pipeline's reproduce-mode validation unchanged.
- `--since-launch` keeps a unit only when one of its findings' latest reproduction for this flow and milestone is not `gone`, or its `files` share a path with a row the latest `implement:<id>` run dispatched. The dispatched ids come from that run's `tasks`.

`plan run start` gains `--tasks <id>[,<id>...]`, stored on the run row as `tasks`.

### gate-list

`tomlctl plan emit gate-list --slug <slug> --milestone <id>`

```json
{"commands": [{"command": "", "source": "gate"}],
 "skipped": [{"task": 4, "accept": 0, "reason": "covered_by <command>"}]}
```

- `commands` holds the milestone's `gates` in stored order, then every accept check of the milestone's rows whose effective owner is `gate` and whose `check` is not empty, in row then entry order, with `source` `accept:<task>:<index>`. A command byte-identical to one already listed is skipped with reason `duplicate`.
- An entry with `run_by = "covered"` is skipped with reason `covered_by <command>`.
- Which commands are held for a second dispatch, and each command's `timeout:`, `rerun:` and `transient:` options, stay the orchestrator's to add.

## Gate probes

A milestone gains `gate_probes`, a list of `{gate, probe, verdict, at}` tables. `plan milestone add` and `plan milestone update` take a repeatable `--gate-probe '<gate> :: <probe>'`, split at the first ` :: `; `<gate>` must equal one of the milestone's gates, and `plan check` reports `plan/gate-probe-unknown`, an error, when it no longer does.

`plan probes --emit --gates [--milestone <id>]` prints one `p` line per stored probe, and `plan probes --record <capture> --gates` stores `holds`, `fails` or `broken` by the premise rules, with `at` the capture's head. A probe is a cheap command that fails where the gate cannot pass in this environment, such as the gate's own `--dry-run`; the gate itself is never a probe. The feature is `gate_probes`.

## Worker return

The `implement-v2-milestone` return object and its index entry gain `needs_files`, a list of repo-relative paths, empty unless the worker returns `failed` because the task cannot be done inside its `files`. It names the files the task would also need. The worker edits none of them. The key is optional in the return schema, and `after` is optional in an args task, so a worker or an orchestrator that predates either still runs.

A task whose args entry carries a non-empty `after` is dispatched once every task it names has returned `done`. Its worker runs its own `after:` premise checks first, before it sets its row `in-progress`, because the emit covers pending rows only: `tomlctl plan probes --slug <slug> --emit --premises --task <id>` written to a file made with `mktemp` and run under `timeout`. A worker that finds its row already `in-progress` is a restart and skips the check, which the first attempt ran. It records nothing, and returns `failed` with the premise named in its note, having edited nothing, when one exits non-zero.

## Reproduce checker

A reproduction may be recorded `by = "checker"`, beside `verifier` and `orchestrator`. In reproduce mode `review-v2.js` pools the findings of `route: "checker"` units into `review-checker` batches of the size its wave pool uses, and sends each unit holding a finding the checker leaves `unsettled` to a `review-verifier`, as it sends every `verifier` unit. A checker records `gone` only when the finding's `old` text is absent at its anchor.

The checker's reproduce prompt carries `mode: reproduce`, `store:`, `flow:`, `milestone:`, `round:`, `run directory:`, a batch number and `findings:`. It writes its rows to `payloads/reproduce-checker-<batch>.ndjson` in the run directory, each row carrying `"by": "checker"`, and loads them with `tomlctl review reproduce --ndjson`; `review reproduce` has no `--by` flag. It returns `{"reproductions": [{"finding": "F1", "verdict": "gone"}]}`, one entry per finding it was given, `verdict` being `gone` or `unsettled`; the script passes that shape as the agent's schema. The reproduce index gains `agents`, the number of agents the launch started.

## Plan batching

`review-v2-pipeline` takes an optional arg `batch_defects`, which `plan-v2-review.js` passes as `true`. Under it a cell's verifier-routed defects that would each get a single verifier go to one batch when there are two or more, except a row carrying `same_as`, which stays single so its verdict can strike the link. The script learns which rows those are from the cell return: each entry of `review add`'s `findings` list, and the cell-return schema, gain an optional boolean `same_as`, present and true when the row names one. The lead's settle return gains an optional `reverify_single`, the ids among its `reverify` that carry a `same_as`, which it reads through the audit projection. The plan wrapper's reverify sends those one verifier each and the rest as one batch.

## Addressed

A plan finding the author fixed by hand is `resolved` and carries `addressed`, the list of targets the hand edit changed. `review triage <id>[,<id>...] --addressed <text>` records it, with `--addressed-file <path>` as its twin, as every other prose mode of the verb has; the text is stored as the finding's `resolution`. `--targets <t>[,<t>...]` names the targets and defaults to each finding's own `target`; a finding with no `target` is refused unless `--targets` is given, so no addressed row reaches the re-check empty. Reopening a finding clears the field. `review stats --plan-outcomes` counts such rows as `author-addressed`, apart from `dismissed` and `resolved`.

`tomlctl review recheck-targets --run <id> <store-flag>` prints the distinct targets a re-check covers: the `applied.target` of every finding `review apply` wrote in that run, and every `addressed` entry. The feature is `review_addressed`.

## Cited and apply

A citation holds when any `;`-separated part of it holds. A part's path may be the target's own file, a path the target's current value names, a path the finding's `new` names, or any file under `docs/flows/<slug>/` of the finding's flow.

`review apply --user-accepted` writes an advisory-tier defect or cleanup finding that carries `new`, with the same compare-and-set on `expect`, stamping `applied.by = "user"`. Without `--user-accepted` such a row is refused as before, and an advisory-kind row is never applied.

## Digest

`review digest` prints `duplicate_candidates` and `ruled_out_hits` as counts. `--with duplicates`, `--with ruled-out` or both, comma-separated, print the named arrays in full under the same keys.

## Caps

- `review cover` cuts a note over its cap to the cap, ending `…(+K)` with K the characters cut, writes it, and reports `note_truncated: K` in its envelope. A `--ruled-out` reason over its cap is cut the same way, with `reasons_truncated` in the envelope counting the reasons cut.
- `review add --dry-run` runs every check the write runs, prints the same `findings` and `refused` lists with no ids minted, and writes nothing.

## Template paths

A `--template` path that resolves on no row of a non-empty row set, or not on a single object, prints `tomlctl: --template path <path> matched nothing` on stderr and leaves the exit status alone, unless the path's first segment is in the crate's table of optional fields, which holds every optional field a documented template names. `--error-format json` suppresses the line.

## Gate stamp

`scripts/gate-stamp.sh run` runs the pre-commit hook's cargo steps, all of them whatever is staged, and on a full pass writes a digest of the working tree to `gate-stamp` in the git directory. `scripts/gate-stamp.sh check` exits 0 only when that file holds the digest of the working tree as it now is. The digest covers the bytes and paths of every tracked or untracked, unignored file under `tomlctl/`, `glimpse/`, `claude/`, `.claude/review-lenses/`, `docs/plans/` and `scripts/`, the last because the dispatch tests read `scripts/shared-blocks.toml` and run `scripts/verify-shared-blocks.sh`.

The hook skips its cargo steps, printing one line that says so, when `check` exits 0. The shared-block verifiers, the documentation gate and the sidecar comparison always run.

## Worker audit

`tomlctl agents audit --transcripts <dir> [--pattern <regex>]...` reads every `agent-*.jsonl` in the directory a Workflow launch reports as its transcript dir, and prints one row per Bash command that matches a pattern: `{agent, command}`, the command cut to 200 characters. With no `--pattern` the patterns are a `cargo build`, `cargo test` or `cargo nextest` invocation, and a recursive `grep`. It writes nothing. The feature is `agents_audit`.
