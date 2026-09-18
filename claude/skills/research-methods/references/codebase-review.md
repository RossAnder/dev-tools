# Judging project code

How a candidate observation about the tree becomes a finding that survives vetting. The lens checklist comes from the dispatch prompt; this is the method under every lens.

## Contract first

Establish what the code is supposed to do before judging what it does: the tests, the types, the doc comments, the public surface, and the plan or ADR the prompt names. A defect is a gap between contract and behaviour. A delta from your own preference is not a defect, and a finding that cannot name the contract it violates is a suggestion at most.

## Boundaries are where defects live

Work the edges of the scope rather than its middle:

- **Inputs**: validation, encoding, path handling, and on this platform CRLF and Windows paths.
- **Error paths**: swallowed errors, partial writes with no rollback, a `Result` mapped to a default, an `unwrap` on user-controlled data.
- **Concurrency and ordering**: shared state, lock order across modules, a write that assumes a read has not raced it.
- **Resource lifetimes**: files, handles, temp directories, child processes, and what happens on the early-return path.
- **Cross-layer invariants**: a field added in one layer needs its decoder, its query, its wire mirror and its construction sites. Grep for the siblings of the thing that changed before declaring the change complete.

## Trace before you claim

Never report a symbol as unused, dead, unreachable or wrong without grepping for it across the whole tree, including tests, generated code, scripts and prose that names it. A call site you did not read is the Counter line of the finding.

## Sweep for every instance

A defect with the same fix at more than one site is reported once, with every site. Reporting the two you saw is what leaves the class alive after review and what makes an implementer discover the other seventeen mid-edit.

- **Derive the sweep from the defect, not the sample.** Ask what the sites have in common that the fix depends on: a call to one function without a guard, a type constructed by hand instead of through its constructor, a string literal that duplicates a constant, an error mapped to a default. Then write searches for that, not for the exact line you read.
- **Cover the forms.** The same defect hides behind aliases, re-exports, wrapper functions, macro or generic instantiations, a method-call spelling and a free-function spelling, and copies in tests, prose and generated code. Run two or three variant searches and say which. When a form cannot be searched for, such as a behaviour reachable only through dynamic dispatch, say so and mark the enumeration incomplete with the floor you did establish.
- **Search the whole tree.** The dispatch scope bounds where findings anchor; the sweep does not stop at its edge. A site outside scope is an instance of this finding, never a backlog item.
- **Report the set, the search and the state.** Every site as `file:symbol`, the search strings verbatim so the orchestrator can re-run them at apply time, and `complete` or `incomplete (≥N)`. The set is the apply flow's file budget, so an instance you leave out is a file the implementer is not allowed to touch.

When the installed `tomlctl` is 0.9.0 or newer, the preferred enumerator is `tomlctl sweep`, run from the repo root with one `-e` per variant search:

```bash
tomlctl sweep -e <regex> [-e <regex>]... [--exclude <glob>] [--max-file-bytes <bytes>] [--max-hits <n>]
```

Its `hits` map one-to-one onto the `Instances` line — each as `file:line`, re-anchored to `file:symbol` where a symbol exists — and the `-e` patterns go verbatim onto `sweep`, so the orchestrator re-runs exactly what you ran. The shipped binary has no Unicode tables, so every pattern uses the ASCII forms `(?-u:\b)` / `(?-u:\w)` / `(?-u:\d)`, and a pattern is capped at 512 bytes. `.claude/**` and `docs/plans/**` are excluded by default, so a pattern never matches its own ledger or plan record. Without the verb, the Grep sweep above is the fallback.

The output also decides the state: `coverage_complete: false` or `truncated: true` means the enumeration is `incomplete`. The first says a skipped file (binary, oversize or unreadable, counted under `skipped`) or a git warning at exit 0 may hide a site; the second says the `--max-hits` cap cut the scan short. In either case state the floor as `incomplete (≥N)`, exactly as for a form the search cannot reach.

## Idiom questions are history questions

Whether a pattern is the project's converged idiom or one module's drift is decided by `git log --oneline -S'<pattern>' -- <path>` and `git blame`, not by which file you read first. A finding that canonises the outlier is a finding the user reverses. When the history shows a deliberate move away from a pattern, that is a deliberate-choice drop under the skill's bar.

## Deduplicate against tooling and against the ledger

- When the prompt carries a `LINT BASELINE` fence, that is the linter's verdict for the scope: drop any candidate it already reports, and do not re-run the command, since sibling lenses share the build lock. Without a fence, run the narrow linter for the language once: `cargo clippy -p <crate>`, `bun run type-check`, `tsc --noEmit -p <config>`, `<linter> <path>`. Baseline it first; a command already red poisons every claim downstream.
- When the prompt names a ledger, list its open, deferred and wontfix items before writing (`tomlctl items list <ledger> --where status=open`, and again for the other two). A candidate that matches one becomes a `related` reference, not a new item.

## Lens notes

- **Performance**: prove the path is hot before optimising it. The evidence ladder is a benchmark or profile, then a complexity argument tied to a real input size, then an allocation count, then intuition. A finding without a hot-path argument is `low — hypothesis` and its Counter names the measurement. Do not build a benchmark; run one that exists.
- **Architecture and DRY**: coupling direction, layering violations, and duplicated knowledge rather than duplicated text. DRY is about facts with one owner, so two similar functions that encode different facts are not a violation. Apply the rule of three before recommending an abstraction.
- **Security**: think in trust boundaries. Where does untrusted data enter, what does it reach, and what asserts it on the way? A finding names the boundary and the missing check; a generic checklist item with no boundary attached is dropped.
- **Completeness and testability**: a gap is a stated requirement with no implementation or no test, cited from the plan, the acceptance line or the doc that states it. An untested private helper is not a gap.
- **Plan against tree**: verify every path, symbol and line the plan cites exists as described. Check task order for a task sequenced after the change that makes it a no-op. Check file claims across tasks marked parallel for collisions. Confirm each acceptance command is runnable as written.

## Read-only, always

No mutation of the repo, the environment or shared state: no redirection into tracked files, no `git add`, `commit`, `checkout`, `reset`, `stash` or `clean`, no installs, migrations, formatters, codegen, or long-running servers. Scratch goes in the session scratchpad. Keep commands narrow: sibling lenses share one `target/` and one working tree, so a whole-crate build or a full suite serialises everyone. When only a full build or suite decides a claim, say so in the Counter line and leave it to the orchestrator's verification step. On a transient or environmental failure, note it and move on rather than retry.
