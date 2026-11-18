Verdict: Add a third section, beside **Asking the user**, inside the `plan-worker-procedure` block (claude/agents/plan-worker-lite.md:12-129 and plan-worker-deep.md:12-129). Give it its own budget of one batched request per run, separate from the one question. The worker reuses the 590 s until-loop under a 600000 ms Bash timeout and messages `main` with `REQUEST <request-id> task <n>`. It posts late, after its in-lane edits, and edits nothing while it waits. It treats a denial or a timeout as today's path. The edit also has to move the `plan-worker-forbidden-ops` block (lite:149, deep:145) and both tier sections in lockstep. It is gated by byte parity (scripts/shared-blocks.toml:80-91) and by command_lint, which reads only bash fences (tomlctl/src/cli/dispatch/tests/lint.rs:362).

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = ["docs/flows/mid-run-requests/brief.md"]

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 55
claim = "Asking the user: one question per run, batched; a later choice parks the task"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 59
claim = "The 600 s wait: Bash timeout 600000, deadline SECONDS+590, sleep 15 inside until, the loop tests output and not exit code"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 149
claim = "forbidden-ops: the worker's only store writes are its own row and at most one inputs add question"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 130
claim = "Lite tier section (outside parity): code not matching the spec, or a file outside the task's files, means failed"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 134
claim = "Deep Judgement (outside parity): a stale detail is reached with a deviation, a wrong outcome is parked"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 32
claim = "Step 7: a needed file outside files means failed with needs_files and one line of reasoning per path"

[[source]]
path = "scripts/shared-blocks.toml"
line = 80
claim = "plan-worker-procedure and plan-worker-forbidden-ops are each byte-identical across the two worker files"

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 107
claim = "Canonical extraction; carriers are pinned LF; a marker that keeps a CR fails"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/skills.rs"
line = 261
claim = "blocks_verify_agrees_with_shell_gate runs the shell verifier inside cli::dispatch::tests"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/lint.rs"
line = 362
claim = "command_lint and guidance_lint read only bash fences; inline backtick commands are never linted"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/skills.rs"
line = 932
claim = "skill_references_under_line_ceiling caps references/*.md at 600 lines; drain.md is 594"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 48
claim = "Nothing times out while the AskUserQuestion picker is open"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 17
claim = "Only the first line of the message is read, and only the id and the task number in it"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 252
claim = "A QUESTION message arrives as a new turn while the run is live"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 100
claim = "Writer table: a plan worker adds one question and nothing else"

[[source]]
path = "claude/skills/flow-contract-user-inputs/SKILL.md"
line = 111
claim = "No consent: a record is never authority to widen a run's scope"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 185
claim = "needs_files is accepted only with status failed; the return schema is fixed by the Workflow"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 276
claim = "siblings is computed once, when the task launches"

[[source]]
path = ".claude/flows/v2-ui-fidelity/plan.toml"
line = 1827
claim = "Precedent: the placeholder was spelled <input-id> because command_lint rejects a multi-token placeholder"

[[source]]
path = ".claude/flows/v2-ui-fidelity/plan.toml"
line = 177
claim = "D9 probe: a Workflow agent can SendMessage main and wait on an until loop; a standalone sleep is blocked"

