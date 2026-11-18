Status: done — drain.md gains step 5a Granted requests and its knock-on rules; report.md gains the requests: section and kind request

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md", "claude/skills/implement-v2-engine/references/report.md"]

[[deviations]]
what = "The step-3 'What the run's agents ran' audit text stays under step 3; its opening now says it runs after step 5a, and step 5a ends by sending the reader back to it."
why = "Keeping the block in place keeps every inbound citation to drain step 3 valid, among them requests.md's 'the directory drain step 3 names' for the transcript directory. Moving the text into 5a would have broken that citation, and the task holds only drain.md and report.md. The detail's own wording, 'say where it now runs and keep its rules', allows either."
means = "Action: move the step-3 agents audit after the Granted requests step"

[[checks]]
command = "grep -q 'Granted requests' claude/skills/implement-v2-engine/references/drain.md && grep -q 'requests:' claude/skills/implement-v2-engine/references/report.md"
result = "pass"

[[checks]]
command = "wc -l claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

drain.md, now 519 lines, under the 600-line ceiling:
- Step 2's table: a `done` row that holds a granted request is read and applied by step 5a.
- Step 3, the lane check: a granted, unapplied path counts as in the requesting row's lane. A path whose grant a worker wrote itself does not, because 5a treats that request as denied and reports its paths as out of lane.
- Step 3, "What the run's agents ran": the audit now runs after step 5a. An accept check that 5a amended counts as allowed.
- New `## 5a. Granted requests`, between step 5 and step 6. No existing step number changes. It runs requests.md's "At the drain: Granted requests" procedure in this order: the transcript audit of rulings a worker wrote itself, then the open items ruled denied, then the audited grants update (`granted scope|amend <request-id>:` reasons), then `plan request applied`. After that it runs the deferred agents audit.
- Step 6, "Relaunch or stop": a grant on a `done` row is not a moved field. A grant on a `failed` or `parked` row is, so that row relaunches once (a parked row only after it is unparked).

report.md:
- The report shape has a new `requests:` section, with lines of the form `task <n> <request-id> <p|c><k> granted|denied|needs-user: <reason>`.
- The section-to-kind table has a new row, `requests:` to `request`. A paragraph says that a self-ruled request goes under `audit:` as a breach and its paths under `out of lane:`.
- `requests:` lines are added to the lines recorded with `needs_person = false`.

The installed schema already has kind `request` (tomlctl/src/plan/schema.rs, `ReportKind::Request`).

The `cli::dispatch::tests` subset was not run. The view marks it `Run by: gate`, and the worker procedure forbids running gate checks. That gate checks the line ceiling and that links resolve. All links I added point to files that exist (requests.md, drain.md), and the anchors are section names cited in prose, not markdown fragment links.
