Verdict: No agent drops target, expect or new: every v2-apply plan finding carried `target` and 68 of 74 defect/cleanup rows carried `new`. 24 confirmed rows with a full edit fell to advisory tier because the verifier's citation failed the cited test (tomlctl/src/review/tier.rs:547-579, query.rs:386), and apply refuses advisory tier (apply.rs:324-326), so the only way to land them was a hand edit and then a dismissal. Triage offers no "addressed" disposition (triage-and-close.md:117-129), and the re-check takes its targets only from applied/already rows (plan-flow.md:287, 327).

## Answer

**Where the edit fields go.** Measured on the v2-apply store, runs U1 and U2: all 81 findings carry `target`. 68 of the 74 defect/cleanup rows also carry `new`. The 7 advisory-kind rows cannot carry an edit at all, because the field catalogue allows `expect`, `new` and `proof` only on defect and cleanup rows. 6 defects had a fix that needed a decision and were written with no `new`. The finder, plan-executor and lead-settle stages lose nothing:

- The lead never writes finding fields; plan-review-lead.md:3 says it "never triages".
- The verifier projection shows `target`, `expect` and `new`.

The loss happens at tier stamping. 34 of 36 advisory-tier rows were not advisory-kind, and 24 of those carried an edit backed by a `confirmed` verdict. Every one of the 24 missed `cited` because of its citation:

| Citation problem | Rows | Findings |
|---|---|---|
| Names code or docs outside the target's file and outside its current value | 9 | F4 F13 F34 F39 F49 F50 F51 F63 F69 |
| Names a path that appears only in `new`, e.g. the file the fix adds to `files` | 7 | F2 F12 F27 F47 F48 F67 F70 |
| Names another document of the same flow, e.g. a design file for a `task:` target | 5 | F3 F10 F11 F28 F65 |
| Names the target's own file, but in compound form (`a:14; b:17`), which `parse_citation` rejects because it contains whitespace | 3 | F66 F68 F74 |

`review apply` refuses advisory tier even with `--user-accepted`, and `--convert` applies to advisory-kind rows only. So the only route left for a valid edit was a hand edit.

**Dispositions triage records.** `review triage` has these modes: `dismiss`, `defer`, `reopen`, `park`, `resolve`, `backlog-ref`, `convert` and `answer` (triage.rs:46-60). `review apply` adds `resolved` with an `applied` table holding `by`, `tier`, `target`, `old` and `new` (apply.rs:719-756). In flow mode the procedure offers:

- **Advisory tier:** park, backlog, convert or dismiss. Defect and cleanup rows in this tier are "never applied" and can only be dismissed or deferred (plan-flow.md:233).
- **Question tier:** `resolve`, on Revise or Keep.

So in v2-apply, 41 hand-fixed rows were dismissed with the reason "Addressed by a plan edit at triage". That includes 13 `cited` rows from the re-check, which plan-flow.md:331-334 says are never applied automatically.

**How the re-check picks its targets.**

- It runs only when §7 applied at least one row (plan-flow.md:287).
- Its `targets` are the distinct `target` of the `applied` and `already` rows only (plan-flow.md:327).
- The lead's recheck compose keeps only the cells those targets name and waives everything else as `recheck: no applied edit touched it` (plan-review-lead.md:67-70).

So a hand-edited record is never re-checked. Neither is a question-tier record the user revised.

**What a dismissal costs.** A dismissal also:

- counts toward `dismissal_rate`;
- feeds the next run's `CALIBRATION` fence as a false-positive exemplar for the lens. `stats --calibration` on v2-apply returns "Addressed by a plan edit…" 14 times under `plan-executor`.
- suppresses any later finding whose `same_as` names it. A `resolved` row turns a confirmed repeat into a `regression` instead (resolve.rs:61-67).

