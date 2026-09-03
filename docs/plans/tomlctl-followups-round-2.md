# Plan: tomlctl follow-ups, round 2

**Plan path**: `docs/plans/tomlctl-followups-round-2.md`
**Created**: 2026-09-03
**Status**: draft

## Context

Eight items promoted out of `.claude/backlog.toml` in a `/backlog` sweep on 2026-09-03. Six were
minted by the `tomlctl-followups` flow as it ran; one came from a sibling repo's `/implement` run;
one was minted during the sweep itself. They are follow-ups, not a feature — the unifying thread is
that most of them are **checks that do not check**: two tests that pass whether or not the code
works, a documentation rule whose gate cannot see the violations it names, a README instruction that
silently returns nothing, a line-ending pin that never took effect, and a clustering view that
cannot answer the question it appears to answer.

Exploration corrected the stated premise of three items and directed research invalidated the
mechanism chosen for a fourth. Those corrections are recorded inline below and are the reason this
plan is not a straight transcription of the eight capture rows.

The intended end state: every one of the eight is either fixed or explicitly closed with its reason
recorded, and the two tests that cannot currently fail can.

## Scope

**In scope**
- The five finding-id leaks in assertion and panic messages.
- The two loose clap stderr assertions and the unfiltered `count_distinct` slow-path test.
- A `--per-tag` flag on the backlog tags cluster view.
- A new `items fingerprint <file> <id>` read verb, plus the README correction that motivated it.
- Tightening `scripts/doc-diff-gate.sh` with a wording-scoped rule, left in `warn` mode.
- Pinning line endings for `.githooks/**` and `scripts/**`, and refreshing every already-pinned path.
- The `/implement` harvest third option, in both the orchestrator and the shared agent block.

**Out of scope**
- Promoting the doc gate from `warn` to `block` — the hook's own precondition is unmet (D5).
- Widening `LEDGER_DENY` to include `R` — measured at 12 false positives (D10).
- A payload-probe form of `items fingerprint` (`--json -`) — offered and declined (D4).
- Enabling `core.hooksPath` in this clone. It is per-clone developer config, not repo state, and
  changing it is not a repo edit. Carried as a risk instead.
- Closing or re-scoping the parent `tomlctl-followups` flow (D2).

**Affected areas**: `tomlctl/src`, `tomlctl/tests`, `tomlctl/README.md`, `scripts`, `.githooks`,
`claude/commands`, `claude/agents`, `claude/skills/tomlctl/references`, `.gitattributes`

## Exploration Notes

Source: three parallel Explore agents over the test/assertion surface, the dedupe/cluster/CLI
surface, and the repo governance surface. Every claim below is quoted from a read of the file
named; three of the eight backlog items had premises that did not survive the read, recorded as
**CORRECTION** lines.

### B-0b8e45d9 — finding ids in user-visible failure text

Five confirmed sites, all in assertion or panic strings that reach CI output:

- `tomlctl/src/io.rs:1982` — `"O44 regression: lock path must not be sidecar ..."`
- `tomlctl/tests/capabilities.rs:171` — `"--ndjson must stay OUTSIDE the shape ArgGroup (R82 + R76); ..."`
- `tomlctl/tests/capabilities.rs:811` — `"pre-T8 prefix must be unchanged, ..."`
- `tomlctl/tests/capabilities.rs:984` — `"stderr must carry the T9 not_found prose, ..."`
- `tomlctl/tests/capabilities.rs:1025` — `"message must be the T9 strict-read prose, ..."`

The classification that matters: a token is a **leak** when removing it loses no information about
the assertion's subject, and **data under test** when it is an `id =` value in an inline TOML
fixture or the row the assertion operates on. Data-under-test hits that must be left alone include
every `id = "R1".."R7"` / `"O1"` fixture row (`src/convert.rs`, `src/dedup.rs:650-995`,
`src/items.rs`, `src/query.rs:1911`), the `e.g. {"id":"R1",...}` example JSON inside error prose
(~11 sites in `src/items.rs`), assertions naming their own row (`src/items.rs:3125`, `:3695-3708`,
`:2627`), and `"cause":"test-R57"` (`src/items.rs:2331,2343`). `T00:00:00Z` in `flow_stale.rs:92-93`
and `src/flow/stale.rs:131,150` is the ISO-8601 time separator, not a `T`-prefixed id.

### B-d57bcdf9 — clap stderr assertions that cannot fail

- `tomlctl/tests/items_dry_run.rs:1069` — `stderr.contains("--strict-read") || stderr.contains("unexpected argument")`; exit code pinned `.code(2)` at `:1060`.
- `tomlctl/tests/blocks.rs:33-36` — bare form only: `contains("unexpected argument") || contains("argument '--") || contains("found argument")`.

Patterns to copy, already tightened: `tests/integration.rs:319` requires **both** flag names;
`tests/integration.rs:129-130` chains `.stderr(contains("required"))` with
`.stderr(contains("--prefix <PREFIX>"))`; `src/cli/dispatch/tests/lint.rs:417` asserts the full
`"unexpected argument '--bogus-flag' found"`.

`error-context` is enabled (`tomlctl/Cargo.toml`), with `suggestions` deliberately off to avoid a
`strsim` dependency — so the flag name is guaranteed in the rendering but typo hints are not.

### B-87020d9e — the sort-engaged count_distinct path is never filtered

`src/query.rs:2470-2486` runs both queries with no predicate, so filter ordering is unobservable.
Branch selection is `window_untouched = q.sort_by.is_empty() && !q.distinct && q.offset.is_none()
&& q.limit.is_none()` (`src/query.rs:543-544`, mirrored at `:747-748` for `run_streaming`); only
then does the fast path at `:580` run. Any of `sort_by` / `distinct` / `offset` / `limit` falls
through to `build_pipeline` (`:469`) → `apply_aggregation_count_distinct` (`:1567`). `apply_filters`
(`:531`) precedes both. So arming the slow branch needs a filtered fixture whose filtered and
unfiltered cardinalities differ, **plus** one of those four fields set.

