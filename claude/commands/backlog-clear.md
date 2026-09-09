---
description: Clear open backlog items unattended in a prepared worktree — triage, fix the mechanical ones, and leave the diff for batched review
argument-hint: [optional kind filter, e.g. debt — defaults to debt,annoyance]
---

# /backlog-clear — work the repo-scoped capture log down

> Thin orchestrator. The procedure — eligibility, triage verdicts, the run record,
> model routing, and the fix constraints — lives in the `backlog-clear` skill
> (`claude/skills/backlog-clear/SKILL.md`).

The complement to `/backlog`, which triages *with* a human. This one picks off the
items that need no human and fixes them, leaving a reviewable diff.

Invoke the `backlog-clear` skill (`claude/skills/backlog-clear/SKILL.md`) and follow it
end to end. It is self-contained and names every contract it depends on by path, so it
runs unchanged under Claude Code, Codex, or any harness that can read a file and run a
shell.

**Run it in a dedicated worktree, not the primary checkout.** The tooling prepares one:
under Codex the `prefix+alt+b` herdr chord creates or resyncs it and starts the agent
there. That split exists because Codex's sandbox forbids every `.git` write, so the
agent can only edit files — which also means it never commits. Reviewing the diff and
committing are the human's, taken in batches.

Two things this carrier does not restate, because the skill owns them: the
orchestrator-only writer rule over `.claude/backlog.toml`, and `BACKLOG-RUN.md` — the
run record that doubles as the skip list, so an interrupted run resumes instead of
repeating.

Under Codex the skill is available directly: `~/.codex/skills/backlog-clear` is a
directory junction onto this repo's copy, so there is one source of truth and no sync
step.
