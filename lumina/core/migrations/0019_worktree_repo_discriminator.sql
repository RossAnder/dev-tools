-- lumina migration 0019: per-repo live-branch uniqueness + liveness alignment.
--
-- ## Why (review R12 over 0018)
-- 0018's `UNIQUE ON worktrees(branch) WHERE outcome IS NULL` is store-GLOBAL,
-- but lumina is a multi-project store whose projects link DIFFERENT git
-- repositories — the ref-CAS race the index prevents is only real
-- per-repository, so two live worktrees on `main` under two different linked
-- repos must coexist. This migration adds a nullable `repo_link_id` FK to
-- `worktrees` (stamped at create time from the owning sprint's primary repo
-- binding; NULL when none resolves) and rebuilds the index over
-- `(COALESCE(repo_link_id,''), branch)` so the uniqueness bucket is per-repo.
--
-- ## Liveness alignment (the R11 deferral, folded in)
-- 0018's liveness axis was `outcome IS NULL` alone, diverging from the repo
-- layer's universal `deleted_at IS NULL` predicate — a SOFT-DELETED row with a
-- NULL outcome kept squatting its branch, and the verdict tools could not free
-- it (they read a tombstone as NotFound). Since this migration rebuilds the
-- index anyway, the predicate gains `deleted_at IS NULL`: a tombstoned
-- worktree frees its branch exactly like a terminal one.
--
-- ## NO backfill of legacy rows — deliberate
-- Pre-0019 rows keep `repo_link_id` NULL and all share the `''` COALESCE
-- bucket, which preserves today's single-repo GLOBAL semantics exactly (one
-- live worktree per branch across all NULL-stamped rows). Rows only enter a
-- per-repo bucket as new creates resolve a binding.
--
-- ## Predicate / constraint notes
--   * `ALTER TABLE … ADD COLUMN … REFERENCES` is legal in SQLite ONLY with the
--     implicit NULL default (precedent: `findings.repo_id` in 0004), so the
--     column is nullable with NO DEFAULT.
--   * A violation of this EXPRESSION index reports
--     `UNIQUE constraint failed: index 'idx_worktrees_live_branch'` — it names
--     the INDEX, not the column path `worktrees.branch` that 0018's
--     plain-column index produced. The repo layer's typed-Validation matcher
--     (`create_worktree`) matches BOTH shapes.
--   * As in 0018, `CREATE UNIQUE INDEX` validates EXISTING rows and sqlx runs
--     the file in one transaction, so a dirty dev DB fails loudly at startup;
--     the new predicate is strictly LOOSER than 0018's (same bucket plus the
--     deleted_at carve-out), so any rows that satisfied 0018 satisfy this.
--     Forward-only; no down-migration.

ALTER TABLE worktrees ADD COLUMN repo_link_id TEXT REFERENCES repo_links(id);

DROP INDEX idx_worktrees_live_branch;

CREATE UNIQUE INDEX idx_worktrees_live_branch
    ON worktrees(COALESCE(repo_link_id, ''), branch)
    WHERE outcome IS NULL AND branch IS NOT NULL AND deleted_at IS NULL;
