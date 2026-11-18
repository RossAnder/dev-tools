# review-v2-cells-and-handoff

## Intent

- B-abc59fb3: review plan --propose bundles correctness, security and test-gaps over mixed rule-sliced slices and spends the ceiling on repeat passes before candidate lenses
- B-4e15b9fa: review plan --propose spends a large diff's ceiling on 40-file ripple sweeper splits while its judgement cells get 100 to 180 file slices
- B-f00449a1: review plan appends cells past the depth ceiling without a refusal, and triage has no disposition for an escalation the user wants fixed
- B-62be032f: review schedule mints independent fix tasks for findings whose fixes interact, so a doc fix can describe behaviour another task changes
- B-8bd7a593: Converting an advisory must reuse its cell's lens, which triage-and-close.md never says, and a multi-file type cleanup fits only --convert defect, which verifiers refute
- B-f438f85d: review stats --by model sums audited and overturned across the three audit classes, so the per-class 30-row floor and bands in measurement.md cannot be read from stats

Make a v2 review's coverage proposal something the orchestrator can launch without rewriting, make every triage pick the user wants fixed actually reach `/apply-v2` and land consistently, and give the cell budget evidence to be tuned from.

- **Proposals.** `review plan --propose` slices judgement lenses by subsystem, puts security where the trust-boundary signal points, keeps test-gaps on changed code and its tests, stops sweeper cells eating the ceiling on a large diff, and never cuts a lens the signals matched while it keeps a repeat pass.
- **Ceiling.** `review plan` refuses cells past the run's ceiling when they are appended, unless the user directs an override, which is recorded on the run.
- **Triage to apply.** An escalation the user wants fixed gets a triage disposition that reaches `/apply-v2` through its settle route. A multi-file cleanup the user converts has a shape a verifier will not refute for being a cleanup. Fixes whose behaviour interacts are ordered so a doc fix never describes behaviour another fix task changes.
- **Measurement.** `review stats` splits the audit classes, and reports how many findings repeat-pass and context-free cells add beyond the first pass, so the cell budget is tuned from evidence across runs.

## Non-goals

- Retuning the depth ceilings, the per-cell lens cap or the follow-up allowance: this plan measures; the numbers change later, from the evidence it produces.
- Removing repeat passes or context-free cells.
- Rewriting lens question text, verifier or finder method, or agent models.
- The v1 commands (`/review`, `/review-apply`) and their ledgers.

## Scope budget

The `tomlctl review` verbs (`plan`, `triage`, `add`, `verdict`, `schedule`, `stats`, `report`, and a new `escalation` group) and their tests; the `review-v2-engine`, `review-v2-method` and `apply-v2-engine` skill references that describe them; the review agents and the `/review-v2` carrier where they name these verbs; the tomlctl skill's `review*.md` flag tables; lens frontmatter only where a slice rule or a lens's file kinds change. One milestone, built as parallel tracks.

## Success criteria

- Replaying the U3 run (mid-run-requests, 46 files) and the U5 run (reportdesignkit, 422 files) through `review plan --propose` from fixtures gives: judgement cells sliced by subsystem, with no correctness or test-gaps cell holding lockfiles or agent prose beside code; security sliced to the roots and evidence files of every signal it matched, lockfiles included when dependency-change names them; test-gaps over changed code and its tests; mechanical cells taking at most about a quarter of the ceiling, merged past the 40-file split before any judgement lens is cut; and no matched lens listed as cut for `ceiling` while a pass-2 repeat is kept.
- `review plan --ndjson` refuses a wave that would take the run past its ceiling, naming the ceiling and the count; with the user's override it writes the wave and the run carries the staged reason, which the report shows, and `review stats` counts the run's overrides and their overflow.
- At triage, the user can say "fix this escalation"; it becomes a finding blocked on evidence that one verifier checks: confirmed blocked, `/apply-v2` schedules it into its settle milestone; unblocked with a fix sketch, it goes straight to a fix task; either way the fix lands through the normal gate and commit.
- A converted multi-file cleanup (fix prose plus the files it touches) is verified as a cleanup and scheduled as one fix task over those files.
- When a doc fix's finding names the file another fix changes (its full path, or a file name no other chosen fix shares), `review schedule` makes the doc task need the code task, and the doc task's spec says to check its edit against the code as it stands after that task.
- `review stats` reports audited and overturned rows per class (CONFIRMED defect, REFUTED defect, checked cleanup) per model and per lens, and per run and pooled per depth: how many verified findings repeat-pass and context-free cells added that are not `same_as` a first-pass finding, and how many of those were then fixed by apply.

## Background

**B-abc59fb3.** Run U3 (46 files, 3907 lines, standard): the 14-cell proposal put correctness+security+test-gaps on slices mixing Cargo.lock, agent prose and test files, spread security over prose despite the trust-boundary signal naming one Rust file, and gave 7 cells to pass-2 repeats while cutting hygiene and doc-drift for ceiling. The orchestrator rewrote the map by subsystem. Outcome evidence: 13 of 31 findings were duplicates, and the two repeat cells plus the context-free cells found almost nothing the first-pass cells had not. Slice security by the trust-boundary roots, keep test-gaps on test files, and rank candidate lenses ahead of repeat passes.

Evidence: `notes/backlog-B-abc59fb3.md`

