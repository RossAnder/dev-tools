# Idea: Context and cost trims from the 2026-10-21 `/doctor` run

> Captured: 2026-10-21
> Status: Resolved 2026-10-21
>
> 1. `implement-deep` stays on opus. `/implement`'s lite gate now admits M as well as S tasks
>    and caps files at 3. The apply flows' `lite_file_scope` in `tomlctl items clusters` now
>    also caps at 3.
> 2. Skill descriptions are trimmed to ≤320 chars, and the description-routed skills keep
>    their triggers. Listing: 20,954 → 8,106 chars.
> 3. Quoted, along with `review-checker`'s description and seven commands' `argument-hint`.
>    All 56 frontmatters now pass a strict YAML parse.
> 4. Kept linked, and the description is trimmed. 5. The plugin was already `false` in
>    `.claude/settings.json`. 6. No change.
> - The ad hoc plan guide is dropped; the project CLAUDE.md points at the plan-format skill.
>   The SessionStart hook is left to the herdr repo.

## Context

A Claude Code `/doctor` health check run from `reportdesignkit` scanned the 50 most recent
sessions (2026-10-12 → 2026-10-21, 10 projects) and the user-scope config. Everything below
lives in this repo (symlinked into `~/.claude/{agents,skills,commands}`), so it was recorded here
rather than changed in place. Token figures are estimates (chars / 4).

## Findings to consider

1. **`implement-deep` model cost.** `/usage` attributed 37% of usage to `implement-deep`
   subagents; the window saw 438 `implement-deep` dispatches against 76 `implement-lite`.
   `claude/agents/implement-deep.md` pins `model: opus` (`effort: medium`). Options: switch it
   to `sonnet`, or loosen the orchestrators' lite-eligibility gate so more clusters land on
   `implement-lite` (already `sonnet`). Tradeoff: `implement-deep` is the default for
   cross-file, ambiguous-spec and security-sensitive work.
2. **Skill listing size.** The user-scope skills' name + description text is ~19k chars
   (~4.7k est. tokens resident every session); the flow-contract descriptions average ~800
   chars, the longest ~1k (`flow-contract-apply-rollback-protocol`,
   `flow-contract-plan-output-format`, `tomlctl`, `research-methods`). The whole listing is
   ~7.5k est. tokens, past the ~1% listing budget on a 200k window, where entries get truncated.
   These skills are loaded by name from the commands, so their descriptions barely need to route;
   trimming them to ~250 chars would save ~3k est. tokens per session.
3. **Unquoted YAML in `flow-contract-apply-dependency-sort`.** Its `description:` is a plain
   scalar containing `: ` (`…selected ledger items: Kahn's…`), which strict YAML rejects.
   Claude Code parses it today; a stricter parser would drop every frontmatter field. Quote it.
4. **`flow-contract-showcase-bundle` and `/test-bootstrap` are idle.** The skill has zero
   lifetime uses; its only caller, `/test-bootstrap`, has 3 lifetime uses, the last ~2026-07-03.
   Together ~0.23k est. resident tokens. Consider not linking them into `~/.claude` by default
   (link on demand when bootstrapping a project).
5. **lumina plugin idle.** `lumina@dev-tools-local` is enabled at project scope here and has
   had no recorded use since ~2026-06-30 (118 lifetime uses). Disable it via `/plugin` if that
   work has wound down.
6. **Rarely used flow contracts — keep.** `flow-contract-apply-rollback-protocol`,
   `flow-contract-reconciler` and `flow-contract-plan-restructure` saw no use in the window, but
   the apply / plan-update commands load them on specific branches, so they stay.

## Related, outside this repo

- `SessionStart` hook `herdr-agent-state.ps1` hit its 10s timeout twice on `/clear` (11.5s,
  12.6s). Windows PowerShell 5.1 cold start is part of that; `async: true` or `pwsh -NoProfile`
  are the candidate fixes (herdr repo / `~/.claude/settings.json`).
- The ad-hoc Plan Format Guide was removed from `~/.claude/CLAUDE.md` in the same run; the user
  wants it maintained here instead. Its original text is in
  `~/.claude/CLAUDE.md.bak-doctor-2026-10-21` (lines 3–46).
