---
name: review-v2-lenses
description: The pluggable lens catalogue behind /review-v2, /optimise-v2 and /review-plan-v2 — the four entry types (lens, facet, method, check) with their TOML frontmatter and fixed body sections, the Owns-section rule, activation tiers, the vocabulary.toml of signals, deterministic detectors, concern tags and routing globs, the optional project layer (`.claude/review-lenses/config.toml` with disable, enable, extend, override, scope and project checks; project vocabulary under the `project:` prefix; project lenses and facets), the built-in collision refusal, content hashing, the lifecycle and promotion path, and listing or validating the merged catalogue with `tomlctl review catalogue`. Use when authoring or maintaining catalogue entries or the vocabulary, configuring a project's review lenses, or fixing a catalogue validation error. Review agents read entry bodies from the paths their cell assignment carries; orchestrators never read them.
---

# Review v2 lens catalogue

The catalogue is every concern a v2 run can compose into a cell. It lives once, globally, and every project gets all of it with no setup; a project only switches entries on or off, varies them by path, and adds what is its own. `tomlctl review catalogue` merges the layers and validates them, and `tomlctl review run start` validates them again in every repo, so every rule below that says "refused" is a gate failure, not a style note.

## Layout

```
review-v2-lenses/            built-in root; deployed to <claude dir>/skills/review-v2-lenses
  vocabulary.toml            signals, concern tags, routing globs
  lenses/<id>.md             concerns a cell runs
  facets/<id>.md             stack knowledge attached to lenses
  methods/<id>.md            procedures lenses cite
  checks/<id>.md             command-backed and cell-run checks
<repo>/.claude/review-lenses/   optional project layer
  config.toml  vocabulary.toml  lenses/<id>.md  facets/<id>.md
```

Only `*.md` files directly inside the four directories are entries; the directory decides the type. Ids are unique across all four types and both layers, so a facet and a lens cannot share one.

## Entry files

An entry is markdown whose first line is `+++`, followed by TOML frontmatter and a closing `+++` line. Line endings are normalised and a BOM is stripped before parsing. Every frontmatter key is checked against its type's field list, and an unknown key is refused.

Common fields:

| Field | Required | Value |
|---|---|---|
| `id` | yes | lowercase letters, digits and `-`; matches the file name by convention |
| `status` | yes | `experimental` \| `stable` \| `deprecated` |
| `version` | no | display only, string or number; never hashed and never a stats key |

**Sections.** The body is split on `## ` headings (level 2 only). A `## ` line inside a ``` or ~~~ fence is body text. Text before the first `## ` heading belongs to no section, so a `# Title` line there is ignored. `###` headings stay inside the section above them. Each section has a key: the heading lowercased, with every run of other characters turned into one `_`, so `Does not flag` is `does_not_flag` and `Probes — code` is `probes_code`. Config `extend` and `override` name sections by that key.

### Lens

| Field | Required | Value |
|---|---|---|
| `families` | yes | non-empty subset of `code`, `perf`, `plan` |
| `mode` | yes | `judgement` (review-finder; the context-free correctness cell runs `review-finder-cf`, the same Opus agent without CLAUDE.md) \| `mechanical` (review-sweeper) \| `executor` (plan-executor) |
| `produces` | yes | kinds from `defect`, `cleanup`, `advisory` |
| `owns` | yes | non-empty list of concern tags from the vocabulary |
| `activation` | yes | `floor` \| `signal` \| `candidate` \| `opt-in` |
| `slice` | yes | `whole-scope` \| `per-subsystem` \| `per-file` \| `per-task` |
| `apply_when` | no | table: `signals` (vocabulary names), `paths` (globs), `min_depth`, `always_at` (`quick` \| `standard` \| `deep`) |
| `after_wave` | no | non-negative integer; the lens is composed only after that wave |
| `methods` | no | method ids the lens cites |
| `v1_category` | no | see below |
| `research_domain` | no | string handed to `research-methods` |

`produces` is all three kinds for a judgement lens, `["cleanup"]` for a mechanical one, and `["advisory"]` for `contrarian`.

