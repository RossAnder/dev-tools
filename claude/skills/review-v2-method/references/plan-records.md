# Plan findings on a v2 flow

A v2 plan has no single markdown file. Its tasks and specs live in `.claude/flows/<slug>/tasks.toml`, its decisions, contracts and handovers in `.claude/flows/<slug>/plan.toml`, and its prose in `docs/flows/<slug>/brief.md` and `docs/flows/<slug>/design/*.md`. A plan-family run is on a v2 flow when the flow has `plan.toml`; your cell assignment names the flow when it is. On a v2 flow a finding names the record it is about in `target`, and a fix is the record's current value in `expect` and its replacement in `new`. `plan_section`, `anchor_old` and `anchor_new` are refused.

**Precedence.** Any lens, method or check that tells you to supply `plan_section`, `anchor_old` or `anchor_new` — the plan lenses, `feasibility`, `ripple`, `test-gaps`, step 5 of `methods/dry-run.md` — is read on a v2 flow as `target`, `expect` and `new`.

## What to read

```bash
tomlctl plan view --slug <slug> --for human
tomlctl plan view --slug <slug> --for worker --task <id>
tomlctl tasks show <id> --slug <slug> --with summary,body,spec
```

Read `docs/flows/<slug>/brief.md` and every `docs/flows/<slug>/design/*.md` in full. `plan view` leaves out stored accept fields, so copy an `expect` from `tasks show`, never from `plan view`.

Never read `docs/flows/<slug>/notes/`. They are the planner's investigation, and a finding has to stand on the plan as written.

## Where each plan section lives

A lens written for a markdown plan names sections. On a v2 flow each is a record:

| The lens says | The record | `target` |
|---|---|---|
| a task's Action, Detail, Acceptance prose | the task row | `task:<ref>.action`, `.detail`, `.acceptance` |
| **Files** | the task's `files` | `task:<ref>.files` |
| **Depends on** | the task's `needs` | `task:<ref>.needs` |
| the effort mark, the checkpoint | the task row | `task:<ref>.effort`, `task:<ref>.checkpoint` |
| **Acceptance** commands | the spec's accept entries | `task:<ref>.spec.accept[<n>]` |
| `## Success Criteria` | the brief | `brief#Success criteria` |
| `## User Decisions` | decisions | `decision:D<n>.<key>` |
| `## Approach` | design files, decisions, contracts | `design/<file>.md#<anchor>`, `decision:D<n>.<key>`, `contract:C<n>.<key>` |
| `## Verification Commands` | milestone gates | `milestone:<id>.gates` |
| `## After Merge` | handovers | `handover:H<n>.<field>` |

A lens check on a section with no record — the Summary, the Status line, the Dependency Graph layout — does not apply on a v2 flow. Count it as not applying in your coverage note; it is not a finding.

## `target`

| `target` | Edits | `file` | `symbol` |
|---|---|---|---|
| `task:<ref>.<field>`, field one of `action`, `detail`, `acceptance`, `effort`, `files`, `needs`, `coupling`, `deps_note`, `checkpoint` | the field's whole value | `.claude/flows/<slug>/tasks.toml` | `<ref>` |
| `task:<ref>.spec.<field>`, field one of `outcome`, `constraints`, `accept`, `premises`, `means`, `design`, `precedent`, `executor` | the whole spec value | same | `<ref>` |
| `task:<ref>.spec.<list>[<n>]`, list one of `constraints`, `accept`, `premises`, `means`, `design`, `precedent` | one entry | same | `<ref>` |
| `task:<ref>.spec.<list>[+]` | a new entry, appended | same | `<ref>` |
| `decision:D<n>.<key>` | one key of a decision | `.claude/flows/<slug>/plan.toml` | `D<n>` |
| `contract:C<n>.<key>`, key one of `kind`, `claim`, `check`, `tasks`, `provides`, `consumes` | one key of a contract | same | `C<n>` |
| `milestone:<id>.<field>`, field one of `goal`, `gates`, `elaboration`, `defer_reason` | the field's value | `.claude/flows/<slug>/tasks.toml` | `milestone:<id>` |
| `handover:H<n>.<field>`, field one of `reason`, `action`, `when`, `done_when`, `source_task`, `done` | nothing: checked, never applied | `.claude/flows/<slug>/plan.toml` | `H<n>` |
| `brief#<Section>`, one of `Intent`, `Non-goals`, `Scope budget`, `Success criteria`, `Background`, `Unknowns` | text inside the section | `docs/flows/<slug>/brief.md` | the section name |
| `design/<file>.md#<anchor>` | text inside the section | `docs/flows/<slug>/design/<file>.md` | `<anchor>` |

