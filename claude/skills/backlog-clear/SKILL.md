---
name: backlog-clear
description: "Clears open items from the repo-scoped capture log at `.claude/backlog.toml` unattended, inside a dedicated worktree the tooling has already prepared — the eligibility filter over `kind`, the judgeability gate that separates a verified resolution from an item this worktree structurally cannot see, the five verdicts and the conservatism rule governing them, the run record that carries the run's base commit and doubles as a skip list, the minimum-change constraints on a fix, the build-discipline limit that forbids whole-suite verification, and the hard rule that the agent never runs git because the sandbox forbids it. Use when clearing, working, or picking off backlog items."
---

# Backlog clear

Work this repo's open backlog down inside a dedicated worktree, one item at a time.
The complement to `/backlog`, which triages *with* a human; this runs unattended.

## You do not touch git

The worktree you are in was created for you by the tooling, on its own branch, off the
current checkout. **Never run `git worktree`, `git commit`, `git branch`, `git
checkout <branch>`, `git merge`, `git rebase`, or `git push`.**

This is not a style preference — it cannot work. Codex's `workspace-write` sandbox
forbids every write under `.git`; `writable_roots` does not override it, and the
sandbox blocks the herdr socket too, so there is no delegated route. Any git write you
attempt fails with `Permission denied`, wasting a turn.

Reads are fine and you need them: `git status`, `git diff`, `git log` all work, and
`git diff` is how you check what you actually changed. `git checkout -- <path>` is a
working-tree write, not a `.git` write, so reverting a file is permitted.

Your output is **edited files plus a run record**. A human reviews the diff and commits
in batches. That mirrors how lumina handles worktrees: the control plane records and
never shells to git, a companion process executes it, agents only do the work.

## You cannot spawn another codex

`codex exec` fails inside this sandbox — it needs to write `~/.codex/tmp`, which is
outside the writable set. Do the triage and the fix yourself, in this session.

## The other rule that outranks the rest

**Never write `.claude/backlog.toml`.** It is orchestrator-only-writer: dispositions
belong to a human or to `/backlog`. If you think an item's status should change, say so
in your report and leave the store alone.

## You are on Windows

Panes run PowerShell 7. Unix idiom fails: no `ls -la`, no `head`, no `which`. Use
`Get-ChildItem`, `Get-Content -TotalCount N`, `(Get-Command x).Source` — or `tomlctl`
and `git`, which behave the same either way.

## Step 0: Pre-flight and the run record

```bash
tomlctl --version                    # the backlog group landed in 0.6
tomlctl backlog list --open
git rev-parse HEAD                   # this run's base commit
```

A missing store reads as zero rather than erroring. On an empty open set, say so and stop.

`BACKLOG-RUN.md` at the worktree root is your run record **and your skip list**. Read it
first if it exists; create it if not. Record the base commit in its header, so a later
reader can tell what tree the verdicts were made against:

```markdown
# Backlog run

Base commit: <sha> (<date>)

| id | verdict | files | note |
|---|---|---|---|
```

Append a row the moment you finish an item, before starting the next, so an interrupted
run resumes instead of repeating.

**Terminal rows are skipped on the next run; non-terminal rows are retried.**
`fixed`, `needs-human` and `already-resolved` are terminal. `unverifiable-here` and
`failed` are not — never treat them as done.

## Step 1: Eligibility

Take only `kind = debt` or `kind = annoyance` unless the invocation names others.
Skip `bug` — it needs root-cause judgement — and `question`, which needs a human answer.

## Step 2: Is this item even judgeable here?

**Do this before triaging.** You are in a clean checkout of committed state at the base
commit. It does NOT contain: untracked or gitignored files from the primary checkout,
uncommitted work in progress, or the primary's working-tree state. An item about any of
those describes something you structurally cannot see, and "I don't see it" is not
evidence that it was fixed.

**Check the vintage first.** An item carrying `base_sha` records the commit it was
captured against, so the question is decidable rather than a guess:

```bash
git merge-base --is-ancestor <base_sha> HEAD
```

Non-zero means this checkout **predates** the item — it describes code you do not have.
Return `unverifiable-here` immediately; do not triage it. An item with no `base_sha` is
of unknown vintage (it predates the field), so fall through to the judgement below.

Return `unverifiable-here` — and change nothing — when any of these holds:

- The item concerns a path that is untracked or ignored (`git check-ignore -v <path>`,
  or absent from `git ls-files`). A dev database, a local log, a build artefact.
- It concerns working-tree state rather than committed content: line endings in the
  working copy, file permissions, staged-vs-unstaged differences.
- It concerns work that was in progress when it was captured and may not be committed
  yet — items whose `created` date is at or after this run's base commit deserve that
  suspicion.
- You cannot locate what it describes at all. That is *ambiguous* between "already
  fixed" and "not in this tree yet", and you must not resolve the ambiguity by guessing.

This is the failure that has actually happened: an item about gitignored database files
at the primary repo root was marked `already-resolved` because they were absent from
the worktree, where they can never appear. It was wrong, and the skip list made it
permanent.

## Step 3: Triage

| Verdict | Means | Terminal |
|---|---|---|
| `fix` | The problem is really present here, and the change is small, self-contained and unambiguous from the item text. | — |
| `needs-human` | Needs a judgement call, a naming or product decision, or touches security, a public API, a migration, or build config. | yes |
| `already-resolved` | You found **positive evidence** it was fixed: the commit that fixed it, or the current code doing the right thing. | yes |
| `unverifiable-here` | Step 2 applies, or you could not locate the subject. | no |
| `failed` | You attempted a fix and could not complete it. | no |

`already-resolved` requires evidence you can cite in the note. Absence of the problem is
not evidence — say `unverifiable-here` instead and let a later run against a better tree
decide. Be conservative generally: a wrong fix costs more than a deferred item, and
anything touching shared code, observable behaviour, security, or build configuration is
not a low-risk change.

## Step 4: Fix

- Change the minimum needed. Nothing incidental — no reformatting, renaming, or tidying
  you were not asked for.
- Stay in the files triage identified. If the fix needs one outside that set, stop and
  record `needs-human` rather than widening it.
- No new dependencies. No public API or build-config changes.
- Preserve behaviour apart from the defect described.
- If the item turns out to be wrong or already fixed, stop and say so. Never invent work
  to justify a change.

Check your own diff before recording `fixed`: `git diff` should touch only the files you
named and nothing incidental. If it widened, `git checkout -- <path>` the strays.

Verify with the narrowest check that covers the change — `cargo clippy`, or the one test
that exercises it. **Do not run full builds or whole suites**: siblings share this
machine and `tomlctl/target`, so a full `cargo build` serialises everyone on the build
lock. That rule is `CLAUDE.md` → *Build discipline in multi-agent flows*.

## Step 5: Report

Print the run-record table and the base commit, then stop. State plainly which items you
changed files for, and call out any `unverifiable-here` rows — those are the ones
needing a human to judge from the primary checkout.

Review is `git diff` in this worktree. Committing and merging are the human's, in batches.
