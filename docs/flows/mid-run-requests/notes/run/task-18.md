Status: done — both plan workers carry an Asking the orchestrator section and request before failing for a file or a stale spec point

```toml run-note
files_touched = ["claude/agents/plan-worker-lite.md", "claude/agents/plan-worker-deep.md"]

[[deviations]]
what = "Renamed the forbidden-ops bullet from 'One row and one question' to 'One row, one question and one request'"
why = "The bullet now allows a third write; no file outside the two workers cites the old label (rg over the repo, flow notes aside)"
means = "detail: widen the 'One row and one question' bullet"

[[deviations]]
what = "Also widened the shared block's failed and parked bullets in 'Done, failed or parked': failed now names a file the request did not get, and parked adds a needs-user item met after the one question was spent"
why = "Without it the status definitions contradicted the new section's denial and needs-user branches"
means = "detail lists step 7 and the forbidden-ops block as the shared-block edits"

[[deviations]]
what = "Lite's stale-symbol bullet in 'When to return failed' also says 'after a request for the correction is denied or unanswered', besides the file-boundary bullet"
why = "The detail says lite's failed list says 'after a request for it is denied'; the stale-code bullet is the spec-correction counterpart of the file bullet"
means = "detail: tier sections say 'after a request for it is denied'"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "grep -q 'Asking the orchestrator' claude/agents/plan-worker-lite.md && grep -q 'Asking the orchestrator' claude/agents/plan-worker-deep.md && grep -q 'REQUEST <request-id> task <n>' claude/agents/plan-worker-deep.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

Both `claude/agents/plan-worker-lite.md` and `claude/agents/plan-worker-deep.md`, with the two shared blocks edited identically (parity verifier passes):

- **plan-worker-procedure, step 7**: a file outside the task's `files` is requested under **Asking the orchestrator** once every in-file edit is done, and edited only on a grant; `failed` with `needs_files` follows only a denied or unanswered request. A wrong spec point goes in the same request.
- **Done, failed or parked**: `failed` names a file the request did not get; `parked` adds a `needs-user` item met after the one question was spent.
- **New section Asking the orchestrator** after Asking the user: when to ask (a needed path including a `Generators`-line output, or a spec point the code shows wrong); post last, once, batched, a rename or added file also as a path item; payload staged in the scratchpad and posted with `tomlctl plan request add --slug <slug> --task <n> --json @<path>`; `ruled` at once skips the wait; otherwise SendMessage `REQUEST <request-id> task <n>` to `main` and the 590 s until-loop polling `plan request list ... --where status=ruled --get id`; read rulings once with `--select paths,changes --lines` and branch per item (granted path, granted change, denied or open = today's path, needs-user = the one question or park). Never run `plan request rule` or `applied`.
- **plan-worker-forbidden-ops**: one `plan request add` per run allowed, `rule` and `applied` named as forbidden; the edit boundary also excepts a path granted to this run's request, and a needed file is requested before the task fails.
- **Tier sections**: lite's failed list (stale symbol and file-boundary bullets) and deep's stale-detail and file-boundary Judgement bullets each fall to `failed` only after a request is denied or unanswered.

Verbs and flags were checked against `claude/skills/tomlctl/references/plan-request.md`; an omitted `paths` or `changes` key is accepted (`entries` in `tomlctl/src/plan/request.rs` returns empty), and the installed binary answers `plan request list --where id=Q999 --where status=ruled --get id` with empty output and exit 0, so the until-loop's test on output holds. The view's `Generators` wording ("ask for it with a scope request") is in `generator_lines`, `tomlctl/src/plan/view.rs`.

The dispatch-tests check is marked `Run by: gate`, so it was left to the gate.