- `<ref>` is the task's derived ref as `tasks show` prints it, never its numeric id.
- A decision's keys are `decision`, `why`, `provenance`, `door`, `binds`, `rejected`, `evidence`, `if_false`, `revisit_if`, `check`, `surfaced` and `vetoed`.
- A design `<anchor>` is the heading's slug, the same form a spec's `design` entries cite.
- `title` is not a target, since a retitle can move the ref every other finding names. Raise a title fault as `advisory`.
- `file` must be the target's file, and `symbol`, when given, its symbol; `review add` refuses either when it differs. Always set `symbol`, since it is what matches a prior finding. `line` is optional on a v2 flow; leave it out.

## `expect` and `new`

Both are JSON values in the payload line — a string, list, object or number, not text quoting one. tomlctl stores them as JSON text; that is not your concern.

- **Whole values.** `expect` is the field's current value exactly as `tasks show` prints it: the whole `files` list, the whole `needs` list of task ids, the whole string. `new` is the whole replacement.
- **List entries.** `expect` is the one entry, every key `tasks show` prints for it. `[<n>]` is only a hint: the entry edited is the one equal to `expect`, so `review add` refuses an `expect` that no entry, or more than one entry, equals. A `new` of `null` removes the entry. A `[+]` target appends `new` and needs no `expect`.
- **Accept entries.** `tasks show` prints a `probe` key on an entry that has been probed. tomlctl writes it, so leave it out of both `expect` and `new`; a row carrying it is refused. Entries are compared with `probe` removed.
- **Prose.** For `brief#` and `design/` targets, `expect` is text that occurs exactly once in the section and `new` is its replacement. Line endings are folded before the comparison.
- `new` needs `expect`, except on a `[+]` append. A row may carry `expect` alone to pin the value it is about.
- Only `defect` and `cleanup` rows carry `expect`, `new` or `proof`. An `advisory` row may carry `target` and nothing more.
- `review add` refuses an `expect` that is not the current value, and its message quotes the value it found. Re-read the record and resubmit; never guess.

## `proof`

A fix tomlctl can check from the store and the tree is applied without asking anyone. Give the check as `proof` only when it holds; `review add` refuses a `proof` that does not.

| `proof` | Holds when | `new` may |
|---|---|---|
| `{"kind": "probe"}` | the target is an accept entry, `task:<ref>.spec.accept[<n>]`, whose recorded probe verdict is not healthy. An entry with no verdict proves nothing | change only the entry's `check` and `positive` |
| `{"kind": "check-class", "class": "<class>"}` | `plan check` reports that class at error severity on the target's task. Only `task:` targets | change only the field the class binds (below). On a list it may only append entries or fill an existing entry's empty `check`; changing or dropping any stored entry is refused |
| `{"kind": "path-missing", "path": "<path>"}` | the target is `task:<ref>.files`, the task claims the path, the tree lacks it, and it is not a `(new)` file | drop that path, or replace it with one other path |
| `{"kind": "ref-unknown", "ref": <id>}` | the target is `task:<ref>.needs` or `contract:C<n>.tasks`, its value names task `<id>`, and no task has that id | drop that id, or replace it with one other id |

| `class` | Binds |
|---|---|
| `dag/dangling-ref` | `needs`, `coupling` |
| `plan/accept-no-check`, `plan/spec-no-accept` | `spec.accept` |
| `plan/design-anchor-dead` | `spec.design` |
| `plan/means-required-no-reason` | `spec.means` |
| `plan/premise-task-unknown` | `spec.premises` |
| `plan/spec-no-outcome` | `spec.outcome` |

