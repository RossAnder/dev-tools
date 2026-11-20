Verdict: `plan emit commit-paths` must print, from one fresh `git status -z` read per commit, the git-spelled changed paths of the milestone's rows' `files` with no flow-directory skip (`plan drift --uncommitted` drops `.claude/flows/<slug>/` and `docs/flows/<slug>/` at tomlctl/src/plan/drift.rs:197-201, which is how B-cbda2a4f lost a row file), and separately the flow-store paths plus ignored ones; it registers in five code places and four doc places, like `lane-check` and `schedule` before it.

## Answer

**How `plan drift --uncommitted` works today.** `uncommitted_files` (drift.rs:174) runs `git status --porcelain=v1 -z --no-renames --untracked-files=all` from `repo_or_cwd_root()`, splits on NUL, keeps `entry[3..]`, and drops every path that starts with `.claude/flows/<slug>/` or `docs/flows/<slug>/` (drift.rs:197-211). Ignored files are never listed because there is no `--ignored`. Paths come back verbatim and unquoted, relative to the repo root, as a `BTreeSet`. Rows' `files` are matched against them only inside `affected_tasks`, through `names()`, which folds `\` to `/` and drops a leading `./` (`normal_path`, tasks/schema.rs:714). The engine's own intersection in drain.md:440-446 is left to the orchestrator: "stage only the candidates among them, compared byte for byte". `lane-check` reuses the same `uncommitted_files` (lane.rs:24, :435), so it inherits the same skip.

**What `commit-paths` must print.** It needs two outputs, and they cannot come from one call made before the first commit. Between the two commits, drain.md:454-467 stamps the rows (`tasks.toml`), runs `backlog reconcile --apply` (`.claude/backlog.toml`), and for an apply milestone runs `review reconcile`, which writes review stores that can sit outside the flow directory (apply-rows.md:261-274). report.md also records the report into `plan.toml` before the second commit. So the leaf takes a selector, for example `--for work|flow`, or it is two leaves. Each run does its own status read.

- **work** (the milestone commit). Each path in git's spelling where the `files` of a row with `checkpoint = <id>` (after the drain has written granted request paths into `files`, requests.md:114) match an uncommitted entry under `normal_path`, with no flow-directory skip. That covers `docs/flows/<slug>/notes/<x>.md` deliverables (B-cbda2a4f) and settle proposals at `docs/flows/<slug>/notes/settle/<F>.md` (apply-rows.md:185, :223). Deletions are included, since `git add --` stages them. The output should also name the row files that matched nothing, or that the ignore rules hide, so the orchestrator can tell "clean" from "ignored" (`git status` without `--ignored` cannot show an ignored row file).
- **flow** (the flow-store commit). Each changed path under `.claude/flows/<slug>/` (which holds the stores, the sidecars and the `gates/` logs, drain.md:366) and under `docs/flows/<slug>/notes/run/` (the run notes and the visual copies, visual.md:143). Then `.claude/backlog.toml` and `.claude/inputs.toml` with their `.sha256` sidecars (drain.md:472, questions.md:97), and each `[[finding_links]]` review store outside the flow directory with its sidecar (apply-rows.md:274). Paths already printed under **work** are left out. Ignored paths go in their own list, each with its `git check-ignore -v` rule, so the `!!` handling in drain.md:477 needs no second command.
- Each list is a JSON array of strings, so `--get <key> --lines` gives one unquoted path per line to paste into `git add --` and `git commit --`. Paths are relative to the repo root, as porcelain v1 prints them whatever the cwd.