The armed fast-path fixture to mirror is `count_distinct_after_filter` (`src/query.rs:2410-2464`):
six rows, `status == open` → 3 distinct, unfiltered control → 5.

### B-6e18ece1 — no single-row tier-B confirmation path

`tomlctl/README.md:143-145` tells the reader to rerun `items find-duplicates --tier B` to confirm
an illustrative digest. `find_duplicates_tier_b` (`src/dedup.rs:283-315`) skips groups with
`idxs.len() < 2` at `:300-304`; the JSON twin repeats it at `:554-557`, and tiers A and C carry the
same gate. A unique row therefore emits `[]`.

**No CLI verb or flag computes and prints a single row's ledger tier-B digest.** The closest
analogue is `backlog check`, which does emit a `dedup_id` for an unstored row — but that is the
backlog 3-field digest (`kind|area|normalise(summary)`, `backlog/ids.rs:32`), explicitly not
`dedup::FINGERPRINTED_FIELDS`. A new verb would call `tier_b_fingerprint_json`
(`src/dedup.rs:205`, the payload-shaped one) or `tier_b_fingerprint_table` (`:201`, the stored-row
one); `tier_b_fingerprint` (`:197`) is `#[cfg(test)]`-gated and unusable in production.

### B-108e130a — the tags cluster view cannot surface a single shared tag

`cluster_tags` (`src/backlog/cluster.rs:195-234`) unions any pair whose tag intersection reaches
`min_shared`, then merges transitively. Singletons are dropped not in `finish` but one level up, in
`components` (`:296-303`, `out.retain(|_, members| members.len() > 1)`).

**`finish` and `group_json` are already overlap-capable** — both take order-agnostic index lists
with no uniqueness constraint, so one index may legally appear in two groups. The partition
assumption lives entirely in the union-find pipeline (`parent: Vec<usize>` assigns each index one
root). A per-tag view would bypass union-find: invert to `BTreeMap<&str, Vec<usize>>`, apply its own
size threshold, hand tuples straight to `finish`. No change to `finish`, `group_json`, or the
emitted group schema.

Key-grammar conflict to resolve: `cluster_tags` keys on a tag *set* (`"ci+windows"`); a per-tag view
keys on one tag (`"ci"`) — same JSON shape, different grammar in one document.

### B-b3799a01 — the sweep rule's gate

**CORRECTION: a gate already exists.** `scripts/doc-diff-gate.sh` implements the rule in *warn*
mode (`MODE="${DOC_GATE_MODE:-warn}"`, exits 0 on findings). The item's framing — that the rule has
no gate — is wrong; what is right is that the gate is shape-specified.

```
LEDGER_DENY='[OWTP]'
LEDGER_RE="^[[:space:]]*(//|[*]|#)[[:space:]]*$LEDGER_DENY[0-9]{1,3}([.][0-9]+)?[-:]"
PHASE_RE='(phase [0-9]+[.][0-9]+|user decision [0-9]+|adr-[0-9]+ d[0-9]+)'
```

**CORRECTION: `P` is already in the character class.** The item claimed P-prefixed ids escape the
class; they escape on *anchoring*. `LEDGER_RE` requires the id to open a comment line, so `per P16`
mid-sentence never matches. There is no `\b` anywhere — the real escape is the mandatory `[-:]`
after `[0-9]{1,3}`, which `T11a:` defeats, and the mandatory `[0-9]{1,3}`, which `T-glistening`
defeats. Markdown escapes wholesale (`SRC_RE` excludes `.md`), and `review round` and `agent name`
have **no pattern at all**.

The script's own header concedes the design: *"A clean G2 run means 'the idiom did not appear',
never 'the comments are fine'."*

### B-652608e4 — line endings

**CORRECTION: the claim is inverted.** `git ls-files --eol` shows `i/lf` for all six files; it is
the **working tree** that is CRLF (`w/crlf`) for every one but `scripts/doc-diff-gate.sh`. Nothing
is committed as CRLF. `attr/` is empty on every row, confirming no pin.

The risk survives the correction, because the hook executes the *working-tree* file — but the fix
is the same and the rationale must be restated: pin with `text eol=lf` so the working tree is LF
regardless of `core.autocrlf`.

`.gitattributes` pins exactly three patterns, each under a comment naming the concrete breakage:
`*.sql text eol=lf`, `*.rs text eol=lf`, `claude/agents/*.md text eol=lf`. House style is
directory-scoped globs over individual paths. The third pin exists for precisely this CRLF-vs-awk
hazard.

### B-b7da5466 — the /implement harvest binary

Sub-agent surfacing text sits at `claude/agents/implement-deep.md:74-86` and
`claude/agents/implement-lite.md:76-88`, **byte-identical**. Orchestrator minting sits at
`claude/commands/implement.md:85`, under `## Phase 4: Report` (not Phase 3 as the capture said).
No passage anywhere licenses adding a task to the current run; `backlog-capture/SKILL.md:23-24`
draws the line at *"In scope means fix it"* and `:30` forbids minting anything you are about to fix.

**Shared-block hazard.** The agent-side passage is inside the `backlog-candidates` block listed in
`scripts/shared-blocks.toml`, spanning both implementer carriers — edits must be byte-identical
across both files, enforced by the pre-commit verifier and by `blocks_verify_matches_shell_extraction`.
The `implement.md` passage is in no shared block and is freely editable.

### Cross-cutting: what actually enforces anything

**CORRECTION: neither venue currently runs in this clone.** `core.hooksPath` is unset
(`git config --get core.hooksPath` exits 1) and `.git/hooks/` holds no `pre-commit`; there is no
`.github/` and no workflow YAML anywhere. `cargo test` runs only by hand.

Existing gate mechanics, for choosing where a new gate lives:

| gate | venue | self-skips? |
|---|---|---|
| `scripts/verify-shared-blocks.sh` | pre-commit, unconditional | no — `exit 2` on missing input |
| `scripts/verify-plan-story-blocks.sh` | pre-commit, unconditional | no — `exit 1` on missing input |
| `scripts/doc-diff-gate.sh` | pre-commit, warn mode | reports, never blocks |
| `command_lint` (`src/cli/dispatch/tests/lint.rs:261`) | `cargo test` | yes — "claude/ dir not found" |
| `carrier_invokes_required_skills` (`.../tests/skills.rs:967`) | `cargo test` | yes — same |

