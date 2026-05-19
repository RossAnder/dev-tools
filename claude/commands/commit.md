---
description: Draft and create a commit message for the staged tree via the `commit-conventions` skill — with the project's resolved dialect and an atomicity-gate confirmation.
argument-hint: (no args — operates on the current staged tree)
---

# /commit — draft + create a commit via the `commit-conventions` skill

> Thin orchestrator. Convention rules, dialect resolution, atomicity test, and message
> composition all live in the `commit-conventions` skill — this file dispatches the skill's
> Step 1-4 procedure and handles user confirmation + the actual `git commit` call.

## Phase 1: Pre-flight

Run `git diff --cached --stat` to confirm staged changes exist.

If the diff is empty, run `git status` and halt with: _"Nothing staged. Run `git add <files>` first, then re-invoke `/commit`."_ Do not draft, do not prompt.

Resolve the active flow (best-effort, non-blocking):

```bash
tomlctl flow resolve --branch "$(git branch --show-current)" 2>/dev/null
```

Capture the resolved slug (if any) for the Phase 4 warning. Flow-less invocation is fine — `/commit` never writes to `execution-record.toml` and does not require a flow.

## Phase 2: Invoke the `commit-conventions` skill

Dispatch the `commit-conventions` skill's Step 1-4 procedure against the staged tree. The skill owns the rules; this command only sequences the user-facing prompts.

1. **Atomicity gate** (skill Step 1). Inspect `git diff --cached`. If the gate reports multi-concern, present an `AskUserQuestion` with three options:
   - **Commit anyway** — bypass the gate and proceed to drafting. Recommend only for trivial multi-concern (doc fix + typo etc.).
   - **Split now** — halt and surface the skill's proposed split. Instruct the user to `git reset` and re-stage the first unit, then re-invoke `/commit`.
   - **Abort** — halt with no commit and no further output.

   Do NOT emit the `IMPLEMENT-AUTOCOMMIT: phase-2-step-5` sentinel — `/commit` is direct user invocation, the gate is mandatory.

2. **Resolve conventions** (skill Step 2). Walk the 6-entry precedence. When a non-default vocab source is adopted, emit the skill's one-line notice (`commit-conventions: vocab source = <tool>; ...`).

3. **Draft the message** (skill Step 3) per the resolved dialect. Run the Step 4 self-validation checklist before presenting to the user.

## Phase 3: Confirm & commit

Present the drafted message via `AskUserQuestion` with three options:

- **Approve** — proceed to `git commit -F -`.
- **Revise** — accept user free-text feedback via the `Other` slot; redraft ONCE and re-present. This is a single revision round; do not loop.
- **Cancel** — halt with no commit.

On **Approve**, invoke `git commit -F -` with the drafted message on stdin. Pin this literal HEREDOC pattern verbatim — neither `~/.claude/CLAUDE.md` nor the repo `CLAUDE.md` documents it, so the pattern lives here:

```bash
git commit -F - <<'COMMIT_MSG'
<subject>

<body>
COMMIT_MSG
```

The closing `COMMIT_MSG` MUST be at column 0 (no leading whitespace) or the heredoc parses incorrectly. The single-quoted opener (`<<'COMMIT_MSG'`) suppresses shell expansion inside the body — required when the message contains backticks, `$`, or `!`.

## Phase 4: Hook-failure handling & post-commit

If `git commit` exits non-zero (pre-commit hook rejected the commit):

- Surface stderr **verbatim**.
- Halt — do NOT retry, do NOT re-prompt for a revised message.
- Do NOT suggest `--no-verify`.
- If the staged set intersects any carrier listed in `scripts/shared-blocks.toml`, append one line: _"This commit was rejected by a shared-block parity check. Fix the drift; do not bypass with `--no-verify`."_

On success:

- Print the committed SHA (e.g. `git rev-parse --short HEAD`).
- If Phase 1 resolved an active flow AND the user did not explicitly opt for a flow-less commit, emit a one-line warning:

  > _"Active flow `<slug>` detected; this commit will NOT appear in `execution-record.toml`. Consider `/implement` if this change belongs to the active plan."_

- Exit cleanly.

## Notes

- `/commit` never writes to `execution-record.toml` — the active-flow warning makes any commit-history vs execution-record divergence visible rather than silent.
- The skill is required. If `commit-conventions/` is not installed (neither at `~/.claude/skills/` nor at `<repo>/.claude/skills/`), halt with installation instructions per the skill's `## Installation` section rather than drafting unguided.
- Convention rules, the Conventional Commits type table, anti-pattern enumeration, breaking-change format, and PR-description rules all live in `claude/skills/commit-conventions/SKILL.md` — this command does not redefine them.
