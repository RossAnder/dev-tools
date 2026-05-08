# `flow-context` shared block — single source of truth

This file is the design checkpoint for Task 14 of `docs/plans/flow-tracking-overhaul.md` and
the verbatim source consumed by Task 15 (the coordinated rewrite of all 9 carriers + the
`tdd.md` second call site at L415).

**Consumers**:

- Task 13 (`claude/agents/flow-bootstrap.md`) — Section 3 below pins the input/output envelope
  shapes its body parses and emits.
- Task 15 (the 9 carrier rewrites) — Section 1 is copied byte-identical between the
  `<!-- SHARED-BLOCK:flow-context START -->` and `<!-- SHARED-BLOCK:flow-context END -->`
  delimiters in each carrier; Section 2 replaces each carrier's pre-flight section
  (carrier-specific `command` literal + `path_args` wiring; identical structure otherwise);
  Section 4 replaces `claude/commands/tdd.md`'s `## Bootstrap-missing fallback` step-1 prose.

**Parity invariant**: Section 1 is the body of the `flow-context` block. After Task 15
lands, `bash scripts/verify-shared-blocks.sh` MUST pass — the block content is identical
across all 9 carriers in `scripts/shared-blocks.toml`'s `flow-context` entry.

---

## Replacement flow-context block (verbatim — copied byte-identical into all 9 carriers)