`cargo fmt` is the only staged-path-gated hook step (`grep -q '\.rs$'`). `skills.rs:181` already
warns that hook-only enforcement lets a `--no-verify` commit land drift with `cargo test` green.

### Tests any new CLI surface must update

- `tests/backlog_read.rs:593` — `assert_eq!(keys, vec!["area","relations","tags"])`. **A fourth view breaks this.** Unit twin at `cluster.rs:539-554`.
- `tests/capabilities.rs:2163` `readme_feature_transcriptions_match_capabilities_features` — hard parity between the `FEATURES` const (`src/cli/types.rs:20-57`), the capabilities sample block, and the README "Feature meanings" table (`README.md:327`).
- `tests/capabilities.rs:27` — a new read verb belongs in the `read_subs` list (`:28-47`).
- `command_lint` — any new flag documented in `claude/skills/*/`, `claude/commands/*.md` or `claude/agents/*.md` must actually parse. Live doc sites: `claude/skills/tomlctl/references/backlog.md:212-225`, `claude/commands/backlog.md:37`.

### Conventions

Tests are **both** inline `#[cfg(test)] mod tests` (33 src files) and black-box `tests/*.rs` via
`assert_cmd` + `predicates`. Shared helpers: `tests/common/mod.rs` (`seed_ledger`, `run_list_query`,
`ids_from`, `assert_sidecar_matches`, `parse_json_error_envelope`, `sandbox`, `cli`, `backlog`,
`git_available`), `src/test_support.rs` (`env_lock`, `RootGuard`, `with_root`), `src/query.rs:1911`
(`fixture()`). No snapshot framework — no `insta`, no `expect-test`; JSON shapes are pinned by
hand-written `assert_eq!` on parsed `serde_json::Value`.

CLI house style: clap derive, `pub(crate)` struct variants, `///` on the variant and inline
`help = "..."` on one-line args, explicit `long = "..."` for multi-word flags, `default_value_t` for
typed defaults, `#[command(flatten)] integrity: ReadIntegrityArgs|WriteIntegrityArgs` always last.
`backlog/dispatch.rs` destructures every variant field-by-field with no `..` rest pattern, so a new
flag fails to compile there rather than being silently dropped.

## Research Notes

Two `research-lite` agents, both vetted against primary sources. Agent-1: 6 findings sampled,
0 dropped, 0 downgraded. Agent-2: 3 findings sampled, 0 dropped, 1 downgraded.

### clap error rendering — what a tightened assertion may rely on

Resolved versions from `tomlctl/Cargo.lock`: `clap` / `clap_builder` **4.6.6**, `assert_cmd`
**2.2.2**, `predicates` **3.1.4**.

- **`UnknownArgument` renders the flag token verbatim.** Format string at
  `clap_builder-4.6.6/src/error/format.rs:358`:
  `"unexpected argument '{invalid}{invalid_arg}{invalid:#}' found"`. Colour is off in tomlctl
  (`default-features = false` omits `color`), so the styles resolve to plain and the bytes are
  literally `error: unexpected argument '--bogus' found` — no ANSI escapes to match around.
  *Impact*: assert `unexpected argument '--bogus' found`; the quoted token is the load-bearing part.

- **The feature-removal guard must be the stderr substring, not `ErrorKind`.**
  `#[cfg(not(feature = "error-context"))] pub use KindFormatter as DefaultFormatter;`
  (`src/error/mod.rs:45-48`). `KindFormatter::format_error` emits only `error: ` +
  `ErrorKind::as_str()`, and `UnknownArgument`'s bare string is `"unexpected argument found"`
  (`src/error/kind.rs:337`) — the embedded `'--flag'` is precisely what disappears.
  **`Error::kind()` is NOT feature-gated** (`src/error/mod.rs:180`), so a unit test matching
  `ErrorKind::UnknownArgument` passes with the feature off and cannot serve as the regression guard.
  *Impact*: B-d57bcdf9's fix has to stay a black-box stderr assertion. An `ErrorKind` unit test is
  a fine behavioural test but is not the guard being asked for.

- **`ArgumentConflict` names both flags; shape depends on count.** One prior arg renders inline
  (`the argument '--a' cannot be used with '--b'`); two or more render one per indented line
  (`src/error/format.rs:151-194`). *Impact*: assert each token as a separate `contains`, never as
  one joined string.

- **`MissingRequiredArgument` renders the placeholder form** `--prefix <PREFIX>` on its own indented
  line (`format.rs:249-259`). Derive sets `value_name` to SCREAMING_SNAKE of the field name; a
  hand-built `Arg` with no explicit `value_name` falls back to the lowercase raw id.
  *Impact*: the placeholder form is assertable for derive-defined args only.

- **A `tip:` line can still appear with `suggestions` off.** The trailing-arg tip
  (`tip: to pass '--bogus' as a value, use '-- --bogus'`) is gated on `error-context`, not on
  `strsim`. *Impact*: use `contains`, never `starts_with` or equality, on UnknownArgument stderr.
  Graded INFERRED — the call-site gating was not fully traced.

- **clap does not promise error wording across patch releases.** Its CONTRIBUTING compatibility
  policy admits "changes in help/error output that are one-off or improving consistency" in patch
  releases. *Impact*: keep the assertion minimal (token presence only) and state its intent in a
  comment; expect to revisit on clap bumps. This is a known, accepted maintenance cost — the
  alternative is a test that cannot fail, which is the defect being fixed.

- **Idiom**: `Assert::stderr` takes a `Predicate<str>` directly, and `.and()`
  (`predicates::prelude::PredicateBooleanExt`) composes several `contains` into one assertion with
  a tree-shaped failure breakdown naming the failing conjunct.

### git line endings — the pin alone does not flip existing files

