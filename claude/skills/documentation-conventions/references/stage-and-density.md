# Stage, deliverable, and density

Loaded from `SKILL.md` when setting up a repo or judging whether a tree is over-documented.

## Classify by deliverable first

Stage is the second question. The first is **what leaves the repo**.

- **`code`** — the artefact is the software. Structure mirrors churn with every refactor and
  are deferred; decisions are cheap and are not.
- **`decisions`** — the artefact is the decision set; the code is apparatus that produced it
  (prototypes whose designs graduate elsewhere, research spikes, evaluation harnesses). This
  **inverts the ladder**: decision records are held to the released bar — complete, current,
  owned — and everything mirroring the apparatus is held at the spike bar. Not *less*
  documentation. Differently shaped.
- **`prototype`** — the code is expected to be replaced. **Structure mirrors are forbidden
  outright, not deferred**: no directory maps, no component inventories of your own code, no
  readiness audits. An inventory of a *foreign* system you are replacing is a decision record
  and is exempt.

## The organising principle

> Cost of a document ≈ (rate of change of the thing it mirrors) × (how structurally it
> mirrors it).

So: **defer mirrors, never defer decisions.** Decision records are append-only and never
churn — they are cheap at every stage and should be written earliest. Reference docs, API
prose, architecture overviews and directory maps churn on every refactor and are the correct
thing to defer.

This is why RFC-first cultures are not a counterexample: they front-load the non-churning
artefact.

## The stage ladder

| Stage | Entry trigger | Required | Forbidden as premature |
|---|---|---|---|
| **S0** spike | A question to answer; code expected to be deleted | One header line: purpose + expiry | Any doc comment, README, decision record |
| **S1** early | Survives the spike; single author; no consumers | Non-obvious *why* only; forward-reference markers; rationale in commit messages | Per-symbol doc comments, README API sections, architecture diagrams, docs for private items |
| **S2** internal consumers | A second caller depends on it, or a non-author edits it | Doc comment on every **crossed boundary** symbol — invariants, error and panic conditions, ownership; one short module header | Tutorials, prose duplicating signatures, embedded benchmark numbers |
| **S3** released | First version tag, publish, or external consumer | Public-API docs complete; changelog; stability and deprecation policy; a runnable example per entry point | Docs for private items; roadmap prose in reference docs |
| **S4** maintenance | Contributor turnover, or on-call incidents | Runbook per paged alert; decision records for load-bearing constraints; freshness stamps on operational docs | Rewriting history; undated "as of vN" performance claims |
| **S5** sunset | Replacement exists, or removal scheduled | Deprecation marker naming a successor and a removal version; migration guide | New feature docs; silent removal |

**Transition triggers are detectable, not felt:** a second caller outside the defining module;
`git log --format=%an -- <path> | sort -u | wc -l` > 1; the first version tag; the first
external issue; the first paging incident; handover to agent-only maintenance (→ S2 minimum
regardless of age).

**Raise a module's stage in its own commit.** Never smuggle a doc-tier upgrade into a feature
change — the upgrade should be reviewable and refusable on its own.

**When a repo stops, close it.** Past ~90 days idle: mark the stage, resolve or explicitly
abandon open findings, delete completed progress logs. A "100% complete" progress log
describes work that no longer needs describing.

## Density is a screen, never a target

Density is a **screening threshold that triggers a look**. It is never a goal, never a gate,
and never a thing to move. A repo may sit in-band and be diseased; a repo may sit out-of-band
and be correct. The published evidence does not support density as a quality predictor — what
correlates with defects is comment/code *inconsistency*, not volume.

**Measure it correctly or not at all.** Comment share = content-comment lines / (content-comment
lines + code lines), non-blank physical lines, where a line containing any code is code;
delimiter lines (bare `/**`, `*/`, lone `*`, banner rules, an XML tag with no prose) count in
neither numerator nor denominator; doc and inline comments are reported **separately**.
Counting delimiters inflates by roughly 4pp. Exclude generated code first and name the
exclusion. `scc`, `tokei` and `cloc` all get this wrong and disagree with each other — use
them as a fast screen, never as the reported number.

**Screening bands** (comment share, per directory):

- **> 30% → look.** Two unrelated corpora put the boundary here: an all-language mean + 1SD
  of 29.6%, and a crates.io per-crate p75 of 28%.
- **> 40% → look hard.** ≈ p90.
- **< 8% → look**, published or boundary-crossing code only. Weakest of the three, and valid
  only after generated code is excluded.
- **8-30% → report the number and take no action.**

Multiply the 30% trigger by nature: **1.0×** application or CLI, **1.5×** published library or
design system, **2.0×** knowledge/decision repo. Prototypes and infrastructure get **no
multiplier — no data exists**; use the relative rule. The mechanism, which survives even where
the numbers do not, is *who reads the source*: application source is read only by its authors,
library source occasionally by consumers, teaching source *instead of* running it.

**Stage does not move density.** The only large-scale measurement finds it stage-invariant —
about 1pp over four years, and downward. Do not set a per-stage density target. What stage
legitimately moves is the *obligation set* (which symbols must carry a doc comment) and the
lint ladder, both above and in `language-cores.md`.

**The relative rule, which is what actually catches regressions:**

> A new module may not exceed the median comment share of its sibling directories in the same
> package. If it does, the excess is the thing to inspect.

Crossing any threshold means "run the block-length cap and the anti-pattern greps over this
directory" — never "reduce the number".