**`v1_category`** is what `export-v1` writes as the v1 `category`. A string applies to every declared family except `plan`; a table maps family to category, as in `v1_category = { code = "quality", perf = "concurrency" }`. The sets are closed: code takes `quality`, `security`, `architecture`, `completeness`, `db`, `testability` or `package-quality`; perf takes `memory`, `serialization`, `query`, `algorithm` or `concurrency`. The plan family has none, so a plan-only lens omits the field — a string on a plan-only lens is refused, as is a table naming a family the lens does not declare.

**Body**, in this order:

1. `## Question` — the one question the lens answers. Required, hashed.
2. `## Owns` — one paragraph per owned tag. Required, hashed. See the rule below.
3. `## Probes — <family>` — one section per declared family, in `families` order. Each declared family's section is required and hashed.
4. `## Does not flag` — required, hashed. Seed it from the v1 rejection clusters (confirmations, stale-comment trust, design intent and ADRs, speculative and style-only, out of scope, negligible gain against risk) and the source agent's own "Do not flag" sentence.
5. `## Edit forms` — the cleanup edits the lens writes, with `direction` (`add` \| `remove` \| `change`).
6. `## Advisory angles` — which `angle` values the lens raises and when.
7. `## Severity calibration` — one line per severity for this lens's defects.

**The Owns rule.** The validator splits the Owns section into paragraphs on blank lines and reads the **first backtick-quoted span of each paragraph** as an owned tag. The set it reads must equal the frontmatter `owns` exactly. So:

- start each tag's paragraph with the tag in backticks, and give every tag its own paragraph, separated by a blank line;
- never put a backtick span in any other paragraph of the section, and never put two tags in one paragraph or a bulleted list, because only the first span of a paragraph counts.

```markdown
## Owns

`logic` — output that is wrong for an input or state the finding can name.

`missing-handling` — an input, state or branch the code does not handle at all, reported as a defect, never as cleanup.
```

Stack-specific text belongs in facets, not in lenses.

### Facet

| Field | Required | Value |
|---|---|---|
| `families` | yes | as for a lens |
| `apply_when` | yes | its `signals` must be non-empty: the stack signals whose roots the facet attaches under |
| `attaches_to` | yes | lens ids, or `"*"` for every lens |

Body: `## General` (pitfalls of the stack that hold for any attached lens), then `## By lens` with one `### For <lens-id>` subsection per attached lens. Probes are concrete pitfalls, not tutorials. Every section is hashed.

A facet attaches to a cell when the cell's family is in its `families`, one of the cell's lenses is in `attaches_to` (or it is `"*"`), and a slice entry meets a root of one of its signals — a slice entry and a root meet when either directory contains the other, and a root of `.` meets every slice. A project scope block and the cell's `facets` edits (`+id` / `-id`) adjust the result.

### Method

Only the common fields. The body is free-form, conventionally `## Procedure`, and every section is hashed. Lenses cite methods by id in `methods`, and the cell assignment carries each method's path.

### Check

| Field | Required | Value |
|---|---|---|
| `families` | yes | as for a lens |
| `executor` | yes | `tomlctl` \| `surveyor` \| `verification` \| `cell` |
| `fence` | `fence` or `produces` | the pack fence it writes, such as `"LINT BASELINE"` |
| `produces` | `fence` or `produces` | the kinds a `cell` check writes |
| `apply_when` | no | as for a lens |
| `activation` | no | as for a lens |
| `discover` | no | for `verification` checks: the command-discovery ladder, an array of strings tried in order |

Body: `## Runs`, `## Interpreting the result`, `## Fence or finding mapping`. Every section is hashed. A `surveyor` check is run by review-surveyor, which writes its fence into `pack.md`. A `verification` check's resolved command runs in its own `verification` dispatch, and its log path is recorded in the run's `baselines`. A `cell` check runs inside a `check:<id>` cell.

## Activation tiers

- `floor` — always proposed for its families.
- `signal` — proposed when one of `apply_when.signals` matched and, when `apply_when.paths` is set, the scope meets those paths. `always_at = "deep"` proposes it at deep regardless.
- `candidate` — proposed as the depth's cell budget allows, ordered by how many of its `apply_when.signals` matched. A candidate whose signals matched but which missed the budget is listed as unselected and needs a waiver.
- `opt-in` — only when the project config's `enable` names it or the user passes `--lens +id`.