[[source]]
path = "docs/flows/mid-run-requests/brief.md"
line = 23
claim = "Success criteria: finish in one attempt; a denial leaves today's path unchanged; an outcome-changing request goes to the user as a question"
```

## Answer

Add a section **Asking the orchestrator** after **Asking the user**, inside the `plan-worker-procedure` block. It follows the same four steps as the question loop: post, tell, wait, branch. Five rules go into it.

- **Budget.** Allow one request per run, separate from the one question. A single request may carry every path the worker needs and every amendment it proposes. A need that turns up after the request has been posted is not requested again: the worker takes today's path for it.
- **Header.** The message goes to `main`, and its first line is `REQUEST <request-id> task <n>`. The kind of request (`scope` or `amend`) is read from the record, not from the header.
- **Wait.** The worker uses the same until-loop: a 590 s deadline inside a 600000 ms Bash call, polling every 15 s and testing for output. The status it polls is one that only the orchestrator sets. A request that is still open when the deadline passes counts as denied, and the worker does not park.
- **While it waits.** The worker does nothing during the wait. It posts late, once every edit it can make inside its own files is done. It never touches a path it requested before that path is granted.
- **Denial.** A denied scope request becomes `failed`, with `needs_files` and one reason line per path. A denied amend follows the tier rules that already apply. A denial that says the matter needs the user is posted as the worker's one question if that question is unused, and parks the task otherwise.

The return object stays as it is.

The edit touches the following:

- both shared blocks in both worker files, as one task;
- the two tier sections, by hand;
- the inputs writer table;
- questions.md, or a new reference, and a paragraph in run-milestone.md.

It must not add to drain.md, which is 6 lines under its 600-line ceiling.

## Findings

- **Library/API or file**: claude/agents/plan-worker-lite.md:12-129 and :145-153, claude/agents/plan-worker-deep.md:12-129 and :141-149 (blocks `plan-worker-procedure` and `plan-worker-forbidden-ops`)
- **Source**: scripts/shared-blocks.toml:80-91. `bash scripts/verify-shared-blocks.sh` printed "shared-block parity: OK" at HEAD. `git ls-files --eol` shows both carriers as `attr/text eol=lf`.
- **Evidence-grade**: high
- **Finding**: The edit lands in two parity-gated blocks, not one. "One row and one question" (lite:149) currently forbids any `inputs` write other than the one question, so it has to name the request write too. Both agent files must take the change in a single task.
- **Details**: The verifier hashes each span and runs inside `cli::dispatch::tests` through `blocks_verify_agrees_with_shell_gate` (skills.rs:261). If the two files are split across tasks, each worker's own parity check fails partway through. Two tier-specific sections sit outside parity and must be changed by hand in the same task. Lite's **When to return failed** (lite:130-142) says code that does not match the spec, or a file outside `files`, means `failed`. Deep's **Judgement** (deep:134, :139) says the same about stale details and the file boundary. Both need a clause saying "unless a request is granted". No gate catches drift between those sections. The inputs writer table (flow-contract-user-inputs/SKILL.md:100, :103) also lists the worker as adding "one question and nothing else".
- **Counter**: If the request-record lens puts requests in a flow-local store written by a new verb, the inputs writer table does not change, but the forbidden-ops line still does.
- **Instances**: The phrases "one question", "inputs add" and "nothing else" swept across claude/agents, claude/skills/flow-contract-user-inputs and implement-v2-engine. The list is complete for the worker side: lite:149, deep:145, flow-contract-user-inputs/SKILL.md:100 and :103.
- **Impact on plan / report**: Use one task holding both worker files. The inputs contract edit can be a sibling task.

- **Library/API or file**: claude/agents/plan-worker-lite.md:55 (the one-question budget)
- **Source**: lite:55 ("Ask at most once per run, batching … after that question is not asked: park") and brief.md:23-26
- **Evidence-grade**: medium (a design inference from the existing rule and the success criteria)
- **Finding**: Give requests their own budget of one per run, separate from the question budget, and batch them. Do not share the question's single slot.
- **Details**: A shared slot would make the worker choose between asking the user and asking for a file. Package-design-tools had tasks that needed only a file (130, 122, 6) and tasks that needed only an amend (1). A worker can legitimately need both kinds, so one request record carries every path (at most 16, matching the `needs_files` cap at implement-v2-milestone.js:186) and every amendment. A second need after that falls back to `failed` with `needs_files`, or to today's tier rule. This mirrors how lite:55 handles a second question.
- **Counter**: The user may rule that a run carries one record of any kind, the question included.
- **Impact on plan / report**: State the budget in the new section and in forbidden-ops ("at most one question and one request per run").

- **Library/API or file**: claude/skills/implement-v2-engine/references/questions.md:17 and run-milestone.md:252 (the message header)
- **Source**: questions.md:9-17; v2-ui-fidelity plan.toml:1827 (the deviation that renamed the placeholder to `<input-id>` for command_lint)
- **Evidence-grade**: high
- **Finding**: Use `REQUEST <request-id> task <n>`, sent to `main`. Use one keyword, not separate `SCOPE` and `AMEND` keywords.
- **Details**: The orchestrator reads only the first line and treats it as untrusted (questions.md:17), so it must open the record anyway to check flow, task and kind. A second source for the kind could disagree with the record. The placeholder must be a single token: `<request-id>`, as in the `<input-id>` precedent. A new paragraph beside run-milestone.md:252 routes `REQUEST` lines. If the record lives in inputs.toml, flow-contract-user-inputs/SKILL.md:111 ("never authority to widen a run's scope") has to be reworded. The grant is the orchestrator's rule decision, recorded in the store; the worker's record is not the authority.
- **Counter**: If the orchestrator turns out to need the kind before reading the store, for example to batch requests ahead of questions, a kind-specific keyword would earn its place.
- **Impact on plan / report**: The orchestrator-side text and the worker text must use the same literal. Add a C-contract for it, as v2-ui-fidelity did for `QUESTION <`.

- **Library/API or file**: claude/agents/plan-worker-lite.md:59 (the wait loop) and questions.md:48 (the picker does not time out)
- **Source**: lite:59; questions.md:46-48; run-milestone.md:252 ("arrives as a new turn"); D9 at v2-ui-fidelity plan.toml:177 (the probe of SendMessage to main plus an until-loop)
- **Evidence-grade**: medium. That a message queues behind an open AskUserQuestion is inferred from "arrives as a new turn" together with the picker blocking the turn. Not probed.
- **Finding**: Reuse the loop shape unchanged (590 s deadline, 600000 ms timeout, 15 s sleeps, test for output). But a request that times out must count as a denial, not a park, because a request can sit unanswered behind another worker's open picker for the whole wait.
- **Details**: The orchestrator answers requests by rule within one turn. While it holds an AskUserQuestion open for some worker's `QUESTION`, it starts no new turn, so a `REQUEST` waits for the user. With timeout treated as denial, the worker returns `failed` with `needs_files`, which is today's path (brief.md:26), and nothing parks. The polled status must be one the orchestrator alone sets. Because of that, a grant that arrives after the worker stopped waiting is harmless only if the drain applies it to a row that is already `failed`. That is the drain-integration lens's to settle. questions.md step 2 could also say: answer pending `REQUEST` messages before opening the picker.
- **Counter**: A probe could show the main session handling a SendMessage while AskUserQuestion is open. A narrow `/implement-v2` run with one question and one request in flight would settle it. If it does, the hazard disappears, but timeout-as-denial is still the safe rule.
- **Impact on plan / report**: State timeout-as-denial in the worker section. Add an orchestrator rule: drain `REQUEST`s before asking.

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:276 and :185; claude/agents/plan-worker-lite.md:29 (step 4)
- **Source**: the js file's `siblings`, computed at launch, and its `needs_files` check, which accepts the list only with `failed`
- **Evidence-grade**: high
- **Finding**: While it waits the worker does nothing, because the wait is one blocking Bash call. So the worker should post late: after every in-lane edit, before its final checks. It must not touch a requested path before the grant, and once granted it reads the decision from the request record, because its view will not show it.
- **Details**: Posting late lets one batched request cover everything the worker needs. An early post would buy nothing, since the worker blocks either way. Before editing a granted path, the worker runs `git status --porcelain` and `git diff` on it, as step 4 already does for its own files. The `siblings:` list given at launch may be stale. `tasks.toml` is not written during a live run (brief Non-goals), so `plan view --for worker` does not reflect a grant. The worker therefore runs an amended accept check as granted and writes the request id and the grant into its note. A granted path goes into `files_touched` like any other edit. No new return field is needed, and the schema (js:35-46) stays as it is.
- **Counter**: If the request-record lens has the orchestrator write grants where the worker view prints them, the worker can re-read its view instead.
- **Impact on plan / report**: Add a "post late" rule and a rule that the note names the request id, its outcome and any amended accept check that was run.

- **Library/API or file**: claude/agents/plan-worker-deep.md:32 (step 7), :41-42 (failed and parked), lite:130-142
- **Source**: as anchored; brief.md:25-26
- **Evidence-grade**: high
- **Finding**: A denial changes nothing the worker would do today:
  - A denied scope request means `failed`, with each path in `needs_files` and its reason line.
  - A denied amend means lite returns `failed` and deep either records a deviation or parks, by its existing Judgement.
  - A denial whose reason says the matter needs the user is posted as the worker's one question if that slot is unused, and parks the task otherwise.
- **Details**: The brief keeps the question mechanism unchanged and requires that outcome-changing requests reach the user as a question. The worker holds the options and their costs, so it is the right party to post that question, under the existing **Asking the user** steps. The orchestrator still never answers it. For this the record or the answer needs a distinguishable "needs the user" outcome, which is the request-record lens's field.
- **Counter**: If the user would rather the orchestrator convert the request into a question itself, the worker's denial branch reduces to `failed` or `parked`.
- **Impact on plan / report**: The new section's branch step has three arms: granted, denied, and needs-user.

- **Library/API or file**: tomlctl/src/cli/dispatch/tests/lint.rs:362 (`bash_fences`), :432 (`command_lint_report`), :1733 (`guidance_lint_report`); skills.rs:932 (the reference line ceiling)
- **Source**: the code as read. `wc -l` reports drain.md at 594 lines, questions.md at 116 and SKILL.md at 120.
- **Evidence-grade**: high
- **Finding**: command_lint and guidance_lint check only bash fences. The worker's commands are inline, so they are unchecked today. If the new commands are put in a bash fence, the gate checks them: every flag must exist in the clap tree of the same commit, with single-token placeholders, no `2>/dev/null` and no pipe into a text tool. drain.md has 6 lines of headroom.
- **Details**: The fence route requires the new verb's task to land in the same milestone as the worker-procedure task, or earlier. Otherwise `cli::dispatch::tests` fails the gate. The until-loop line is a compound shell line, so keep it inline as the question loop does. The orchestrator-side request handling should go in questions.md (renamed in scope) or a new `references/requests.md` linked from SKILL.md, where `skill_markdown_links_resolve` checks the link. drain.md should get at most a one-line pointer. A worker editing `claude/**/*.md` runs `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`, per CLAUDE.md.
- **Counter**: If the drain-integration lens's text is placed in drain.md, the ceiling forces a split anyway.
- **Impact on plan / report**: Order the tomlctl verb task before, or in the same milestone as, the worker-procedure task. Put the orchestrator text in a reference other than drain.md.

Out of lens, for siblings. js:302 releases only a task's launch-time files from `held`. A granted path is therefore never in `held`, and safety depends wholly on the grant rule that no row of the milestone holds it. That is for the live-run-holders lens.

Coverage: Read in full: plan-worker-deep.md, questions.md, brief.md. Read in part: plan-worker-lite.md (the procedure, the forbidden-ops line and the tier section), shared-blocks.toml, verify-shared-blocks.sh, lint.rs (fence, lint and scan-set functions), skills.rs (parity agreement and the line ceilings), flow-contract-user-inputs/SKILL.md (writer rules), run-milestone.md:252, drain.md (needs_files and line count), implement-v2-milestone.js (return schema, siblings, held), and the v2-ui-fidelity plan.toml decisions D9 and D11 and the task-20 attempt. Checks run: `verify-shared-blocks.sh` (OK), `git ls-files --eol` (LF-pinned), `wc -l`, a diff of the two procedure blocks (identical), and an rg sweep of agents audit, which constrains nothing. Ruled out: `agents audit` (it matches only build and test commands); the CRLF risk (carriers are pinned LF); and the 500-line SKILL.md ceiling (implement-v2-engine's SKILL.md is 120 lines).