- **Source of the CRLF worktree**: system-level `core.autocrlf=true` from
  `C:/Program Files/Git/etc/gitconfig`. `core.eol` and `core.safecrlf` are both unset. Per
  gitattributes(5) a path's `eol` attribute wins outright over both. *Impact*: this is a
  Windows-only symptom; a Linux clone checks these out LF today. The pin is harmless there.

- **A `text eol=lf` pin does NOT rewrite already-checked-out files.** Proven in-repo:
  `claude/agents/*.md` was pinned in commit `b8d7368` (2026-09-02) and
  `git ls-files --eol` still reports `i/lf w/crlf attr/text eol=lf` for `implement-deep.md` and
  `implement-lite.md`. The attribute governs conversion, and git converts only when it writes the
  file. `git add --renormalize .` re-stages through the clean filter, but this index is *already*
  LF, so it yields zero index change and therefore zero re-checkout.
  *Impact*: **the plan needs an explicit worktree-refresh step**, and it must also repair the three
  `claude/agents/*.md` files the previous pin left CRLF. Safe per-file form:
  `rm <paths> && git checkout -- <dirs>`. The repo-wide `git rm --cached -r . && git reset --hard`
  form destroys uncommitted changes and must not be used here.

- **The shebang is load-bearing.** `.githooks/pre-commit:11-14` invokes each verifier **directly**
  (`"$ROOT/scripts/verify-shared-blocks.sh"`), not via `bash <file>`. On Linux nothing strips the
  CR, so `#!/usr/bin/env bash\r` makes `env` fail with `env: 'bash\r': No such file or directory`.
  Git Bash / MSYS2 tolerates CRLF completely — shebang and body, `set -euo pipefail\r` included —
  which is why the defect is invisible on the dev machine. *Impact*: "it works here" is not
  evidence against the pin.

- **`*.sh` does not match `.githooks/pre-commit`** (no extension), and gitattributes patterns that
  match a directory do **not** recurse — the trailing-slash `dir/` form is a no-op, `dir/**` is
  required (gitattributes(5)). *Impact*: pin `.githooks/** text eol=lf` and `scripts/** text eol=lf`.
  A bare `*.sh` pin would leave the hook itself — the one file git executes directly — unfixed, and
  would also miss `scripts/shared-blocks.toml` and `scripts/templates/flow-context.md`, which are
  CRLF and consumed by the same gawk whole-line matcher.

- **No side effects to expect.** `eol` affects only checkout/checkin conversion; `git diff` compares
  index-normalised content so it is unchanged, and `core.safecrlf` is unset so no warning can fire.
  The commit stages no content change — the index is already LF. An empty `git status` after the
  refresh is the success signal.

**Downgraded**: the agent's claim that `set -euo pipefail\r` fails on Linux bash with
`set: pipefail<CR>: invalid option name` was self-graded low-confidence and could not be executed.
It is moot regardless — execution dies at the shebang before line 2 — so the plan does not rely on
it and it is recorded here only to note it was considered.

### Directed research additions

One `research-lite` agent on whether a line-oriented shell gate can scope to assertion-macro message
arguments. Vetted: 4 findings sampled, 0 dropped, 0 downgraded; the recommended match set was
re-run and reproduced exactly.

- **The gate reads a zero-context staged diff, one physical line at a time.**
  `scripts/doc-diff-gate.sh:79-92`: `git diff --cached -U0 --no-color --diff-filter=ACM` piped to an
  awk that emits `path<TAB>lineno<TAB>content` per `^+` line. *Two consequences*: it can see no
  multi-line context, and it can never flag a line the current commit does not stage — so it cannot
  reach any of the five existing leaks. **A hand sweep is unconditional.**

- **Every existing rule is comment-only.** `doc-diff-gate.sh:200` short-circuits with
  `if (!is_c) next`, where `is_c` tests for a comment-opening line. All five leaks are string
  literals in code, so the new rule must be a new branch placed *before* that gate — a structural
  change, not a regex tweak.

- **Macro-scoped matching has 0/5 recall.** In all five sites the identifier is on the message
  continuation line and the macro name is 2-3 lines above: `io.rs` 1980 `assert!(` / 1981 condition
  / 1982 message; `capabilities.rs` 169→171, 809→811, 982→984, 1023→1025. `grep -A/-B` and an awk
  paren-balance state machine both fail for the same reason — under `-U0`, a message-only edit
  stages the continuation line without its opener. gawk is already a dependency
  (`doc-diff-gate.sh:49`), so a state machine would cost nothing; it simply would not work.

- **Message-shaped wording is the only viable discriminator, and `LEDGER_DENY` is what makes it
  precise.** Matching a quoted string that contains both a gated identifier and one of
  `regression|must|expected|got` yields exactly 5 hits repo-wide: four of the five leaks plus
  `tests/flow_stale.rs:93` (`"last_activity must be \`<today>T00:00:00Z\`, got: {last}"` — the `T00`
  of an ISO timestamp). Pre-stripping ISO datetimes with the script's existing `DATE_RE` plus
  `T[0-9]{2}:[0-9]{2}:[0-9]{2}Z` removes that one, giving **0 false positives**.

- **Widening the prefix class to include `R` is unusable**: 18 hits, 12 false positives
  (`items.rs:3695/3701/3708`, `items_dedupe.rs:80/248`, `items_dry_run.rs:152`,
  `render_progress_log.rs:423/427/431`, `integration.rs:418`). `LEDGER_DENY='[OWTP]'`
  (`doc-diff-gate.sh:66`) deliberately excludes `R` — its comment records that `R<n>` is a durable
  lumina requirement id cited on purpose, and `E<n>` an execution-record entry.

- **All data-under-test sites stay clean** under the `[OWTP]`-scoped rule: the `id = "R1"` fixtures
  in `dedup.rs:650-995` and `query.rs:1914-1970`, the `e.g. {"id":"R1",...}` error-prose examples in
  `items.rs`, `"cause":"test-R57"`, and the self-naming assertions at `items.rs:3125` and `:2627` —
  every one excluded by the `R` carve-out rather than by an allowlist.

## User Decisions

Eight decisions across two `AskUserQuestion` batches. Each records the finding that prompted it.