`min_depth` refuses the lens below that depth, and `after_wave` holds it back until that wave is done. `experimental` status makes any entry `opt-in` whatever its `activation` says. A `deprecated` entry is refused in new cells but still resolves in stats. A disabled lens's tags show as `disabled` in the digest, not as unselected.

## Vocabulary

`vocabulary.toml` has three top-level tables and no other keys: `[signals.<name>]`, `[concerns.<tag>]` and `[routing]`.

### Signals

| Key | Required | Value |
|---|---|---|
| `kind` | yes | `stack` \| `diff` \| `health` \| `judged` |
| `meaning` | yes | one sentence |
| `detect` | every kind but `judged` | the detector table below; refused on a judged signal |
| `hint` | `judged` only | what the surveyor cites as evidence when it proposes the signal |

Judged signals (`trust-boundary`, `hot-path`, `new-seam`) are never detected. review-surveyor proposes them as candidates with evidence paths, and the orchestrator accepts or rejects each one.

### Detectors

`review survey` evaluates every `stack`, `diff` and `health` detector deterministically, stacks first, and records each true signal with its `roots`: the directories it matched, `.` for the repository root. A detector that cannot be evaluated (git unavailable, a manifest that does not parse) records the signal as `unknown` with the reason, never as false.

The detect keys are `files`, `contains`, `manifest`, `count`, `window` and `threshold`; any other key is refused.

- **`files`** — globs over repo-relative, `/`-separated paths. `**/` matches at any depth, the root included, so `**/Cargo.toml` finds a crate three levels down. Absent means every candidate file.
- **`contains`** — one regex or an array of them; a file passes when any matches. Each is compiled through `query::compile_user_bytes_regex`: the regex build has no Unicode tables, so write `(?-u:\b)` and `(?i-u:…)`, use explicit classes such as `[ \t]` and `[A-Za-z0-9_]`, and never write `\w`, `\d`, `\s`, `\b` or a bare `(?i)`. Matching is multi-line and CRLF-aware, so `^` and `$` are line anchors. A pattern is capped at 512 bytes. Write patterns as TOML literal strings (`'…'`) so backslashes reach the regex unchanged. The catalogue gate compiles every pattern.
- **`manifest`** — dotted key paths into TOML (`.toml`) and JSON (`.json`) manifests only; a file passes when any path exists, such as `dependencies.tokio` or `devDependencies.react`. Segments split on `.`, so a package name containing a dot cannot be named this way. MSBuild XML — `.csproj`, `Directory.Packages.props`, `<PackageReference>`, legacy `<Reference Include=…>`, `packages.config` — is matched with `files` plus `contains`.

When `files`, `manifest` and `contains` are all present, a file must pass each one present.

**Stack** — evaluated over `git ls-files`. True when any file passes; the roots are the parent directories of the passing files. Anchor stacks on manifests or project files (`Cargo.toml`, `tsconfig.json`, `*.csproj`, `package.json`) rather than source extensions, so roots name projects rather than every source directory.

**Diff** — evaluated over the changed files of `git diff -M <base>..HEAD` inside the run's scope. `files` filters the changed paths. `contains` is matched against each file's patch text: the header lines (`deleted file mode`, `rename from`, `new file mode`) and the hunk lines with their `+`, `-` or space prefix, so `^-` finds a removed line and `^[-+]` a changed one. The roots are the parent directories of the passing files.

**Health** has three shapes:

1. **Count at HEAD** — `count` and no `window`. `count` is a table keyed by a stack signal name, or `"*"` for any repo, whose values are `{ files = [globs], patterns = [literals] }`. An entry is evaluated only when its stack signal is true (`"*"` always is). The measure is the number of occurrences of the patterns across the tracked files its `files` match.
2. **Count over a window** — `count` and `window`. The measure is occurrences in lines added minus occurrences in lines removed, over the commits of the last `window` days, in the files each entry's `files` match.
3. **Commits per file** — `files`, `window` and no `count`. The measure is the number of commits in the window that touch each file; `contains` filters those commits by subject line.

`window` is a whole number of days from 1 to 36,500; a larger value is refused when the vocabulary is parsed. The window is passed to git as an absolute cutoff, `--max-age=<unix seconds>`, computed as now minus `window` days. A cutoff at or before the Unix epoch passes no filter, so the window then covers all history.

