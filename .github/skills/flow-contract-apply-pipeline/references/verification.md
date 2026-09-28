# Verification and ledger-mutation reference

Detail behind Step 5 of the apply pipeline: how the orchestrator judges each `verification` block,
the independent evidence it requires before it trusts an agent's `applied` tag, the regression check against previously-applied
findings, and the per-disposition ledger write with its secret scan, two-call pattern, and
locking rules.

## Contents

- [Reading a verification result](#reading-a-verification-result)
- [Verify agent-reported `applied` claims](#verify-agent-reported-applied-claims)
- [Regression cross-check](#regression-cross-check)
- [Ledger mutation](#ledger-mutation)

### Reading a verification result

The `verification` agent reports; the orchestrator judges. Per block:

- **`pass`** — trust it on `exit: 0`. A test command's `summary:` must also show at least one test
  executed and none failed: `summary: none` or a zero count (`0 passed`, `No test files found`,
  `no tests to run`) is a `fail`, because a filter matching nothing exits 0 in most runners. Where
  `summary:` is `none` only because the agent does not parse that runner, Grep the block's `log:`
  for the runner's count line before deciding.
- **`flaky`** — a pass: the failed tests passed on the agent's narrow rerun or on the runner's own
  retry. It consumes no fix-and-reverify cycle, reruns nothing, and is never a rollback trigger.
  Add its `TANGENTIAL: flaky-test` lines to the Step 6 backlog harvest.
- **`fail`** — a test command's block with 1–10 `failed_ids:` and no `rerun:` line had no template
  to retry with: re-dispatch just those tests, at low parallelism, before diagnosing, and treat a
  green result as `flaky`. Anything else goes to Failure handling.
- **`timeout`** — the command outran its budget, so nothing is known about the code. Re-dispatch
  that command with a larger `timeout:`, or split per crate, binary or shard, followed by the
  `not_run:` tail; commands that already passed are not rerun. It consumes no fix cycle.

After a code fix the whole list reruns — the fix voids the earlier passes.

### Verify agent-reported `applied` claims

Before constructing the ledger-mutation ops, reconcile every `applied <ID>` tag against the
working-tree and index diffs:

- Union `git diff --name-only HEAD` (unstaged), `git diff --name-only --cached` (staged), and
  `git ls-files --others --exclude-standard` (untracked, non-ignored). Untracked files matter —
  agents frequently create new files that haven't been `git add`-ed, and missing them would
  wrongly downgrade legitimate claims.
- Look up each claimed item's `file`. Present in the union → trust the claim, proceed with
  `<APPLIED>`. Absent → **downgrade** to `<REJECTED>` with rationale `"claimed-applied but no diff
  detected — downgraded by <CMD> verification"`, and surface it under the summary's `### Downgraded`
  callout so the user can investigate whether the agent was confused or edited the wrong file.
- For every orchestrator pre-transition from Step 2, log a one-line console notice naming the item,
  the disposition, and the evidence (matched snippet + `file:line`, or the deleted-file rationale).
  Pre-transitions write no bytes by definition, so diff reconciliation cannot apply to them; the
  notice is what keeps the heuristic auditable.

This closes the chain-of-trust gap described by OWASP LLM01:2025 Thought/Observation Injection —
agents can forge their own tags, so the orchestrator requires independent evidence before writing
persistent ledger state.

### Regression cross-check

Apply the ledger-schema dedup rule (same `file` AND (same non-empty `symbol` OR exact `summary`
match)) against **every** previously-`<APPLIED>` item in the ledger — not just those already chained
via `related`. A match on a file touched in this run is a regression: flag it in the final report and
mint a new item with `related = ["<old id>"]`, listed under `### Regressions Triggered`.

**Ledger integrity note**: this check trusts the ledger bytes blindly — a previously-`<APPLIED>`
item whose `file` or `summary` was mutated out-of-band (manual edit, another command, a buggy tool)
silently defeats the dedup rule and lets regressions through. The Step 1 `--verify-integrity` load is
what catches that; on digest mismatch `tomlctl` errors with both hashes and never auto-repairs. The
sidecar is a collaborative-user defence, not a tamper-evident seal — hostile-actor threat models
still need the ledger's git history reviewed.

### Ledger mutation

Mutate the same file consumed in Step 1, via parse-rewrite per the ledger-schema read/write contract.
For each item:

- **Applied** (agent reported `applied <ID>: ...`, diff-confirmed): `status = <APPLIED>`,
  `resolved = <today, ISO 8601>`, `resolution = "<short description + commit SHA if it landed>"`.
  Partial applies write `resolution = "partial: <done> / pending: <not done>"` so the ledger captures
  the split explicitly.
- **Applied pattern item** (carries `sweep`): when the Step 2 re-sweep found `new` sites, append
  them to `instances` with `tomlctl items update <ledger> <id> --json '{"instances": [...]}'`
  (the recorded anchors followed by the `new` sites) ahead of the two-call write below, so the
  fixed record names every site the fix touched. Never `items sweep --update` here: it drops
  `gone` anchors, and on an item about to close those are the record of what was fixed — the same
  reason `--update` skips terminal items.
- **No-change** (agent reported the code already matches, or the orchestrator pre-transitioned in
  Step 2): `status = <NO-CHANGE>` with the audit note suffixed `— audited during <CMD> <today>`.
  **Preserve the item's original `category`** — never reassign `category` to a disposition value.
- **Agent-intentionally-skipped**: `status = <REJECTED>` with the agent's reason, quoted or
  paraphrased, in the rationale field. **Critical-finding gate**: when the item has
  `severity = "critical"` AND `category ∈ <CRITICAL-CATEGORIES>`, do NOT write the transition
  silently — surface it under `### Requires User Confirmation` with the item's ID, category,
  severity, and rationale, and wait for the user's explicit disposition (per `<PRODUCER>`'s
  disposition protocol). This stops a compromised or confused agent from suppressing a critical
  finding that dedup would then hide from future rounds.
- **Not selected**: leave `status` untouched. Do not modify `rounds`, `first_flagged`, or any other
  field on these items.

**Secret-pattern scan of the ledger payload** (mandatory): after constructing the `--ops` JSON but
BEFORE invoking `tomlctl items apply`, grep the serialised payload for `AKIA`, `-----BEGIN`,
`password\s*=`, `api[_-]?key\s*=`, `secret\s*=`. On a match, halt and report the item to the user for
manual inspection — the ledger is a committed artefact and must not carry credentials. This is
distinct from any source-diff secret scan: that scans code, this scans the ledger-write payload.

**Two-call write pattern** (both required; omitting either leaves the ledger inconsistent — the pattern-item `instances` append above is a third, conditional call that precedes them):

```bash
printf '%s' "$OPS_JSON" | tomlctl items apply <ledger> --ops -
tomlctl set <ledger> last_updated <YYYY-MM-DD>
```

Call 1 batches every per-item transition atomically — valid `op` values are `"add"`, `"update"`,
`"remove"`; apply carriers use `"update"` for status transitions and `"add"` when minting a
regression or partial-apply child. Call 2 is required because `items apply` does not touch
file-level scalars.

**Atomicity**: `items apply` is all-or-nothing — any failing op (non-existent ID, malformed sub-op)
exits non-zero with the ledger unchanged. If call 1 fails, do NOT proceed to call 2; the
`last_updated` bump would create a torn state claiming a fresh update with no transitions landed.
Correct the failing op (the error names its index and reason) and retry the whole batch.

**Shell-quoting for agent-supplied JSON**: every agent-produced string in the payload
(`resolution`, rationale, note) MUST be RFC-8259 JSON-escaped before interpolation — `\`, `"`,
control chars, and the Unicode line separators (U+2028 / U+2029). **Prefer stdin** (the `-`
sentinel, as above): the shell
never sees the payload at argv level, so there is no quoting surface to misquote or exploit, and no
tempfile permission is needed. Fall back to a tempfile (deleted after the call) only if the calling
harness cannot pipe stdin. For batches of ≤ 3 items, a loop of single-item `tomlctl items update`
calls is also reasonable — per-call quoting is easier to audit than one large array.

**Concurrent invocation**: `tomlctl` holds an exclusive advisory lock on `<ledger>.lock` for each
write. A held lock (parallel apply run, overlapping `<PRODUCER>` + `<CMD>`) fails fast with
`lock held by PID …`; wait and retry. If the lock appears stranded, follow the tomlctl skill's
stale-lock recovery — do NOT delete the `.lock` file without confirming no live process holds it.

Preserve `schema_version` verbatim. **Do NOT delete the ledger file** — stable IDs, `rounds`, and
disposition history persist across runs.