**B-4e15b9fa.** reportdesignkit run U5 (422 files, 30619 changed lines, standard, ceiling 16): the proposal gave 11 cells to ripple sweepers split at 40 files, 4 correctness+domain-model+interaction cells over 75 to 180 files each, and one pass-2 repeat over 100 files, cutting intent-fidelity, security, contrarian, doc-drift and 7 more for ceiling. Ripple's work grows with the names a diff removes or renames, not with slice size, so the 40-file split is the wrong unit once the diff is large. The orchestrator rewrote the map by subsystem: 6 judgement cells, 3 context-free cells, 3 ripple+hygiene sweepers over 71, 133 and 218 files (all sealed thin but found the escalations), plus intent-fidelity, contrarian, doc-drift and a 9-file hotspot repeat. Consider capping sweeper cells per lens (merge past the 40-file split before cutting judgement lenses) and splitting judgement slices by subsystem rather than by rule.

Evidence: `notes/backlog-B-4e15b9fa.md`

**B-f00449a1.** reportdesignkit run U5: wave 1 filled the 16-cell standard ceiling; at triage the user said four sweeper escalations should be fixed in the apply. compose.md says added cells must stay within the ceiling and the escalation then goes to the close summary or the backlog, but the user wanted a fix, not a capture, and the triage grammar has no such disposition. The only route into /apply-v2 was a 17th cell (C62, a probe), which review plan --ndjson accepted silently: the ceiling is computed only in --propose (cells.rs ceiling/budget), never on append. Either enforce it on append with an explicit user-directed override recorded on the run, or add a triage disposition that turns an escalation into a follow-up cell outside the ceiling.

Evidence: `notes/backlog-B-f00449a1.md`

**B-62be032f.** mid-run-requests apply-1: F48 (code: plan request add refuses on a malformed generated map) and F62 (doc: generated.md omits that refusal) were scheduled as separate tasks. The F48 fix switched to a warning, while F62's task wrote the refusal into the doc, so the two contradicted and an extra task (33) and launch were needed. Triage had judged the pair distinct. When one finding's fix changes the behaviour another finding's doc fix describes, schedule should order them (needs) or merge them, or triage should link them.

Evidence: `notes/backlog-B-62be032f.md`

**B-8bd7a593.** reportdesignkit run U5: converting contrarian advisory F164 (switchDocuments takes kind beside a SwitchTarget carrying its own kind) with lens type-design was refused with '`lens` is "type-design"; cell C59 covers contrarian' (add.rs). The lens rule is now documented in triage-and-close.md's Convert section (2026-11-18). Still open: the fix is a signature change across six files, which a cleanup row cannot express (one exact old text), so it went in as --convert defect; the verifier then refuted it (no reachable wrong output, 'a cleanup, not a defect'), and the user's convert pick landed nowhere. Give convert a shape for a multi-file cleanup (fix prose plus fix_files), or let the verifier downgrade a defect to cleanup rather than refute it.

Evidence: `notes/backlog-B-8bd7a593.md`

**B-f438f85d.** measurement.md works around it by reading a model under 30 audited rows as insufficient on every band and counting per class with review list --kind ... --for audit per store. A per-class split (confirmed defect, refuted defect, checked cleanup) in stats would let the five-run review read the bands directly.

Evidence: `notes/backlog-B-f438f85d.md`

Interview answers (2026-11-18): an escalation to fix goes to apply's settle route as a blocked finding; interacting fixes are ordered by `review schedule`, not merged or left to triage; test-gaps covers changed code plus its tests; done is judged by replaying U3 and U5; a ceiling override is user-directed only, with the reason stored on the run; repeat and context-free yield counts verified findings not `same_as` a first-pass finding, with the applied share beside it; sweeper cells are capped at about a quarter of the ceiling and merge past the 40-file split before a judgement lens is cut.

## Unknowns

- replay-fixtures: Where do the U3 (mid-run-requests) and U5 (reportdesignkit) review stores live, what did `review plan --propose` emit for each against what the orchestrator actually wrote as the cell map, and what inputs (signals with roots and evidence, scope files, tracked-file list, depth, changed_lines, catalogue) would a test fixture need to replay `--propose` without the original repository checked out?
- propose-shaping: How does `review plan --propose` in tomlctl/src/review/cells.rs pick, slice, bundle, floor-fit and repeat cells end to end, and where would each change land: subsystem slices that keep lockfiles and prose apart from code, security sliced to the trust-boundary signal's roots, test-gaps over changed code plus its tests, a cap of about a quarter of the ceiling on mechanical cells, and repeat passes admitted only after every signal-matched lens? Which tests pin today's behaviour?
- ceiling-append: How would `review plan --ndjson` enforce the run's ceiling on append, given the carrier writes its own context-free correctness cells after the proposal, follow-up waves count against the ceiling, and plan-executor cells sit outside it; and where on the run record would a user-directed override reason live so the report and `review stats` can show it?
- triage-to-settle: What does `/apply-v2`'s settle route need from a finding (kind, lens and cell attribution that `review add` checks, `blocked_on`, verification route), how would a triage disposition persist an escalation (today only in the Workflow index) as such a finding, and what shape would let a converted multi-file cleanup (fix prose plus `fix_files`, no single exact `old`) be verified as a cleanup rather than refuted as a defect and then scheduled?
- schedule-interaction: How does `review schedule` turn findings into fix tasks and `needs` edges, and which finding fields (file, symbol, edit, fix, fix_files, lens, text) could detect that a doc or prose fix names a file or symbol another scheduled fix changes, without flagging unrelated pairs?
- stats-yield: How does `review stats` compute audited and overturned rows by model and by lens, where do a cell's pass, its agent (review-finder-cf) and a finding's `same_as` and apply outcome live, and what would it take to split the three audit classes and report per-run and pooled first-pass versus repeat and context-free verified-unique yield with the applied share?