`count` patterns are literal substrings matched verbatim and case-sensitively, never regexes, counting non-overlapping occurrences. Each line is matched with `\n` prepended (after stripping a patch's `+` or `-`), so a literal starting `\n` matches only at the start of a line: `"\ntest("` counts a top-level `test(` call without counting `latest(`.

`threshold` is an integer — true when a measure reaches it — or `{ per_kloc = N }`, true when the measure per 1000 lines reaches `N`: lines of the entry's files at HEAD in shape 1, lines added to them in the window in shape 2. A count signal is true when any entry reaches the threshold, and its roots are the roots of each stack that did; for a `"*"` entry they are the parent directories of the files that contributed. A commits-per-file signal is true when any file reaches it, and its roots are those files' directories. The `churn-hotspot` files are the top decile by commit count in its window. `review plan --propose` gives each one its own slice.

### Concern tags

`[concerns.<tag>]` with `meaning` and `families`. A tag names a slice of a lens's responsibility — finer than the lens, so a probe can record its nearest tag and the digest can say what a waiver gives up.

**Ownership.** For every (family, tag) the vocabulary declares, exactly one lens across both layers must list the tag in `owns` and declare that family. A lens's families must include at least one of each owned tag's families. A tag's families must not exceed its owner's: a family nobody owning the tag serves is an `OwnerCount` error with zero owners. `deprecated` lenses do not count as owners. `experimental` and project-disabled lenses **do** count. In `vocabulary.toml`, the comment line above each group of tags names the lens that owns them; author a lens's `owns` from its group, and add a tag to the vocabulary before any lens owns it. When parallel authors collide on a tag, the catalogue gate settles it — move the tag, never loosen the validator. Print the generated map with `tomlctl review catalogue --owners`.

### Routing

`[routing]` holds `executable` globs (source whose edits change behaviour) and `tests` globs. `review add` routes a cleanup row to the Opus verifier when its edit target, or any of its `instances`, is a test path and the direction is `remove`, or is a non-test executable path. Every other cleanup goes to the Haiku checker. A test path is never treated as executable. A project `[routing]` appends to both lists.

## Project layer

Everything under `.claude/review-lenses/` is optional; an absent directory means the defaults.

### `config.toml`

| Key | Value | Effect |
|---|---|---|
| `disable` | entry ids | switched off in this project |
| `enable` | entry ids | opt-in entries switched on in this project |
| `[extend.<id>]` | `<section key> = [strings]` | each string appended to that section as a `- ` bullet; the key `probes` appends to every Probes section |
| `[override.<id>]` | `<field> = value` or `<section key> = "text"` | `activation`, `apply_when`, `after_wave`, `slice` and `attaches_to` replace the frontmatter field; any other key must name a section of the entry, such as `question` or `probes_code`, and its string replaces the section body |
| `[scope."<glob>"]` | `disable = [ids]`, `facets = ["+id", "-id"]` | path-scoped variation; nothing else is allowed in a scope block |
| `[[checks]]` | `id`, `families`, `paths`, `command`, optional `fence` and `activation` | a project check, always run by the `verification` agent |

Any other top-level key is refused, as is an id in `disable`, `enable`, `extend`, `override` or a scope block that names no entry. `extend` and `override` apply before hashing, so they change the content hash the run binds — that is how `stats` sees a project's variation. A scope glob is matched against each slice entry's text, so `packages/legacy/**` covers both `packages/legacy/a.ts` and `packages/legacy/src/**`. A lens a scope block disables is refused in any cell whose slice holds an entry the block covers.

Worked example:

```toml
disable = ["doc-consolidation"]          # built-in entries switched off here
enable = ["plan-premortem"]              # opt-in entries switched on here

[extend.idioms]                          # append-only additions
does_not_flag = ["Builder structs in tomlctl/src/cli/types/ follow clap's derive layout, not the crate's own idiom."]
probes = ["Does a new verb module follow the read/mutate split of the existing ones?"]

[override.test-pruning]                  # replace a field or a section
activation = "opt-in"

[scope."packages/legacy/**"]             # path-scoped variation
disable = ["test-pruning"]
facets = ["-react"]

[[checks]]                               # a project check
id = "harness-gates"
families = ["code"]
paths = ["claude/**"]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
```

### Project vocabulary and entries

`.claude/review-lenses/vocabulary.toml` uses the built-in format. Every signal and concern tag it adds carries the reserved `project:` prefix, such as `project:legacy-auth`, and no built-in name ever does. Its `[routing]` globs extend the built-in ones.

`.claude/review-lenses/lenses/*.md` and `facets/*.md` add project entries in the built-in format. Project methods and checks are not read from directories; a project check goes in `[[checks]]`. A project lens may own only `project:` tags. A project facet may attach to built-in lenses.

**Collision refusal.** A project entry or check whose id equals a built-in id is refused. To change a built-in entry, use `extend`, `override` or `disable` — never a same-named copy — so a project never silently forks a shared lens.

## Content hashing

`review plan` binds each concern's content hash, path, methods and `v1_category` into the cell, and `review run start` records the catalogue's tree sha and the project config's sha.

- A lens's hash is sha256 over its **Question, Owns, every Probes section and Does not flag**, each as heading plus body with trailing whitespace and edge blank lines trimmed. Edit forms, Advisory angles, Severity calibration, the frontmatter and `version` are not hashed.
- A facet's, method's or check's hash covers every section.
- A project `[[checks]]` row is hashed over its TOML.
- The tree sha covers every vocabulary and entry file of both layers; `config.toml` has its own sha.

A lens edit therefore shows in `review stats` without anyone bumping a number.

## Lifecycle and promotion

`status` runs `experimental` → `stable` → `deprecated`. An experimental entry is opt-in. A deprecated entry is refused in new cells and still resolvable for stats, and it stops counting as a tag owner.

**Reworking a stable lens.** The plan for a rework is a new `experimental` id that a project or user opts into beside the old lens, then a swap. The validator does not allow that when the rework owns the old lens's tags: it counts every non-deprecated lens, experimental and disabled ones included, so two live lenses owning one (family, tag) fail `--check` with "needs exactly one owner, has 2". Until the validator changes, use one of two paths:

- trial the rework side by side under new tags of its own, added to the vocabulary with it, then move the old tags to the rework and mark the old lens `deprecated` in the same edit;
- or edit the stable lens in place and let the content hash carry the change into stats.

**Probes and promotion.** A `probe:<slug>` cell carries a one-paragraph `question` instead of a lens and runs the probe template in the `review-v2-method` skill. It binds no catalogue content and records its nearest concern tag. Consider promotion when `review stats --by probe` shows probes recurring under one tag across three or more runs. Promote first to a project lens owning a `project:` tag, then to a built-in lens once it has proved portable.

## Listing and validating

```bash
tomlctl review catalogue --check
tomlctl review catalogue --family code --signals lang:rust,schema-change
tomlctl review catalogue --owners
tomlctl review catalogue --vocabulary
tomlctl review catalogue --builtin <dir> --project <dir> --check
tomlctl review stats --all --by probe
```

The default output is one row per entry: `type`, `id`, `hash`, `status`, `families`, `mode`, `produces`, effective `activation`, `apply_when`, `matched` (its signals among `--signals`), `attaches_to`, `path`, `layer` and `disabled`. `--builtin` defaults to `<claude dir>/skills/review-v2-lenses`, and `--project` to `.claude/review-lenses` at the repo root. `--check` prints `{"ok":true}`, or fails listing every error with the file to fix. In this repo, the `catalogue_validates` dispatch gate runs the same validation over the built-in catalogue plus `.claude/review-lenses/`, and the command lint parses every `tomlctl` line in a bash fence of an entry.

## lumina mapping

lumina's `research-explore` lenses map onto the catalogue as follows. lumina itself is not edited, and the catalogue is written so lumina could adopt it later.

| lumina lens | Catalogue |
|---|---|
| `codebase` | `review survey` and review-surveyor, plus `plan-grounding` |
| `library` | `external-validity` |
| `risk` | `plan-risk` |
| `completeness` | `plan-grounding` and `test-gaps` for the judgement half; `ripple` for the mechanical half |
| `domain` | `domain-model` |
| `contrarian` | `contrarian`, whose argue-against and scope-conservatism angles also carry `story-review`'s devil's advocate |

Defect severities `critical` / `major` / `minor` match lumina's `Critical` / `Major` / `Minor`.
