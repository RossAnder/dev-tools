---
paths:
  - "**/*.rs"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.js"
  - "**/*.jsx"
  - "**/*.mjs"
  - "**/*.cjs"
  - "**/*.vue"
  - "**/*.svelte"
  - "**/*.astro"
  - "**/*.cs"
  - "**/*.py"
  - "**/*.go"
  - "**/*.java"
  - "**/*.kt"
  - "**/*.sh"
  - ".githooks/*"
  - "**/*.md"
---

# Documentation

Write a comment only when it carries what the signature cannot: a constraint the type can't express, a failure mode, a *why*, or machine-read semantics (`# Safety`, `@deprecated`). Otherwise write nothing.

- Never put a finding id, task ref, plan phase, review round or agent name into source, and never cite `file:NN`; cite the symbol, the test, or nothing.
- State what the code does. Rejected alternatives and change history ("previously…") belong in the commit message or a decision record.
- Change a line, and fix or delete its comments in the same edit. Delete commented-out code.
- A number a command can compute is written as the command; a measurement carries value, date and the producing command. Versions live in manifests.
- `TODO(<owner-or-#ref>)` or not at all.
- Every `unsafe` block gets `// SAFETY:`; every public `unsafe fn` a `# Safety` section.
- Four lines at a declaration, fifteen at a module header; past twenty, it belongs in a doc.

Before returning, reread the comment lines you added against these rules.

For documentation work (a new doc file or README, an ADR, a cleanup or audit, or setting a repo's policy), invoke the `documentation-conventions` skill.
