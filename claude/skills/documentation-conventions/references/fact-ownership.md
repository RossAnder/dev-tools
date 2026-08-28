# Fact ownership, volatile facts, and authority order

Loaded from `SKILL.md` Step 1 and Step 3.

## One fact, one home

Duplication of *knowledge* is the defect, not duplication of wording. Audience-scoped
restatement at a **different altitude** is legitimate — a tutorial may restate reference.
Same altitude, same audience, two locations is always a defect: both copies are load-bearing
and neither is subordinate.

| Class of fact | Canonical home | May be linked from | Never restated in |
|---|---|---|---|
| Why a design is this way | One decision record | README, CLAUDE.md, code comments | Any second narrative file |
| Current API surface / signatures | Source + generated reference | Tutorials, how-tos | Hand-written API tables |
| Build / test / lint invocation | One commands block | CI config comments | Per-agent files, skill files |
| Measurements, benchmarks | A committed artefact with a generator | Prose, with citation + date | Any prose claim without an artefact reference |
| Counts ("N of M", tool-surface counts) | Derived at read time by a command | Nowhere as a literal | Prose, headings, plan text |
| Version pins, dependency lists | Manifest + lockfile | Prose as "see `Cargo.toml`" | README install snippets |
| Schema / DDL | The migration | Prose, by path | A prose schema table |
| A contract shared by N carriers | One block, transcluded + parity-checked | The N carriers, by reference | Duplicated prose in each |
| A reversed decision's rationale | The **superseding** record | The old record's status header | A third "lessons" or "notes" file |
| Task / run state | The ledger or execution record | Progress views | Any narrative status doc |

**State the home in the home.** The owning file says it owns the fact — "this is stated in
exactly one place, and this is that place" — so a later reader knows a copy elsewhere is a bug.

**Never add a "notes", "clarifications", or "lessons" file.** It is a shadow authority by
construction.

## Volatile facts

The highest-churn content there is. These rules are absolute.

1. **If a command can compute it, write the command, not the number.** Counts, file totals,
   "the 12 commands", tool-surface counts. Or write the enumeration so the count is implicit.
2. **Every measurement carries value + date + the producing artefact, or it is deleted.**
   `~40ms (2026-08-14, bench/parse.rs)`. A bare number is unfalsifiable and therefore
   uncorrectable.
3. **A performance claim without a baseline and a platform is deleted, not corrected.**
   No variance, no baseline, no platform spec means the number is worthless.
4. **Express uncontrolled measurements as ranges or order-of-magnitude.** "~10-20× slower"
   survives machine variance; "17.3× slower" invites a correction pass on every re-measure.
   A figure inside the file's own stated run-to-run spread is not a finding.
5. **Version numbers live in manifests.** Prose states *floors* ("requires ≥ 1.54"), which do
   not churn — never *currents*, which do.
6. **Never write a claim about a future version.** "Removed in Astro 7" is a defect scheduled
   to detonate. Write what is true now and let the upgrade edit the line.
7. **Date-stamp and accept staleness** rather than silently refreshing. An absolute date, not
   a relative one, so staleness is a plain comparison.

## Exclusions before measuring

Any density or volume measurement excludes generated code first, and **names the exclusion in
its output**: build-script output, FFI bindings (`*-sys`), `.d.ts` rollups, ORM scaffolding,
protobuf/OpenAPI clients, vendored trees. On one 401-crate corpus the same files yielded
11.9% or 21.2% depending solely on whether five generated crates were in scope.

## Authority order

When two documents disagree, resolve in this order and **do not deliberate further**:

1. **Executable reality wins over all prose.** Code, tests, lockfiles, command output. If the
   doc and the code disagree, the code is what is true; the doc is a defect to file.
2. **Among prose, narrowest scope wins.** The nearest instruction file to the affected path;
   a module doc beats a repo README.
3. **Among same-scope records, the newest accepted decision wins** — follow `superseded by`
   links to their terminus *before* reading.
4. **A generated artefact beats a hand-written restatement** of the same fact, always.
5. **If a tie survives all four, do not choose and do not average.** Stop, name both
   locations, and ask. A tie is a fact-ownership bug; picking a winner silently makes it
   permanent.

Rule 5 is the anti-stall clause. Agents systematically over-trust prose relative to code, so
rule 1 must be applied deliberately rather than assumed.