**What an addressed-by-author disposition needs.**
- **Store:** status `resolved`, not a new status. That keeps regression detection and stays out of `user_dismissal`, so it is out of calibration and the dismissal rate. Add a companion field, say `addressed`: a list of target strings checked by `target::parse`. It is plan-only, uses the STATUS visibility, and is added to `ALL_COMPANIONS` so that `set_status` clears it.
- **Verbs:**
  - a `review triage` mode, e.g. `--addressed-file <reason> --targets <t1,t2>`, that defaults to the finding's own `target`;
  - a `stats` arm in `disposition()`, e.g. `author-addressed`;
  - a read that emits a run's re-check targets as `applied.target` ∪ `addressed`;
  - a capability feature, added to the pre-flight list at plan-flow.md:38.
- **Instructions:**
  - a grammar row in triage-and-close.md;
  - plan-flow §7: the Advisory bullet, and Revise recording its target;
  - §10: run the re-check when there is an applied row or an addressed row, and take targets from the new read;
  - §11's close summary.
- **Done without a store change:** the existing `--resolve-file` already keeps these rows out of dismissals and calibration. `plan_outcomes` counts them as `resolved`, which in flow mode means only this case. But the re-check targets would then have to be assembled by hand from the findings' own `target`.

**What edit fields supplied at the source need.** The finders and executor already supply them, so new finder instructions buy little. The planner has to choose one or more of these:

- **(a) Widen `citation_holds`:**
  - split compound citations on `;` (3 rows);
  - count paths named in `new` (7 rows);
  - count every document of the flow (5 rows).
- **(b) Let `review apply --user-accepted` take an advisory-tier defect or cleanup row that carries `new`.** `expect` is still compare-and-set inside the store lock, so a stale edit is still refused. The row is stamped `applied.by = "user"` and so reaches the re-check targets on its own. This covers all 24 rows, including the 9 code citations, which (a) cannot reach.
- **(c) Change the verifier instruction or the verdict contract.** review-verifier.md:43 already asks for a citation in the target's own file, and 24 of 24 verifiers cited their evidence instead. A separate citation of the edit, or one citation per verdict enforced at `review verdict`, would make the rule hold.

Option (b) plus the addressed disposition together remove the hand-fix-then-dismiss path. This is `needs-plan`: verbs, stats, skills and agents have to move together.

## Findings

- **Library/API or file**: tomlctl/src/review/tier.rs:547-579 `cited`/`citation_holds`; tomlctl/src/review/query.rs:385-388 `parse_citation`
- **Source**: tier.rs:564-569 (path must be in `named_paths`, which is the target file plus paths in its *current* value); query.rs:386 (whitespace → None); `tomlctl review show <ids> --slug v2-apply --for audit` over every advisory-tier row with `new`: 24 of 24 latest verdicts `confirmed`, with citations classified as in the table above
- **Evidence-grade**: high
- **Finding**: Confirmed plan edits are stamped advisory because the cited test excludes the evidence verifiers actually cite. This is what B-cd288c8e describes, and it happens at tier stamping, not at the source. `needs-plan`
- **Details**: The 24 confirmed rows split as follows: 9 cite code outside the target, 7 cite a path only in `new`, 5 cite another flow document, and 3 cite the target's own file in a compound citation. `review verdict` stores citations unvalidated (verdict.rs:106), so a compound citation lands but cites nothing. Tiers are frozen at open (tier.rs:121-123), so a fix applies only to new rows. Files a fix touches: tier.rs, query.rs or verdict.rs, review-verifier.md, review-v2-method references/plan-records.md.
- **Counter**: The finding fails if `cited` was meant to require a citation inside the edited record. plan-records.md:102-107 says it is, but no verifier followed that across 24 rows, so either the rule or the verdict contract has to change.
- **Impact on plan / report**: The fix for B-cd288c8e is a tier or apply change, not a finder-instruction change.

- **Library/API or file**: tomlctl/src/review/apply.rs:324-326 (advisory refused); tomlctl/src/review/triage.rs:119-121, 316-321 (`--convert` is advisory-kind only); tomlctl/src/review/fields.rs:778-786 (`expect`/`new`/`proof` defect or cleanup only)
- **Source**: the file:line references above; plan-flow.md:233 "They are never applied"
- **Evidence-grade**: high
- **Finding**: A defect or cleanup row that is advisory tier and carries a valid `new` has no tool path to apply. Hand-editing is the only route, which forces the dismissals behind B-690fbabb.
- **Details**: Allowing `review apply --user-accepted` on such rows reuses the existing stale check and the `applied` stamp (apply.rs:719-756). `stats` would count them `user-accepted` (stats.rs:1068-1069), and they would join the re-check targets with no new store field. Files: apply.rs, plan-flow.md §7 and §10, tomlctl references/review.md.
- **Counter**: The finding fails if the advisory tier is meant to keep the user away from edits the tool cannot vouch for. But `--user-accepted` already gates user-tier edits, which carry no evidence requirement either (tier.rs:86-91).
- **Impact on plan / report**: This covers most of both items. Take it before the addressed disposition, which then covers only edits that differ from `new` or that touch other records.

