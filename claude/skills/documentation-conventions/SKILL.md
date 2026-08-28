---
name: documentation-conventions
description: ALWAYS invoke before adding a comment block longer than two lines, creating any `.md` file, writing a doc comment, adding a TODO, or putting a measurement, count, or version number into prose — and before any documentation cleanup, audit, or ADR. DO NOT write project documentation directly. Resolves the project's documentation policy by precedence (`.claude/documentation-conventions.toml` → lint and compiler config → an existing docs standard → CLAUDE.md → measured baseline → defaults), applies the redundancy test to every proposed comment, routes each fact to the one file that owns it, and runs a deletion pass over newly-added lines. Not for commit messages (use `commit-conventions`) or plan documents (use `flow-contract-plan-output-format`).
---

# Documentation Conventions

## When to apply

- About to write a comment block longer than two lines, or any doc comment.
- About to create a `.md` file, an ADR, or a section in an existing doc.
- About to write a TODO, a stability marker, or a "not settled yet" note.
- About to put a number, measurement, version, or count into prose or a comment.
- Finishing a batch of edits — run the deletion pass (Step 4) before returning.
- The user asks to document, clean up comments, audit docs, or add an ADR.

**The default is to write nothing.** Documentation is justified per item, not per change.
An undocumented change is normal; a documented one needs a reason that survives Step 2.

## Step 1: Resolve project policy

First match wins. **If `.claude/documentation-conventions.toml` exists, read it and STOP —
do not open any reference file for policy.** Layers 2-6 exist only to reconstruct what it
states outright.

1. **`.claude/documentation-conventions.toml`** — authoritative for stage, deliverable,
   block caps, ADR convention, enforcement scope. Read via `tomlctl get`.
2. **Lint and compiler config** — `Cargo.toml [lints]`, `.oxlintrc.json`, eslint jsdoc
   rules, `GenerateDocumentationFile` / `NoWarn` in `.csproj` or `Directory.Build.props`.
3. **An existing project standard** — `docs/documentation-standards.md`, `docs/adr/README.md`.
4. **`CLAUDE.md` / `CONTRIBUTING.md`** — prose scan.
5. **Measured baseline** — comment share of *added* lines over recent commits.
6. **Defaults** — stage from first tag presence; deliverable `code`.

**Layer 2 beats layer 1 on any fact it covers.** This inverts `commit-conventions`, and it
is deliberate: executable reality wins over prose. A config that restates a lint setting is
the duplicated-fact defect this skill exists to prevent — the config *names* the file that
owns each setting, it never copies the value.

When two documents disagree and the precedence above does not settle it, apply the authority
order in `references/fact-ownership.md`. If a tie survives it, **stop and ask** — never pick
silently. The failure mode is not choosing wrong, it is deliberating.

## Step 2: The redundancy test

> **Delete the comment and hand a reader only the signature. What can they now get wrong?**
> If the answer is "nothing", the comment is redundant. Do not write it.

A comment earns its place only by carrying one of four things the signature cannot:

1. **A constraint the type cannot express** — units, ranges, invariants, encoding, ownership.
2. **A failure mode invisible in the signature** — what it throws, panics on, blocks on,
   allocates, or writes.
3. **Why, not what** — the constraint imposed elsewhere, the workaround for a named external
   bug, the reason a value is what it is.
4. **Machine-consumed semantics** — `# Safety`, `[Obsolete]`, `@deprecated`, release tags.

**Name a falsifier.** The best comments state what would invalidate them and where the check
lives — "a px literal here would mean the clamp had run; `barMetrics.test.ts` redoes the sum".
A comment that cannot be falsified is decoration.

**Never argue.** No rejected alternatives, no "two independent reasons", no "X is not the
answer", no counterfactual case for the design. State what the code does and one falsifier;
the argument belongs in a decision record or nowhere. Never preserve a wrong claim in order
to correct it — delete it and state the right one once.

**Prefer one inline `// why` at the non-obvious line to a doc block above the function.** A
doc block on a private or internal symbol that restates its name, parameters, and return type
is the default failure mode of generated documentation.

## Step 3: Route the fact

Before writing a sentence, name the file that **owns** that fact. If it is not this one, link
instead. One code change should require at most one documentation edit — if it requires N,
the N−1 are restatements to delete.

Read `references/fact-ownership.md` for the ownership table, the volatile-fact rules
(measurements, counts, versions), and the authority order.

For a decision record, read `references/adr-admission.md` **before** opening an ADR. Its
eight-gate test is strict by design and applies to every amendment as well as every new
record; most decisions fail it and route to a lower rung.

## Step 4: Deletion pass

Run over **your own diff, added lines only** — never the whole file, and never bulk-strip
existing comments. Read `references/deletion-pass.md` for the delete-without-asking list, the
flag-only list, and the greps.

## Rules that do not vary

Regardless of stage, language, or config:

- Every `unsafe` block gets a `// SAFETY:` naming the invariant; every public `unsafe fn` a
  `# Safety` section. Never suppress `clippy::missing_safety_doc`.
- Never write a finding id, task ref, plan phase, review round, or agent name into source.
- Never cite `file.ts:NN` from a comment — cite the symbol, the test, or nothing.
- Every measurement carries value + date + the command that produced it, or is deleted.
- If a number can be computed by a command, write the command, not the number.
- Delete commented-out code. Git holds it.
- When you change a line, update or delete every comment in its block in the same edit.
- Every forward reference carries a lookup key (`TODO(<owner-or-#ref>)`) or is not written.
- Never describe a replacement design in a TODO — state only that the shape is provisional.
- Never paste a chat response into `docs/`.

## References

| File | Read when |
|---|---|
| `references/fact-ownership.md` | Deciding where a fact lives; two docs disagree; writing a number |
| `references/adr-admission.md` | Considering an ADR, or amending one |
| `references/deletion-pass.md` | Step 4, or any cleanup task |
| `references/stage-and-density.md` | Setting up a repo, or judging whether a tree is over-documented |
| `references/language-cores.md` | Writing doc comments in Rust, C#, TS, React, or Vue |
| `templates/documentation-conventions.toml.example` | Generating the per-repo config |
