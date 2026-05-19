---
name: commit-conventions
description: ALWAYS invoke before drafting any `git commit`, `git commit --amend`, `git commit -s`, or `gh pr create` message — and before harness-driven commit steps (notably `/implement` Phase 2 step 5 git checkpoint). DO NOT write commit messages directly. Resolves project dialect via polyglot precedence (`.claude/commit-conventions.toml` → commitlint → `scopes.txt` → CONTRIBUTING/CLAUDE.md → `git log` inference → default Conventional Commits).
---

# Commit Conventions

## When to apply

Apply whenever a commit or PR creation is imminent. Triggers:

- A commit step (`git commit`, `--amend`, `-m`, `-s`).
- A workflow ending in "commit", "ship", "push", "open a PR", or similar.
- Staged changes exist and the next action would be to commit.
- The orchestrator is in `/implement` Phase 2 step 5 about to commit a batch.
- The user typed `/commit`.

If a commit is requested but nothing is staged, run `git status` first and surface that before drafting.

## Step 1: Atomicity gate

Inspect what will be committed with `git diff --cached --stat` and `git diff --cached`.

Staged changes must describe **one logical change**. Test: can the change be summarised in a single declarative sentence without "and"?

If staged changes span multiple concerns:

1. STOP. Do not draft a combined message.
2. Propose a split — identify the logical units.
3. Reset and re-stage: `git reset` then `git add -p` or `git add <paths>`.
4. Commit each unit separately, looping back to step 1.

A commit must leave the tree buildable and testable. Half-finished work plus an unrelated fix is two commits.

Exception: a user-requested squash covering a whole branch is fine — pick one dominant type/scope and list constituent changes in the body.

### `/implement` carve-out (sentinel-token contract)

Detect `/implement`-driven invocation by matching the literal sentinel token `IMPLEMENT-AUTOCOMMIT: phase-2-step-5` in the surrounding prompt context. `/implement`'s Phase 2 step 5 MUST emit this token verbatim before invoking the skill.

- **With the token**: the gate is informational only. The batch boundary is fixed by the dependency graph, not negotiated per-commit. Report a one-line assessment and proceed to Step 2 without offering the split protocol.
- **Without the token**: the gate is mandatory and the split protocol above applies.

This makes the carve-out an explicit contract, not a heuristic — the model must see the sentinel, not infer intent.

## Step 2: Resolve project conventions

Walk top to bottom; first match wins. See `references/detection.md` for per-source parse routines and the polyglot regex set.

1. **`.claude/commit-conventions.toml`** — authoritative. Read via `tomlctl get .claude/commit-conventions.toml dialect` and per-dialect sub-tables (`[conventional]`, `[gitmoji]`, `[plain]`, `[custom]`). Honour any `ban_subjects = [...]` list verbatim.
2. **Release / changelog / versioning tool config (polyglot)** — first-found wins; ecosystem order TS/Vue/React → Rust → .NET/C# → universal. Two flavours: *vocab sources* (commitlint, commitizen, release-please, git-cliff `[git.commit_parsers]`) export concrete type/scope vocabularies; *CC-presence signals* (semantic-release, changesets, cargo-release, GitVersion, Nerdbank.GitVersioning) pin the dialect to Conventional Commits without vocab, falling through for scope while keeping the dialect pinned. `git config commit.template` is also consulted.
3. **Repo-root or `.claude/`-rooted `scopes.txt`** — newline-delimited scopes.
4. **`CONTRIBUTING.md` / `CLAUDE.md`** — heuristic prose scan for "Conventional Commits", "gitmoji", or pinned-regex blocks.
5. **`git log` inference** — classify the last 50 commits into `{conventional-commits, gitmoji, plain, custom}`. Threshold: ≥50% match. Skip on empty log.
6. **Default** — Conventional Commits, no scope restriction, type list per the table below.

If 1-5 yield nothing and the change is trivial, omit the scope rather than invent one.

## Step 3: Compose the message

Format (Conventional Commits dialect):

```
<type>(<scope>)<!?>: <subject>

<body, optional>

<footers, optional>
```

The `!` is included if and only if the change is breaking. The `<scope>` is omitted (along with its parens) if no scope applies.

### Type (Conventional Commits default set)

| Type | Use for | SemVer |
|---|---|---|
| `feat` | New capability | MINOR |
| `fix` | Bug fix | PATCH |
| `perf` | Perf improvement | PATCH |
| `refactor` | Restructure, no behavior change | none |
| `docs` | Docs only | none |
| `test` | Tests only | none |
| `build` | Build / deps | none |
| `ci` | CI config | none |
| `chore` | Housekeeping | none |
| `revert` | Reverting a prior commit | depends |
| `style` | Formatting only | none |

If the change spans multiple types, return to Step 1 — it should be multiple commits.

### Subject

- **Imperative mood**: "add", "fix", "remove" — never "added", "adds", "fixing".
- **Lowercase first letter** unless the project says otherwise.
- **No trailing period.**
- **Hard ceiling 72 chars; aim for ≤ 50.**
- Describe **what changed**, not why. The body holds the why.

