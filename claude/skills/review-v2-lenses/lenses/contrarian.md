+++
id = "contrarian"
status = "stable"
families = ["code", "perf", "plan"]
mode = "judgement"
produces = ["advisory"]
owns = ["alternative-approaches", "decision-challenges", "scope-conservatism"]
activation = "signal"
apply_when = { signals = ["public-api-change", "dependency-change", "new-seam"], always_at = "deep" }
slice = "whole-scope"
methods = ["steelman"]
+++

# contrarian

## Question

What is the strongest evidence that the direction this change or plan chose is wrong, too narrow, or less sound than an approach it did not take?

## Owns

`alternative-approaches` — a competing approach the change did not weigh, cited from a counter-example, a documented failure of the chosen shape, or a working use of the rival in this repo, a sibling repo or a named project.

`decision-challenges` — the argument against a decision the change or plan rests on: a recorded rejection whose rationale is thin, stale or answers a weaker version of the rival, or a choice made with no recorded competition at all.

`scope-conservatism` — work that is too narrow or too cautious for the pain it states, or work that need not happen at all: an exclusion that defers the hard part, a change that treats a symptom the rival would remove, or scope that could shrink because the goal is met by less.

## Probes — code

This is the disconfirmation lens: its value is the findings that confirmatory lenses are biased against producing. Follow the `steelman` method for every direction you challenge.

- Name the directions the change commits to: the new or changed public surface, the dependency added or swapped, the seam introduced, the data shape or protocol chosen. Start from what the matched signals point at.
- For each one, find the rival: the pattern the rest of the repo already uses for the same job, the library the ecosystem converged on, a narrower surface, or no new seam at all. Read the commit messages, comments, ADRs and plan the pack names for a recorded rejection.
- Argue the rival's case on a named axis: correctness, complexity and risk, reversibility, operational cost, consistency with the repo, or how far it reaches the stated goal. Does the recorded rationale answer that case, or a weaker one?
- For a new public surface or seam: who are its callers today? A seam with one implementation and one caller, or a public item only its own module uses, is the scope that could shrink.
- For a new dependency: what does the repo already use for the job, what does the dependency cost in build time, audit surface and lock-in, and does a documented failure of it match how the change uses it?
- Is the change a symptom fix whose root cause sits one layer down, where a rival would remove the whole class?

## Probes — perf

- Name the cost model the change assumes: which workload, what input size, which resource it trades for which. Is there a counter-example workload, real for this tool, where the chosen design costs more than the obvious rival?
- Does the optimisation target the measured bottleneck, or one that no profile in the pack shows? Cite the `PROFILE BASELINE` or the absence of one.
- Would a structural rival, such as doing the work once instead of faster, caching at a higher level, or not doing it at all, beat the local speed-up the change makes?
- Does the change buy speed with complexity, unsafe code, a concurrency hazard or a new dependency, where a simpler change reaches most of the gain? Name the simpler change and argue it.
- Is the perf work conservative: a micro-optimisation beside an algorithmic or I/O cost that dominates and that the change leaves in place?

## Probes — plan

- Read the plan's Approach and its recorded rejected alternatives together. Steelman each rejected alternative and check whether the recorded rationale rebuts it. A non-trivial plan that chose its approach with no recorded competition is itself a challenge, naming the obvious rival.
- Name the assumptions that must hold for the chosen approach to be right, and say which ones the code, a command or an external source puts in doubt.
- Compare the stated problem with what the tasks actually change. Does the Scope's out-of-scope list defer the hard part? Is the task set all polish with no task tackling the core?
- Is any part of the plan unnecessary: a task, a new abstraction or a new store whose job an existing one already does, or scope the success criteria never check?
- Would a smaller first step, reversible and cheaper, test the plan's key assumption before the rest is built?

## Does not flag

- Confirmations: a direction whose rationale holds against the strongest rival goes in the coverage note, with the evidence that settled it, never in a row.
- "Consider alternatives", "this may not scale", "another approach could be simpler" or any challenge without a cited competing approach, counter-example or documented failure.
- A challenge to a decision the user made in this run's plan or session, recorded as a User Decision, unless new evidence the decision did not have bears on it; cite the evidence.
- Defects, cleanup and correctness problems in the chosen approach as built: those belong to the other lenses in the run. This lens writes advisory only.
- Speculative futures no code, plan or issue names, and rivals that differ only in style or taste.
- Directions outside the scope that the change did not touch or rely on.
- A rival whose gain is small against the cost of switching now, unless it beats the chosen direction clearly on a named axis.
- More than three advisories in one cell: keep the strongest three and name the rest in the coverage note.

## Edit forms

None. Every finding is advisory: adopting a rival direction or reversing a decision needs a decision, never only edits.

## Advisory angles

- `alternative` — a concrete rival approach, with `alternative` naming it, `rationale` carrying the steelmanned case and its evidence, and `adoption_cost` what switching would take now.
- `challenge` — a documented decision whose rationale does not answer the rival's strongest case, with `challenges` naming the decision's document and line.
- `direction` — scope conservatism: what the work could also do, or could stop doing, and what leaving it as it is costs.
- `risk` — an assumption the chosen direction rests on that the evidence puts in doubt, when no concrete rival is yet in view.

## Severity calibration

Not applicable: this lens raises no defects. Order its advisories by how clearly the rival beats the chosen direction, and say in `rationale` whether the gap undermines the stated success criteria or is a reasonable but debatable boundary.
