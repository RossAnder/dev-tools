# Findings Report Template

Report-format reference for Step 3 of the `optimise` skill. The skill loads
this file before consolidating the five research agents' output in the main
conversation.

## Filename derivation

Persist the report to a scope-keyed file so `optimise-apply` can read it
from disk if conversation context has been compacted. Derive the filename
from the scope using the same convention as the `review` skill.

- Directory scope — `.claude/optimise-findings--src-prime-api-endpoints.md`
- Feature / area scope — `.claude/optimise-findings--auth.md`
- Git-derived scope (no args) —
  `.claude/optimise-findings--{branch-name}.md`, or
  `optimise-findings--recent.md` on the main branch

Use lowercase, replace `/` and `\` with `-`, collapse multiple `-` into one,
and strip leading `-`. Include the resolved filename in the report header
so `optimise-apply` can load the same file later.

## Numbering rule

Use globally unique item numbers across every severity section. Do not
restart numbering per section. The first Critical item is 1, Warnings
continue from the next integer after the last Critical item, and
Suggestions continue from the next integer after the last Warning.

## Structure (render these sections in this order)

Top-level heading: `## Optimization Findings`

Immediately under the heading, list the scope — a bullet-per-file or
comma-separated list of every path reviewed.

Then three severity sections as level-3 headings, in this order:

1. `### Critical (measurable impact)`
2. `### Warnings (likely overhead)`
3. `### Suggestions (marginal or future)`

Any section with zero items is still worth listing with a short "None"
line so the reader can see it was considered.

## Item fields

Each numbered item in any severity section uses the same five fields:

- **Heading line** — `N. **[file:line]** (category) — one-line summary`,
  where `category` is one of `memory`, `serialization`, `query`,
  `algorithm`, or `concurrency`.
- **Current** — what the code does now and its cost.
- **Recommended** — the specific change to make, with a short code sketch
  if it helps.
- **Evidence** — links to docs, benchmarks, or Context7 results that
  support the recommendation. Cite sources concretely.
- **Risk** — tradeoffs or things to verify after the change lands.

## Report-level rules

- Deduplicate items that multiple agents flagged. Merge into a single
  entry and note which lenses caught it.
- Include research citations for every item. If research was inconclusive,
  say so and describe the tradeoff explicitly.
- An empty report is valid — not every change has optimization
  opportunities. Do not invent items to pad the report.
- Do not recommend optimizations that sacrifice readability for negligible
  gains.
- Include the resolved filename from the filename-derivation section in
  the report header so `optimise-apply` can load it.
