Verdict: The tests, the cap of three and "zero is normal" live only in engine prose (open-and-challenge.md:144-150, write-and-prove.md:80-88, elaborate.md:51). Code holds only the record shape and one rule, that a `surfaced` decision still `agent-assumed` blocks approval (schema.rs:258, approve.rs:226). So the question budget is a prose change. A decision `class` is a small additive field, about 6 Rust files plus docs. It needs omit-when-default and a schema-version stamp, because the store drops unknown keys (schema.rs:16).

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = []

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 144
claim = "Surface: the three tests, the cap of three ranked by if_false, zero is normal, record shape with surfaced = true, the default offered first, and how each answer is recorded"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 80
claim = "Surface again reuses the three tests and the item shape; Design sets door and provenance"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 51
claim = "Elaboration reuses the three tests, at most three questions in one AskUserQuestion call; the close lists agent-assumed decisions because no gate shows them (line 231)"

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 68
claim = "The gate shows assumptions and one_way_doors; the refusal table includes a surfaced decision still agent-assumed (line 138)"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 85
claim = "Provenance and Door vocabularies via the vocabulary! macro"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 239
claim = "Decision struct; is_unanswered = surfaced && AgentAssumed"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 16
claim = "Unknown keys are dropped on read; write_version stamps SCHEMA_VERSION only for attempt/gate_round/report (line 516)"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 724
claim = "decision_to_toml writes every field unconditionally"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 27
claim = "DECISION_KEYS allowlist (also feeds review_keys at line 62); upsert_decision at 188; field projection at 462-482"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 350
claim = "unanswered_decisions, the only surfacing rule in code, used by approve and stage"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 58
claim = "gate_report: assumptions = agent-assumed sorted by binds.len(); one_way_doors = door one-way; assumption() projection at 161"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 226
claim = "approve refuses on unanswered surfaced decisions"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 246
claim = "stage reports unanswered decisions as a prove-stage blocker"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 464
claim = "plan check's only decision class is plan/decision-bind-unknown"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 383
claim = "counts and the rendered Standing/Asked lines read provenance, door, surfaced, vetoed (line 644)"

[[source]]
path = "glimpse/src/model/plan.rs"
line = 176
claim = "glimpse mirrors Decision with serde(default) and no deny_unknown_fields, so a new key is tolerated"

[[source]]
path = "claude/skills/tomlctl/references/plan-store.md"
line = 160
claim = "Decision field table documenting provenance, door and surfaced"