**Registration**, from the footprint of `373da972` (lane-check) and `566c2dea` (schedule):
1. clap: a `CommitPaths(PlanEmitCommitPathsArgs)` variant in `PlanEmitOp` (tomlctl/src/cli/types/plan.rs:88-104), plus an args struct with `target: PlanTarget`, `--milestone`, the selector and `#[command(flatten)] integrity: ReadIntegrityArgs`, as `PlanEmitLaneCheckArgs` has at plan.rs:768.
2. Dispatch: an arm in `emit::dispatch` (tomlctl/src/plan/emit.rs:45-53). `plan/dispatch.rs:55` already routes `PlanOp::Emit` there. If the leaf gets its own module, it also needs a `mod` line in `plan/mod.rs`. Load through `load_milestone_store` (emit.rs:57) so an unknown milestone is refused like the other leaves.
3. Capabilities: a feature string in `FEATURES` (tomlctl/src/cli/types/mod.rs:206-214 block, e.g. `plan_emit_commit_paths`), the same string in `capabilities_features_contains_every_plan_feature` (tomlctl/tests/capabilities.rs:1670, list near :1861), and the carrier pre-flight's required-feature list (claude/commands/implement-v2.md:29). The subcommand tree is derived from clap (src/capabilities.rs:165), so it needs no hand entry.
4. output_options: one `read(&["plan","emit","commit-paths","--slug",PLAN_SLUG,"--milestone","A", ...], One, "<key>").git()` case beside the other emit leaves (tomlctl/tests/output_options.rs:1328-1387). The file's header says the leaf set and the case set must agree (output_options.rs:1-3, check at :2590-2594). `.git()` is needed because the leaf runs `git status`.
5. Tests: integration cases in `tomlctl/tests/plan_emit.rs` (gate-list's are at :659-880). These need no new `mod` line, since the file is already a module of `it.rs`.
6. Skill and docs: a `## plan emit commit-paths` section with a flag table and output keys in `claude/skills/tomlctl/references/plan-engine.md`, plus its synopsis line (:92-98) and contents entry (:14-19). The lint in `cli::dispatch::tests` checks these flag tables and fenced commands against clap (lint.rs:2010-2068). Also update the `plan emit` routing row and the reference blurb in `claude/skills/tomlctl/SKILL.md:63,:88`, the stale three-leaf pointer in `claude/skills/tomlctl/references/plan.md:534-538`, and `tomlctl/README.md:93,:606`. Then rewrite drain.md step 10 (:440-447, :469-477), its "Stamping on re-entry" (:483), run-milestone.md:24-28 if it adopts the leaf, and the apply-rows.md:274 staging paragraph.

## Findings

- **Library/API or file**: tomlctl/src/plan/drift.rs:197 `uncommitted_files`
- **Source**: drift.rs:197-211; drift test `status_entries_drop_their_codes_and_the_flow_directories` (drift.rs:334); drain.md:440-446
- **Evidence-grade**: high
- **Finding**: The milestone commit takes its candidates from a list that skips `docs/flows/<slug>/` by construction, so a row file there is never staged. The new leaf must match rows' files against an unskipped status read.
- **Details**: `skip = [".claude/flows/<slug>/", "docs/flows/<slug>/"]` is applied before any row matching, and the unit test asserts that `docs/flows/s/notes/run/1.md` is dropped. drain.md step 10 stages "the candidates among" `--get uncommitted_files`, so a row file under that prefix cannot appear. The flow-store commit then stages only `docs/flows/<slug>/notes/run/`, so a deliverable such as `notes/preview-cpu.md` falls out of both commits.
- **Counter**: A row's `files` never naming a path under `docs/flows/<slug>/` would make this moot, but apply-rows.md:185 says a settle row's only file is `docs/flows/<slug>/notes/settle/<F>.md`, and B-cbda2a4f observed exactly such a row.
- **Instances**: settle proposal files (apply-rows.md:185, :223, "The milestone commit then holds the proposal files") hit the same gap. No `notes/settle/` file was ever committed (`git log --all -- 'docs/flows/*/notes/settle/*'` is empty), so that path is unexercised. Sweep: `uncommitted_files` callers (drift.rs:40, lane.rs:435), plus Grep `porcelain` over `claude/skills/implement-v2-engine` and `apply-v2-engine`. Complete.
- **Impact on plan / report**: the **work** list must use a status read with no flow-dir skip. `parse_status` (drift.rs:205) already takes `skip` as a parameter, so the change is a second caller or a `skip` argument on `uncommitted_files`, and `plan drift`'s own drift semantics stay unchanged.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:454-479 (step 10 sequencing)
- **Source**: drain.md:454-469; apply-rows.md:261-274; report.md:54
- **Evidence-grade**: high
- **Finding**: The flow-store path set can only be computed after the stamp, the backlog reconcile, the review reconcile and the report record, so `commit-paths` needs a per-commit selector or two calls, not one up-front emission.
- **Details**: `tasks update --commit`, `backlog reconcile --apply`, `review reconcile` (which writes stores outside `.claude/flows/<slug>/`) and the report write all run between the two commits. A list emitted before commit 1 would miss a backlog or external review store that only becomes dirty afterwards.
- **Counter**: If every one of those writes always landed under `.claude/flows/<slug>/`, a directory pathspec would stay valid, but apply-rows.md:274 names review stores outside it, and the backlog is outside it.
- **Impact on plan / report**: Design the leaf as `--for work|flow` (or two leaves), with drain.md calling the flow form immediately before the second commit.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:472-477 (flow-store pathspec)
- **Source**: drain.md:472; questions.md:97; apply-rows.md:274; visual.md:143; drain.md:366
- **Evidence-grade**: high
- **Finding**: The flow-store set is spread across four documents today. The leaf should fold them into one list: the flow store dir, `notes/run/`, the backlog pair, the inputs pair and the external linked review-store pairs, with ignored paths and their rules listed apart.
- **Details**: drain.md's `git status --porcelain --ignored --` pathspec lists only the flow dir, `notes/run/` and the backlog pair. inputs is added by questions.md, and external review stores by apply-rows.md. Non-`-z` porcelain C-quotes unusual paths and prints an untracked directory as one entry, which `-z --untracked-files=all` avoids. lane.rs:41-43 already holds `BACKLOG_STORE`, `INPUTS_STORE` and `SIDECAR_SUFFIX` for reuse.
- **Counter**: The engine choosing to keep inputs.toml out of the flow-store commit unless the question loop wrote it (the current conditional wording, drain.md:479) would make the inputs pair conditional. That is a planner decision: the leaf cannot tell who wrote it.
- **Impact on plan / report**: Output keys of roughly `{work: [...], flow: [...], ignored: [{path, rule}], unmatched: [...]}` (names are the planner's call). Ask the user whether inputs.toml is staged whenever dirty.

Out of lens, one line: `plan emit lane-check` also inherits the skip, so an edit outside any lane under `docs/flows/<slug>/` (outside `notes/run/`) is never reported as out of lane (lane.rs:435).

Coverage: read in full tomlctl/src/plan/emit.rs and drift.rs. Read in part lane.rs:340-460, cli/types/plan.rs:75-105, cli/types/mod.rs:190-229, output_options.rs (header, :484-537, :1320-1400), drain.md:430-499, run-milestone.md:15-44, apply-rows.md:245-275, questions.md:88-102, plan-engine.md:1-100 and :200-244, plan.md:530-538, and the brief's background. Checks run: `git show --stat` and the registration diffs of 373da972 and 566c2dea, `git check-ignore -v` on the store paths (only `.review-v2-runs/` is ignored here), and `git log --all` for settle files. Ruled out: a missing `mod` line in tests/it.rs (plan_emit.rs already exists); hand-registering the subcommand in capabilities (the tree is derived from clap); rows naming directories (no evidence any row does, so not pursued).

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = ["docs/flows/implement-v2-run-hardening/brief.md"]

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 174
claim = "uncommitted_files runs porcelain v1 -z --no-renames --untracked-files=all and skips both flow directories"

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 231
claim = "rows' files match uncommitted paths through normal_path in names()"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 714
claim = "normal_path folds backslashes and drops a leading ./"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 41
claim = "BACKLOG_STORE, INPUTS_STORE and SIDECAR_SUFFIX constants; lane-check reuses uncommitted_files"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 45
claim = "emit::dispatch arms and load_milestone_store refusal of an undeclared milestone"

[[source]]
path = "tomlctl/src/plan/dispatch.rs"
line = 55
claim = "PlanOp::Emit routes to emit::dispatch"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 88
claim = "PlanEmitOp enum where a new leaf variant registers"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 211
claim = "FEATURES list holding plan_emit, plan_emit_lane_check, milestone_schedule"

[[source]]
path = "tomlctl/tests/capabilities.rs"
line = 1670
claim = "capabilities_features_contains_every_plan_feature lists each plan feature"

[[source]]
path = "tomlctl/tests/output_options.rs"
line = 1328
claim = "emit leaf cases; header requires the leaf set and case set to agree"

[[source]]
path = "tomlctl/src/capabilities.rs"
line = 165
claim = "capabilities subcommand tree is walked from clap"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 440
claim = "step 10 milestone commit stages candidates from plan drift --uncommitted; flow-store commit pathspec at 472"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 185
claim = "settle row files are docs/flows/<slug>/notes/settle/<F>.md and go in the milestone commit; external review stores join the flow-store commit (274)"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 97
claim = "inputs.toml and its sidecar are staged in the flow-store commit"

[[source]]
path = "claude/skills/tomlctl/references/plan-engine.md"
line = 86
claim = "plan emit section, synopsis and per-leaf flag tables where a new leaf is documented"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "carrier pre-flight required feature list"

[[source]]
path = "docs/flows/implement-v2-run-hardening/brief.md"
line = 43
claim = "B-cbda2a4f: a docs/flows row file omitted by the drift list"
```
