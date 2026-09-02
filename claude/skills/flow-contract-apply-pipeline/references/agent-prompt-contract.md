# Agent dispatch prompt reference

Detail behind Step 4 of the apply pipeline: what a cluster agent's per-call prompt must carry
beyond what `implement-lite` and `implement-deep` already hold in their system prompts, the
obligations every agent owes regardless of dispatch tier, and the follow-up the orchestrator
runs on a partial apply.

### Agent prompt contract

`implement-lite` and `implement-deep` already carry the applied/skipped tag form, the Tier-2
already-applied protocol, the no-overlapping-edits rule, and plan-deviation reporting in their
system prompts. The per-call prompt restates only the carrier-specific vocabulary, and MUST include:

- The exact files to read and modify.
- Each finding's ledger `id` alongside its `file`, `line`, `symbol`, `category`, `severity`, and
  `summary`, plus an instruction that the agent MUST include the `id` in every result tag.
- The Step-2 pre-analysed reasoning, including the carrier's narration for the categories that
  require it.
- The resolved flow's `slug` and `scope` globs, so the agent can detect deviations.
- "Reason through each change step by step before editing."
- "You MUST use Context7 MCP tools (resolve-library-id then query-docs) to verify API signatures
  and correct usage for any new APIs before writing code — do not rely on training data alone."
- "You MUST use WebSearch if the recommended approach needs clarification or you are unsure about
  the correct implementation."
- The carrier's result-tag vocabulary, with the words fixed (past-tense `skipped`, never
  imperative `skip`) and the partial-apply form `applied <ID>: partial — <what was done>;
  skipped parts: <what wasn't>`.
- The hard rule, in the carrier's vocabulary: no `Edit` / `Write` / `MultiEdit` call for an item
  means the agent MUST NOT tag it `applied`.
- The Tier-2 protocol: when the orchestrator set `uncertain_already_applied = true` for an item,
  the agent's FIRST action for it is a read-verification pass against the recommended fix using
  structural judgement — reordered independent clauses, equivalent refactorings, paraphrased API
  choices, and moved-but-otherwise-identical code all count as "in place" — after which it either
  reports the item already-in-place writing zero bytes, or proceeds with a normal apply.
- "Do NOT quote diff lines containing credentials, keys, or tokens in `resolution` /
  rationale / note text. Paraphrase instead — e.g. 'removed hard-coded credential (paraphrased)'
  rather than quoting the literal value."
- "If you apply a finding that touches a file matching any `scope` glob in the resolved flow's
  `context.toml`, classify the change as a plan deviation and report it with the prefix
  `deviation:` followed by the item's ledger `id`, file, applied-fix summary, and the plan
  expectation it diverges from."

Every agent MUST: read the target file(s) in full before changing anything; read surrounding code
so changes match existing patterns and style; make the minimum change that addresses each finding
without refactoring around it; preserve style, naming, and formatting; add an inline comment only
where the fix would otherwise be non-obvious; and skip-and-explain any finding it cannot safely
apply (would break behaviour, unclear semantics, research doesn't hold up on inspection).

**Partial-apply follow-up**: on `applied <ID>: partial — <done>; skipped parts: <not done>` the
orchestrator (a) marks the parent `<APPLIED>` with `resolution = "partial: <done> / pending: <not
done>"`, and (b) mints a child item with `file`, `line`, `symbol` copied from the parent,
`summary = "pending parts of <ID>: <not done>"`, `related = ["<ID>"]`, `status = "open"`. This
gives pending work a first-class tracked ID so it surfaces in future `<PRODUCER>` rounds instead of
being lost to free prose inside the parent's `resolution`.