Imperative test: prefix the subject with "If applied, this commit will…". If it isn't a grammatical English sentence, rewrite.

### Body

Include a body when the change is non-obvious, a reviewer would ask "why?", a trade-off was made, or relevant context lives outside the tracker. Blank line between subject and body; wrap at 72; explain **why**, not what. Skip the body for mechanical changes — typo fixes, dep bumps, formatting passes.

### Footers (RFC-2822 trailers)

One per line, `Token: value`, in a trailing block separated by a blank line:

- `BREAKING CHANGE: <migration>` — required if `!` is in the header.
- `Refs: #123` — references without closing.
- `Fixes: #123` / `Closes: #123` — auto-closes on merge.
- `Signed-off-by: Name <email>` — DCO. Generated by `git commit -s`; never type by hand.

**Do not manually add `Co-Authored-By:` or "Generated with Claude Code" lines** — those are controlled by `attribution.commit` in settings.

### Per-dialect branches

- **Conventional Commits** — rules above apply.
- **Gitmoji** — emoji or `:emoji_code:` prefix (`:sparkles: add OAuth flow`). 50/72, body, footers still apply. Map type→emoji per project config or the public Gitmoji table.
- **Plain** — no prefix; subject + body discipline only. Imperative mood and 50/72 still bind.
- **Custom regex** — must match the pinned regex (`.claude/commit-conventions.toml [custom] pattern`). On failure, emit the regex back with a note on which group failed and ask for guidance — never silently coerce.

See `references/examples.md` for worked examples across every dialect and a cookbook for tricky cases (multi-file refactors, partial reverts, revert-of-revert, mixed-language repos).

## Step 4: Self-validation checklist

Run through this checklist on the drafted message. Fix any failure before committing.

- [ ] Type is in the project's allowed set (or the dialect's default set).
- [ ] Scope is in the project's scope list, or correctly omitted.
- [ ] Subject is imperative, lowercase-leading, no period, ≤ 72 chars (≤ 50 preferred).
- [ ] "If applied, this commit will [subject]" parses as English.
- [ ] Body (if present) explains *why*, blank line after subject, wrapped at 72.
- [ ] Footers (if present) are RFC-2822 trailers, one per line.
- [ ] No manually written `Co-Authored-By:` or generation banners.
- [ ] If `!` is in the header, a `BREAKING CHANGE:` footer is present.
- [ ] The staged diff is one logical change (Step 1 honoured, or the `/implement` sentinel applied).
- [ ] If `.claude/commit-conventions.toml` defines `ban_subjects`, the subject is not in that list.
- [ ] If `core.hooksPath` is set (e.g. `.githooks/`), be aware that the commit will run that directory's `pre-commit` — its failure aborts the commit and may emit diagnostics unrelated to the message.

If `commitlint` is installed, pipe the draft through it as a final check: `echo "<msg>" | npx --no-install commitlint`.

## Anti-patterns

Refuse to produce these:

- Subjects like `wip`, `fix`, `update`, `changes`, `stuff`, `misc`.
- `address review comments` without naming which.
- `feat: improvements` — type without a real subject.
- Mixing unrelated changes into one commit.
- Manually adding the AI attribution trailer.
- Subjects or bodies that restate the diff in prose.
- Past tense (`fixed`, `added`) or third person (`fixes`, `adds`).
- Subjects > 72 chars.
- Over-scoping like `feat(src/components/ui/forms/inputs/text)` — keep scope to one or two segments.

If the only message that fits is an anti-pattern, the **commit** is wrong, not the message. Split it.

## Breaking changes

Two markers, both required:

1. `!` after the type/scope in the header.
2. `BREAKING CHANGE:` footer describing the migration path.

```
feat(api)!: rename `getUser` to `fetchUser`

The old name conflicted with the cache layer's `get*` getters. The
new name is consistent with the `fetch*` pattern used for I/O calls
elsewhere in the module.

BREAKING CHANGE: clients calling `getUser` must migrate to
`fetchUser`. Behavior is identical; a codemod is available at
scripts/codemods/rename-getUser.sh.
```

Release tooling (semantic-release, release-please) consumes this footer to compute a MAJOR bump and populate the changelog. Make it useful.

## PR descriptions

When opening a PR (`gh pr create` or equivalent): open with one sentence on user-visible impact; list notable commits when the branch has > 3; call out anything reviewers should scrutinise (perf, security, migration); link the issue with `Fixes: #N`; do not append AI attribution (controlled by `attribution.pr` in settings). For the full PR-body shape, test-plan checklist convention, and worked examples, see `references/pr-descriptions.md`.

## Installation

This skill activates at `~/.claude/skills/commit-conventions/` (user-level) or `<repo>/.claude/skills/commit-conventions/` (project-level, overrides user-level on naming collision). Symlink or copy the directory; the frontmatter is identical in both cases.
