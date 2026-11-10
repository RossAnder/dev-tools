Status: done — settle drain and apply-row failure rules added to the engines, workers and carriers

## What changed

- `claude/skills/implement-v2-engine/references/findings.md`: new sections "Settle drain" (read each proposal file and its citations, fail a row whose file opens neither word, asks for more investigation or does not answer `blocked_missing`; tier `mechanical`/`cited`/`user`; one `AskUserQuestion` point for user-tier proposals and unconfirmed refutations; `review settle --accept --tier --fix-file`, `--refute --citation`, `--defer --proposal`; `review schedule` of the accepted findings into the checkpoint that follows the settle milestone) and "Failure on an apply row" (no reclassification, parked read as failed unless a handover reason, lite moves to deep with spec unchanged on the first failure, deep re-specs on the first, second failure parks; failures counted from `[[changes]]` reasons opening `apply failure <n>: `). Both sit between "Reproduce check" and "Reconcile".
- `claude/skills/implement-v2-engine/references/drain.md`: two pointers only — the Never-up-a-tier paragraph points apply rows at findings.md, and step 6's routing sentence goes by way of the settle drain when a row settles a finding.
- `claude/skills/implement-v2-engine/SKILL.md`: step-table row "Settle drain"; the role rule on escalation names the apply-row exception; the user-question bullet names the settle question; the loop sentence, the findings.md reference summary and Re-entry's failed-rows paragraph point at the new sections.
- `claude/agents/plan-worker-lite.md`, `claude/agents/plan-worker-deep.md`: identical new section "Tasks that answer review findings" inside the `plan-worker-procedure` block — a fix task cannot reclassify (needs a decision/evidence is `failed`; `parked` only for a handover-kind step), and a settle task writes its run note plus the proposal file opening `Proposal:` or `Refuted:`, answering the `Blocked on evidence:` line, citing what it read and proposing an edit-level fix.
- `claude/skills/apply-v2-engine/SKILL.md`: preview rule moved from `fix_shape = needs-plan` to `blocked_on` (evidence-blocked findings with no proposal get a settle task); step 5 adds `settle-<n>` (no gates) before `apply-<n>`, the id `n` free of both names; step 6 passes `--settle-checkpoint settle-<n>` and reports the verb's `settle` rows; Re-entry does not reuse an empty `apply-<n>` whose `settle-<n>` holds rows; Out of scope rewritten; description and step table name the settle milestone.
- `claude/commands/implement-v2.md`, `claude/commands/apply-v2.md`: pre-flight feature lists gain `review_blocked_on`, `review_settle`, `review_triage_answer`; apply-v2's intro names the settle path.

## Deviations

None from a `suggested` means (the task carries none). Choices made where the detail left the edit open:

- The fix milestone of a settle milestone is found as the checkpoint that follows it in store order (the apply skill adds them adjacent), rather than parsing a milestone goal or adding a store field. Alternative weighed: a goal-text convention, rejected as fragile.
- Failure counting uses a `reason` prefix `apply failure <n>: ` on the replan's audited update, the same prefix-count idiom as the gate's and reproduce check's rounds.
- A `Refuted:` whose citation does not hold up on reading is put to the user with the user-tier proposals, since the design only covers a cited refutation.
- With every selected finding evidence-blocked, the apply skill resolves the fix milestone's gates from the blocked findings' `file` paths, so the "no test command" stop does not fire on an empty edited-path set.

## Handover

None.

## Checks

- Compile check: none; only markdown edited.
- `grep -q 'review settle' … findings.md && grep -qi 'second failure' … findings.md`: pass.
- `grep -q 'Proposal:'` in both worker files: pass.
- `grep -q -- '--settle-checkpoint' claude/skills/apply-v2-engine/SKILL.md`: pass.
- `bash scripts/verify-shared-blocks.sh`: `shared-block parity: OK`.
- Every new `tomlctl` invocation's flags checked against `--help` of the working-tree build (`review settle`, `review schedule --settle-checkpoint`, `plan milestone add` without `--gate`) and the new `items list` reads run against this flow's `tasks.toml` (exit 0).
- The three feature names are not yet in `tomlctl/src/cli/types/mod.rs`; the sibling version-bump task adds them. Until it lands, the carriers' pre-flights would stop on an installed binary.

TANGENTIAL: none
