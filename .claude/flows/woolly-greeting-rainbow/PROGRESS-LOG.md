<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Resolve the tomlctl repo root without spawning git — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | strip-an-inherited-git-environment-from-integration-test-git-spawns | 2026-09-29 | | 2 files |
| E3 | strip-an-inherited-git-environment-from-unit-test-git-spawns | 2026-09-29 | | 3 files |
| E5 | recover-the-dispatch-from-the-head-of-a-long-transcript | 2026-09-29 | | 1 file |
| E6 | add-a-current-user-ownership-check | 2026-09-29 | | 4 files |
| E9 | reap-agents-whose-transcript-went-quiet | 2026-09-29 | | 1 file |
| E11 | document-reaping-and-stale-starts-in-the-agents-reference | 2026-09-29 | | 2 files |
| E12 | discover-the-repo-root-in-process | 2026-09-29 | | 2 files |
| E21 | resolve-the-root-through-the-walk | 2026-09-29 | | 2 files |
| E22 | correct-prose-that-names-the-old-root-lookup | 2026-09-29 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | test_support::git now runs in root via git_command(root) (current_dir) in tomlctl/src/test_support.rs | 2026-09-29 | | the helper sets the working directory with current_dir; the effect is the same | — |
| E7 | owner.rs judges only drive-letter paths (Prefix::Disk / VerbatimDisk); every other path prefix gives Ok(false) | 2026-09-29 | | stricter: prefix-less and volume-GUID paths also hand off to git; canonical verbatim drive paths are still judged | — |
| E8 | orchestrator moved the new mod tests to the end of tomlctl/src/test_support.rs | 2026-09-29 | | clippy::items_after_test_module fails the plan lint command (-D warnings) | — |
| E10 | reap tests named without a reap_ prefix in tomlctl/src/agents/record.rs | 2026-09-29 | | acceptance grep -c fn reap must report 1; reap_-prefixed test names would raise it | — |
| E13 | is-D-a-git-dir hands off whenever D is named .git or holds any HEAD entry, in tomlctl/src/repo_root.rs | 2026-09-29 | | git looks for objects/ and refs/ in the common dir, so a gitdir with no local refs/ would slip past; this hands off more often, never less | — |
| E14 | git-dir validity checks HEAD contents and looks for objects/ + refs/ under the commondir target, in tomlctl/src/repo_root.rs | 2026-09-29 | | a linked worktree gitdir has no local refs/; the spec check would hand off instead of giving Root | — |
| E15 | probe .git with symlink_metadata and hand off on a symlink or junction .git; gitfile read whole and strictly (single line, no stray whitespace, inner .. hands off); ownership also covers the common dir; config.worktree always scanned | 2026-09-29 | | each is stricter and only hands more cases to git | — |
| E20 | task 5 files widened to tomlctl/src/main.rs | 2026-09-29 | | mod repo_root carries cfg_attr(not(test), allow(dead_code)) until io.rs calls discover; the task wiring the caller removes it | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-29 | 29 entries: status-transition × 2, task-completion × 9, deviation × 8, verification × 8, checkpoint × 2 | 161a0d1, 28adb41, 4baa5b3, 9a1f3b9, be12a7d, dc30183, f30ebe1 |
