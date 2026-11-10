Status: done — the review method states blocked_on and its burden of proof, and the verifier's method, agent file and pipeline prompt carry the unblock

## What changed

- `claude/skills/review-v2-method/SKILL.md`
  - The kinds paragraph now says a defect or cleanup that cannot be fixed as written keeps its kind and carries `blocked_on`.
  - The `fix_shape` row is gone from the defect field table. `fix` is now "written as far as it can go" on a blocked row, because `review add` still requires it.
  - New `### Blocked` subsection after cleanup. It says:
    - a finding is fixable by default, and blocked is a claim that must name what is missing;
    - which fields each `blocked_on` value requires, with the caps from the field catalogue (`blocked_missing` ≤400, `blocked_options` 2–6 entries of ≤300 each);
    - a decision with one plainly dominated option is not a decision;
    - anything the finder could settle within its own slice is not missing;
    - `review add` refuses an incomplete block and the retired shape field.
  - Verdicts: a new bullet says a blocked finding's classification is refuted like its claim, with `unblocks` plus `fix_sketch` on a confirmed or plausible verdict, by the verifier only, with no file edits. The single-verdict flags (`--unblocks --fix-sketch-file`) and the NDJSON keys are documented.
  - Return objects: verdict-return may carry `unblocks` and `fix_sketch`, exactly as `review verdict` printed them.
- `references/finding-kinds.md`
  - `fix_shape` is dropped from all five example rows.
  - New "defect — blocked" section with:
    - a defect blocked on `decision`, whose two options each carry their consequence;
    - a cleanup blocked on `evidence`;
    - a "not blocked" case where one option is dominated.
- `references/plan-records.md`: `fix_shape` is dropped from the worked row.
- `references/verifying.md`: new "confirmed and unblocked" example verdict, plus how to send it as a single verdict. It also says that a panel seat or a checker cannot unblock and that a refuted finding cannot be unblocked.
- `claude/agents/review-verifier.md`
  - The description mentions the unblock.
  - New step 7 "Challenge a block" (Decide is now step 8).
  - New **Unblock** paragraph: what to pass, where the sketch is staged, no code edits, verifier only, never on a refutation.
  - The record step lists `--unblocks --fix-sketch-file`, and the return step lists the extra keys.
  - Shared blocks are untouched; parity is OK.
- `claude/workflows/review-v2.js`
  - The verifier prompt now tells the agent to refute a `blocked_on` claim by trying to write the fix.
  - Non-panel prompts give the single-verdict flags with a `payloads/<finding>.sketch.md` path in the run directory, and the batch-line keys. Panel prompts say a seat cannot unblock.
  - `SCRIPT_VERSION` goes from 3 to 4.

## Deviations

None from a `suggested` means. Choices I made:

- I wrote the panel-seat and refuted-verdict restrictions into the method and the prompt. `review verdict` already refuses both (`verdict.rs`, in `apply_one`'s unblocks guard), and spelling them out stops an agent from making a call that is bound to be refused.
- I described the sketch as staged with `--fix-sketch-file` rather than given inline with `--fix-sketch`. This matches how the method already stages rationales.

## Handover

None of my own. The installed `tomlctl` at the time of writing predates the `--unblocks` flags: `tomlctl review schema verdict-return` printed no `unblocks`. The verifier text becomes usable once `cargo install --path tomlctl` is rerun after the milestone lands. The engine's pre-flight already asks for that reinstall.

## Checks

- Compile: no Rust edited. The Workflow script parses cleanly when wrapped as an async function body. Plain `node --check` rejects its top-level `return`, which is the Workflow script form and was already there.
- Accept "the method no longer instructs fix_shape or needs-plan": passes.
- Accept "the verifier agent and the pipeline prompt carry the unblock": passes.
- Accept "the shared blocks are intact" (`bash scripts/verify-shared-blocks.sh`): shared-block parity OK.
- Accept "the Workflow-script and command gates" (`cargo test … cli::dispatch::tests`): Writes build: yes, so it is left to the gate.

## Tangential

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the review verdict flag table may still lack --unblocks, --fix-sketch and --fix-sketch-file (also raised by the verdict task's note) | verifiers reading the flag reference will not see the flags the method now tells them to use
