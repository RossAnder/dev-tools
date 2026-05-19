# Plan: Commit-Conventions Skill

**Plan path**: `docs/plans/precious-frolicking-steele.md`
**Created**: 2026-05-19
**Status**: Draft

## Context

A claude.ai chat produced an initial sample for a `commit-conventions` skill at `C:\Users\rossa\Downloads\files(1)\` (a `SKILL.md`, an `examples.md`, and a `scopes.txt`). The sample is already model-discoverable in shape and covers Conventional Commits, atomicity gating, RFC-2822 trailers, and a self-validation checklist — but it stops short of three things needed to land the skill in `dev-tools`:

1. **Polyglot convention detection.** The sample resolves project conventions from `commitlint.config.js` → skill-local `scopes.txt` → `CLAUDE.md` → directory inference. It does not look at the project's actual commit history, doesn't classify the dialect (Conventional Commits vs gitmoji vs plain vs custom regex), and has no Claude-native config file that can be written by `/commit` or read by `tomlctl`.
2. **Slash-command + auto-trigger surface.** The sample is model-discoverable only. The user wants both — a `/commit` slash command that wraps the skill (for explicit invocation against an already-staged tree) AND auto-discovery on the model's intent to run `git commit` / `gh pr create`.
3. **Integration with the existing harness.** `/implement` Phase 2 step 5 runs `git commit` as a checkpoint between dependent batches and is the only first-party command that auto-commits. The skill must be referenced from there so harness-driven commits inherit the same atomicity gate and message style as user-driven commits.

This plan ships the skill into `claude/skills/commit-conventions/` so it can be symlinked or copied to either `~/.claude/skills/commit-conventions/` (user-level) or `<repo>/.claude/skills/commit-conventions/` (project-level).

## Scope

- **In scope**:
  - New skill at `claude/skills/commit-conventions/` with `SKILL.md`, `references/{examples,detection,pr-descriptions}.md`, and a `templates/commit-conventions.toml.example`.
  - New slash command `claude/commands/commit.md` — thin wrapper that runs the skill against the staged tree and (with confirmation) invokes `git commit`.
  - One-line cross-reference in `claude/commands/implement.md` at Phase 2 step 5 (git checkpoint) pointing at the skill.
  - Brief Skills subsection in `CLAUDE.md` mentioning the skill exists and where its config lives.
  - **Polyglot detection coverage**: TypeScript / Vue / React (commitlint, commitizen, semantic-release, release-please, changesets), Rust (git-cliff, cargo-release), .NET / C# (GitVersion, Nerdbank.GitVersioning), and universal git-native (`commit.template`). Vocab vs presence-signal distinction documented per tool in `references/detection.md`.
- **Out of scope**:
  - Changing `/optimise-apply` or `/review-apply` — both explicitly say "No auto-commit" (carriers do not invoke `git commit`), so no integration is needed there. Users invoking the skill manually still benefit; no carrier edit required.
  - Replacing `commitlint`. The skill reads commitlint config as one input but does not aim to subsume it.
  - PR-template authoring beyond the short PR-description reference doc.
  - Mutating `~/.claude/skills/` from the planning command. Installation to user-level is a documented one-liner the user runs themselves.
- **Affected areas**: `claude/skills/**`, `claude/commands/{commit,implement}.md`, `CLAUDE.md`.
- **Estimated file count**: 7 unique files (5 new, 2 edited).

## Research Notes

_No directed research was launched — the initial sample bundle already encodes the Conventional Commits spec, the atomicity discipline, and the RFC-2822 trailer convention. Inputs that informed the design were all read directly from the repo or the samples directory:_

| Source | Finding | Impact on plan |
|---|---|---|
| `C:\Users\rossa\Downloads\files(1)\SKILL.md` | Sample resolves project conventions from `commitlint.config.{js,…}` → skill-local `scopes.txt` → `CLAUDE.md` → directory inference. Has type table, subject rules, atomicity gate, anti-pattern list, breaking-change protocol. | Use as the spine of the new SKILL.md. Polish, but don't rewrite. |
| `C:\Users\rossa\Downloads\files(1)\examples.md` | Worked good/bad pairs across fix, feat, breaking, revert, refactor; common anti-patterns + rewrites; tricky cases (multi-scope refactor, revert-of-revert, tiny commit needing a body). | Move verbatim to `references/examples.md`. No content change needed. |
| `C:\Users\rossa\Downloads\files(1)\scopes.txt` | Newline-delimited scope list with `#` comments; placeholder values. | Convert into `templates/commit-conventions.toml.example` — TOML is the Claude-native config shape per User Decision 2. Keep the line-by-line `scopes.txt` discoverability as a secondary fallback in detection (commitlint users may already maintain it). |
| `claude/skills/test-author/SKILL.md` | Model-discoverable skill, no slash command, polyglot via 5-step framework-detection precedence; everything inline (no `references/` folder — CLAUDE.md:44 says so explicitly). | Mirror the model-discoverability and numbered-precedence patterns. The `references/<topic>.md` IA used by this plan is borrowed from Anthropic's published skill conventions, not test-author. |
| `claude/skills/tomlctl/SKILL.md` | Authoritative-doc pattern (top-level `README.md` defers to SKILL.md). Frontmatter `description` is dense and trigger-rich. | Frontmatter description should enumerate trigger surfaces (`git commit`, `git commit --amend`, `gh pr create`, harness-driven commits) so the model auto-discovers reliably. |
| `claude/commands/implement.md:444` | Phase 2 step 5 is the single first-party site that runs `git commit` — between dependent batches as a checkpoint. Failure path: "If `git commit` fails (e.g. a pre-commit hook rejects the change): do NOT proceed to step 5b". | Cross-reference is a one-line insertion at step 5 BEFORE the actual commit command. Does not touch any `SHARED-BLOCK:*` content, so `verify-shared-blocks.sh` parity is unaffected. |
| `claude/commands/{optimise-apply,review-apply}.md` (lines 742 / 777) | Both explicitly say "No auto-commit. The orchestrator does not invoke `git commit`." Both are SHARED-BLOCK carriers (`apply-constraints` block). | Do NOT edit either file — the skill applies when the user manually commits the resolved changes, but the carriers themselves don't run commits and shouldn't be touched. |
| `claude/commands/tdd.md` | No `git commit` invocations. `/tdd` dispatches `/implement` for the GREEN phase; commits happen transitively via `/implement` Phase 2 step 5. | Hooking `/implement` covers `/tdd` transitively. No `tdd.md` edit needed. |
| `CLAUDE.md` (repo root) | Documents `.githooks/pre-commit` running `scripts/verify-shared-blocks.sh` on every commit; shared-block parity covers `claude/commands/{optimise,review,…,implement,tdd}.md` and `claude/agents/flow-implement-{deep,lite}.md`. | The `implement.md` cross-reference edit must land OUTSIDE the two shared blocks present in that file (`flow-context` lines 6-36, `execution-record-schema` lines 81-266). `apply-*` lives in `optimise-apply.md` / `review-apply.md`; `forbidden-working-tree-ops` lives in `flow-implement-{deep,lite}.md`. Step 5 prose is outside all of them — safe. |

## User Decisions

### Phase 4 answers

1. **Invocation surface**: _Both — add `/commit` wrapper_ (model-discoverable AND a slash command). — prompted by the sample's silence on slash-command surface and the test-author precedent of model-only.
2. **Config location**: _`.claude/commit-conventions.toml`_. The repo's TOML-via-`tomlctl` convention applies; `scopes.txt` remains a secondary fallback in detection (for projects already using commitlint-adjacent tooling) but is not the primary write target. — prompted by the sample mixing `scopes.txt` and `CLAUDE.md` mentions, and dev-tools' preference for `.claude/`-rooted TOML.
3. **Flow hook**: _Explicit cross-reference_ in `/implement` Phase 2 step 5. — prompted by `claude/commands/implement.md:444` being the only first-party commit site.

### Phase 5 outcome

_Phase 5 (directed research) skipped — the answers above add no library/API topics that weren't already covered by the sample inputs and existing repo conventions._

## Approach

### Information architecture

```
claude/skills/commit-conventions/
├── SKILL.md                              # Model-discoverable entry point
├── references/
│   ├── examples.md                       # Worked good/bad commit examples (from sample)
│   ├── detection.md                      # Polyglot convention-detection algorithm
│   └── pr-descriptions.md                # PR-description rules
└── templates/
    └── commit-conventions.toml.example   # Per-project config template

claude/commands/
├── commit.md                             # NEW: thin /commit wrapper
└── implement.md                          # EDIT: single-line cross-reference at Phase 2 step 5

CLAUDE.md                                 # EDIT: brief Skills subsection
```

### SKILL.md design (the spine)

1. **Frontmatter description** uses imperative + negative-constraint phrasing for reliable auto-discovery. Community measurement (650-trial study) shows passive enumeration ("Use before invoking…") activates ~50% of the time; imperative + negative-constraint phrasing reaches ~100%. Draft text: _"ALWAYS invoke before drafting any `git commit`, `git commit --amend`, `git commit -s`, or `gh pr create` message — and before harness-driven commit steps (notably `/implement` Phase 2 step 5 git checkpoint). DO NOT write commit messages directly. Resolves project dialect via polyglot precedence (`.claude/commit-conventions.toml` → commitlint → `scopes.txt` → CONTRIBUTING/CLAUDE.md → `git log` inference → default Conventional Commits)."_ Do NOT use the `paths` frontmatter field — Claude Code issue #49835 documents it as a discovery-killer.

2. **When to apply** — keep the sample's triggers; add: _the orchestrator is in `/implement` Phase 2 step 5 and is about to commit a batch; the user typed `/commit`._

3. **Step 1: Atomicity gate** — verbatim from sample (the discipline is universal). Add an explicit note that under `/implement`-driven invocation, the atomicity gate is informational only — the batch boundary is set by the dependency graph and not negotiated per-commit, so the skill reports the assessment but does not block. Under `/commit`-driven invocation, the gate is mandatory and offers the split protocol.

   **Carve-out detection (sentinel token)**: the skill detects `/implement`-driven invocation by matching the literal sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5` in the surrounding prompt context. `/implement`'s Phase 2 step 5 cross-reference (Task 7) MUST emit this token verbatim before invoking the skill; the skill matches on it before downgrading the gate. Without the token, the gate stays mandatory. This makes the carve-out an explicit contract rather than a heuristic.

4. **Step 2: Resolve project conventions** — replace the sample's flat precedence with a numbered list pointing at `references/detection.md`. New precedence:
   1. `.claude/commit-conventions.toml` — authoritative when present. Read via `tomlctl get .claude/commit-conventions.toml dialect` (and the per-dialect sub-tables).
   2. **Release / changelog / versioning tool config** (polyglot — covers TypeScript / Vue / React, Rust, .NET / C#, universal git-native). First-found wins; if multiple are present, the order below resolves ties. Two flavours: *vocab sources* export concrete `type` / `scope` vocabularies the skill respects; *CC-presence signals* pin the dialect to Conventional Commits without enumerating vocab (the skill falls through to the next precedence layer for scope vocab while keeping the dialect pinned). See `references/detection.md` for per-tool parse routines.
      - **TypeScript / JS / Vue / React** *(vocab)*: `commitlint.config.{js,cjs,mjs,ts,cts,mts}` / `.commitlintrc.{js,cjs,mjs,ts,cts,mts,json,yml,yaml}` / `package.json:commitlint` / `package.yaml:commitlint` (parse `type-enum`, `scope-enum`); `.cz.{toml,json}` / `cz.config.{js,ts,cjs}` (commitizen — parse `types` / `scopes` when the adapter exposes them); `release-please-config.json` (parse `changelog-sections[].type`).
      - **TypeScript / JS / Vue / React** *(presence)*: `.releaserc.{json,yaml,js,ts,toml}` / `release.config.{js,cjs,mjs,ts}` (semantic-release); `.changeset/config.json` (changesets — note: changeset-driven release is decoupled from commit messages, but the project still benefits from CC discipline on commits themselves).
      - **Rust** *(vocab)*: `cliff.toml` (git-cliff — parse `[git.commit_parsers]` regex → group mapping for the type vocabulary).
      - **Rust** *(presence)*: `release.toml` (cargo-release); `Cargo.toml:[package.metadata.release]` (cargo-release inline form).
      - **.NET / C#** *(presence)*: `GitVersion.yml` / `.gitversion` (GitVersion — confirms `commit-message-incrementing` semantics); `version.json` (Nerdbank.GitVersioning — weak signal but indicates commit-driven versioning).
      - **Universal** *(vocab when populated, else presence)*: `git config commit.template` resolves to a path; if the target file contains `<type>` / `<scope>` placeholders, extract them as vocab hints; otherwise treat as a presence signal.
   3. Repo-root or `.claude/`-rooted `scopes.txt` — newline-delimited scopes (back-compat with sample).
   4. `CONTRIBUTING.md` / `CLAUDE.md` — heuristic prose scan for "we use Conventional Commits" / "gitmoji" / pinned-regex blocks.
   5. **`git log` inference** — classify the last 50 commits into one of {conventional-commits, gitmoji, plain, custom}. Threshold: ≥50% match for a dialect to be claimed. If `git log` is empty (fresh repo), skip.
   6. Default — Conventional Commits, no scope restriction, type list per the sample's table.

5. **Step 3: Compose the message** — keep the sample's structure (type, scope, subject, body, footers). Add per-dialect branches:
   - **Conventional Commits** — sample rules apply.
   - **Gitmoji** — emoji or `:emoji_code:` prefix; the rest of the discipline (imperative mood, 50/72) still applies.
   - **Plain** — no prefix; subject + body discipline only.
   - **Custom regex** — message must match the project's regex; skill emits the regex back to the user when the draft fails to match.

6. **Step 4: Self-validation checklist** — sample's checklist; add one line: _"If `.claude/commit-conventions.toml` defines `ban_subjects`, the subject is not in that list."_ Also surface the pre-commit hook risk: if `core.hooksPath = .githooks/` is set, the commit will run `.githooks/pre-commit`. Mention this so the model doesn't surprised by hook failures.

7. **Anti-patterns** — sample's list, verbatim.

8. **Breaking changes** — sample's pattern (`!` + `BREAKING CHANGE:` footer), verbatim.

9. **PR descriptions** — one-paragraph summary pointing at `references/pr-descriptions.md`.

10. **Installation note** — short section at the bottom: _"This skill activates whether installed at `~/.claude/skills/commit-conventions/` (user-level, applies to every project) or `<repo>/.claude/skills/commit-conventions/` (project-level, overrides user-level on naming collision). Either symlink or copy the directory; `SKILL.md`'s frontmatter is identical in both cases."_

### `references/detection.md` (NEW)

Standalone reference covering polyglot detection in full detail. The SKILL.md numbered precedence list cites this doc rather than inlining the regex.

Content:
- Precedence list expanded with rationale and edge cases.
- For each commitlint config flavour: a minimal parse routine (e.g. `node -e "console.log(JSON.stringify(require('./commitlint.config.js').rules))"` to extract enums on JS projects; the model can read the file directly for static JSON formats).
- `git log` dialect classifier: regex tables for the four dialects, count-based threshold rule, tie-breaking (latest dialect wins on a tie).
- `.claude/commit-conventions.toml` schema with worked example.
- Failure mode: when no signal is detected and `git log` is empty, default to Conventional Commits and emit a one-line console notice _"commit-conventions: no project signal — defaulting to Conventional Commits. Add `.claude/commit-conventions.toml` to override."_

### `references/examples.md` (NEW — content from sample)

Move `C:\Users\rossa\Downloads\files(1)\examples.md` content verbatim. Add an "Other dialects" section at the bottom with one gitmoji example and one plain-style example, so the doc isn't Conventional-Commits-only.

### `references/pr-descriptions.md` (NEW)

Short reference (≤80 lines) covering:
- One-sentence user-visible-impact opener.
- Commit-list rendering (when >3 commits).
- Reviewer-attention call-outs (perf, security, migration).
- `Fixes: #N` linkage.
- Attribution comes from `attribution.pr` setting, not the body.

### `templates/commit-conventions.toml.example` (NEW)

```toml
# Per-project commit-conventions config. Copy to .claude/commit-conventions.toml and edit.
# Read by the commit-conventions skill in priority order; overrides commitlint and git-log inference.

dialect = "conventional-commits"  # one of: conventional-commits, gitmoji, plain, custom

[conventional-commits]
allowed_types = ["feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "revert", "style"]
allowed_scopes = ["api", "auth", "db", "ui", "cli", "docs", "ci", "deps"]
require_scope = false
require_body_for = ["feat", "perf"]  # types that always need a body
subject_max_length = 72
subject_preferred_length = 50
body_wrap_at = 72
ban_subjects = ["wip", "fix", "update", "changes", "stuff", "misc", "address review comments"]

[gitmoji]
# Only used when dialect = "gitmoji". Set the emoji vocabulary you allow.
allowed_emoji = [":sparkles:", ":bug:", ":zap:", ":recycle:", ":memo:", ":white_check_mark:"]

[custom]
# Only used when dialect = "custom".
header_regex = '^\[[A-Z]+-[0-9]+\]\s.+'  # e.g. "[PROJ-123] subject"
```

The skill reads the active `[<dialect>]` sub-table only. The TOML is intentionally ecosystem-agnostic — projects that already maintain `commitlint.config.js`, `cliff.toml`, `GitVersion.yml`, or another release-tool config don't need this TOML; it exists as an override / first-precedence escape hatch for cases where the detected vocab is wrong.

### `claude/commands/commit.md` (NEW slash command)

Thin orchestrator. Phases:

1. **Pre-flight** — run `git diff --cached --stat`. If no staged changes, run `git status` and halt with a message.
2. **Invoke the skill** — apply the commit-conventions skill against the staged tree:
   - Run the atomicity gate from SKILL.md Step 1. If multi-concern, offer the split protocol via `AskUserQuestion` (commit anyway / split now / abort).
   - Resolve conventions via the detection precedence.
   - Draft a message per the dialect's rules.
3. **Confirm & commit** — present the drafted message to the user. On approval, invoke `git commit -F -` with the drafted message on stdin via a single-quoted bash heredoc (`git commit -F - <<'COMMIT_MSG'\n<subject>\n\n<body>\nCOMMIT_MSG`). The literal HEREDOC form is pinned here because neither `~/.claude/CLAUDE.md` nor the repo `CLAUDE.md` documents one — the pattern lives in Claude Code's built-in commit guidance. On rejection, offer a single round of revision.
4. **Hook-failure handling** — if `git commit` exits non-zero (pre-commit hook rejected), surface stderr verbatim, halt without retry, and explicitly do NOT suggest `--no-verify` if the staged set intersects `scripts/shared-blocks.toml` carriers (mirrors `/implement` Phase 2 step 5 halt protocol).
5. **Post-commit** — print the committed SHA. If the user ran `/commit` from inside an active flow (detect via a cheap `tomlctl flow resolve` call in Phase 1 pre-flight), emit a warning ("active flow `<slug>` detected; commit will not appear in execution-record.toml — consider `/implement` instead") and require explicit user confirmation before proceeding. `/commit` never writes to `execution-record.toml`; the explicit warning makes the resulting commit-history vs execution-record divergence visible rather than silent.

The command is short — ≤100 lines including the prompt-allowlist note. It does NOT carry flow-bootstrap (no flow resolution needed) and is not a shared-block carrier. The ≤100 LOC ceiling matches the carrier skeleton format being established by `docs/plans/harness-progressive-disclosure.md` (pilot on `/review`) — `/commit` is therefore born already-compliant and will not need a later rewrite when progressive disclosure propagates.

### `claude/commands/implement.md` edit

One-line insertion at Phase 2 step 5 (line ~444), BEFORE the "stage and commit" sentence:

> _Before invoking `git commit`, apply the `commit-conventions` skill if installed (`~/.claude/skills/commit-conventions/` or `<repo>/.claude/skills/commit-conventions/`) — the skill drafts a Conventional-Commits-compliant message from the staged diff. Emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5` in the dispatch prose so the skill matches on it and downgrades the atomicity gate to informational only; the batch boundary is fixed by the dependency graph._

This edit lands OUTSIDE all four shared-block ranges in `implement.md`. `scripts/verify-shared-blocks.sh` parity remains intact. Verify with `bash scripts/verify-shared-blocks.sh` after the edit.

> **Forward-compatibility note (re-anchor obligation)**: `docs/plans/harness-progressive-disclosure.md` plans to eventually rewrite `claude/commands/implement.md` into a ≤100 LOC skeleton (deferred — pilot is `/review` only). When that rewrite lands, the "stage and commit" prose anchor used by this task disappears. At that point the cross-reference must be re-expressed as part of the new Phase 2 prose summary OR moved into the contract skill that handles the git checkpoint. The follow-up plan that overhauls `/implement` is responsible for re-anchoring this cross-reference; until then the line-444 form documented here is correct and safe.

### `CLAUDE.md` edit

Add a `## Commit conventions` section after `## Testing discipline` (and before `## Flow registry & plansDirectory`), pointing at the new skill. The repo's other skill (`test-author`) is documented in-place under `## Testing discipline:### test-author skill` (CLAUDE.md:42-44) — leave it there rather than consolidating, since each skill is most discoverable next to its functional domain.

> _**`commit-conventions`** — Model-discoverable skill that drafts commit messages and PR descriptions per the project's resolved convention (Conventional Commits, gitmoji, plain, or custom regex). Lives at `claude/skills/commit-conventions/`. Per-project config at `.claude/commit-conventions.toml`. Also invocable as `/commit`._

### Out-of-band install path

The skill ships in this repo at `claude/skills/commit-conventions/`. To use:

```bash
# User-level (applies everywhere)
cp -r claude/skills/commit-conventions ~/.claude/skills/

# Project-level (per-repo, overrides user-level)
cp -r claude/skills/commit-conventions <other-repo>/.claude/skills/
```

The plan does not perform this copy — that is a per-user operational step.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
shared-blocks: bash scripts/verify-shared-blocks.sh
```

There is no automated test target specifically for the skill (skills are markdown — frontmatter and prose). Manual verification is in the `## Verification` section.

## Tasks

### Phase 1: Author skill content (parallel — all new files)

#### 1. Write `claude/skills/commit-conventions/SKILL.md` [M]
- **Files**: `claude/skills/commit-conventions/SKILL.md`
- **Depends on**: —
- **Action**: Author the SKILL.md per the "SKILL.md design" subsection of `## Approach`. Spine = sample SKILL.md; replace Step 2 (convention resolution) with the 6-entry numbered precedence; add per-dialect branches in Step 3; add the `.claude/commit-conventions.toml` reference; add the installation note.
- **Detail**: Frontmatter `name: commit-conventions`; `description` uses imperative + negative-constraint phrasing (see SKILL.md design item 1 for the literal draft). Do NOT use the `paths` frontmatter field. Required heading skeleton (verbatim order): `# Commit Conventions` (H1), then H2 sections — `## When to apply` / `## Step 1: Atomicity gate` / `## Step 2: Resolve project conventions` / `## Step 3: Compose the message` / `## Step 4: Self-validation checklist` / `## Anti-patterns` / `## Breaking changes` / `## PR descriptions` / `## Installation`. Body cites `references/detection.md` for the detection algorithm, `references/examples.md` for examples, `references/pr-descriptions.md` for PR rules. Do NOT inline the full detection algorithm into SKILL.md — keep it in `detection.md` to control SKILL.md length.
- **Acceptance**: File parses as valid YAML frontmatter + markdown (`python -c "import yaml,re; yaml.safe_load(re.match(r'---\n(.*?)\n---', open(p).read(), re.S).group(1))"` exits 0). The frontmatter `name` matches the directory name. The frontmatter `description` begins with an imperative verb (regex `^(ALWAYS|Invoke|Use)` is informational; reviewer judgement decides imperative-vs-passive). All 9 required H2 headings present (grep). `references/*.md` paths cited are all created in tasks 2-4. Word count ≤1500 (the sample is ~1300).

#### 2. Write `claude/skills/commit-conventions/references/examples.md` [S]
- **Files**: `claude/skills/commit-conventions/references/examples.md`
- **Depends on**: —
- **Action**: Use the `Read` tool with the absolute path `C:\Users\rossa\Downloads\files(1)\examples.md` (this is OUTSIDE the repo — `Read` accepts absolute Windows paths; do NOT use shell `cp` which fails on Windows-path quoting in this environment). Copy the file's contents verbatim into the target file at `claude/skills/commit-conventions/references/examples.md`. Then append a new "## Other dialects" section with one gitmoji example (e.g. `:sparkles: add OAuth2 PKCE flow for mobile clients`) and one plain-style example (e.g. `add OAuth2 PKCE flow for mobile clients` with body explaining why).
- **Acceptance**: File exists; contains every example from the sample plus the two new dialect examples; no line exceeds 200 columns (`python -c "import pathlib; assert all(len(l) <= 200 for l in pathlib.Path(p).read_text().splitlines())"`).

#### 3. Write `claude/skills/commit-conventions/references/detection.md` [M]
- **Files**: `claude/skills/commit-conventions/references/detection.md`
- **Depends on**: —
- **Action**: Author the polyglot detection reference per the "`references/detection.md`" subsection of `## Approach`. Cover all 6 precedence entries with rationale, edge cases, and (for `git log` inference) the four dialect-classifying regexes with the 50% threshold.
- **Detail**: Required heading skeleton (verbatim order): `# Commit-Conventions Detection` (H1), then H2 sections — `## Precedence overview` / `## 1. .claude/commit-conventions.toml` / `## 2. Release/changelog/versioning tool config (polyglot)` / `## 3. scopes.txt` / `## 4. CONTRIBUTING.md / CLAUDE.md heuristic scan` / `## 5. git log dialect inference` / `## 6. Default fallback` / `## TOML schema`. The `## 2.` section MUST contain H3 sub-sections per ecosystem: `### 2.1 TypeScript / Vue / React` / `### 2.2 Rust` / `### 2.3 .NET / C#` / `### 2.4 Universal (git config commit.template)`.
  - **TypeScript / Vue / React** parse routines:
    - `commitlint.config.{js,cjs,mjs,ts,cts,mts}` — static-read (model reads file as text) preferred; dynamic-eval fallback `node -e "console.log(JSON.stringify(require('./commitlint.config.js').rules))"` only when static read fails. Dynamic eval requires running JS in the project's directory and may pull dev dependencies — call out this side effect.
    - `.commitlintrc.{json,yml,yaml}` — static read + standard JSON/YAML parse.
    - `package.json:commitlint` — read `package.json`, navigate to `commitlint` key.
    - `package.yaml:commitlint` — same but YAML.
    - `.cz.toml` / `.cz.json` / `cz.config.{js,ts,cjs}` — parse `types` (array of objects with `type` field) and `scopes` (array of strings) when present; some adapters (e.g. `cz-conventional-changelog`) inherit defaults — treat absence of `types` as "use cz-conventional-changelog defaults = CC types".
    - `release-please-config.json` — parse `packages.*.changelog-sections[]` (or root `changelog-sections[]` for single-package); each entry's `type` field contributes to the type vocab.
    - `.releaserc.{...}` / `release.config.{...}` — presence-only; do not parse plugin configs. Pin dialect to `conventional-commits`.
    - `.changeset/config.json` — presence-only; emit a one-line console notice: `commit-conventions: changesets detected — release flow is decoupled from commits, but CC discipline still applies to the staged change.`
  - **Rust** parse routines:
    - `cliff.toml` — TOML; read `[git]` table's `commit_parsers` array; each entry has `message` (regex) and `group` (type-or-skip). The set of `group` values across non-`skip` entries forms the type vocab. Worked example for the dev-tools repo would parse the canonical `git-cliff` default config.
    - `release.toml` — presence-only; pin dialect to `conventional-commits`. Optionally parse `pre-release-commit-message` template for scope hints.
    - `Cargo.toml:[package.metadata.release]` — presence-only; same treatment as `release.toml`.
  - **.NET / C#** parse routines:
    - `GitVersion.yml` / `.gitversion` — YAML; check for the presence of `mode: ContinuousDelivery` or `commit-message-incrementing` keys to confirm CC discipline. Presence-only.
    - `version.json` — JSON; presence of `nbgvAdditionalFiles` / `version` fields confirms Nerdbank.GitVersioning. Weak signal — combine with `git log` inference if no stronger vocab source surfaces.
  - **Universal** parse routine:
    - Run `git config --get commit.template` to resolve the template path. If empty or the file doesn't exist, skip. Otherwise read the template; scan for `<type>`, `<scope>`, `(type)`, `(scope)` patterns. If patterns present, extract as vocab hints; otherwise treat as presence signal.
  - **Tie-breaking when multiple tools are present**: walk the ecosystem order (TS/Vue/React → Rust → .NET/C# → Universal) and within each ecosystem the order listed above. First vocab source wins; presence signals from later-checked tools only contribute the dialect pin if no earlier vocab source pinned it. Emit a one-line console notice naming the tool whose vocab was adopted: `commit-conventions: vocab source = <tool>; <N> additional config files ignored (lower precedence).`
  - For `git log` inference (precedence 5), show the exact commands (`git log -n 50 --pretty=format:%s`) and the classification regexes for the four dialects.
  - Include the schema for `.claude/commit-conventions.toml` with worked examples for each dialect.
  - Include the "no signal" fallback notice text verbatim so SKILL.md can quote it.
  - `scopes.txt` parse contract: lookup order `./scopes.txt` then `.claude/scopes.txt`, first-found wins; format is newline-delimited scopes with `#` introducing line comments and blank lines ignored. Trim whitespace per line.
- **Acceptance**: File exists; all 8 required H2 headings present (grep); all 4 ecosystem H3 sub-sections under `## 2.` present (grep); each precedence entry has a parse routine; the four dialect regexes are present and unambiguous; word count ≤3500 (raised from 2500 to accommodate polyglot parse routines).

#### 4. Write `claude/skills/commit-conventions/references/pr-descriptions.md` [S]
- **Files**: `claude/skills/commit-conventions/references/pr-descriptions.md`
- **Depends on**: —
- **Action**: Author the short PR-description reference per the "`references/pr-descriptions.md`" subsection of `## Approach`. Cover: opener sentence, commit-list rendering, reviewer-attention call-outs, `Fixes: #N`, attribution-via-settings note (no manual `Co-Authored-By:` or generation banners).
- **Acceptance**: File exists; ≤80 lines of markdown; no line exceeds 200 columns; opens with an H1 (`# ...`).

#### 5. Write `claude/skills/commit-conventions/templates/commit-conventions.toml.example` [S]
- **Files**: `claude/skills/commit-conventions/templates/commit-conventions.toml.example`
- **Depends on**: —
- **Action**: Author the example config per the "`templates/commit-conventions.toml.example`" subsection of `## Approach`. Include `dialect`, all four per-dialect sub-tables, and comments explaining each field.
- **Acceptance**: File parses as valid TOML (`tomlctl get <file> dialect` returns the value); every field has a one-line comment.

#### 6. Write `claude/commands/commit.md` [M]
- **Files**: `claude/commands/commit.md`
- **Depends on**: 1 (SKILL.md must define the skill the command wraps)
- **Action**: Author the `/commit` slash command per the "`claude/commands/commit.md`" subsection of `## Approach`. Four phases (pre-flight, invoke skill, confirm & commit, post-commit). No flow-bootstrap. Uses `AskUserQuestion` for the atomicity-split prompt (options: `Commit anyway` → bypass gate and proceed; `Split now` → halt and instruct the user to re-stage; `Abort` → halt with no commit) and for the final commit confirmation (options: `Approve` → run `git commit -F -`; `Revise` → re-draft once based on user free-text; `Cancel` → halt). Halts surface stderr verbatim and exit cleanly; no retries beyond the single revision round.
- **Detail**: Reference the skill by name (`commit-conventions`) and instruct the orchestrator to dispatch the skill's Step 1-4 procedure. The command does NOT redefine commit-message rules — it delegates entirely to SKILL.md. The HEREDOC form for `git commit` is pinned in the `## Approach` subsection — copy that form verbatim; do NOT reference any external "global commit guidance" (none exists in either CLAUDE.md).
- **Acceptance**: File exists; `wc -l` ≤100 (excluding example HEREDOCs); cites the skill explicitly by name (grep for `commit-conventions`); does not duplicate convention rules (no occurrences of `Conventional Commits` spec text body); opens with an H1.

### Phase 2: Wire into existing harness (after Phase 1)

#### 7. Edit `claude/commands/implement.md` to cross-reference the skill [S]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 1
- **Action**: Insert the one-line cross-reference paragraph at Phase 2 step 5 (around line 444), BEFORE the "stage and commit the current batch's changes" sentence. The literal anchor to insert BEFORE is `**Git checkpoint**: If there are subsequent batches that depend on this one, stage and commit the current batch's changes` (a 14-word substring confirmed unique in the file as of plan-write date — use this rather than line 444 if drift has moved the line). Text per the "`claude/commands/implement.md` edit" subsection of `## Approach` (includes the sentinel token `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`). Run `bash scripts/verify-shared-blocks.sh` BEFORE staging the edit (dry-run check) and again after staging to confirm parity.
- **Detail**: The edit MUST land outside both `SHARED-BLOCK:*` ranges present in this file (`flow-context` lines 6-36, `execution-record-schema` lines 81-266). Step 5 prose at line ~444 sits well outside both. Confirm with `bash scripts/verify-shared-blocks.sh` after the edit.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0. `grep -n "commit-conventions" claude/commands/implement.md` returns the inserted line. The "stage and commit" prose immediately following remains semantically intact.

#### 8. Edit `CLAUDE.md` to document the skill [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 1
- **Action**: Add a `## Skills` subsection after the existing `## Testing discipline` section (and before `## Flow registry & plansDirectory`). Content per the "`CLAUDE.md` edit" subsection of `## Approach`.
- **Acceptance**: `grep -n "commit-conventions" CLAUDE.md` returns the inserted line; the section is in the correct location (between Testing discipline and Flow registry); markdown structure (heading levels) is consistent.

## Dependency Graph

```
Batch 1 (parallel, no file overlap): Tasks 1, 2, 3, 4, 5
Batch 2 (parallel, after Batch 1):   Tasks 6, 7, 8
```

Tasks 6, 7, 8 all wait on Task 1 (the SKILL.md), but neither modifies the same file as another, so they parallelize once Task 1 lands.

## Verification

End-to-end:

1. **Frontmatter validity** — `python -c "import yaml, re; s=open('claude/skills/commit-conventions/SKILL.md').read(); fm=re.match(r'---\n(.*?)\n---', s, re.S).group(1); yaml.safe_load(fm)"` (or equivalent) — parses without error.
2. **Skill registry** — start a fresh Claude Code session in this repo and check that `commit-conventions` appears in the skills list and is discoverable by a request like "draft a commit message for the staged changes".
3. **Convention detection paths** — for each detection source, set up a minimal fixture and confirm the skill picks the expected dialect AND the expected vocab:
   - Drop a `.claude/commit-conventions.toml` with `dialect = "gitmoji"` → skill emits gitmoji draft.
   - **TS/Vue/React fixture**: a `commitlint.config.js` with custom `type-enum` `['feat', 'fix', 'patch']` → skill emits CC draft restricted to those three types. Repeat with `.cz.json` containing custom `types` → same. Repeat with `release-please-config.json` containing `changelog-sections` → same. Repeat with `.releaserc.json` (presence only) → skill emits CC, falls through to `scopes.txt`/`git log` for scope.
   - **Rust fixture**: a `cliff.toml` with non-default `commit_parsers` (e.g. group `epic` for `^EPIC:`) → skill recognises `epic` as a valid type. Repeat with bare `release.toml` (presence only) → skill emits CC, falls through.
   - **C# fixture**: a `GitVersion.yml` with `mode: ContinuousDelivery` → skill emits CC, falls through for scope vocab. Repeat with `version.json` (Nerdbank.GitVersioning) → same.
   - **Universal fixture**: set `git config commit.template /tmp/msg.txt` with content `<type>(<scope>): subject` → skill extracts `<type>` / `<scope>` as vocab hints; CC dialect.
   - Remove all of the above; rely on `git log` inference in a repo whose history is Conventional → skill picks Conventional.
   - Fresh repo with no history and no config → skill emits the "defaulting to Conventional Commits" notice.
4. **Atomicity gate** — stage a multi-concern change; run `/commit`; confirm the split protocol fires with an `AskUserQuestion`.
5. **`/implement` integration** — run `/implement` on a small flow with two batches; confirm Phase 2 step 5 logs an invocation of the commit-conventions skill before the `git commit` command.
6. **Shared-block parity** — `bash scripts/verify-shared-blocks.sh` exits 0 after the `implement.md` edit.
7. **`tomlctl` smoke** — `tomlctl get claude/skills/commit-conventions/templates/commit-conventions.toml.example dialect` returns `"conventional-commits"` (the template is a parseable TOML).
8. **Lint / build / test (unchanged scope, sanity)** — `cargo build --manifest-path tomlctl/Cargo.toml`, `cargo test --manifest-path tomlctl/Cargo.toml`, `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` all pass (no source changes; expected to be already passing).

Manual verification is the primary gate; the repo's `/review` Agent 6 (the package-quality lens — fires conditionally on any reviewed scope under `claude/commands/` or `claude/skills/`) provides automated frontmatter / structural / trigger-coverage / shared-block-compliance checks. Invoke `/review claude/skills/commit-conventions/ claude/commands/commit.md` post-merge. Citation by lens name rather than line range to survive the `/review` carrier rewrite landing under `docs/plans/harness-progressive-disclosure.md`.

## Risks

- **Risk**: The polyglot detection's `git log` inference misclassifies a project that mixes dialects (e.g. early plain commits, later Conventional). **Mitigation**: The detection precedence puts `.claude/commit-conventions.toml` first; the 50% threshold for `git log` inference is documented in `references/detection.md` so users can predict the call. When the user disagrees, they drop the TOML file and the heuristic is bypassed.
- **Risk**: The `commit-conventions` skill conflicts with an existing project's commitlint config (different scope vocab). **Mitigation**: Precedence puts `commitlint.config.*` ahead of the skill's defaults — projects that already run commitlint inherit their config without further configuration.
- **Risk**: `/implement`-driven commits trigger the atomicity-split prompt and stall the harness. **Mitigation**: SKILL.md Step 1 explicitly carves out `/implement`-driven invocation: the atomicity gate is informational only in that mode; the batch boundary is fixed by the dependency graph.
- **Risk**: The `implement.md` edit accidentally lands inside a shared block, breaking parity. **Mitigation**: The cross-reference is a single short paragraph inserted at Phase 2 step 5 prose, which sits outside all four shared blocks in the file. Task 7 acceptance gates on `verify-shared-blocks.sh` exit 0.
- **Risk**: The skill's auto-discovery fires on commit-adjacent operations that should NOT be gated (e.g. `git stash`, `git rebase --continue`). **Mitigation**: SKILL.md frontmatter `description` and "When to apply" enumerate only `git commit` / `git commit --amend` / `gh pr create` — neither `git stash` nor `git rebase` is in the trigger list. The model honours the explicit trigger list.
- **Risk**: The `/commit` command runs `git commit` without the user expecting an actual filesystem mutation. **Mitigation**: Phase 3 (Confirm & commit) of the command requires an `AskUserQuestion` approval before invoking `git commit`. No silent commit.
- **Risk**: Project ships its own `commit-conventions` skill at user-level that diverges from the per-project version. **Mitigation**: Per Anthropic's documented skill-loading precedence, `<repo>/.claude/skills/` overrides `~/.claude/skills/` on naming collision; verify against current Claude Code skill docs at install time. Cloned repos can therefore silently override a user-level skill — review project-level `.claude/skills/` content before first commit on a new clone (mirrors the supply-chain note in `CLAUDE.md` Developer setup).
- **Risk (polyglot tool conflicts)**: A project ships multiple release/changelog tool configs that disagree on vocab — e.g. a TS monorepo with both `commitlint.config.js` (one type set) and `release-please-config.json` (different `changelog-sections[].type`). The skill must pick deterministically rather than merging or warning-and-halting. **Mitigation**: Detection.md tie-breaking is documented and deterministic — ecosystem order (TS → Rust → .NET → Universal) and within-ecosystem order (commitlint → commitizen → release-please for TS; cliff → release.toml for Rust; GitVersion → Nerdbank for .NET). First vocab source wins; the skill emits a one-line console notice naming the source adopted so the user can override via `.claude/commit-conventions.toml` if the inference is wrong.
- **Risk (false positive on cargo-release in dev-tools)**: This repo is a Rust workspace and may eventually adopt `cargo-release` for tomlctl publishing. If a `release.toml` lands without explicit CC discipline, the skill would pin the dialect to Conventional Commits regardless of the project's actual intent. **Mitigation**: `.claude/commit-conventions.toml` always wins (precedence 1); a project that does not want the auto-detection result simply commits the TOML override. Low probability — dev-tools commits already follow CC.

## Forward compatibility with `docs/plans/harness-progressive-disclosure.md`

The progressive-disclosure overhaul targets every carrier under `claude/commands/*.md`. Its pilot rewrites `/review`; remaining carriers (including `/implement`) are deferred to follow-up plans. This commit-conventions plan was reviewed against the overhaul; the four touch-points and their resolutions are recorded here so propagation does not silently invalidate this work.

1. **Task 7 (`implement.md` cross-reference)** — when `/implement` is eventually overhauled into a ≤100 LOC skeleton, the "stage and commit" line-444 anchor disappears. The follow-up plan that overhauls `/implement` MUST re-anchor the commit-conventions cross-reference into the new skeleton's Phase 2 prose summary, or move it into the contract skill that handles the git checkpoint. See the inline forward-compatibility note in Task 7's Detail block.
2. **`/review` Agent 6 verification citation** — the pilot rewrites `/review`. Verification step references Agent 6 by lens name (`package-quality`) rather than line range, so the citation survives the rewrite. The post-merge `/review claude/skills/commit-conventions/ claude/commands/commit.md` invocation remains valid because Agent 6's conditional-fire trigger (any path under `claude/skills/` or `claude/commands/`) is part of the lens's contract, not its location.
3. **`/commit` as a new carrier** — Task 6 already targets ≤100 LOC and designs `/commit` as a thin wrapper that delegates to `SKILL.md`. This is the same skeleton shape the progressive-disclosure pilot establishes. `/commit` is born compliant; no later rewrite needed when the pattern propagates.
4. **`references/<topic>.md` subfolder pattern** — the commit-conventions skill uses `claude/skills/commit-conventions/references/{examples,detection,pr-descriptions}.md`; the contract skills in the overhaul plan (`claude/skills/flow-contract-*/SKILL.md`) are flat single-file skills. The two patterns coexist for different purposes — commit-conventions is a user-facing skill with substantial reference content that benefits from progressive disclosure of its own (SKILL.md spine + on-demand reference reads), whereas contract skills carry pure block content with no internal phases. No reconciliation required. The installation note in SKILL.md calls this out.
