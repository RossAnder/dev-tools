# Commit-Conventions Config Generation Runbook

A repeatable procedure for generating a `.claude/commit-conventions.toml` for **any** project.

## Why bother

Without this file, the `commit-conventions` skill's Step 2 walks its full 6-layer detection
precedence on **every** commit — which pulls `references/detection.md` (~4.4k tokens) into
context and runs a 50-commit `git log` classification. Both are re-paid per commit, and in a
multi-commit flow (`/implement` commits once per batch checkpoint) that compounds. Pinning the
dialect in a layer-1 config short-circuits all of it to a single `tomlctl get`, with **zero
loss of message quality** — the file just records the convention the repo already follows.

Rule of thumb: any repo you run `/commit` or `/implement` in more than a handful of times
should have this file. It pays for itself in a session or two.

## Prerequisites

- `tomlctl` on PATH (the skill reads the file via `tomlctl get`).
- The `commit-conventions` skill installed (user- or project-level).
- Run from the target repo root (so `git log` sees its history).

## Step 1 — Detect the dialect

First check for authoritative tooling (these pin the dialect without guessing):

```bash
# Vocab/dialect sources — presence pins conventional-commits (and may export a type vocab):
ls .commitlintrc* commitlint.config.* .czrc .cz.toml 2>/dev/null      # commitlint / commitizen
rg -l 'commit_parsers|conventional' cliff.toml .git-cliff.toml 2>/dev/null   # git-cliff
git config --get commit.template                                       # a pinned template
```

If none, classify the history. The dialect is whichever pattern ≥50% of recent subjects match:

```bash
# conventional-commits: `type(scope)!: subject`
git log --format='%s' -100 | grep -cE '^[a-z]+(\([a-z0-9/_-]+\))?!?: '
# gitmoji: leading emoji or :code:
git log --format='%s' -100 | grep -cE '^(:[a-z_]+:|\p{Emoji})'
# else → plain (no prefix) or custom (a ticket-id regex like `[PROJ-123]`)
```

`conventional-commits` is the safe default when signals are weak or the log is empty.

## Step 2 — Extract the type vocabulary

```bash
git log --format='%s' -400 | grep -oE '^[a-z]+' | sort | uniq -c | sort -rn
```

Keep the entries that are real Conventional-Commits types; **discard typos and
scope-as-type mistakes** (e.g. a stray `sqlx`, `harness`, or `change` is noise, not a type).
When in doubt, use the standard set:
`feat, fix, perf, refactor, docs, test, build, ci, chore, revert, style`.

## Step 3 — Extract scopes and decide the gate

```bash
git log --format='%s' -400 \
  | grep -oE '^[a-z]+\(([a-z0-9/_-]+)\)' | sed -E 's/^[a-z]+\(//; s/\)$//' \
  | sort | uniq -c | sort -rn
```

Now judge the **shape** of the scope distribution — this decides `require_scope`:

- **Stable, closed vocabulary** (a fixed set of crates/packages/areas, no long tail):
  set `require_scope = true` and list them. New scopes then get flagged, which is what you want.
- **Evolving / long-tailed** (a few stable areas + many one-off per-feature or per-story
  scopes): set `require_scope = false` and list only the stable areas as **advisory**
  documentation. This is the common case for active mono-repos — a hard list would false-flag
  every legitimately-new feature scope.

Map scopes to durable *areas* (crate names, top-level dirs), not ephemeral feature names.

## Step 4 — Write the file

`.claude/commit-conventions.toml` (only the sub-table matching `dialect` is read):

```toml
dialect = "conventional-commits"

[conventional-commits]
allowed_types = ["feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "revert", "style"]
allowed_scopes = ["<area>", "..."]   # advisory when require_scope = false
require_scope = false                # true only for a closed vocabulary (Step 3)
require_body_for = ["feat", "perf"]  # types that always need a body — tune to taste
subject_max_length = 72
subject_preferred_length = 50
body_wrap_at = 72
ban_subjects = ["wip", "fix", "update", "changes", "stuff", "misc", "address review comments"]
```

For `gitmoji` / `plain` / `custom` dialects, see the schema in `references/detection.md`
(§ TOML schema) for the per-dialect field set — only the active sub-table matters.

Do **not** write a `.sha256` sidecar; this is a hand-authored config, not a tomlctl flow file.

## Step 5 — Verify the short-circuit

```bash
tomlctl get .claude/commit-conventions.toml dialect                 # → "conventional-commits"
tomlctl get .claude/commit-conventions.toml conventional-commits    # → the sub-table
```

Then draft one commit through the skill and confirm it (a) reads the TOML, (b) does **not**
open `references/detection.md`, and (c) does **not** run `git log` inference. If it still walks
detection, the file is missing the `dialect` key or `dialect` holds an unrecognised value —
the skill emits a `commit-conventions: … falling through to layer 2` notice in that case.

## Field-tuning notes

- `require_body_for` — add `refactor` if your team expects a rationale on every refactor;
  drop to `[]` for a low-ceremony repo.
- `ban_subjects` — extend with project-specific low-signal subjects you keep seeing.
- `subject_max_length` — 72 is the ceiling; lower it (e.g. 60) only if the team enforces it.
- Revisit after a major restructure: new crates/areas mean new stable scopes.