- **Library/API or file**: claude/skills/review-v2-engine/references/plan-flow.md:287, 327; claude/agents/plan-review-lead.md:67-70; claude/workflows/plan-v2-review.js:75-77
- **Source**: the file:line references above; `tomlctl review stats --slug v2-apply --plan-outcomes` returns U1 advisory `dismissed: 21` and U2 cited `dismissed: 13`
- **Evidence-grade**: high
- **Finding**: Re-check targets come only from rows that `review apply` applied. Records changed by hand, and question rows the user revised, are never re-checked.
- **Details**: `applied.target` is already stored on each finding (apply.rs:733), so a read verb can emit the targets as `applied.target` ∪ an `addressed` list. The workflow and lead only need a non-empty one-line `targets` list, so they need no change. plan-flow §10's run condition and targets line change, and so does §7 Revise.
- **Counter**: The finding fails if §10 meant to re-check tool edits only. Nothing in plan-flow.md or the lead says so; it gives the reason as "reviews what the edits changed" (plan-flow.md:10).
- **Impact on plan / report**: The addressed disposition must name its targets in structured form, not as free text in a reason.

- **Library/API or file**: tomlctl/src/review/stats.rs:316-326 `user_dismissal`, 1344-1346 (calibration), 1055-1084 `disposition`; tomlctl/src/review/resolve.rs:61-67; fields.rs:179-184 `is_suppressing`
- **Source**: the file:line references above; `tomlctl review stats --slug v2-apply --calibration` lists "Addressed by a plan edit at triage…" as a dismissal exemplar under plan-executor (14), feasibility (4) and others
- **Evidence-grade**: high
- **Finding**: Dismissing hand-fixed rows pollutes the next run's CALIBRATION fence and the dismissal rate. It also suppresses later repeats instead of flagging them as regressions. Recording them as `resolved` avoids all three.
- **Details**: `plan_outcomes` already counts a resolved row with no `applied` table and a non-question tier as `resolved`, separate from `dismissed`. A distinct `author-addressed` label needs one arm in `disposition()` keyed on the new companion field. The companion field must be added to `ALL_COMPANIONS` (fields.rs:213-231) so that `set_status` clears it on reopen. Files: fields.rs, stats.rs, triage.rs, cli/types/review/triage.rs, capabilities.rs, triage-and-close.md, plan-flow.md, references/review.md.
- **Counter**: The finding fails if CALIBRATION filtered dismissal reasons by content. It does not: `user_dismissal` excludes only the `v1:` sync prefix (stats.rs:325).
- **Impact on plan / report**: The new disposition goes on `resolved` plus a companion field, not on a new status. That avoids touching every status matrix (export, sync, digest, approve).

Coverage: Read in full: plan-flow.md, review-finder.md, plan-executor.md, plan-records.md, plus the relevant sections of review-v2-method SKILL.md, plan-review-lead.md, review-verifier.md, triage-and-close.md, tier.rs, add.rs, apply.rs, triage.rs, stats.rs, fields.rs, query.rs, resolve.rs and review-v2.js/plan-v2-review.js. Checks run: `review list`, `review show --for audit`, `stats --plan-outcomes` and `stats --calibration` on the v2-apply store. Ruled out:
- The finder or executor omitting `target`: all 81 rows carry it.
- The lead dropping fields: it writes none.
- Projections hiding the fields from the verifier or the triage read: the verifier and implementer projections show PLAN.

Sibling note: B-590ba0b1 and B-fddac023 cover the hand-assembled lists. The proposed re-check-targets read is the same kind of fix.
