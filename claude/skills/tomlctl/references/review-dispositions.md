# tomlctl — review dispositions reference

The `tomlctl review` verbs that settle a verified finding: writing a flow-mode plan finding's record edit, triaging a finding to its disposition, and exporting code and perf findings to a v1 ledger. The store shape, the shared flag bundles, payload sources and output conventions are [review.md](review.md)'s, under Shared behaviour and Store shape; this file holds only these verbs and the flow-mode plan records they act on. Which findings are triaged, and how, is the `review-v2-engine` skill's call, not this one's.

## Contents

- [Plan records](#plan-records) — flow mode, `target`, `tier`, `proof`, `apply`
- [Dispositions and export](#dispositions-and-export) — `triage`, `export-v1`

## Plan records

A plan-family run is in **flow mode** when its store is a flow's (`--slug`) and that flow has `.claude/flows/<slug>/plan.toml`; otherwise it is in **markdown mode**, which behaves as the rest of [review.md](review.md) describes. The reviewer-side how-to — what to read, which record each markdown section maps to, worked payloads — is `review-v2-method`'s `references/plan-records.md`; this section is the tool's side.

- **`target`** names one record: `task:<ref>.<field>`, `task:<ref>.spec.<field>`, `task:<ref>.spec.<list>[<n>]` or `[+]`, `decision:D<n>.<key>`, `contract:C<n>.<key>`, `milestone:<id>.<field>`, `handover:H<n>.<field>`, `brief#<Section>` or `design/<file>.md#<anchor>`. `file` and `symbol` must be the target's own, and `line` is optional.
- **`expect` and `new`** are JSON values, stored as JSON text since TOML holds no null. `expect` must equal the record's current value — for a list entry, exactly one entry must equal it, with an accept entry's tool-written `probe` removed on both sides; for prose, it is text occurring exactly once in the section. `new` needs `expect`, except on a `[+]` append; a `new` of `null` removes a list entry. Neither may carry `probe`. An advisory row may carry `target` alone.

**Tiers.** `tier` is computed when a flow-mode finding first opens — by `verdict`, `resolve`, a `triage --reopen`, or `add` for an advisory — and frozen after. The first that holds wins:

1. `question` or `user`: the target is any `decision:*`, any `task:*.spec.outcome`, or `brief#Intent`, `brief#Non-goals`, `brief#Scope budget` or `brief#Success criteria`. Kind `advisory` or no `new` is `question`, which `apply` and `triage --park` refuse and the user closes with `triage --resolve`; a defect or cleanup with `new` is `user`.
2. `advisory`: kind `advisory`, a `handover:*` target, or no `new`.
3. `mechanical`: a `proof` that tomlctl verifies from the store and the tree.
4. `cited`: the latest deciding `verifier` or `panel` verdict is `confirmed` or `plausible`, and its citation holds. A citation is one `path:line` or several joined by `;`, and holds when any part holds: its path is the target's own file, a path the target's current value names, a path the finding's `new` names, or any file under `docs/flows/<slug>/` of the finding's own flow, and the file has the cited line. A `checker` verdict never cites a row into `cited`.

Anything else is `advisory`. Agreement alone never lifts a row past `advisory`. A frozen tier is never migrated: a row stamped `user` before `question` existed stays `user`.

**Proofs.** `proof` is `{kind, …}`, and `add` refuses one that does not hold or a `new` that changes more than it covers. `expect` and `new` are capped at 800 serialised characters each on a proven row.

| `kind` | Holds when | `new` may |
|---|---|---|
| `probe` | the target is a `spec.accept` entry whose recorded probe verdict is not healthy; no verdict proves nothing | change only the entry's `check` and `positive` |
| `check-class` (`class`) | `plan check` reports that class at error severity on the target's task; `task:*` targets only | change only the field the class binds (`plan-records.md` lists them). On a list it may only append entries or fill an existing entry's empty `check`; changing or dropping a stored entry is refused |
| `path-missing` (`path`) | the target is `task:<ref>.files`, the path is absent from the tree and not a `(new)` file note | drop the path or replace it with one other |
| `ref-unknown` (`ref`) | the target is `needs` or `contract:*.tasks` and the value names no task | drop the id or replace it with one other |

### `tomlctl review apply`

Writes the record edits of open flow-mode defect and cleanup findings. Before any write it refuses, as one `kind=validation` error listing each refused id with its reasons: a finding of another family, kind or status than an open plan defect or cleanup; no `tier`; `advisory` tier, except a defect or cleanup carrying `new` under `--user-accepted`, which is written like any other edit and stamped `applied.tier = "advisory"` (without the flag the refusal names it; an advisory-kind row is never applied); `question` tier, naming `triage --resolve` as its exit; `user` tier without `--user-accepted`; any finding without `--user-accepted` once `plan.toml` holds an `[[approval]]`; no `target` or no `new`; a `handover:*` target; a `new` the target store's validator rejects; and a mechanical proof that no longer holds. `--scope`, and a flow without `plan.toml`, are refused outright.

- **Writes.** Each target is written through its own store's path with `expect` re-checked inside that store's lock: task rows and specs through the task update path (actor `review`, reason `review <run> <id>`; a `[[changes]]` row only when the store's `audit` is on), decisions and contracts through the plan records path, milestones through the milestone path. A brief or design edit replaces `expect`'s one occurrence in the section, under an exclusive lock and an atomic write, keeping the file's line endings; a non-UTF-8 file is refused.
- **Order.** Each finding is stamped straight after its own write, so a failure leaves at most one edit unstamped. The error reads "applying <id> failed; stamped before it: [ids]. <id>'s edit may or may not have landed: re-run `review apply` with only the ids not listed as stamped, and an edit that landed reads as already applied".
- **Already made.** A value already equal to `new` — for `[+]`, an equal entry exists; for a removal, no entry equals `expect` — is reported under `already`, written nothing, and stamped with `recovered = true`. This is decided before a mechanical proof is re-checked, since a made fix no longer satisfies its proof. A brief or design edit reads as already made only when `new` occurs once in the section with every occurrence of `expect` inside it and either `expect` is part of `new` or the finding carries `applying = true`; a deletion (empty `new`) with `expect` gone needs the mark. Without either, a `new` found with `expect` gone may be chance, and the finding is stale.
- **Write-ahead mark.** Before a brief or design write, the finding gets `applying = true` in the review store. The stamp drops it, and so does a write that finds the edit stale; it stays when the write fails or lands without its stamp, so a re-run recovers an edit that landed. The field is absent otherwise, and record targets never carry it: their compare-and-set judges the whole value or entry, which a chance match cannot satisfy.
- **Stale.** Any other mismatch lands under `skipped_stale` as `{id, target, expected, found}`, and the rest proceed. Under `--dry-run`, a finding whose record an earlier finding in the same call already edits (same file, symbol and field; for list entries and prose, also the same `expect`; appends never collide) lands there too, as `{id, target, expected, found: null, edited_by: <earlier id>}` instead of `applied`. Real runs are unchanged.
- **Stamp.** Status `resolved`, `resolution = "applied to <target>"`, and `applied = {at, tier, target, old, new, by, recovered}`, where `old` and `new` are JSON text and `by` is `tool`, or `user` under `--user-accepted`. Before approval no `[[changes]]` row exists, so `applied` is the edit's record.

Prints `{ok, applied: [{id, target, tier}], already: [ids], skipped_stale}`; `--dry-run` adds `dry_run: true` and writes nothing.

```bash
tomlctl review apply F3,F7 --slug <slug> --dry-run
tomlctl review apply F9 --slug <slug> --user-accepted
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The flow store; `--scope` is refused. | required |
| `--user-accepted` | bool | The user accepted these edits: admits `user` tier, an advisory-tier defect or cleanup carrying `new`, and an approved plan, and records `by = "user"`. | off |
| `--dry-run` | bool | Report the outcome without writing. | off |

## Dispositions and export

### `tomlctl review triage`

Applies one disposition to each id. A row already in the requested end state is `unchanged`; under `--expect-status`, an id no longer at that status lands under `skipped_stale` as `{id, field, expected, found}`, checked inside the locked write. A move clears every earlier disposition's companions. A `--reopen` also retires the finding's v1 link (see [Link fields](review.md#store-shape)), so the next `sync-v1` does not undo it and a re-export mints a regression row. A `superseded`, `merged`, `converted`, `exported` or `scheduled` finding is never moved. Prints `{ok, mode, applied: [{id, from, status}], unchanged, skipped_stale, refused, minted?}`.

```bash
tomlctl review triage F3,F9 --slug <slug> --dismiss-file <run-dir>/payloads/dismiss.txt --expect-status open
tomlctl review triage F4 --slug <slug> --defer-file <run-dir>/payloads/defer.txt --trigger-file <run-dir>/payloads/trigger.txt
tomlctl review triage F8 --slug <slug> --convert cleanup --ndjson <run-dir>/payloads/F8-convert.json
tomlctl review triage F5,F6 --slug <slug> --addressed-file <run-dir>/payloads/addressed.txt --targets task:add-guard.spec.accept
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. | required |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--dismiss` / `--dismiss-file` | text \| path | Status → `dismissed`, recording `dismiss_reason`. | one disposition required |
| `--defer` / `--defer-file` | text \| path | Status → `deferred`, recording `defer_reason`; needs a trigger. | one disposition required |
| `--trigger` / `--trigger-file` | text \| path | What reopens a deferred finding; with `--defer` only. | none |
| `--reopen` / `--reopen-file` | text \| path | Status → `open` from `dismissed`, `deferred`, `parked`, `backlogged`, `refuted` or `resolved`. | one disposition required |
| `--park` / `--park-file` | text \| path | Status → `parked`; advisory only, and never a `user` or `question` tier row. | one disposition required |
| `--resolve` / `--resolve-file` | text \| path | Status → `resolved`, recording `resolution` and today's `resolved`. | one disposition required |
| `--backlog-ref` | `B-<hex>` | Status → `backlogged`, recording the backlog item id. | one disposition required |
| `--convert` | `cleanup` \| `defect` | Mint the advisory's replacement from `--ndjson`; status → `converted`. Exactly one advisory id. | one disposition required |
| `--answer-file` | path | The user's answer to an open finding blocked on `decision`, at most 800 characters; the status stays `open`. | one disposition required |
| `--addressed` / `--addressed-file` | text \| path | Status → `resolved`: the author fixed the plan finding by hand. The text is stored as `resolution`, and the changed targets as `addressed`. | one disposition required |
| `--targets` | target, comma-separated, repeatable | The plan targets the hand edit changed; with `--addressed` only. | each finding's own `target` |
| `--ndjson` | `-` \| path | The replacement as one JSON object; with `--convert` only. | none |
| `--run` | run id | The open run the replacement is minted in when the advisory's run is finished; with `--convert` only. | the advisory's run |
| `--expect-status` | status | Move only the ids still at this status. | no check |

`--convert` defaults the replacement's lens, anchor, summary and, in flow mode, `target` from the advisory, validates and routes it, records an `orchestrator` verdict on it and leaves it `unverified` for its route; `minted` reports `{id, kind, severity, route, status}`, and a replay reports the replacement already minted.

`--convert` accepts a `question` tier advisory, though `--park` refuses one: it is how a question becomes a proposed edit. The replacement targets the same user-owned record, so once verified it is tiered `user` when it carries a `new`, and `question` again when it does not. A `user` tier replacement still applies only through `review apply --user-accepted`, so the conversion never makes the edit without the user's Accept.

The replacement is minted in an open run, which cannot finish until it is verified. While the advisory's run is open it is that run, and a `--run` naming any other is refused. Once the advisory's run is finished, the call is refused unless `--run` names an open run of the advisory's family; an unknown, finished or other-family run is refused too. The replacement then takes the named run as `run` and `seen_in`, keeps the advisory's `cell` as provenance, is routed by the named run's catalogue and depth, and its `orchestrator` verdict is filed under that run.

`--answer-file` (mode `answer`) records the file's text as `decision_answer` with `unblocked_by = "user"` and clears the block, keeping `blocked_options` as what the answer chose among; `schedule` binds the answer to the finding's fix task as a constraint. A finding not `open`, or blocked on anything but `decision`, is refused, and a replay of the same answer is `unchanged`.

`--addressed` (mode `addressed`) records a plan finding the author fixed by hand: `resolution` takes the text, `resolved` today's date, and `addressed` the targets, from `--targets` or else the finding's own `target`. A finding of another family, or one with no `target` when `--targets` is absent, is refused, and one refused id refuses the whole set, so nothing is written; a replay with the same text and targets is `unchanged`. A later reopen clears `addressed`. `review recheck-targets` reads the list, and `review stats --plan-outcomes` counts such a row as `author-addressed`.

### `tomlctl review export-v1`

Exports open defect and cleanup findings into a v1 ledger for `/review-apply` or `/optimise-apply`. The ledger's family comes from `--to`: `review-ledger.toml` or a path under `.claude/reviews/` takes `code` findings with `R` ids; `optimise-findings.toml` or a path under `.claude/optimise-findings/` takes `perf` findings with `O` ids.

- **Refused**, with a reason: the plan family, advisory, any status but `open`, another family than the ledger's, a blocked finding ([Blocked findings](review.md#store-shape)), `effort = large`.
- **`--all`** takes every open defect and cleanup finding of the ledger's family in this store — there is no run filter — and holds back, under `held`, test removals and cleanup whose latest verdict is `plausible`; those export only by explicit id.
- **Rows.** A defect exports with its latest severity (`major` → `warning`, `minor` → `suggestion`), cleanup as `suggestion`; `category` is the bound lens's `v1_category`, or `quality` / `algorithm` for a probe or check cell; `description` is built from the implementer projection. The v1 row carries `v2_ref = "<store path>#F<n>"`.
- **Matching.** A v1 row whose `v2_ref` names the finding — the string, or an entry of the array — is `linked`. Otherwise an open row is `reused`, its `rounds` bumped, only on the strict fold: its `v2_ref` names the finding this finding's `same_as` names, or its `file`, `symbol` and `summary` all equal the finding's and its `v2_ref` names a finding of this store with the same `lens`. A v1 row carries `category`, never `lens`, so a row without a resolvable `v2_ref` never meets the second clause, and that clause never matches a row this same call wrote. Failing that, the loose v1 merge rule (same `file`, and the same non-empty `symbol` or the exact `summary`) is read for settled rows only: a deferred, wontfix, wontapply or verified-clean match is skipped, and a fixed or applied match gets a new id with `related` (`regression`). Anything else — an open row that matches only loosely included — is `minted`. A reused row whose `v2_ref` names another finding becomes an array naming both, rather than being overwritten.
- **Retired links.** A row the finding retired on reopen is never `linked` and never skipped as settled; it joins the merge rule as a candidate. Open again in v1, it is `reused` and the link is restored, its `retired_links` entry dropped; fixed, applied or settled, it yields a `regression` row with `related` naming it.
- **Writes.** The v1 ledger is written and committed first; then each finding becomes `exported` with `exported_as` and `export_ledger`. A new ledger is seeded, and the ledger's `last_updated` becomes the later of its own and the newest run date among the findings.

Prints `{ok, ledger, exported: [{finding, v1, action}], skipped, held, refused, dropped_deps, skipped_stale}`; under `--dry-run` the same fields sit under `would_change` beside `dry_run: true`, and nothing is written. `dropped_deps` lists `depends_on` ids that map to no exported v1 row.

```bash
tomlctl review export-v1 F1,F4 --scope <scope> --to .claude/reviews/<scope>.toml
tomlctl review export-v1 --all --slug <slug> --to .claude/flows/<slug>/review-ledger.toml --dry-run
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids | Finding ids, comma- or space-separated. Excludes `--all`. | required without `--all` |
| `--slug` / `--scope` | name | The store. | required, one of |
| `--all` | bool | Every exportable finding of the ledger's family in this store. | off |
| `--to` | path | The v1 ledger written. | required |
| `--dry-run` | bool | Report `would_change` without writing either file. | off |