**D1 — Scope: one plan, all eight items.** *Prompted by*: the batch spanning six unrelated areas
with two `direction` items whose design was open. The user chose to keep the batch together rather
than split the mechanical work from the design work.

**D2 — Flow: proceed on `main` with scope overlapping `tomlctl-followups`.** *Prompted by*: the
parent flow sitting at `status=review` on the same branch with near-identical scope globs. Accepted
consequence: flow resolution will tie between the two and fall back to `active-latest`, so `/review`
and `/optimise` may need an explicit `--flow`.

**D3 — B-108e130a: a `--per-tag` flag on the existing tags view.** *Prompted by*:
`tests/backlog_read.rs:593` asserting the view keys are exactly `["area","relations","tags"]`, with
a unit twin at `cluster.rs:539-554`. Keeping the fix inside the existing `tags` key leaves both
assertions green. The key grammar shifts from a tag set (`"ci+windows"`) to a single tag (`"ci"`)
only when the flag is passed.

**D4 — B-6e18ece1: add `items fingerprint <file> <id>`.** *Prompted by*: `README.md:143-145`
pointing at a confirmation path that emits nothing, and the absence of any verb that prints a single
row's tier-B digest. Reads a stored row and reuses `tier_b_fingerprint_table` (`src/dedup.rs:201`).
Agreed output shape:

```json
{
  "id": "R1",
  "tier": "B",
  "dedup_id": "a3f1c09e2b7d4e58",
  "fields": {
    "file": "src/auth.rs",
    "summary": "token not refreshed",
    "severity": "high",
    "category": "correctness",
    "symbol": "refresh_token"
  }
}
```

The payload-probe variant (`--json -`) was offered and not taken, so `tier_b_fingerprint_json`
stays unused by this verb.

**D5 — B-b3799a01: tighten `scripts/doc-diff-gate.sh`, keep `MODE=warn`.** *Prompted by*: the
correction that the gate already exists, and the hook's own comment setting a precondition for
promotion — *"once its false-positive rate is proven against real commits rather than specimens"*.
This plan does not satisfy that precondition, so it does not flip the mode.

**D6 — Gate scope: assertion and panic macro message arguments only.** *Prompted by*: B-0b8e45d9's
five leaks living in string literals while `LEDGER_RE` matches only comment-opening lines — and the
counter-risk that matching all string literals would flag every `id = "R1"` fixture row and the ~11
`e.g. {"id":"R1",...}` examples in `src/items.rs` error prose, which are data under test. Scoping to
macro message arguments catches all five leaks with no allowlist to go stale.

**D7 — B-b7da5466: edit both the orchestrator and the shared block.** *Prompted by*:
`claude/commands/implement.md:85` being freely editable while the agent-side text sits inside the
`backlog-candidates` shared block spanning `implement-deep.md` and `implement-lite.md`. The agents
gain a signal that a discovery is cheap and in-file; the orchestrator gains the judgement call.
**Constraint**: the two carrier edits must be byte-identical, enforced by
`scripts/verify-shared-blocks.sh` and `blocks_verify_matches_shell_extraction`.

**D8 — B-652608e4: pin the new paths and refresh every already-pinned path.** *Prompted by*: the
in-repo proof that `claude/agents/*.md`, pinned in `b8d7368`, is still `w/crlf`. End state is that
`git ls-files --eol` agrees with `.gitattributes` for every pinned path — which requires repairing
the three agent carriers the earlier pin missed, not just the new ones.

**D9 — Checkpoint cadence: `milestones`.** *Prompted by*: ~15-18 unique files across 8 largely
independent items, two of which move cross-cutting parity gates. Checkpoints land after the new CLI
verb (which moves `FEATURES`, the README table and three test files together) and after the
shared-block edit (where a verifier failure would block every later commit).

**D10 — Adopt the wording-based gate rule and accept both blind spots.** *Prompted by*: Phase-5
research proving D6's mechanism unachievable. The rule matches a quoted string containing both a
`LEDGER_DENY` identifier and one of `regression|must|expected|got`, with ISO datetimes pre-stripped.
Two accepted blind spots, both recorded so no later reader mistakes them for oversights:
1. It catches **4 of 5** leak classes. `tests/capabilities.rs:171`'s `R82 + R76` stays invisible
   because `R` is deliberately outside `LEDGER_DENY` as a durable lumina requirement id — widening
   to include it costs 12 false positives.
2. It is **staged-diff-only**, so it prevents future leaks and catches none of today's. The hand
   sweep in T1 is unconditional and is not made redundant by the gate.

### Phase 5 outcome

Ran, and it invalidated D6's mechanism. See **Directed research additions** above. D6's *intent*
survives; its stated implementation does not. The substitute is D10.

## Approach

Each item is fixed at its own site; there is no shared refactor and no common abstraction to build.
The design work is therefore mostly about **ordering and file exclusivity**, not architecture.

Three decisions shape the task graph:

**The clap assertions must stay black-box.** `Error::kind()` is not feature-gated, so a unit test
matching `ErrorKind::UnknownArgument` passes with `error-context` removed and cannot be the
regression guard. The guard is the embedded `'--flag'` token in stderr, which is exactly what the
bare `KindFormatter` fallback drops. Accepted cost: clap does not promise error wording across patch
releases, so this assertion will need revisiting on clap bumps. That is strictly better than a test
that cannot fail.

**The `--per-tag` view bypasses union-find rather than extending it.** `finish` and `group_json`
already accept overlapping member lists; only `parent: Vec<usize>` assumes a partition. So the new
branch inverts to `BTreeMap<&str, Vec<usize>>` and applies its own size threshold, because the
singleton drop currently lives in `components` — which is union-find-shaped and will not be on the
new path. Keeping it inside the existing `tags` key is what leaves the two exact-key assertions
green.

**The line-ending pin is inseparable from its refresh.** A pin alone provably does nothing to
already-checked-out files — `claude/agents/*.md` has been pinned since `b8d7368` and is still CRLF.
Splitting the pin from the refresh would recreate the exact half-done state this item exists to
correct, so T9 carries both despite exceeding the usual file cap.

