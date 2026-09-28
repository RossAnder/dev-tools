---
name: documentation-conventions
description: Documentation work — writing a new doc file or README, opening or amending an ADR, cleaning up or auditing comments and docs, or setting a repo's documentation policy. Resolves the project's policy by precedence, routes each fact to the one file that owns it, gates ADR admission, and scopes cleanups to added lines. Not for commit messages (use `commit-conventions`) or plan documents (use `flow-contract-plan-output-format`).
---

# Documentation Conventions

The per-line rules, meaning when a comment earns its place and what never goes into source, live in the path-scoped `documentation.md` rule at user or project level, which loads when source or Markdown files are touched. This skill covers the decisions those rules leave open: which policy applies, where a fact lives, whether a decision deserves a record, and how to clean up without destroying information.

## Resolve project policy

First match wins. If `.claude/documentation-conventions.toml` exists, read it with `tomlctl get`, check lint and compiler config (layer 2) only for facts the toml also covers, and skip layers 3–6: they only reconstruct what the toml states.

1. **`.claude/documentation-conventions.toml`**: stage, deliverable, block caps, ADR convention, enforcement scope.
2. **Lint and compiler config**: `Cargo.toml [lints]`, `.oxlintrc.json`, eslint jsdoc rules, `GenerateDocumentationFile` / `NoWarn` in `.csproj` or `Directory.Build.props`.
3. **An existing standard**: `docs/documentation-standards.md`, `docs/adr/README.md`.
4. **`CLAUDE.md` / `CONTRIBUTING.md`**.
5. **Measured baseline**: comment share of *added* lines over recent commits.
6. **Defaults**: stage from first-tag presence; deliverable `code`.

Layer 2 beats layer 1 on any fact it covers, because executable config is what is true, and a config that restates a lint setting is a duplicated fact. When documents still disagree, apply the authority order in `references/fact-ownership.md`; if a tie survives it, name both locations and ask rather than pick.

## Route the fact

Before writing a sentence, name the file that owns the fact; if it is not this one, link to it. One code change should need at most one documentation edit, and the rest are restatements to delete. Read `references/fact-ownership.md` for the ownership table and the volatile-fact rules, and `references/adr-admission.md` before opening or amending any decision record: most decisions fail its test and route to a lower rung.

## Cleanups

Work on added lines, or on existing comments one at a time against a named defect with the reason stated. Never bulk-strip a file. `references/deletion-pass.md` has the delete and flag lists.

## References

| File | Read when |
|---|---|
| `references/fact-ownership.md` | Deciding where a fact lives; two docs disagree; writing a number |
| `references/adr-admission.md` | Considering an ADR, or amending one |
| `references/deletion-pass.md` | Any cleanup or audit |
| `references/stage-and-density.md` | Setting up a repo, or judging whether a tree is over-documented |
| `references/language-cores.md` | Writing doc comments in Rust, C#, TypeScript, React or Vue |
| `templates/documentation-conventions.toml.example` | Generating the per-repo config |
