# The deletion pass

Loaded from `SKILL.md` Step 4, and for any cleanup task.

## Scope — read this first

**Run over added lines only.** `git diff -U0`, filtered to the enforcement scope. Never the
whole file.

Bulk-stripping existing comments is forbidden. Removing meaningful comments from a file an
agent will later read measurably degrades its performance on that file — the effect is large
and its sign depends on the task. "Do not generate redundant comments" and "delete the ones
that are there" are different operations, and only the first is safe by default. Existing
comments are removed one at a time, against a named anti-pattern, with the reason stated.

**Removal beats non-generation.** Over-commenting is a model prior that instructions only
partially suppress, so the pass is not optional even when the rules were followed. Run it
before returning, and before any checkpoint commit.

## Delete without asking

On added lines, no confirmation needed:

- A comment restating the signature, the identifier, or the line below it.
- Commented-out code.
- Any finding id, task ref, plan phase, review round, agent name, or plan path.
- Past-tense change narration — "previously this ran…", "signature changed from…", "added per
  review feedback". Git owns this.
- **Argument-closing constructs** (see the patterns below) — the rejected alternative belongs
  in a decision record or nowhere.
- A claim preserved only so the comment can refute it.
- Multi-word ALL-CAPS emphasis; decorative banner rules; a section banner in a file whose
  siblings have none.
- A bare number a command computes; a measurement lacking a date and a producing artefact.
- Empty ritual sections — a `# Errors` heading followed by "returns an error if this fails".
- A file header summarising the file's contents in a tree whose other files carry none.

## Flag, do not delete

- Any comment asserting a *why* not recoverable from the code — this is the only class where
  deletion loses information permanently.
- `# Safety`, `// SAFETY:`, and every tool pragma (`# noqa`, `# type: ignore`, `// nolint`,
  `eslint-disable`). These are code, not commentary.
- A TODO carrying a lookup key.
- A block over 20 lines that may be a legitimate module header — length alone does not
  convict.
- Anything on a line you did not change.

## The argumentative-comment patterns

**Do not grep the subjunctive.** `would be` / `would read` matches the *good* pattern —
comments naming their own falsifier — at roughly 15 hits in 15. Gating it destroys the best
comment class in a corpus.

The discriminator is argument-*closing* construction plus explicit history:

```
(two|three|four) (independent|separate|distinct) (reasons|arguments|grounds)
for (two|three|four) reasons
(either|neither) alone (would|is enough|settles|suffices)
would (settle|suffice|be enough)
is not the (answer|fix|way)
(the temptation to|it is tempting to)
we (considered|rejected|chose not)
one might (be tempted|expect|assume)
(used to be|previously (was|ran|did)|originally (was|ran))
```

Run on comment-prefixed lines only, minus `where\b[^.]{0,40}used to be`. Measured ~5% false
positives. Two patterns were tried and **removed** as net-negative: bare `the naive` (the
technical sense dominates) and bare `either alone` without a closer.

**The high-confidence escalation:** an added block **over 20 lines carrying two or more
markers**. That composite has no false positives in calibration and identifies the shape a
human must delete rather than trim. Name it explicitly when it fires.

**Its honest limit is recall, not precision.** These patterns flag a specific idiom, not the
disease. A clean run is not evidence the comments are fine — block length is the volume
instrument, this is a tripwire. Say so whenever you report a clean result.

## Block length

Four lines at a declaration, fifteen at a module header, **hard stop at twenty**. A block
over twenty lines is a relocation defect regardless of quality: move the content to a design
doc or decision record and leave a one-line pointer.

Density inverts where code is thin — type-only and constant-only modules are the worst
offenders, because they give an agent nothing to write but prose. **A type-only or
constant-only module gets a one-line header, no exceptions.**

Splitting an eighty-line block into four nineteen-line blocks satisfies the cap and defeats
its purpose. If you are tempted, the content belongs elsewhere.

## Reporting

Report what you deleted by category and count, and what you flagged. If the pass found
nothing, say which checks ran — a bare "no issues" is indistinguishable from not looking.