Two file-claim collisions force edges that are not conceptual dependencies but are real scheduling
constraints, since `/implement` dispatches on the `Files` line: `tomlctl/src/cli/types.rs` is
claimed by both T4 and T6, and `tomlctl/tests/capabilities.rs` by both T1 and T7. Each is resolved
by an edge rather than by merging the tasks.

## Verification Commands

```bash
cargo build --manifest-path tomlctl/Cargo.toml
cargo test --manifest-path tomlctl/Cargo.toml
cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
cargo audit --file tomlctl/Cargo.lock
```

Prefix a full verification pass with `CARGO_INCREMENTAL=0` (PowerShell:
`$env:CARGO_INCREMENTAL=0; cargo …`) — sccache cannot cache incremental compilations.

Additional gates, run by hand because nothing runs them automatically in this repo:

```bash
bash scripts/verify-shared-blocks.sh
bash scripts/verify-plan-story-blocks.sh
bash scripts/doc-diff-gate.sh
cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
```

`cargo fmt` cannot be narrowed to a path — it is crate-wide by construction and may report files
this plan never touches.

Manual verification after T9, which no test covers:

```bash
git ls-files --eol .githooks scripts claude/agents | grep -v 'w/lf'
```

Expected output: **empty**. Any surviving `w/crlf` row is a path the refresh missed.

## Execution Policy

- **Checkpoints**: `milestones`
- **Checkpoint markers**: after T6, and at the end of the plan
- **Max parallel agents**: 6
- **Commit granularity**: `per-task`

Checkpoint A sits after T6 because that is where the new verb lands, moving `types.rs`,
`cli/dispatch.rs` and `README.md` together across a parity gate. Checkpoint B closes the plan after
the shared-block edit, where a `verify-shared-blocks.sh` failure would otherwise block every later
commit. Both groups are valid topological cuts of the graph below.

## Tasks

### 1. Sweep the five finding-id leaks from failure messages

- **Files**: `tomlctl/src/io.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: none
- **Action**: Rewrite five assertion/panic message strings so they no longer name a harness finding
  id, keeping each assertion's subject intact.
- **Detail**: `io.rs:1982` (`"O44 regression: …"` → state the invariant without the id);
  `capabilities.rs:171` (`"… (R82 + R76); got stderr:\n{stderr2}"`), `:811` (`"pre-T8 prefix …"`),
  `:984` and `:1025` (both `"… the T9 … prose …"`). Say what the assertion checks, not which finding
  asked for it. Do **not** touch identifier-shaped tokens that are data under test — the `id = "R1"`
  fixture rows, the `e.g. {"id":"R1",...}` error-prose examples, `"cause":"test-R57"`, the
  self-naming assertions at `items.rs:3125` and `:2627`, or the `T00:00:00Z` ISO separators.
- **Acceptance**: `git grep -nE '"[^"]*\b[OWTP][0-9]{1,3}\b' -- 'tomlctl/src/io.rs' 'tomlctl/tests/capabilities.rs'`
  returns no line that is a failure message. `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities`
  passes.
- **Effort**: S

### 2. Tighten the two loose clap stderr assertions

- **Files**: `tomlctl/tests/items_dry_run.rs`, `tomlctl/tests/blocks.rs`
- **Depends on**: none
- **Action**: Replace both assertions so each requires the offending flag token to appear in stderr,
  making them fail if `error-context` is ever removed from the clap feature list.
- **Detail**: `items_dry_run.rs:1069` currently ORs `contains("--strict-read")` against
  `contains("unexpected argument")`; `blocks.rs:33-36` asserts only the bare form. Both must assert
  the full `unexpected argument '<flag>' found` shape — that exact string is what
  `clap_builder/src/error/format.rs:358` renders with the feature on, and the bare
  `unexpected argument found` (`error/kind.rs:337`) is what it degrades to without. Follow
  `tests/integration.rs:319`, which requires both conflicting flag names. Use `contains`, never
  `starts_with` or equality — a `tip:` line can follow. Leave the `.code(2)` exit-code pin at
  `items_dry_run.rs:1060` in place. Add a brief comment on each assertion naming the feature it
  guards, since clap does not promise this wording across patch releases.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test items_dry_run --test blocks`
  passes. Both assertions name a flag token; neither passes on the bare-kind rendering alone.
- **Effort**: S

### 3. Arm the sort-engaged count_distinct test with a filter

- **Files**: `tomlctl/src/query.rs`
- **Depends on**: none
- **Action**: Give `count_distinct_with_sort_is_valid_but_wasteful` a filtered fixture whose filtered
  and unfiltered distinct cardinalities differ, so a filter-ordering regression in the sort-engaged
  pipeline is detectable.
- **Detail**: The slow branch is selected when `window_untouched` is false — that is, when any of
  `sort_by` / `distinct` / `offset` / `limit` is set (`query.rs:543-544`). The existing test sets
  `sort_by` but passes no predicate, so ordering is unobservable. Mirror the fixture already used by
  `count_distinct_after_filter` (`query.rs:2410-2464`): six rows where `status == open` yields 3
  distinct categories against an unfiltered 5. Keep the fast-vs-slow equality assertion; add the
  filtered assertion alongside it so both paths are compared under a predicate.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml query::` passes, and the new
  assertion's expected value is 3 rather than 5 — i.e. it would fail if filtering ran after
  aggregation.
- **Effort**: S

### 4. Add a `--per-tag` flag to the backlog tags cluster view

- **Files**: `tomlctl/src/backlog/cluster.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/backlog/dispatch.rs`
- **Depends on**: none
- **Action**: Add a `--per-tag` boolean to `BacklogOp::Cluster` that switches the tags view from
  pairwise-union-find grouping to one group per individual tag, with items free to appear in several.
- **Detail**: Add the flag in `types.rs` next to `min_shared_tags` (~`:1132`), matching house style —
  explicit `long = "per-tag"`, inline `help = "..."`. Destructure it in `backlog/dispatch.rs`'s
  `Cluster` arm and forward it; that module destructures field-by-field with no `..` rest pattern, so
  omitting it fails to compile there. In `cluster.rs`, add a grouping function alongside `cluster_tags`
  that inverts to `BTreeMap<&str, Vec<usize>>` and hands `(tag, reason, members)` tuples straight to
  `finish`. It must apply its own `members.len() > 1` threshold, because the existing singleton drop
  lives in `components` (`:296-303`), which is union-find-shaped and not on this path. `finish` and
  `group_json` need no change — both already accept overlapping member lists. Keep the result under
  the existing `tags` key so the exact-key assertions stay green. Note the key grammar difference in
  a comment: set-keyed (`"ci+windows"`) by default, single-tag (`"ci"`) under the flag. Add an inline
  `#[cfg(test)]` unit test covering an item that lands in two groups.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml backlog::cluster` passes, including
  the new overlap test. `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_read` still
  passes — in particular the `assert_eq!(keys, vec!["area","relations","tags"])` at `backlog_read.rs:593`,
  which must be unaffected because no view key is added.
