Status: done — /plan-new points at the human-only After Merge rule; /implement runs `install:` keys, does the landing work, and reports After Merge as user-only steps.

## What changed

- `claude/commands/plan-new.md`, `## Phase 6: Design`, step 5: the parenthetical list of After Merge examples (install a binary, resync a cache, restart sessions, update memory, run a predicted criterion by hand) is replaced by a pointer to the plan-output-format skill's **After Merge** rule (only steps no agent can perform, `_None._` the expected case), plus one clause naming that installs, deploys and config-dir links go under an `install:` key in `## Verification Commands` and follow-on work goes to a backlog capture.
- `claude/commands/implement.md`:
  - Phase 1 extraction: `## Verification Commands` is extracted with its `install:` keys.
  - Phase 3: the ordered list now ends `lint + audit → the plan's install: commands`, with a sentence saying they run last so they run only when everything before passed, a failed install is a `fail` block like any other, and a plan without the key ends at the audit.
  - Phase 4: a new **Landing work.** paragraph after the backlog harvest: update any auto-memory note the run made stale, and capture follow-on work, trials or next plans named in the plan's `## After Merge` or `## Risks` as backlog candidates through the same gate.
  - The paragraph after the report template now says the After Merge bullets are the steps only the user can take, under the plan-output-format **After Merge** rule; "steps the user must take by hand" is gone.

No heading in either file was changed. No new backticked `tomlctl` command was added (the landing-work paragraph reuses the harvest's existing `--origin implement --flow <slug>` flags).

## Deviations

None from a suggested means. The detail gave the outcome per site; each edit follows it directly.

## Handover

None.

## Checks

- Compile check: not applicable (markdown only).
- Accept 1, `! grep -q 'install a binary, resync a cache' claude/commands/plan-new.md`: pass.
- Accept 2, `grep -q 'install:' claude/commands/implement.md`: pass.
- Accept 3, `! grep -q 'steps the user must take by hand' claude/commands/implement.md`: pass.
- Contract C4 check (format contract defines `` `install: <command>` `` and implement names `install:`): pass.

TANGENTIAL: none
