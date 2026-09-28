# Agent dispatch prompt reference

Detail behind Step 4 of the apply pipeline: what a cluster agent's per-call prompt must carry
beyond what `implement-lite` and `implement-deep` already hold in their system prompts, the
obligations every agent owes regardless of dispatch tier, and the follow-up the orchestrator
runs on a partial apply.

### Agent prompt contract

`implement-lite` and `implement-deep` already carry, in their system prompts, the tag form and
its escalate reasons, the Tier-2 already-applied protocol, the no-overlapping-edits rule, the
external-docs rule, the verification limits, and the report lines (`deviation:`, `note: check`,
`## Files touched` with its `(new)` marker). The per-call prompt carries context, not a restatement
of that method, and MUST include:

- The exact files to read and modify: the cluster's `files[]`, which is the union of each item's `file`, its `instances` files and the files its `description` names, plus any growth the pre-analysis re-sweep found. For a pattern item, list every site as `file:symbol` so the agent works the set rather than rediscovering it.
- Each finding's ledger `id` alongside its `file`, `line`, `symbol`, `category`, `severity`, and
  `summary`, plus an instruction that the agent MUST include the `id` in every result tag.
- The Step-2 pre-analysed reasoning, including the carrier's narration for the categories that
  require it.
- Any API signature or version fact the pre-analysis settled, so the agent checks it rather than
  rediscovering it.
- The carrier's result-tag vocabulary, with the words fixed (past-tense `skipped`, never
  imperative `skip`) and the partial-apply form `applied <ID>: partial — <what landed>;
  pending: <what did not>`. `implement-lite` takes the prompt's words over its own list.
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
- The `DEV SERVER:` line from Step 4's dispatch discipline, for a cluster that changes something
  visible.

Every agent MUST: read the target file(s) in full before changing anything; read surrounding code
so changes match existing patterns and style; make the minimum change that addresses each finding
without refactoring around it; preserve style, naming, and formatting; add an inline comment only
where the fix would otherwise be non-obvious; and skip-and-explain any finding it cannot safely
apply (would break behaviour, unclear semantics, research doesn't hold up on inspection).

**Partial-apply follow-up**: on `applied <ID>: partial — <done>; pending: <not done>` the
orchestrator (a) marks the parent `<APPLIED>` with `resolution = "partial: <done> / pending: <not
done>"`, and (b) mints a child item with `file`, `line`, `symbol` copied from the parent,
`summary = "pending parts of <ID>: <not done>"`, `related = ["<ID>"]`, `status = "open"`. This
gives pending work a first-class tracked ID so it surfaces in future `<PRODUCER>` rounds instead of
being lost to free prose inside the parent's `resolution`.
