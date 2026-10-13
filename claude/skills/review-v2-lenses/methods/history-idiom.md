+++
id = "history-idiom"
status = "stable"
+++

# History idiom

Decide from history, not from the first file you opened, whether a pattern is the project's converged idiom or one module's drift. The `research-methods` skill's codebase-review reference holds the v1 form of this procedure; this entry is the one a v2 cell follows.

## When it applies

Any finding that says code should look like other code: naming, error-handling style, module layout, a helper that should be used in place of hand-rolled logic, a test idiom. The first file you open is not a fair sample. A finding that pushes the converged code toward an outlier is one the user reverses.

## Procedure

1. **Count both spellings now.** Search the tree for the pattern and for its alternative, scoped to the language and layer in question. A lopsided count is a lead, not a verdict: the minority may be the new direction.
2. **Ask the history which way it is moving.** Run `git log --oneline -S'<pattern>' -- <path>` for each spelling to see when occurrences were added and removed, and read the subjects of the commits that changed the count. Recent commits that replace A with B make B the idiom, whatever the count says today.
3. **Blame the outlier.** Use `git blame` on the lines that differ. A line from an old, untouched commit is drift left behind. A line from a recent commit that changed this pattern on purpose may be the start of a migration.
4. **Look for a stated rule.** A CLAUDE.md line, an ADR, a lint configuration or a comment at the site settles it ahead of either count.

## Recording the result

- **Converged idiom, outlier drifted** — a cleanup with `direction = "change"` that moves the outlier to the idiom. Name the commits or the count in the `edit`. A defect only when the drift itself produces wrong behaviour.
- **History shows a deliberate move away from a pattern** — the newer form is the idiom, and nothing gets flagged for following it. A finding that argues for the old form is an advisory with `angle = "challenge"`, and its `challenges` names the commit or decision that made the move.
- **Mid-migration, both forms live** — not a per-site cleanup. If the migration has stalled, raise it once as an advisory with `angle = "direction"`.
- **History inconclusive** — no finding. Put a line in the coverage note naming the pattern and the searches.
