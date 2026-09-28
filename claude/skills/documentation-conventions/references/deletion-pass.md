# The deletion pass

## Scope

Added lines only: `git diff -U0`, filtered to the enforcement scope. Existing comments are removed one at a time, against a named defect below, with the reason stated. Never bulk-strip a file: removing meaningful comments from a file an agent will later read degrades its work on that file, and a comment's *why* is the one thing deletion loses permanently.

## Delete without asking (added lines)

- A comment restating the signature, the identifier, or the line below it.
- Commented-out code.
- A finding id, task ref, plan phase, review round, agent name or plan path.
- Change narration: "previously this ran…", "added per review feedback".
- Argument: a rejected alternative, "two independent reasons", a claim kept only so the comment can refute it.
- Multi-word ALL-CAPS emphasis and decorative banners.
- A bare number a command computes, or a measurement without a date and a producing command.
- An empty ritual section, such as a `# Errors` heading that says "returns an error if this fails".
- A file header summarising contents in a tree whose other files carry none.

## Flag, do not delete

- A comment asserting a *why* not recoverable from the code.
- `# Safety`, `// SAFETY:`, and tool pragmas (`# noqa`, `# type: ignore`, `// nolint`, `eslint-disable`). These are code, not commentary.
- A TODO carrying a lookup key.
- A block over twenty lines that may be a legitimate module header.
- Anything on a line you did not change.

## Block length

Four lines at a declaration, fifteen at a module header, and twenty is the hard stop: past it, move the content to a design doc or decision record and leave a one-line pointer. Splitting one long block into several short ones satisfies the cap and defeats it. A type-only or constant-only module gets a one-line header.

## Pattern greps

Where the repo has a doc gate, such as `scripts/doc-diff-gate.sh`, it owns the argument and history patterns: run it rather than re-deriving them. That gate reads only the staged diff or a commit range (`--range A..B`), so unstaged work is checked by rereading your added lines against the lists above. Never grep the subjunctive, because `would be` matches comments that name their own falsifier, the best class there is. A clean pattern run shows the idiom is absent, not that the volume is right; block length is the volume check.

## Reporting

Report deletions by category and count, and what you flagged. On a clean pass, name the checks that ran.
