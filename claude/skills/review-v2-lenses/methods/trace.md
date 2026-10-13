+++
id = "trace"
status = "stable"
+++

# Trace

Establish the contract before judging the code, and read every use of a symbol before making a claim about it. The `research-methods` skill's codebase-review reference holds the v1 form of this procedure; this entry is the one a v2 cell follows.

## Contract first

A defect is a gap between what the code promises and what it does, so find the promise before judging the behaviour. Read, in roughly this order:

- the tests that exercise the code, including the ones that only pin an edge case;
- the types and signatures, and the invariants their constructors enforce;
- the doc comments and the public surface — help text, a schema, an exported name;
- the plan, ADR, CLAUDE.md line or acceptance criterion the cell or pack names.

A finding must name the contract it breaks. A delta from your own preference names none, and is not a defect. When the contract is genuinely silent, the gap is a question for the user — an advisory with `angle = "risk"` or `"direction"` — not a defect.

When the contract and the code disagree, decide which one is wrong before writing. A stale comment against correct code is a cleanup on the comment. Correct prose against wrong code is a defect on the code, and the comment is its evidence.

## Trace before you claim

Never call a symbol unused, dead, unreachable, misused or wrong until you have read its uses across the whole tree:

1. Search for every spelling of the name — call sites, re-exports and aliases, trait and interface implementations, registrations and string lookups, tests, scripts, generated code, and prose that names it. A short or common name needs its qualified forms too.
2. Read the call sites, not just the hit list. A use that passes a different argument, holds a lock, or runs only under a feature flag is what decides the claim.
3. Follow the value one step either way: where the input comes from and where the result goes. A missing check in a function is not a defect when every caller already enforces it, and a correct function is still wrong in use when one caller breaks its precondition.
4. Name what you could not reach. Dynamic dispatch, reflection, FFI, a plugin loaded by name, a macro that builds the identifier: each is a use the search cannot see.

## Recording the result

- The `evidence` entries cite what the trace found — the contract as `path:line`, and the call sites that decide the claim.
- The `counter` field names the strongest reason the claim could still be wrong: the call site you did not read, the dynamic path you could not search, the test that would show the code is fine. A finding with nothing to put there was not traced.
- A dead-code cleanup states the searches that came back empty, so the checker can re-run them.
- A trace that cleared the code becomes a line in the coverage note, never a finding.