```markdown
<!-- SHARED-BLOCK:flow-context START -->
## Flow Context

Flow resolution + doctor checks are delegated to the `flow-bootstrap` sub-agent
(`claude/agents/flow-bootstrap.md`). Each carrier's Step-0 builds a JSON input envelope,
dispatches the agent, gates on `envelope.ok`, and binds `envelope.resolved.{slug,
context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for
downstream phases. Canonical input/output envelope shapes: see `flow-bootstrap.md` Contract
section (mirrored at `scripts/templates/flow-context.md` Section 3).

All `.claude/...` paths resolve to the project-local `.claude/` at the git top-level. No
fallback to `~/.claude/`. **Status vocabulary**: `status ∈ {draft, in-progress, review,
complete}`; auto-transitions to `complete` from non-`plan-update-complete` ops are
forbidden (route through `review`); unknown values fail-soft to `in-progress` on read.
**Slug derivation**: filename minus `.md` (multi-file plan: parent directory name); no
further slugification. **Canonical artifacts**:
`.claude/flows/<slug>/{review-ledger,optimise-findings,execution-record,plan-review-findings}.toml`
— read from `envelope.resolved.artifacts.*`, never recompute inline; persist back to
`context.toml` on next write when absent. **Completed-flow handling**: `status = "complete"`
flows are filtered out of scope-glob + branch-match resolution but remain targetable via
explicit `--flow <slug>`. **Legacy `.claude/active-flow` ignore**: the pre-overhaul
single-line slug file is no longer consulted; the registry lives at
`.claude/active-flow.toml` (multi-entry, gitignored per-clone state).
<!-- SHARED-BLOCK:flow-context END -->
```

---

## Per-carrier Step-0 collapse template

Each carrier's Step-0 / pre-flight section collapses to the ~10-line invocation below.
Replace `<COMMAND>` with the carrier's literal command name (`review`, `optimise`,
`optimise-apply`, `review-apply`, `plan-new`, `plan-update`, `implement`, `review-plan`,
`tdd`). Replace `<PATH_ARGS_JSON>` with the carrier's path-argument projection (typically
`$ARGUMENTS`-derived; an empty array `[]` when the carrier takes no path args).
`<REQUIRE_ARTIFACTS>` is the carrier-specific list of artifact keys that downstream phases
will consume (`["review_ledger"]` for `/review`, `["optimise_findings"]` for `/optimise`,
`["execution_record"]` for `/implement`, `/plan-update`, `/tdd`, `["plan_review_findings"]`
for `/review-plan`, `[]` for `/plan-new`).

```markdown
## Step 0: Pre-flight (flow resolution + doctor)

Dispatch the `flow-bootstrap` sub-agent with a single JSON-encoded input envelope. The
agent emits one JSON object on stdout; parse it as `envelope`. All downstream phases consume
fields from `envelope.resolved` and `envelope.doctor`.

Input envelope (build at dispatch time):

```json
{
  "command": "<COMMAND>",
  "flow_override": <--flow value or null>,
  "path_args": <PATH_ARGS_JSON>,
  "branch": <git branch --show-current or null>,
  "worktree": <git rev-parse --show-toplevel or null>,
  "cwd": <pwd or null>,
  "require_artifacts": <REQUIRE_ARTIFACTS>,
  "staleness_threshold": "7d"
}
```

Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"`. After parse:

1. **Gate on `envelope.ok`**. If `false`, surface `envelope.errors` to the user verbatim
   and halt. Do not proceed to scope analysis or any downstream phase.
2. **Bind for downstream**: `slug = envelope.resolved.slug`, `context_path =
   envelope.resolved.context_path`, `artifacts = envelope.resolved.artifacts` (object with
   `review_ledger` / `optimise_findings` / `execution_record` / `plan_review_findings`),
   `doctor_ok = envelope.doctor.ok` when `envelope.doctor` is non-null.
3. **No-flow fallback**: when `envelope.resolved.resolved == false`, the carrier follows
   its flow-less convention (`/review` → `.claude/reviews/<scope>.toml`; `/optimise` →
   `.claude/optimise-findings/<scope>.toml`; plan/implement/tdd carriers prompt the user
   per `envelope.warnings`). `envelope.resolved.tie_candidates` (when non-empty) lists the
   slugs surfaced for the user prompt.
4. **Doctor-fail handling**: when `envelope.doctor.ok == false`, surface
   `envelope.doctor.checks` (filtering for `ok == false`) and ask the user before the
   carrier mutates any artifact. Auto-repair (`tomlctl flow doctor --fix`) is the
   orchestrator's call — bootstrap is read-only.
5. **Staleness**: read `envelope.resolved.stale.stale` (boolean) plus
   `envelope.resolved.stale.reason`. When `true` AND the carrier is `/review` or
   `/optimise`, invoke the `plan-update` skill with literal arg `reconcile` before
   continuing.
```

---

## Input/Output envelope reference

The two JSON code blocks below are byte-identical (key-set-identical at minimum) to the
shapes in `claude/agents/flow-bootstrap.md`'s Contract section. They are the canonical
contract between Task 13 (the bootstrap agent) and Task 15 (the 9 carriers that consume
its output).

### Input envelope (caller → flow-bootstrap)

```json
{
  "command": "review|optimise|...|tdd",
  "flow_override": null,
  "path_args": [],
  "branch": "feat/x",
  "worktree": "/abs/path",
  "cwd": "/abs/path",
  "require_artifacts": ["execution_record"],
  "staleness_threshold": "7d"
}
```

Field semantics:

- `command` — required string; one of `review`, `optimise`, `optimise-apply`,
  `review-apply`, `plan-new`, `plan-update`, `implement`, `review-plan`, `tdd`. The
  bootstrap agent uses this to decide whether to invoke `tomlctl json get
  .claude/settings.json plansDirectory` (only for `plan-new`, `plan-update`, `review-plan`).
- `flow_override` — optional string or null. The carrier's `--flow <slug>` literal when
  supplied; otherwise `null`.
- `path_args` — array of strings. The carrier's path-argument projection (file paths,
  directories, glob patterns) for the scope-glob resolver step. `[]` when the carrier takes
  no path args.
- `branch` — optional string or null. The result of `git branch --show-current` when
  non-empty; `null` otherwise (detached HEAD or non-git invocation).
- `worktree` — optional string or null. Absolute path of the git top-level (`git rev-parse
  --show-toplevel`); `null` outside a git repo.
- `cwd` — optional string or null. The carrier's current working directory; `null` when
  the carrier wants the agent to default to its own `cwd`.
- `require_artifacts` — array of strings. Subset of `{"review_ledger",
  "optimise_findings", "execution_record", "plan_review_findings"}` that the carrier needs
  populated downstream. The bootstrap agent does NOT mutate; it surfaces missing artifacts
  via `envelope.resolved.warnings`.
- `staleness_threshold` — string. Currently fixed at `"7d"` per plan; reserved for future
  per-carrier override.

### Output envelope (flow-bootstrap → caller)

```json
{
  "ok": true,
  "resolved": {
    "resolved": true,
    "slug": "feature-x",
    "source": "active-binding",
    "ties_broken": false,
    "tie_candidates": [],
    "context_path": ".claude/flows/feature-x/context.toml",
    "artifacts": {
      "review_ledger": ".claude/flows/feature-x/review-ledger.toml",
      "optimise_findings": ".claude/flows/feature-x/optimise-findings.toml",
      "execution_record": ".claude/flows/feature-x/execution-record.toml",
      "plan_review_findings": ".claude/flows/feature-x/plan-review-findings.toml"
    },
    "plan_path": "docs/plans/feature-x.md",
    "scope": ["src/foo/**"],
    "branch": "feat/x",
    "status": "in-progress",
    "stale": {
      "stale": false,
      "age_seconds": 12345,
      "reason": "updated within threshold"
    },
    "warnings": []
  },
  "doctor": {
    "ok": true,
    "checks": [],
    "fixes_applied": []
  },
  "plans_directory": ["docs/plans/"],
  "warnings": [],
  "errors": []
}
```

Field semantics:

- `ok` — boolean. `true` when steps 3–5 of the bootstrap procedure (resolve, doctor,
  optional plansDirectory read) completed without halting. Warnings do not flip `ok` to
  `false`.
- `resolved` — object or `null`. Pass-through of `tomlctl flow resolve --json --with-staleness`
  output. When `resolved.resolved == false`, the inner shape is the abbreviated step-6
  envelope (`{"resolved": false, "source": "none|scope-glob", "ties_broken": false|true,
  "tie_candidates": [...], "warnings": [...]}`); the outer `resolved` is still set, NOT
  null. `null` only when the bootstrap agent halts before invoking resolve (version-check
  failure).
- `doctor` — object or `null`. Pass-through of `tomlctl flow doctor --slug <slug> --json`
  output. `null` when `resolved.resolved == false` (doctor is skipped) OR when the doctor
  invocation itself failed (the failure surfaces in `errors`).
- `plans_directory` — array of strings, string, or `null`. Pass-through of `tomlctl json
  get .claude/settings.json plansDirectory`. `null` when the setting is unset OR when the
  command is not one of `plan-new` / `plan-update` / `review-plan`. The literal
  `"__DONT_ASK__"` sentinel is normalised to `null` (carriers treat as "use default
  `docs/plans/`").
- `warnings` — array of strings. Soft-failure messages from steps 3–5 (e.g. `tomlctl json
  get` non-`not_found` errors, `tomlctl flow doctor` non-zero exit). Carriers may surface
  to the user but MUST NOT halt on warnings.
- `errors` — array of strings. Hard-failure messages. Non-empty means `ok == false`.

---

## tdd.md L415 second-call-site rewrite

The current prose at `claude/commands/tdd.md:415` reads:

> 1. Resolve the parent flow via the standard flow-resolution order (see `## Flow Context` above).

This is INSIDE `## Bootstrap-missing fallback` — `/tdd` has already passed Step 0 and is
re-resolving the parent flow as a low-level lookup before deciding whether the project's
test framework is detectable. Replace with a direct `tomlctl` call (no nested sub-agent
dispatch — the parent flow is already known to exist; this is just a re-read for the
`plan_path` projection):

```markdown
1. Re-resolve the parent flow via `tomlctl flow resolve --flow <parent-slug> --json`
   (where `<parent-slug>` is the parent flow slug already bound from Step 0's
   `envelope.resolved.slug`). The `--flow` flag forces the explicit-flag resolver path,
   so this is a deterministic single-flow lookup, not the full 6-step algorithm. Parse
   stdout as `parent_envelope`; extract `plan_path = parent_envelope.plan_path`. If
   `parent_envelope.resolved == false` (the parent flow's `context.toml` was deleted
   between Step 0 and now), halt with the literal message `parent flow context.toml
   missing — re-run the parent command first`.
```
