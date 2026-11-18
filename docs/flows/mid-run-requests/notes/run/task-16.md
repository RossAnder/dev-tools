Status: done — requests.md written, REQUEST routed from run-milestone.md, linked from SKILL.md in all four places

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/references/requests.md",
  "claude/skills/implement-v2-engine/references/run-milestone.md",
  "claude/skills/implement-v2-engine/SKILL.md",
]

[[deviations]]
what = "A request still open when the drain reaches the Granted requests step is ruled denied by the orchestrator with the reason 'unanswered before the worker's wait ended', then marked applied"
why = "plan request applied refuses an open request (plan-request.md), so without a ruling such a request would stay outstanding for ever and its record would never reach the report; the worker already took the denial route, so denied matches what happened"
means = "detail step 3 lists only ruled, unapplied requests"

[[deviations]]
what = "With no kept transcript directory, the orchestrator-ruled grants are not dropped silently: each granted path goes to the replan as a needs_files entry (drain.md 'Widening a row from needs_files', a done row's status untouched) and each granted change to step 6 as a re-spec input"
why = "the detail says such a run applies only tomlctl-ruled items but not what becomes of the rest; leaving them unapplied would exempt their paths from the clean-tree check indefinitely, and the worker may already have edited the granted file. Alternatives weighed: leave them outstanding (stuck exemption), or park the row for the user (costly for a routine session end). Re-judging through the existing widening rules keeps the audit gap closed without a new mechanism"
means = "detail step 3, no-transcript sentence"

[[deviations]]
what = "A breached (worker-ruled) request is marked applied with nothing written, and each path it granted is reported under needs_human as an out-of-lane edit nobody owns, so the milestone does not gate while it stands"
why = "'treated as denied, never applied' leaves the worker's edit in the tree; drain.md step 3's rule for an edit nobody owns is the existing path, and marking it applied stops re-entry from re-processing it"
means = "detail step 3, audit sentence"

[[deviations]]
what = "A parked row that holds grants keeps its parked status; the grant waits in its spec until the parked handling (step 6, questions.md 'At the drain', or the next invocation's re-ask) unparks it. Only a failed row is reset to pending in the grant row"
why = "resetting a parked row to pending would bypass the parked re-ask and the question loop, which own unparking; the relaunch still happens once the row is unparked"
means = "detail step 3: 'one on a failed or parked row is, so that row is relaunched once'"

[[deviations]]
what = "Added a holder case the detail does not name: a started, uncommitted holder in another milestone means no grant for that path and the granted row is parked by the orchestrator"
why = "add and rule should refuse such a holder, so reaching it means the mechanical check missed one; ordering it either way could hold a milestone behind another's uncommitted work, which drain.md's widening section already sends to the user"
means = "detail step 3 holder ordering"

[[tangential]]
kind = "question"
area = "claude/skills/implement-v2-engine/references/drain.md"
summary = "requests.md cites drain.md step 5a ('Granted requests') and the report's requests: section, which sibling tasks add; if their headings differ the citations need to follow"
why = "the cross-file references are only consistent once the drain and report tasks land with the names their specs give"

[[checks]]
command = "grep -q 'REQUEST <request-id> task <n>' claude/skills/implement-v2-engine/references/requests.md && grep -q 'requests.md' claude/skills/implement-v2-engine/SKILL.md && grep -q 'requests.md' claude/skills/implement-v2-engine/references/run-milestone.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- `claude/skills/implement-v2-engine/references/requests.md` (new), laid out like questions.md: what a worker sends (the `REQUEST <request-id> task <n>` literal, first line only, the record is the content); step 1 reading and validating the record (task, milestone, open run, status), with requests ruled before any question picker opens (D10) and a question record never ruled; step 2 ruling path items (need shown by `why`, not ruled out by the brief's Non-goals or row constraints) and change items per field within D11's limits, `needs-user` for anything touching outcome, success criterion or decision (D9), the apply-row refusals, and the staged `plan request rule` call with the refuse-then-deny loop; the writes during a live run; the drain's Granted requests step (D12): the `agents audit --pattern 'plan\s+request\s+(rule|applied)'` breach audit first, the no-transcript rule, ruling left-open items denied, one audited `tasks update --ndjson` row per task with the `granted scope|amend <request-id>:` reason, holder ordering, `plan request applied`, and the relaunch rule; the report entries; and re-entry.
- `run-milestone.md`: a clean-tree exemption bullet for paths of granted, unapplied requests, and a "A request mid-run" paragraph beside the QUESTION routing that sends a first line opening `REQUEST ` to requests.md.
- `SKILL.md`: requests.md named in the role bullet on user questions, in the drain-and-replan row's Reference column (step 5a), in a new "Unapplied requests" Re-entry paragraph after the parked-rows one, and in the references list. CRLF line endings preserved.

The dispatch-tests check is marked `Run by: gate`, so it was left to the gate. Every fenced command uses flags confirmed against the installed binary's `--help` (`plan request list` takes the read bundle's `--where`/`--select`/`--lines`; `agents audit` takes `--pattern`) and single-token placeholders; the lint tokenizer keeps single-quoted text whole, so the `|` inside the audit regex does not split the command.
