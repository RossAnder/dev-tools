+++
id = "steelman"
status = "stable"
+++

# Steelman

Hunt for evidence that the chosen direction is wrong. The other lenses ask whether the work is correct as built; this asks whether it should have been built this way at all, which is the finding confirmatory lenses are biased against producing. Its output is advisory: a decision for the user, never a defect.

## Procedure

1. **State the direction.** Write in one or two sentences what the change or plan chose: the dependency, the data shape, the seam, the protocol, the scope. Quote the plan's Approach, the ADR, or the code that embodies it.
2. **Name the rival.** Identify the approach not taken — a rejected alternative the plan records, the obvious other design, a different library, a smaller or a larger scope. When the plan records rejected alternatives, start there.
3. **Argue the rival's strongest case.** Make the best argument that it should have won, on a named axis: correctness, complexity and risk, reversibility, operational cost, consistency with the rest of the repo, or how far it reaches the stated goal. Then check whether the recorded rationale actually answers that argument. A rationale that is thin, stale, or answers a weaker version of the rival is the finding.
4. **Ground it.** Every advisory cites at least one of:
   - **a competing approach** in use — in this repo, a sibling repo, or a named project or library, by `file:line` or URL;
   - **a counter-example** — an input, workload or usage the chosen design handles badly and the rival handles well, traced in the code;
   - **a documented failure** of the chosen shape — an issue, postmortem, changelog entry, deprecation notice or paper describing it going wrong, by URL with its version and fetch date.

   "Consider alternatives", "this may not scale" or "another approach could be simpler" with no citation is not a finding. Drop it.
5. **Name what must hold.** List the assumptions that must be true for the chosen direction to be the right one, and say which of them the evidence puts in doubt.
6. **Check scope conservatism.** Ask whether the work is too narrow for the pain it states: an exclusion that defers the hard part, a change that treats a symptom the rival would remove. Name what it could also do and what leaving that out costs.
7. **Write the advisory.** `angle = "alternative"` when you name a concrete rival, `"challenge"` when the target is a documented decision (with `challenges` naming its document and line), and `"direction"` for a scope or direction concern. `rationale` carries the steelmanned case and the evidence, `alternative` the rival, and `adoption_cost` what switching would take now. Raise at most the strongest three; a weaker but credible rival goes in only when it beats the chosen direction on some axis.

## Coverage

The coverage note names each direction you challenged, the rival you argued for it, and for each one whose rationale held, the evidence that settled it.
