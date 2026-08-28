# ADR admission

Loaded from `SKILL.md` Step 3, before opening **or amending** any decision record.

## The discriminator

> Cost to reverse is measured at the moment of reversal, not the moment of decision — and it
> is a function of **dependents**, not of confidence.

Three corollaries that do the actual work:

- **Confidence is orthogonal to admissibility.** A low-confidence one-way door is an ADR; a
  high-confidence choice with no dependents is not. Settling a question is not the same as
  binding future work — conflating the two is what turns every deliberate choice into a record.
- **Fast-moving experimental work fails on the cost *curve*, not the cost *level*.** If
  nothing is layered on the choice, overturning it on day 30 costs what it cost on day 2. A
  flat curve is a two-way door however deliberate the choice was.
- **"Might be overturned later" is not a rejection reason.** A correct ADR can be superseded.
  What disqualifies is the absence of dependents, not the presence of doubt.

## The test

Apply in order. **First failure routes the fact and stops.** No gate is skippable — "I'll
write it anyway, it's cheap" is the failure this test exists to prevent.

| Gate | Test | Fail routes to |
|---|---|---|
| **0. One question** | State it as a single interrogative with a one-sentence answer | You have N decisions. Split and re-run the test on each — most will fail Gate 2 |
| **1. Live alternative** | Name an option a competent contributor would otherwise propose, and one reason it lost that is not "we prefer the other" | Code comment or commit message. There is no decision, only an implementation |
| **2. Dependents** | List what must change on reversal — call sites, modules, wire/schema versions, published contracts, other records. **Count with a command, not from memory** | 0-1 → module doc or code comment. Only-inside-one-experiment → working sheet |
| **3. Rising cost curve** | Will reversal cost *more* in three months, because dependents accumulate, data is written in this shape, or a contract is published? | Flat → two-way door → working-sheet entry |
| **4. Binds someone** | Name the future decision it constrains and the reviewer sentence it enables ("this violates ADR-00NN") | Design doc — which decides nothing — or module doc |
| **5. Not derivable** | Delete the record. Can a reader recover the *rationale* from code + tests + commit message? | Recoverable → delete. The code already says it |
| **6. Not mechanism** | Does the Decision body hold a constant, formula, algorithm, or diagram? | Relocate the mechanism to a design doc and cite it. The record keeps the choice and the cost |
| **7. Decided** | Is discussion still open? | RFC or a grill. A record written to *conduct* the argument is a re-litigation engine |
| **8. Ceiling** | Would this pass ~100 records, or is the log growing faster than the source it governs? | Consolidate before adding. Growth *rate* is the alarm, not the count |

Gate 2's `0-1` threshold is a tunable default — state the counting command, not the number.

## Apply the test to amendments

This is the load-bearing rule, and it is convention-neutral.

Under supersede-by-default the gate fires naturally at each new record. **Under amend-in-place
there is no gate at all unless one is stated**, because "it is about themes, so it goes in
ADR-0020" always resolves true. An ungated amendment intake is how a record accumulates thirty
sub-decisions sharing one status field and one lifecycle.

If a proposed amendment fails Gate 0 — it is not one question with one answer — it is not an
amendment. Split it, and route each part.

## Declare the convention once

A repo picks **supersede** or **amend-in-place** and states it in `docs/adr/README.md` with
its reasoning. Both work; mixing them is the defect. Whichever is chosen:

- An accepted record is **immutable in substance**. Typos and link rot may be fixed;
  conclusions and consequences may not.
- The superseding record (or the amendment) **owns the whole argument**, including why the
  earlier position was wrong. The old record gets exactly one edit: its status line.
- The discussion for a reversal happens **there and nowhere else**. No third file may
  summarise, re-argue, or "clarify" it. If you are writing "as previously discussed" or "note
  that despite ADR-N", stop — you are minting a shadow authority.
- Index supersession chains. A reader who lands on an old record via search must see the
  pointer forward.

## The routing ladder

| Rung | What belongs here | Retention |
|---|---|---|
| Commit message | Why *this change*, now; rejected micro-alternatives; the measurement that justified it | Permanent, immutable, free |
| PR description | Why this *set* of changes; what was descoped | Permanent; never cited from source |
| Code comment | A constraint the type cannot express; one falsifier; a safety obligation | Lives and dies with the line |
| Module doc | The module's contract, invariants, what it deliberately does not do | Lives with the module |
| Design doc | Mechanism, structure, arithmetic. **Decides nothing** | Snapshot; obsolescence expected |
| Working sheet | Provisional choices with no dependents yet | **Deleted at graduation** |
| Decision record | One question, one answer, the rejected alternative, the cost accepted | Immutable |
| RFC | A proposal still under discussion | Until closed; then disposable |

## Working sheets for fast-moving work

Provisional decisions go to `docs/decisions/`, kept separate from `docs/adr/`. One
**entry-keyed** sheet per experiment — not one file per decision, which inflates a number
sequence and demands an index.

A working sheet states its own rules in its header:

- **Latest-wins on conflict**, with earlier entries retained: "where a later bullet
  contradicts an earlier one on the same entry, the later wins — the reasoning that was
  reversed is worth reading."
- **A decision to omit is a decision.** Record it, or the next agent designs the omitted thing.
- **Premise invalidation is propagated**, not silently absorbed: when a shared assumption
  changes, say at the section head which earlier entries it invalidates.
- **Every open entry carries a retirement trigger** — `Revisit if <condition>`, naming the
  lever that would reopen it.

**The expiry rule:** the sheet is deleted when the experiment graduates. The few entries that
acquired dependents are promoted to records *at that moment*, written once with hindsight.
This defers admission to the point where Gate 2 can actually be answered.