- **Effort**: M

### 5. Document the `--per-tag` flag

- **Files**: `claude/skills/tomlctl/references/backlog.md`
- **Depends on**: 4
- **Action**: Add a `--per-tag` row to the `backlog cluster` flag table and state the key-grammar
  difference it introduces.
- **Detail**: The table is at roughly `:212-225` and already carries the `--min-shared-tags` default-2
  row. Say plainly what the default cannot do — surface a single shared tag without transitively
  collapsing the store — since that is the question the flag exists to answer. Any `tomlctl …`
  invocation added inside a ```bash fence is re-parsed by `command_lint` against the real clap parser,
  so the flag spelling must match T4 exactly.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml command_lint` passes.
- **Effort**: S

### 6. Add the `items fingerprint <file> <id>` verb and correct the README

- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/README.md`
- **Depends on**: 4
- **Action**: Add an `ItemsOp::Fingerprint` variant that reads one stored row by id and prints its
  tier-B digest, then correct the README passage that currently points at a confirmation path
  emitting nothing.
- **Detail**: The variant takes a positional file path and item id plus a flattened
  `ReadIntegrityArgs` last, per house style. Its dispatch arm reuses `dedup::tier_b_fingerprint_table`
  (`src/dedup.rs:201`) — **not** `tier_b_fingerprint` (`:197`), which is `#[cfg(test)]`-gated. Emit via
  `output::print_json` in the shape agreed in D4: `id`, `tier`, `dedup_id`, and a `fields` object
  carrying the five `FINGERPRINTED_FIELDS` values. Add a `fingerprint` entry to the `FEATURES` const
  (`types.rs:20-57`) so downstream carriers can gate on it, and the matching row to the README
  "Feature meanings" table (~`README.md:327`) — `capabilities.rs:2163` asserts those two agree, and
  will fail if only one is edited. No `SUBCOMMANDS` entry is needed: that const lists top-level
  commands and `items` is already present. Finally, rewrite `README.md:143-145` so it names this verb
  instead of `items find-duplicates --tier B`, which skips every group of fewer than two members
  (`dedup.rs:300-304`) and so emits nothing for the unique row the passage describes.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes, including
  `readme_feature_transcriptions_match_capabilities_features`. Running the verb against a seeded
  ledger prints a 16-hex `dedup_id`, and the README's stated command reproduces it.
- **Effort**: M

### 7. Add black-box coverage for the fingerprint verb

- **Files**: `tomlctl/tests/capabilities.rs`
- **Depends on**: 1, 6
- **Action**: Add a black-box test asserting the new verb's output shape, and add it to the
  read-only-subcommand coverage if it belongs there.
- **Detail**: Use the `tests/common/mod.rs` helpers — `seed_ledger` to build the fixture and
  `sandbox` / `cli` to invoke. Assert the four top-level keys and that `dedup_id` is 16 hex
  characters; assert on the parsed `serde_json::Value`, not on raw bytes, since there is no snapshot
  framework in this crate. If the verb accepts `ReadIntegrityArgs`, add it to the `read_subs` list at
  `:28-47`, which asserts read verbs expose `--verify-integrity` and `--strict-read` while hiding the
  three write-integrity flags. Depends on T1 only for the file claim — the two edits do not interact.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes.
- **Effort**: S

### 8. Tighten the doc gate with a wording-scoped rule

- **Files**: `scripts/doc-diff-gate.sh`
- **Depends on**: none
- **Action**: Add a rule that flags a staged line containing a quoted string carrying both a
  `LEDGER_DENY` identifier and message-shaped wording, leaving `MODE` at its `warn` default.
- **Detail**: The new branch must sit **before** the `if (!is_c) next` short-circuit at `:200`, since
  every existing rule is comment-only and the target strings are code. Match a double-quoted string
  containing a `[OWTP][0-9]{1,3}` token and one of `regression|must|expected|got`, in either order.
  Pre-strip ISO datetimes using the existing `DATE_RE` plus `T[0-9]{2}:[0-9]{2}:[0-9]{2}Z` before
  matching — without that, `tests/flow_stale.rs:93` is a false positive on the `T00` of a timestamp.
  Reuse `LEDGER_DENY` (`:66`) rather than writing a second prefix class; its comment records why `R`
  and `E` are excluded, and widening it costs 12 false positives. Report through the existing
  `report()` helper at the same severity as the comment-scoped ledger rule. Do **not** change `MODE`,
  `SRC_RE` or `EXCLUDE_RE`.
- **Acceptance**: `bash scripts/doc-diff-gate.sh` exits 0 against a clean tree and reports nothing.
  Staging a specimen line such as `assert!(x, "T7 regression: must hold");` produces exactly one
  finding; staging `let s = "id = \"R1\"";` produces none.
- **Effort**: M

### 9. Pin line endings and refresh every pinned path

- **Files**: `.gitattributes`, `.githooks/pre-commit`, `scripts/verify-shared-blocks.sh`,
  `scripts/verify-plan-story-blocks.sh`, `scripts/shared-blocks.toml`,
  `scripts/templates/flow-context.md`, `claude/agents/implement-deep.md`,
  `claude/agents/implement-lite.md`, `claude/agents/verification.md`
