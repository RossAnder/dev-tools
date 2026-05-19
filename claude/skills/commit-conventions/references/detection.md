# Commit-Conventions Detection

This document defines the polyglot detection algorithm the `commit-conventions` skill uses to resolve a project's commit dialect (the message-shape convention) and its vocab (allowed types, scopes, emoji, regex). `SKILL.md` cites this document for the algorithm; the numbered precedence list in `SKILL.md` Step 2 maps directly to the sections below.

Two distinct outputs are produced:

- **Dialect** — one of `conventional-commits`, `gitmoji`, `plain`, or `custom`. Drives Step 3 message composition.
- **Vocab** — concrete sets of allowed `type` values, `scope` values, emoji, or a `header_regex`. Drives Step 4 self-validation.

A precedence layer can contribute *both* dialect and vocab (a *vocab source*), or just the dialect (a *presence signal*). When a layer is presence-only, the algorithm falls through to lower layers to fill the vocab while the higher layer's dialect pin sticks.

## Precedence overview

| # | Source | Contributes | Halt? |
|---|--------|-------------|-------|
| 1 | `.claude/commit-conventions.toml` | dialect + vocab | yes (authoritative) |
| 2 | Release / changelog / versioning tool config (polyglot) | dialect + vocab (per tool — see §2 for the vocab / presence split) | dialect pins on first hit; vocab continues until first vocab source wins |
| 3 | `scopes.txt` | scope vocab only | no |
| 4 | `CONTRIBUTING.md` / `CLAUDE.md` heuristic scan | dialect (weak) | no |
| 5 | `git log` inference | dialect (weak) | no |
| 6 | Default fallback | dialect (`conventional-commits`) + default vocab | yes |

The algorithm walks the layers top-to-bottom. Layer 1 short-circuits. Otherwise it accumulates: dialect is set by the first layer that pins one; vocab is set by the first layer that supplies one. Layer 6 always returns a result.

## 1. .claude/commit-conventions.toml

Authoritative. When present, the skill reads it and stops.

Read via `tomlctl`:

```bash
tomlctl get .claude/commit-conventions.toml dialect
tomlctl get .claude/commit-conventions.toml '<dialect>'   # sub-table for the active dialect
```