A `path-missing` or `ref-unknown` row's `expect` and `new` are the whole list. A proven row holds at most 800 characters in each of `expect` and `new`, serialised. A fix outside these shapes is still a finding: drop the `proof`, and it can be applied on a verifier's citation instead.

## How a fix is applied

tomlctl computes each finding's `tier` when it opens, and `review apply` writes `applied` and the write-ahead mark `applying`; never supply any of the three.

- **user** — a defect or cleanup with a `new` whose target is a decision, a `spec.outcome`, or the brief's Intent, Non-goals, Scope budget or Success criteria. The user is asked, whatever the evidence, and an accepted edit is applied.
- **question** — an advisory, or a row without `new`, on one of those user-owned targets. The user answers it by revising the record or keeping it; it is never applied, parked or vetoed.
- **mechanical** — a `proof` that holds. Applied without asking.
- **cited** — the verifier's or panel's confirmed or plausible verdict cites a `path:line` in the target's own file, or in a path the target's value names. A checker call never cites a row into this tier, and a plan cleanup routes to the verifier, never the checker. Applied and listed at the approval gate.
- **advisory** — everything else, including any other row without `new`. Shown, never applied.

A tier is frozen once stamped, so a row stamped `user` before `question` existed stays `user`.

A verifier who means its verdict to carry a fix cites a `path:line` range that exists in such a file.

## PRIOR on a v2 flow

Rows carry no line here, so a prior row matches on `file` and `symbol` alone: the same task ref, record id, brief section or design anchor in the same file. Build PRIOR per record file the slice touches:

```bash
tomlctl review list --in .claude/flows/<slug>/tasks.toml --for finder --slug <slug>
tomlctl review list --in .claude/flows/<slug>/plan.toml --for finder --slug <slug>
```

and once per prose file in the slice.

## Worked examples

Paths and refs are invented. One JSON object per line, as `review add` takes it.

A mechanical `files` fix:

```json
{"kind": "cleanup", "lens": "plan-executor", "file": ".claude/flows/export/tasks.toml", "symbol": "add-the-export-verb", "summary": "Files claims tests/export_cli.rs, which is absent and not a (new) file", "edit": "Replace tests/export_cli.rs with tests/export.rs, the test the acceptance runs.", "direction": "change", "effort": "trivial", "target": "task:add-the-export-verb.files", "expect": ["src/export.rs", "tests/export_cli.rs"], "new": ["src/export.rs", "tests/export.rs"], "proof": {"kind": "path-missing", "path": "tests/export_cli.rs"}}
```

A defect on an accept entry whose probe failed:

```json
{"kind": "defect", "lens": "plan-executor", "file": ".claude/flows/export/tasks.toml", "symbol": "add-the-export-verb", "summary": "The acceptance runs a test binary no task creates", "severity": "major", "failure_scenario": "At acceptance the implementer runs `cargo test --test export_cli`; no task creates tests/export_cli.rs, so it fails with `no test target named export_cli`, as the recorded probe did.", "fix": "Run the existing tests/export.rs instead.", "effort": "trivial", "counter": "A later task could create the file; no task's files names it.", "grade": "high", "evidence": ["tasks show add-the-export-verb: accept entry 0 probe verdict broken"], "target": "task:add-the-export-verb.spec.accept[0]", "expect": {"behaviour": "exports rows", "check": "cargo test --test export_cli", "kind": "forward", "positive": "", "writes_build": false}, "new": {"behaviour": "exports rows", "check": "cargo test --test export", "kind": "forward", "positive": "", "writes_build": false}, "proof": {"kind": "probe"}}
```

A prose fix in the brief, applied only on a cited verdict:

```json
{"kind": "cleanup", "lens": "plan-grounding", "file": "docs/flows/export/brief.md", "symbol": "Background", "summary": "Background names the removed --ledger flag", "edit": "Name --to in place of --ledger.", "direction": "change", "effort": "trivial", "target": "brief#Background", "expect": "the `--ledger` flag", "new": "the `--to` flag"}
```
