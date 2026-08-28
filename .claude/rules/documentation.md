---
paths:
  - "**/*.rs"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.vue"
  - "**/*.cs"
  - "**/*.js"
  - "**/*.mjs"
  - "**/*.md"
---

# Documentation

The default is to write nothing. An undocumented change is normal.

Before writing a comment, apply the redundancy test: **delete it and hand a reader only the
signature — what can they now get wrong?** If nothing, do not write it. A comment earns its
place only by carrying a constraint the type cannot express, a failure mode invisible in the
signature, a *why*, or machine-consumed semantics (`# Safety`, `[Obsolete]`, `@deprecated`).

- Never restate the signature, the identifier, or the line below.
- Never narrate the change — no past tense, no "previously", no changelog in comments.
- Never argue: no rejected alternatives, no "two reasons", no "X is not the answer". State
  what the code does and one falsifier; the argument goes in a decision record or nowhere.
- Never write a finding id, task ref, plan phase, review round, or agent name into source.
- Never cite `file.ts:NN` — cite the symbol, the test, or nothing.
- Prefer one inline `// why` at the non-obvious line to a doc block that restates the signature.
- Four lines at a declaration, fifteen at a module header. Past twenty, it belongs elsewhere.
- Delete commented-out code. Change a line, and fix its comments in the same edit.
- A number a command can compute → write the command. A measurement → value + date + the
  command that produced it, or delete it. Versions live in manifests.
- `TODO(<owner-or-#ref>)` or not at all — and never describe the replacement design in it.
- Every `unsafe` block gets `// SAFETY:`; every public `unsafe fn` a `# Safety` section.

Before returning from a batch of edits, re-read your own added lines and delete what the
rules above forbid — removal is more reliable than not generating it.

**Doing documentation work — a cleanup pass, an ADR, a README, a docs audit, or setting a
project's policy? Invoke the `documentation-conventions` skill.**