The schema and worked examples are in the [TOML schema](#toml-schema) section at the end of this document.

Rationale: this file is Claude-native — it is the only source written explicitly *for* the skill. Detection layers 2-5 are inferred from third-party tools or commit history and can be wrong; this file cannot.

Edge cases:
- File present but `dialect` missing → treat as malformed; emit `commit-conventions: .claude/commit-conventions.toml is missing the dialect key — falling through to layer 2`.
- `dialect` set to a value outside `{conventional-commits, gitmoji, plain, custom}` → halt and emit the same notice. The skill does not invent dialects.
- The sub-table for the active dialect is missing → use that dialect's default vocab (see §6 and the schema below).

## 2. Release/changelog/versioning tool config (polyglot)

Walk the four ecosystems in this order:

1. TypeScript / Vue / React
2. Rust
3. .NET / C#
4. Universal (`git config commit.template`)

Within each ecosystem, walk the tool list in the order documented in that subsection. **First vocab source wins** — once a vocab is collected, the skill stops looking for more vocab but keeps the earliest dialect pin. Presence signals from later-checked tools only contribute the dialect pin if no earlier source pinned it.

On completion of layer 2, emit a one-line notice naming the chosen vocab source (or noting that no vocab was found):

```
commit-conventions: vocab source = <tool>; <N> additional config files ignored (lower precedence).
```

### 2.1 TypeScript / Vue / React

Walked in this order:

#### `commitlint.config.{js,cjs,mjs,ts,cts,mts}` — vocab

Static read first: read the file as text and locate the `type-enum` and `scope-enum` array literals. This works for the common idiomatic shape:

```js
module.exports = {
  rules: {
    'type-enum': [2, 'always', ['feat', 'fix', 'docs', ...]],
    'scope-enum': [2, 'always', ['api', 'ui', ...]],
  },
};
```

If the file uses computed expressions, spreads, or external imports that defeat textual extraction, fall back to dynamic eval:

```bash
node -e "console.log(JSON.stringify(require('./commitlint.config.js').rules))"
```

**Side-effect call-out**: dynamic eval runs JavaScript inside the project directory. It may transitively load dev dependencies via `require`, may execute top-level side-effects in the config file, and may fail if `node_modules/` is not installed. Use static read by default; only resort to eval when the static parser cannot extract the enums. Never run dynamic eval on a config you have not visually inspected first.

Dialect pin: `conventional-commits`.

#### `.commitlintrc.{json,yml,yaml}` — vocab

Static read + standard JSON/YAML parse. Same `type-enum` / `scope-enum` path inside the parsed object. No dynamic-eval fallback is needed for these flavours.

Dialect pin: `conventional-commits`.

#### `package.json:commitlint` — vocab

Read `package.json` (always JSON), navigate to the top-level `commitlint` key, then to `rules.type-enum[2]` and `rules.scope-enum[2]`.

Dialect pin: `conventional-commits`.

#### `package.yaml:commitlint` — vocab

Same as `package.json:commitlint` but the source is YAML (used by some pnpm-monorepo setups). Same key navigation.

Dialect pin: `conventional-commits`.

#### `.cz.toml` / `.cz.json` / `cz.config.{js,ts,cjs}` (commitizen) — vocab

Parse `types` (array of objects, each with at least a `type` field — sometimes `name`, `description`) and `scopes` (array of strings) when present.

Some adapters (notably `cz-conventional-changelog`) inherit defaults from upstream and don't enumerate `types` in the local config. Treat absence of `types` as: dialect = `conventional-commits`, vocab = the default Conventional Commits type set (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`, `style`).

Dialect pin: `conventional-commits`.

#### `release-please-config.json` — vocab

Parse `packages.*.changelog-sections[]` (monorepo form). For single-package projects the array lives at the root `changelog-sections[]`. Each entry has at minimum a `type` field; collect all `type` values into the vocab. Entries with `"hidden": true` are still part of the vocab — `hidden` controls changelog rendering, not message acceptance.

Dialect pin: `conventional-commits`.

#### `.releaserc.{json,yaml,js,ts,toml}` / `release.config.{js,cjs,mjs,ts}` (semantic-release) — presence

Presence-only. Do not parse plugin configs (`@semantic-release/commit-analyzer` rules can rewrite the type semantics, but parsing them across plugin versions is fragile).

Dialect pin: `conventional-commits`. Vocab: fall through.

#### `.changeset/config.json` (changesets) — presence

Presence-only. Emit a one-line notice:

```
commit-conventions: changesets detected — release flow is decoupled from commits, but CC discipline still applies to the staged change.
```

Dialect pin: `conventional-commits`. Vocab: fall through.

### 2.2 Rust

#### `cliff.toml` (git-cliff) — vocab

TOML. Read the `[git]` table's `commit_parsers` array. Each entry is an inline table with a `message` regex and either a `group` (type-or-section name) or `skip = true`. The set of `group` values across non-`skip` entries forms the type vocab — but the values are *section names* (e.g. `"Features"`, `"Bug Fixes"`), not raw Conventional Commits types.

The skill extracts the leading literal from each `message` regex to recover the CC type. Example:

```toml
[git]
commit_parsers = [
    { message = "^feat",  group = "Features" },
    { message = "^fix",   group = "Bug Fixes" },
    { message = "^perf",  group = "Performance" },
    { message = "^chore\\(release\\)", skip = true },
]
```

Yields vocab: `{feat, fix, perf}`. The `skip = true` entry contributes nothing. Entries whose `message` is `.*` (catch-all) also contribute nothing.

Dialect pin: `conventional-commits` (git-cliff's primary use case; users running gitmoji-via-cliff are a tiny minority and will have a `.claude/commit-conventions.toml` override).

#### `release.toml` (cargo-release) — presence

Presence-only. Optionally read `pre-release-commit-message` (a Tera template like `"chore: release {{version}}"`); the literal type prefix gives a weak vocab hint but is not collected as authoritative vocab.

Dialect pin: `conventional-commits`. Vocab: fall through.

#### `Cargo.toml:[package.metadata.release]` (cargo-release inline) — presence

Same treatment as `release.toml`. Presence of the `[package.metadata.release]` table is the signal.

Dialect pin: `conventional-commits`. Vocab: fall through.

### 2.3 .NET / C#

#### `GitVersion.yml` / `.gitversion` (GitVersion) — presence

YAML. Check for the presence of `mode: ContinuousDelivery` or any `commit-message-incrementing` key — both indicate the project consumes commit-message metadata for version bumps, which only works with a CC-style dialect.

Dialect pin: `conventional-commits`. Vocab: fall through.

#### `version.json` (Nerdbank.GitVersioning) — presence

JSON. Presence of either `version` or `nbgvAdditionalFiles` confirms Nerdbank.GitVersioning. This is a **weak signal** — `version.json` exists in every NB.GV project regardless of whether the team uses CC commits. Combine with `git log` inference (§5) before treating it as decisive; if `git log` shows <50% CC matches, drop the dialect pin and let lower layers decide.

Dialect pin: `conventional-commits` (weak — overridable by §5). Vocab: fall through.

### 2.4 Universal (git config commit.template)

Run:

```bash
git config --get commit.template
```

If the command returns empty or the resolved path does not exist, skip this layer.

Otherwise, read the template file and scan for placeholder patterns:

- `<type>`, `<scope>` — angle-bracket form
- `(type)`, `(scope)` — paren form

If any of these patterns appears, the template is documenting a CC-style shape. Extract any enumerated values found in adjacent comments (e.g. `# type: feat | fix | docs`) as vocab hints. If only the placeholders appear without enumerations, the template is a presence signal.

Dialect pin: `conventional-commits`. Vocab: as extracted, else fall through.

If the template contains neither the placeholder patterns nor obvious CC structure (just a freeform editor template), treat as no signal and skip.

## 3. scopes.txt

Lookup order:

1. `./scopes.txt` (repo root)
2. `.claude/scopes.txt`

First-found wins. Only the first file is read — later locations are ignored even if both exist.

Format:

- One scope per line.
- Lines starting with `#` are comments and ignored.
- Blank lines ignored.
- Trim leading/trailing whitespace per line.

Example:

```
# Top-level modules
api
auth
db
ui

# CI / build
ci
deps
```

Yields scope vocab: `{api, auth, db, ui, ci, deps}`.

`scopes.txt` contributes scope vocab only. It does not pin a dialect.

## 4. CONTRIBUTING.md / CLAUDE.md heuristic scan

Heuristic prose scan, in this lookup order: `CONTRIBUTING.md` (repo root) → `CLAUDE.md` (repo root).

Match (case-insensitive) for these phrases:

- `conventional commits` / `conventionalcommits` → pin dialect to `conventional-commits`.
- `gitmoji` → pin dialect to `gitmoji`.
- A fenced code block whose first line is `# commit regex` or `# commit-regex` (or whose immediately preceding sentence contains "commit regex" / "commit pattern") → pin dialect to `custom` and set `header_regex` to the block's first non-comment line.

This is a **weak signal**. The phrase might appear in a negative context ("we don't use Conventional Commits") that the heuristic won't detect. Treat layer 4 hits as overridable by layer 1 (any later `.claude/commit-conventions.toml` write will win — but since layer 1 is walked first, layer 4 only fires if layer 1 was empty).

Vocab: not contributed. The skill falls through to layer 6 for defaults.

## 5. git log dialect inference

Inspect recent commit history:

```bash
git log -n 50 --pretty=format:%s
```

If the output is empty (a fresh repo with no commits), skip this layer.

Otherwise, classify each subject line into exactly one of four dialects via the regexes below. Each subject is classified independently.

### Classifier regexes

| Dialect | Regex (Perl-compatible / ripgrep `-P`) |
|---|---|
| `conventional-commits` | `^(feat\|fix\|perf\|refactor\|docs\|test\|build\|ci\|chore\|revert\|style)(\([^)]+\))?!?:\s.+` |
| `gitmoji` | `^(:\w+:\|[\x{1F300}-\x{1FAFF}\x{2600}-\x{27BF}])\s` |
| `plain` | `^[^:]+$` (no colon-separated prefix; classifier of last resort *after* gitmoji/CC have been ruled out) |
| `custom` | Not an inference output. Only set when `.claude/commit-conventions.toml` declares `dialect = "custom"` with a `header_regex`. |

Order of evaluation per subject:

1. Try `conventional-commits` first.
2. If no match, try `gitmoji`.
3. If no match, try `plain`.
4. If no match (e.g. subject contains a colon but is not CC), classify as `plain` for the count, but do not increment `gitmoji`/`CC`.

The gitmoji regex covers both literal `:emoji_code:` shortcodes and direct Unicode emoji in the SMP planes commonly used by gitmoji (U+1F300–U+1FAFF and the Misc-Symbols/Dingbats range U+2600–U+27BF). It deliberately requires a trailing space to avoid matching colons inside subject prose.

### Threshold and tie-breaking

- A dialect is **claimed** when its match count is ≥ 50% of the inspected subjects (≥ 25 of 50 by default).
- If two dialects tie at exactly 50%, the **latest** dialect wins: re-walk the commit list in reverse-chronological order and the first commit classified into one of the tying dialects pins the result.
- If no dialect reaches 50%, this layer contributes nothing and the algorithm falls through.

Edge cases:

- Repos with `<50` commits: use whatever `git log` returns; the 50% threshold still applies to the actual count.
- Merge commits whose subjects start with `Merge ` are still counted (they fail all three regexes and classify as `plain`). This biases noisy histories toward `plain` — acceptable, since `plain` is a permissive dialect that imposes no header structure.

## 6. Default fallback

When no signal is detected from layers 1–5 AND `git log` is empty (so layer 5 produced no inference), emit this notice verbatim:

```
commit-conventions: no project signal — defaulting to Conventional Commits. Add .claude/commit-conventions.toml to override.
```

Default dialect: `conventional-commits`.

Default vocab:

- Allowed types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`, `style`.
- Allowed scopes: unrestricted (any lowercase identifier acceptable).
- `require_scope = false`.
- Subject limits: hard 72, preferred 50.
- Body wrap: 72.

These defaults match the table in `SKILL.md` Step 3 — the two must stay in sync.

## TOML schema

The `.claude/commit-conventions.toml` schema. Copy `templates/commit-conventions.toml.example` to start. Only the sub-table matching the active `dialect` is read.

```toml
# Per-project commit-conventions config. Read by the commit-conventions skill at layer 1
# of the detection precedence — overrides commitlint, git-cliff, scopes.txt, CLAUDE.md
# heuristics, and git-log inference.

dialect = "conventional-commits"  # one of: conventional-commits, gitmoji, plain, custom

[conventional-commits]
allowed_types = ["feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "revert", "style"]
allowed_scopes = ["api", "auth", "db", "ui", "cli", "docs", "ci", "deps"]
require_scope = false
require_body_for = ["feat", "perf"]   # types that always require a body
subject_max_length = 72
subject_preferred_length = 50
body_wrap_at = 72
ban_subjects = ["wip", "fix", "update", "changes", "stuff", "misc", "address review comments"]

[gitmoji]
# Only used when dialect = "gitmoji".
allowed_emoji = [":sparkles:", ":bug:", ":zap:", ":recycle:", ":memo:", ":white_check_mark:"]
subject_max_length = 72
subject_preferred_length = 50

[plain]
# Only used when dialect = "plain". No type/scope prefix; subject discipline still applies.
subject_max_length = 72
subject_preferred_length = 50
body_wrap_at = 72
ban_subjects = ["wip", "update", "changes", "stuff", "misc"]

[custom]
# Only used when dialect = "custom".
header_regex = '^\[[A-Z]+-[0-9]+\]\s.+'   # e.g. "[PROJ-123] subject line"
subject_max_length = 72
body_wrap_at = 72
```

### Worked examples per dialect

**Conventional Commits (typical OSS repo):**

```toml
dialect = "conventional-commits"

[conventional-commits]
allowed_types = ["feat", "fix", "docs", "test", "chore"]
allowed_scopes = ["core", "cli", "docs"]
require_scope = true
```

A commit `feat(core): add retry budget` is accepted; `feat: add retry budget` is rejected (`require_scope = true`); `wip` is rejected unconditionally.

**Gitmoji (frontend project that uses gitmoji-cli):**

```toml
dialect = "gitmoji"

[gitmoji]
allowed_emoji = [":sparkles:", ":bug:", ":zap:", ":lipstick:", ":memo:"]
```

A commit `:sparkles: add dark-mode toggle` is accepted; `:fire: drop legacy auth` is rejected (`:fire:` not in `allowed_emoji`).

**Plain (small personal repo):**

```toml
dialect = "plain"

[plain]
ban_subjects = ["wip", "stuff"]
```

A commit `add dark-mode toggle` is accepted; `wip` is rejected.

**Custom (ticketed-workflow repo):**

```toml
dialect = "custom"

[custom]
header_regex = '^\[(PROJ|OPS)-[0-9]+\]\s.+'
```

A commit `[PROJ-123] add retry budget` is accepted; `add retry budget` is rejected; `[FOO-1] add x` is rejected (`FOO` not in the regex's alternation).

When validation fails for the `custom` dialect, the skill echoes the active `header_regex` back to the user so they can see exactly what shape is required.
