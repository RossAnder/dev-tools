+++
id = "sweep"
status = "stable"
+++

# Sweep

Enumerate every site of a defect or cleanup pattern, so one finding carries the whole class rather than the sample you happened to read. The `research-methods` skill's codebase-review reference holds the v1 form of this procedure; this entry is the one a v2 cell follows.

## When it applies

Any finding whose fix would be the same edit at more than one place: a missing guard repeated at each call, a type built by hand instead of through its constructor, a literal that duplicates a constant, an error collapsed to a default, a renamed symbol still named in prose. One pattern is one finding. Reporting the two sites you saw leaves the rest alive after review, and the implementer meets them mid-edit with no budget to touch them.

## Procedure

1. **Name what the sites share.** Before searching, say what the fix depends on — the call made without the guard, the constructor bypassed, the value duplicated. Search for that property, never for the line you happened to read; a search built from the sample finds the sample.
2. **Search every spelling.** The same pattern hides behind aliases and re-exports, wrapper functions, macro and generic instantiations, method-call and free-function forms, and copies in tests, comments, docs, scripts and generated code. Write two or three variant searches and keep each one verbatim. When a form cannot be searched for at all — reachable only through dynamic dispatch, or assembled at runtime — say which, and treat the enumeration as incomplete.
3. **Search the whole tree.** The cell's slice decides where a finding is anchored; it does not bound the search. A site outside the slice is still an instance of this finding, not a separate item and not a backlog candidate.
4. **Prefer `tomlctl sweep`.** Run it from the repository root with one `-e` per variant search. It walks the git-tracked files and excludes `.claude/**` and `docs/plans/**` by default, so a pattern never matches its own store or plan record. Prefer `(?-u:\b)`, `(?-u:\w)` and `(?-u:\d)` (a Unicode `\b` drops the lazy DFA to the slower PikeVM); the bare forms and `(?i)` compile, but `\p{..}` is refused. Keep each pattern under 512 bytes. Each hit gives a file, a line and the index of the pattern that matched it. Where the verb is unavailable, run the same variants through Grep.
5. **Decide the state.** The enumeration is `complete` only when every form was searchable and the scan was whole. It is `incomplete (≥N)`, with `N` the sites you did establish, when a form could not be searched, when `tomlctl sweep` reports `coverage_complete: false` (a binary, oversize or unreadable file was skipped, or git warned), or when it reports `truncated: true` (the `--max-hits` cap stopped the scan).
6. **Read each hit.** A match is a candidate site, not an instance. Open it and confirm the shared property holds there; drop a match where it does not, and say how many you dropped.

## Recording the result

- **Cleanup** lists every file it edits in `instances` (repo-relative paths, at most 50), and its `edit` names the symbol at each site. `instances` is the apply flow's file budget: a site you leave out is a file the implementer may not touch. A pattern spanning more than 50 files is too wide for one row: split it by subsystem, one row each, or raise it as an advisory when adopting it needs a decision.
- **Defect** names the other sites in `fix`, and one `evidence` entry carries the sweep.
- Either kind records the search strings verbatim and the state, so the apply step can re-run exactly what you ran: `sweep -e '<regex>' -e '<regex>': 14 sites, complete`.
- **One line per sweep in the coverage note, even when it finds nothing.** Give the sweep's purpose, the count and the verdict — `sweep (callers of open_store without lock): 6 sites, all guarded — OK`. Otherwise a sweep that came back empty and a sweep never run look the same, and the cell that skipped its sweeps reads as clean.