- **Depends on**: none
- **Action**: Add `.githooks/**` and `scripts/**` pins to `.gitattributes`, then refresh the working
  tree so every path matching a `text eol=lf` pin is actually LF on disk.
- **Detail**: Exceeds the three-file cap deliberately — a pin without its refresh provably does
  nothing, which is the state `b8d7368` left behind and this task exists to correct. Use
  `.githooks/** text eol=lf` and `scripts/** text eol=lf`: a `*.sh` pattern would miss
  `.githooks/pre-commit`, which has no extension and is the one file git executes directly, and would
  also miss `shared-blocks.toml` and `flow-context.md`. A trailing-slash `dir/` form is a no-op in a
  gitattributes file; `dir/**` is required. Follow house style — a comment paragraph above each rule
  naming the concrete breakage, matching the three existing pins. Then refresh: `rm` the affected
  files and `git checkout --` them. Do **not** use `git rm --cached -r . && git reset --hard`; the
  working tree carries uncommitted changes to `.claude/backlog.toml` and that form would destroy them.
  The refresh must include the three `claude/agents/*.md` files the earlier pin left CRLF.
- **Acceptance**: `git ls-files --eol .githooks scripts claude/agents | grep -v 'w/lf'` returns
  nothing. `bash scripts/verify-shared-blocks.sh` still passes, confirming the rewrite did not disturb
  block extraction. `git status` shows no content diff for the refreshed files — the index was already
  LF, so only the working tree changes.
- **Measured baseline** (run 2026-09-03, not predicted): that command currently returns exactly 8
  rows — `.githooks/pre-commit`, `scripts/shared-blocks.toml`, `scripts/templates/flow-context.md`,
  `scripts/verify-plan-story-blocks.sh`, `scripts/verify-shared-blocks.sh`, and the three
  `claude/agents/*.md` files (`implement-deep`, `implement-lite`, `verification`) which already carry
  `attr/text eol=lf` and are still `w/crlf`. Those 8 are the refresh set and match this task's Files
  line. `git status --porcelain` over the same paths is empty, so nothing uncommitted is at risk —
  re-check that before the `rm`.
- **Effort**: M

### 10. Add the same-run task option to the /implement harvest

- **Files**: `claude/commands/implement.md`
- **Depends on**: none
- **Action**: Give the Phase-4 harvest an explicit third disposition — propose a task for the current
  run — alongside minting a backlog item, with a stated test for which applies.
- **Detail**: The harvest currently sits at roughly `:85` under `## Phase 4: Report` and offers only
  deferral. State the test the capture asked for: cost to fix now, blast radius, and whether the
  plan's verification already covers the touched file. Preserve the existing boundary that a plan
  deviation stays a `type=deviation` record and only out-of-plan discoveries are backlog candidates.
  This file is in no shared block and is freely editable — do not replicate the wording into any
  carrier under `claude/agents/`, which T11 handles under a different constraint.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml carrier_invokes_required_skills`
  passes — the edit must not disturb the `backlog-capture` skill invocation the test asserts.
- **Effort**: M

### 11. Let implementer agents signal a cheap in-file discovery

- **Files**: `claude/agents/implement-deep.md`, `claude/agents/implement-lite.md`
- **Depends on**: 9, 10
- **Action**: Extend the `TANGENTIAL:` contract inside the `backlog-candidates` shared block so an
  agent can mark a discovery as cheap and in-file, giving the orchestrator a signal to act on.
- **Detail**: **The edit must be byte-identical in both files.** This passage is a shared block listed
  in `scripts/shared-blocks.toml`, spanning `implement-deep.md:74-86` and `implement-lite.md:76-88`,
  and both `scripts/verify-shared-blocks.sh` and `blocks_verify_matches_shell_extraction` enforce
  that. Keep the existing rule that the agent is never the writer — this adds a field to what it
  reports, not permission to act. Depends on T9 because both files are in that task's CRLF refresh
  set; editing them before the refresh would mix a line-ending rewrite into a content change. Depends
  on T10 because the orchestrator-side rule must exist for the signal to feed.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes. `cargo test --manifest-path
  tomlctl/Cargo.toml blocks` passes, including `blocks_verify_matches_shell_extraction`.
- **Effort**: M

## Dependency Graph

```
CHECKPOINT A  (after task 6)
CHECKPOINT B  (end of plan)
```

Per-task `Depends on` edges are authoritative and are not mirrored here. Tasks 1, 2, 3, 4, 8, 9 and
10 have no predecessors and form the opening frontier.

## Risks

- **Neither enforcement venue runs in this clone.** `core.hooksPath` is unset, `.git/hooks/pre-commit`
  is absent, and there is no CI. So T8's tightened gate and T9's refreshed hook will not execute for
  this developer until `git config core.hooksPath .githooks` is run, and `cargo test` remains manual.
  Enabling it is per-clone config, outside this plan's scope — but a gate nobody runs is worth
  knowing about before relying on it.
- **The clap assertions in T2 are a standing maintenance cost.** clap's compatibility policy admits
  error-wording changes in patch releases, so a `clap` bump may break `items_dry_run` and `blocks`.
  This is accepted: a test that breaks loudly on a dependency bump beats one that cannot fail.
- **T9 rewrites nine files' bytes in the working tree.** The refresh is a `rm` plus `git checkout`,
  which discards any uncommitted change to those paths. None are currently modified, but that must be
  re-checked at execution time — the tree already carries unstaged edits to `.claude/backlog.toml`
  and its sidecar.
- **T11 edits a shared block.** A non-identical edit fails the pre-commit verifier and a Rust test,
  and because T11 sits at Checkpoint B a failure there blocks the plan's final commit rather than an
  intermediate one.
- **The `--per-tag` key grammar is overloaded.** The `tags` view will emit set-keys by default and
  single-tag keys under the flag, in a field readers may parse. Documented in T5, but a consumer that
  splits the key on `+` will behave differently under the flag.
- **T6 changes advertised capabilities.** Adding a `FEATURES` entry is observable to any downstream
  carrier gating on `tomlctl capabilities`; the parity gate ensures the README matches but not that
  consumers expect the new entry.
