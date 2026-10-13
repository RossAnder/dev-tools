+++
id = "duplication"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["duplicated-logic", "duplicated-facts"]
activation = "candidate"
slice = "whole-scope"
v1_category = "quality"
+++

# Duplication

## Question

Does the change write down a fact or a piece of logic that already has an owner, so that the next edit to it has to find every copy?

## Owns

`duplicated-logic` — the same logic written more than once where one helper would serve, past the rule of three: a third copy, or a second copy of logic with a known reason to change.

`duplicated-facts` — one fact with more than one owner: a constant, a list, a path, a status vocabulary, a rule or a format written out in several places that must agree.

## Probes — code

- For each constant, list, path, enum-like set or format string the change adds, search the whole tree for the same value. Is there already an owner it should reference?
- Does the change copy a block from elsewhere and edit it slightly? Name every copy, and say what the copies share and what genuinely differs.
- Count the sites before recommending an extraction. Two copies are reported only when they encode one fact that must change together; three or more copies of the same logic qualify on their own.
- Do two similar-looking functions encode the same fact, or different facts that happen to look alike today? Only the first is duplication.
- Does a test fixture, a doc or a script restate a value the code owns, so that it will silently drift? Report the code-side owner and every restatement.
- Does a mirror across layers (a wire type, a schema, a decoder) duplicate a fact the build could derive from one source?
- An extraction that needs only edits (a shared helper, a constant referenced from one place) is cleanup with every site in `instances`. Copies that already disagree, so one of them is wrong for a nameable input, are a defect.

## Does not flag

- Confirmations: a value that is already referenced from its owner is a coverage note, never a finding.
- A comment claiming two copies are kept in sync, taken on trust without comparing them.
- Duplication a comment, ADR or CLAUDE.md line explains, such as a deliberate copy for crate or package independence or a byte-identical block a gate enforces. A finding against it is advisory with `angle = challenge`, naming the decision.
- Two copies of logic that encode different facts, similar text that is not shared knowledge, and an abstraction proposed before the rule of three.
- Single-use helpers that aid clarity, and repeated test setup a test reads better with inline.
- Copies outside the scope that the change did not add to.
- An extraction whose gain is negligible against the coupling it introduces between modules that change for different reasons.
- Naming and pattern divergence belong to `idioms`; duplicated prose guidance to `doc-consolidation`; layer placement of the extracted helper to `architecture`.

## Edit forms

- `change` — replace each copy with a call to one helper, placed where the copies' common caller or data lives, every copy in `instances`.
- `change` — replace each restated value with a reference to the constant or list that owns it.
- `add` — introduce the shared constant or helper in `target_file` when no owner exists yet, together with the `change` rows that point the copies at it.
- `remove` — delete a copy once its callers use the owner.

## Advisory angles

- `direction` — a fact is restated across layers or languages where a generated or derived source would end the drift; state the generation step and its cost.
- `challenge` — a documented decision to keep copies apart now costs more than it saves; name the decision in `challenges`.
- `risk` — two copies agree today but sit in modules that change for different reasons, so a named next change will update one and miss the other.

## Severity calibration

- `critical` — copies that already disagree in a way that loses or corrupts data, such as two writers using different formats for one file.
- `major` — copies that already disagree, so one path gives the wrong result for a nameable input.
- `minor` — copies that agree today but must change together, with a named reason they will need to.