[[source]]
path = "docs/plans/v2-planning.md"
line = 75
claim = "Design rationale: three tests carry the weight, empty is normal, vetoed measures surfacing (HiL-Bench recall 34.6%); one record type for surfaced items (line 211)"
```

## Answer

**Where things live.** The three tests, the cap and the "zero is normal" stance are prose only:
- Surface: `open-and-challenge.md` §5, lines 144-199.
- Surface again: `write-and-prove.md` §2, lines 80-88.
- Elaboration: `elaborate.md` line 51.

No code counts questions or classifies decisions. The rationale is in `docs/plans/v2-planning.md` lines 46, 61, 75 and 795-796, which together make it a documented deliberate choice. Any change has to reopen that rationale, not just edit the prose. The rationale carries its own escape hatch: line 651 says "Dogfooding decides whether a stronger prompt is needed". The `package-design-tools` evidence in the brief is that dogfood: D22 and D46 were reversed, and D37-D62 were redesigns after the build.

**What code holds.** Code holds the record and one rule:
- A decision has `provenance` (`user-stated`, `user-confirmed`, `agent-assumed`), `door` (`one-way`, `two-way`), and the booleans `surfaced` and `vetoed`.
- `is_unanswered = surfaced && agent-assumed` (schema.rs:258) is the only surfacing rule. `plan approve` refuses on it (approve.rs:226) and `plan stage` reports it (stage.rs:246).
- `plan check` has no surfacing class at all. Its only decision class is `plan/decision-bind-unknown` (check.rs:464).
- Nothing forces a one-way door to be surfaced. The gate only lists one-way decisions (approve.rs:66).

**Budget.** A question budget for taste, UX, cost and vocabulary questions is a prose edit to the three references, at zero code cost. A budget enforced in code would need a per-step question count against `[[run]]` rows. Nothing models that today, and it would not buy much over prose.

**`class` field.** A `class` field, for example `structure | taste | ux | cost | vocabulary`, costs about this much:
- **Rust, about 6 files:**
  - `schema.rs`: a vocabulary, the field, read and write, and tests.
  - `records.rs`: `DECISION_KEYS` (which also widens `review_keys` for free), `upsert_decision`, the field projection and tests.
  - `approve.rs`: add it to `assumption()` and optionally group the gate by class.
  - `view.rs`: counts and the Standing line.
  - Optionally `stage.rs` and `store.rs`, if the class feeds a rule.
- **Tests:** `tomlctl/tests/plan_records.rs`, `plan_approve.rs`, `plan_view.rs`. No new verb, so `output_options.rs` is untouched.
- **Docs:** `tomlctl/references/plan-store.md`, `plan.md` (lines 125, 386, 414), `review-v2-method/references/plan-records.md:56`, and the three engine references.
- **glimpse** is tolerant (serde default) and needs only an optional display line.
- **Two traps:**
  - Unknown keys are dropped on read (schema.rs:16), so an older binary would silently strip `class` on its next write. Extend `write_version` (schema.rs:516) to stamp a bumped `SCHEMA_VERSION` when any decision has a non-default class.
  - `decision_to_toml` writes every field unconditionally (schema.rs:724). Omit `class` while it is the default, so existing stores stay byte-stable.

**Cheapest lever.** The cheapest single code lever for the "a one-way door is never assumed silently" criterion is to widen the approval rule so that `door = one-way && agent-assumed` refuses unless `surfaced`. That is one predicate at schema.rs:258 or store.rs:350, plus its tests and the approve.md refusal row.

## Findings

- **Library/API or file**: claude/skills/plan-v2-engine/references/open-and-challenge.md:144-150 (§5 Surface); write-and-prove.md:80-88; elaborate.md:51
- **Source**: file reads at HEAD 52fe01b9
- **Evidence-grade**: high
- **Finding**: All three surfacing points use one set of three tests and a cap of three, and all of it is prose. Test 1 ("a different plan") excludes taste and UX choices by construction. A budget or class is a prose change in these three files.
- **Details**: Surface caps at three, ranked by `if_false`, and logs "nothing survived" with `plan run skip --step surface`. Surface again repeats tests 1-3 with no cap stated. Elaboration asks "at most three" in ONE `AskUserQuestion` call, so a larger elaboration budget may need more than one call. The default goes first, and taking it records `user-confirmed`.
- **Counter**: A Rust site that counts or caps surfaced decisions would refute this; `rg 'surfaced' tomlctl/src` found none beyond the shape and `is_unanswered`.
- **Impact on plan / report**: The question-budget decision, and how taste questions are admitted, land in prose. Edit all three references together, or elaboration and Surface again will drift from Surface.

- **Library/API or file**: tomlctl/src/plan/schema.rs:258 `Decision::is_unanswered`; store.rs:350 `unanswered_decisions`; approve.rs:226; stage.rs:246
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: The only surfacing rule in code is that a surfaced decision must be answered before approval. `plan check` has no surfacing class (check.rs:464 is its only decision check). Nothing forces a one-way or agent-assumed decision to be surfaced.
- **Details**: `gate_report` (approve.rs:58-78) lists `agent-assumed` decisions sorted by `binds.len()` and lists every one-way door. Neither drives a refusal. To enforce "never assumed silently", widen `is_unanswered` or add a sibling predicate (`door == OneWay && provenance == AgentAssumed && !surfaced`) in store.rs. Both approve and stage pick it up.
- **Counter**: If the brief's criterion means disclosure at the gate, which already happens, rather than asking, no code change is needed.
- **Impact on plan / report**: Offers a one-predicate enforcement option, versus a prose-only stance.

- **Library/API or file**: tomlctl/src/plan/schema.rs:16 (unknown keys dropped), :516 `write_version`, :724 `decision_to_toml`; records.rs:27 `DECISION_KEYS`, :188 `upsert_decision`, :462 projection; approve.rs:161 `assumption`; view.rs:383, :644
- **Source**: file reads; schema test `unknown_keys_are_dropped` (schema.rs, tests module)
- **Evidence-grade**: high
- **Finding**: A `class` field on Decision is a small additive change to about 6 Rust files. It must be omitted while it holds the default, and the schema version must be stamped when it is set. Otherwise an older installed tomlctl strips it silently on the next write.
- **Details**: `from_toml` drops unknown keys, and `write_version` stamps `SCHEMA_VERSION` (3) only when attempts, gate rounds or reports exist. Add `|| any decision has a non-default class` and bump to 4, following the pattern the schema-module header describes. `DECISION_KEYS` also feeds `review_keys`, so plan review can target `class` with no extra work. Glimpse's mirror (glimpse/src/model/plan.rs:176) uses `serde(default)` and tolerates the key.
- **Counter**: If the planner keeps class in prose (for example a `why` prefix) instead of the store, none of this applies. The cost of that is losing gate grouping and per-class veto rates.
- **Impact on plan / report**: Without a schema stamp the field cannot be trusted across binaries. The rows that would carry this are `schema.rs`, `records.rs`, `approve.rs`, `view.rs`, the tests in `tomlctl/tests/plan_records.rs`, `plan_approve.rs` and `plan_view.rs`, and the docs in `claude/skills/tomlctl/references/plan-store.md`, `plan.md` and `claude/skills/review-v2-method/references/plan-records.md`.

- **Library/API or file**: docs/plans/v2-planning.md:75, :211, :651, :795-796
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: Asking little and treating "zero is normal" is a documented deliberate choice, grounded in HiL-Bench and AssumptionMiner. Its own risk line names dogfooding as the trigger for revisiting it, and this brief's evidence is that trigger.
- **Details**: Line 211 makes `vetoed` the measure of whether surfacing earns its place, and keeps a single record type for surfaced items. A `class` field fits that design, since it is still one record type, and gives a veto rate per class.
- **Counter**: If the user regards the v2-planning stance as still binding for taste questions, the budget change is a user decision, not a mechanical one. The `question-budget` unknown in the brief already routes it to the user.
- **Impact on plan / report**: Record the change as a decision that cites v2-planning.md:651 as its revisit trigger.

Coverage: Read in full: open-and-challenge.md §5, write-and-prove.md §1-2, approve.md, and the elaborate.md surfacing lines. Read the decision and plan-store parts of schema.rs, records.rs:20-280 and 455-490, approve.rs:40-245, store.rs:340-375, stage.rs:235-290, check.rs:460-505, view.rs:375-425 and 630-665, and glimpse/src/model/plan.rs:165-200. Ran `rg` sweeps for `surfaced`, `provenance` and `door` across tomlctl/src, tests, claude/ and docs. Ruled out: a code-side cap (none exists); glimpse breakage (serde default); `output_options.rs` impact (no new verb). Out of lens: `AskUserQuestion`'s per-call question limit bounds elaboration's "ONE call" rule. The design-step lens should confirm that limit.
