---
name: flow-contract-plansdirectory-prompt
description: First-use plansDirectory prompt contract — fires at carrier Step 0.5 ONLY when envelope.plans_directory == null (the bootstrap agent normalises both the unset case and the literal "__DONT_ASK__" sentinel to null). Defines option-list construction (always docs/plans/ recommended, other → free-text, and Don't ask again; conditionally .claude/plans/ when the directory exists), recommended-first single-select AskUserQuestion ordering, headless/acceptEdits empty-answer detection (bind docs/plans/ in-memory, persist nothing), the Don't ask again → "__DONT_ASK__" sentinel arbitration, the other → free-text follow-up, the tomlctl json set persist idiom (sidecar-skipped per P16), and the final in-memory bind that downstream phases consume in place of envelope.plans_directory == null. Consult at Step 0.5 of /plan-new, /plan-update, or /review-plan when plansDirectory is unresolved.
---

## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Gate: fire ONLY when `envelope.plans_directory == null` (the bootstrap agent normalises both the unset case AND the literal `"__DONT_ASK__"` sentinel to `null` — see `flow-bootstrap.md` Contract). When non-null, skip this step entirely; the resolved value is already bound for downstream phases. The wording below is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan` (per Task 17 of `docs/plans/flow-tracking-overhaul.md`); do not edit one carrier's copy without mirroring the other two — drift will surface at the next `diff` audit.

1. Build the option list. Always include `docs/plans/` (recommended), `other → free-text`, and `Don't ask again`. Conditionally include `.claude/plans/` ONLY when `[ -d .claude/plans/ ]` returns true at carrier dispatch time (the option must not appear when the directory is absent — listing a non-existent target risks the user picking it).
2. Dispatch `AskUserQuestion` as a single-select (`multiSelect: false`) with the option list from step 1, in the order: `docs/plans/` (recommended) → `.claude/plans/` (when included) → `other → free-text` → `Don't ask again`. Recommended-first ordering follows CLAUDE.md guidance. The upstream `plansDirectory` schema (https://json.schemastore.org/claude-code-settings.json) is string-only, so the persisted value is always a single string — multi-directory configurations require manually adding a `tomlctl.plansDirectories` array to `.claude/settings.json` (see `tomlctl/src/flow/find_plans.rs` for the namespaced key's read precedence) and are out of scope for this prompt.
3. **Headless / `acceptEdits` empty-answer detection**: if the AUQ response is an empty-string answer (per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618), [#29547](https://github.com/anthropics/claude-code/issues/29547)), bind `plans_directory = "docs/plans/"` IN-MEMORY for the remainder of this carrier invocation and DO NOT persist anything — neither the string nor the sentinel. The next interactive session will re-fire this prompt because `settings.json` still lacks the key. Then proceed to step 7 (skip steps 4–6).
4. **Arbitration rule**: if the user selected `Don't ask again`, the persisted value is the literal string `"__DONT_ASK__"`. Otherwise, the persisted value is the chosen path string.
5. **Free-text follow-up**: if the user selected `other → free-text`, dispatch a follow-up `AskUserQuestion` with a single option labelled `Enter directory path` plus the AUQ "Other" affordance to capture the user's typed value. The persisted value is that typed string. If the follow-up returns empty (no path supplied), treat as "skip — use default" (step 7's fallback covers this case — bind in-memory only, do NOT persist); do NOT substitute `docs/plans/` here.
6. **Persist**: write the result to `.claude/settings.json` via:

   ```bash
   cat <<'EOF' | tomlctl json set .claude/settings.json plansDirectory --json -
   <JSON value: a single string — either "__DONT_ASK__" sentinel OR a directory path like "docs/plans/">
   EOF
   ```

   `tomlctl json` skips sidecar maintenance on `settings.json` per P16, so the harness's out-of-band writes (e.g. `/config`) remain compatible.
7. Bind `plans_directory` for downstream phases: if the user selected `Don't ask again` (sentinel persisted) OR the free-text follow-up returned empty (nothing persisted), treat as `"docs/plans/"` in-memory (the default-of-defaults). Otherwise bind the chosen path string as written. Any downstream code that consumed `envelope.plans_directory == null` should now consume this in-memory value.
