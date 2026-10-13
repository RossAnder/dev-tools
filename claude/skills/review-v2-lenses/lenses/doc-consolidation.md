+++
id = "doc-consolidation"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["duplicated-docs", "obsolete-docs"]
activation = "signal"
apply_when = { signals = ["doc-growth"] }
slice = "whole-scope"
methods = ["sweep"]
v1_category = "quality"
+++

# doc-consolidation

## Question

Which guidance does this scope state in more than one document, and which documents describe features or procedures that no longer exist, so that merging or removing them leaves one current home for each fact?

## Owns

`duplicated-docs` — the same guidance, at the same altitude and for the same audience, written in more than one doc: a procedure in a README and again in a guide, a rule copied into several skill or agent files that no shared block or link ties together, a reference table restated by hand beside the source it was taken from. Duplicated knowledge is the defect, not duplicated wording; a tutorial that restates reference for a different reader is not a copy.

`obsolete-docs` — a doc, or a section of one, that describes a removed feature, a retired command or flag, a superseded procedure or a decision that was reversed, so a reader following it does something the tree no longer supports.

## Probes — code

Work from the docs the window grew, then search outward: the slice decides where a finding is anchored, not where its copies may live.

- For each doc that grew, name the facts it states that a reader would act on: a command, a procedure, a rule, a list, a table of options. For each one, search the tree for its distinctive phrases, flags and identifiers, prose and comments included, and list every other doc that states it.
- Where two docs state one fact, decide which one owns it: the file whose purpose the fact serves, the one others already link to, or the one generated from the source. Read both before choosing; the owner is the copy that is current, or the one a fix should make current.
- Do the copies agree? Copies that disagree are a duplication finding whose fix also resolves the disagreement, citing the code that decides which copy is right.
- Is a fact restated where the tree already has a mechanism for one home: a shared block with a parity check, a generated reference, a manifest, a single commands block? A hand-written copy beside that mechanism is the cleanup.
- Is there a "notes", "lessons" or "clarifications" file that shadows another doc's authority? Name the doc it shadows and what it adds that the owner lacks.
- For each feature, command, flag, path or procedure a doc describes, confirm it still exists: search for the symbol, run the help text, read the file. A doc describing something that is gone is obsolete; a doc describing something that changed is `doc-drift`'s.
- Is a whole doc obsolete, or only a section? Recommend removing the smallest unit that is wrong, and name the live doc that already covers anything the removed text still says correctly.
- A merge or removal that needs only edits is cleanup. When choosing the owner needs a decision about the documentation's structure, it is advisory.

## Does not flag

- Confirmations: a doc that is the single home of its facts goes in the coverage note, never in a row.
- A doc's claim that it is canonical, or that another doc is deprecated, taken on trust; read both and decide from content and links.
- Duplication a project deliberately keeps: a shared block registered for parity checking, a mirror a CLAUDE.md line or ADR describes, a generated copy. A finding against it is advisory with `angle = challenge`, naming the decision.
- Restatement at a different altitude or for a different audience, such as a tutorial walking through reference, a one-line summary that links to the full text, or a changelog entry.
- Stylistic consolidation: two docs that could be shorter together but state no fact twice, and preferences about how docs are split or titled.
- Docs outside the scope that the change did not grow, unless a grown doc duplicates them.
- A consolidation whose churn outweighs the gain: two short copies of a fact that rarely changes, where every link to them would have to move.
- Prose that contradicts current behaviour, comments that no longer describe the code, and behaviour no doc covers belong to `doc-drift`; a fact duplicated in code, such as a constant or a list, belongs to `duplication`.

## Edit forms

- `remove` — delete a duplicated passage from the doc that does not own the fact, replacing it with a link to the owner when the reader still needs the pointer; list every copy in `instances`.
- `remove` — delete an obsolete doc or section, naming in `edit` the live doc that covers anything it still says correctly.
- `change` — merge two partial copies into the owner, keeping the union of what is correct, then reduce the other copy to a link.
- `change` — reword a passage that restates a fact so it links to the fact's home instead of copying it.

## Advisory angles

- `direction` — the scope's docs need a restructure, such as one guide split into reference and how-to or a set of per-file copies replaced by a shared block, that is more than edits; state the target layout and its cost.
- `challenge` — a documented decision to keep copies, such as a mirror kept for a tool that cannot follow links, now costs more than it saves; name the decision in `challenges`.
- `alternative` — generating a doc from its source instead of maintaining it by hand.

## Severity calibration

- `critical` — an obsolete doc whose procedure, followed as written, destroys data or breaks the user's setup.
- `major` — duplicated or obsolete guidance that directs readers to a removed command or a superseded procedure on a common path, or copies that disagree about a rule users act on.
- `minor` — an obsolete or duplicated passage on a rarely read path, where the reader would notice the command fails and find the current doc.
