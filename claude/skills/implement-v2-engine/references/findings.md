# Findings: the rules for apply rows

What changes when a milestone holds rows that fix or settle findings from a v2 review store. Every section here is skipped when the milestone holds no apply row, so a plan milestone runs exactly as [run-milestone.md](run-milestone.md) and [drain.md](drain.md) say, and reads nothing from this file.

`<slug>`, `<id>` and `<scratch>` mean what they mean in those references: the flow, the milestone and the session scratchpad.

## Apply rows

A row is an **apply row** when a `[[finding_links]]` entry in the flow's `tasks.toml` names its `ref`. Each entry carries:

| Field | Means |
|---|---|
| `ref` | The task row the link belongs to |
| `store` | The review store's repo-relative path: `.claude/flows/<s>/review-v2.toml` for a flow-bound review, `.claude/review-v2/<scope>.toml` for a flow-less one |
| `fixes` | Findings the row's work resolves |
| `settles` | Findings the row investigates, coming back with a proposal rather than a fix |

`review schedule` writes these links when it mints the rows, and marks each finding `scheduled` with its task. List the milestone's apply rows and the stores they name:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --pluck ref --raw --lines
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --where-in ref=<ref1,ref2> --select ref,store,fixes,settles --lines
```

When the second command prints nothing, the milestone holds no apply row and nothing below applies to it.

A store path maps to the store flag every `tomlctl review` verb takes: `.claude/flows/<s>/review-v2.toml` is `--slug <s>`, and `.claude/review-v2/<scope>.toml` is `--scope <scope>`. `<store flag>` below is that flag.

## What the worker sees

The worker's view, `tomlctl plan view --slug <slug> --for worker --task <n>`, prints a `## Findings` section for an apply row: each finding its links name, read from its review store at the moment the view runs through the implementer projection, headed with the finding id, whether the row `fixes` or `settles` it, and the store. A store that cannot be read, or a finding no longer in it, is one line in that section, never a failed view. Workers therefore need no review verb of their own, and the args file carries nothing extra for an apply row.

## Anchor premises on a started row

`review schedule` gives each apply row one `base` premise per finding, whose check is `tomlctl review anchor <finding> <store flag>` (with `--gone` for a `remove` cleanup): it holds while the code the finding cites still reads as the finding says. Premises rerun before every launch ([run-milestone.md](run-milestone.md), step 3), and a row whose first worker already made the edit no longer matches its anchor.

So when acting on the recorded verdicts, read a `fails` verdict on an anchor premise, one whose `check` opens `tomlctl review anchor `, by whether its row is started, as the pre-flight's listing defines it (an `agent` of `plan-worker-lite` or `plan-worker-deep`):

| Row | An anchor premise that `fails` |
|---|---|
| Started | Is the row's own work. It does not withhold the row, and the relaunch goes ahead |
| Not started | Withholds the row, as any false premise does, and goes to the drain replan |

A `broken` anchor premise withholds the row either way, since it is the check that is wrong. Every other premise of an apply row is read as run-milestone step 3 reads it.

## Linked review stores in the clean-tree and lane checks

`review reconcile`, and the steps that record reproductions, settle proposals and schedule accepted findings, write a review store while the milestone is open. A store under `.claude/flows/<slug>/` is never listed by either check. Any other store a `[[finding_links]]` entry of the flow names, with its `.sha256` sidecar, is exempt in the same way as `.claude/backlog.toml`:

- from the pre-flight's clean-tree check ([run-milestone.md](run-milestone.md), step 1), so a dirty linked store does not stop the engine;
- from the drain's out-of-lane rule ([drain.md](drain.md), step 3), so a dirty linked store is not an edit outside every worker's lane.

List every store the flow links, for both checks:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --pluck store --distinct --raw --lines
```

The exemption covers those paths and their sidecars only. Any other path under `.claude/review-v2/` or another flow's directory is still someone else's edit.

## Reconcile

Runs in [drain.md](drain.md), step 10, after the milestone commit is stamped on its rows and before the flow-store commit, when the milestone holds an apply row.

For each distinct store the milestone's links name, from the listing under **Apply rows**, with its store flag:

```bash
tomlctl review reconcile --slug <s> --flow <slug>
tomlctl review reconcile --scope <scope> --flow <slug>
```

It moves each finding scheduled into `<slug>` by the state of its task: a `fixes` finding whose task is `done` with a commit is `resolved`; a `settles` finding whose task is committed goes back to `open`; a finding whose task is deferred or gone goes back to `open` with its apply link cleared; anything else stays `scheduled`. It prints `{ok, flow, review_store, resolved, reopened, in_flight, retitled}` and writes the store only when something moved. A finding listed under `in_flight` after its milestone is stamped belongs to a row of another milestone; it is not an error.

A refusal stops the engine before the flow-store commit, as a failed commit does: make no flow-store commit, leave the milestone commit and its stamps in place, and report the store and the refusal in `needs_human`, so the user fixes the store and the reconcile and the flow-store commit are made by hand or by a later invocation.

**Staging.** A reconciled store under `.claude/flows/<slug>/` is already in the flow-store commit's paths. A store outside it joins that commit with its sidecar: add `<store>` and `<store>.sha256` to the `git status --porcelain --` and `git add --` lists of the flow-store commit, and to its `git commit --` pathspec, so the pre-commit hook compares the pair. It never goes in the milestone commit, which stays the rows' `files` alone: a milestone is still exactly two commits.
