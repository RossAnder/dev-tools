+++
id = "hygiene"
status = "stable"
families = ["code"]
mode = "mechanical"
produces = ["cleanup"]
owns = ["dead-code", "stale-todos", "unused-config"]
activation = "candidate"
apply_when = { signals = ["rename-or-removal", "churn-hotspot"] }
slice = "per-subsystem"
methods = ["sweep", "trace"]
v1_category = "quality"
+++

# Hygiene

## Question

What in this slice can a reference search prove is no longer used, done or read, so that removing it changes nothing?

## Owns

`dead-code` — functions, types, constants, files and scripts with no reference anywhere in the tree, proven by a search for every spelling of the name, tests, scripts, configuration and prose included.

`stale-todos` — TODO and FIXME markers whose work the code already does, or whose cited reference is resolved or no longer exists.

`unused-config` — configuration keys, feature flags, declared dependencies, build features and imports that nothing reads.

## Probes — code

Every row rests on a search. Run the sweep method for each candidate, search every spelling of the name across the whole tree, and record the search strings and the hit count in the row's evidence. A candidate whose verdict needs a judgement the search cannot settle — whether a public item has callers outside the repo, whether a framework finds it by convention, whether a TODO's intent is met — goes in your return under `escalate`, never into the store.

- Dead code: an item declared in the slice with zero references beyond its declaration; a file no module declaration, import, include, build step or script names; a script no hook, command, workflow or doc invokes.
- Stale TODOs: a marker naming a function, a flag or a behaviour that a search shows now exists; a marker citing a ticket, ledger or backlog id the store shows resolved or the tree no longer holds.
- Unused config: a key in a configuration file whose name no reader looks up; a feature flag or build feature no conditional checks; a dependency declared in a manifest with no import, use or invocation anywhere in its package; an import no line of its file uses.
- Leftovers of the change itself, when the diff removed a caller: the helper, constant or configuration key that caller was the last user of.

## Does not flag

- Confirmations: a candidate the search shows is used goes in the coverage note, never in a row.
- Anything the compiler's unused warnings or the `LINT BASELINE` already report.
- A name found unused by reading one file or trusting a comment; only a whole-tree search proves an absence.
- Items reached without their name being spelled: entry points (main functions, binaries, tests, benchmarks, hooks), items a framework discovers by convention or by glob, items reached by reflection, serialisation or derive output, items gated behind a build feature or platform the search did not cover, and public items of a library consumed outside the repo. These go under `escalate` when they look dead.
- A TODO whose work is still outstanding, however old, and a TODO style a project rule prefers; tidying wording is not hygiene.
- Comments that misdescribe the code beside them; comment rot belongs to `doc-drift`.
- A renamed or removed symbol still named elsewhere; stale references belong to `ripple`.
- Code kept deliberately, as a comment, an ADR or a CLAUDE.md line says; and removals whose gain is negligible against the risk of a reference the search could not see.
- Anything outside the slice.

## Edit forms

- `remove` — dead code, a stale TODO or FIXME line, an unused configuration key, feature, dependency or import. `old` carries the exact text removed; one pattern across many sites is one row with every file in `instances`.
- `change` — a manifest or configuration entry trimmed of the unused part only, such as one feature dropped from a list, with `old` carrying the entry as it stands.

Removing dead code from an executable source routes to the Opus verifier; that routing is computed, so write the row the same way either way.

## Advisory angles

None. Hygiene writes cleanup only; anything that needs a decision goes under `escalate` in the return.

## Severity calibration

None. Hygiene writes no defects; a candidate that would be wrong for some input is a finding for a judgement lens, and goes under `escalate`.
