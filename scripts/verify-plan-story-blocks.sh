#!/usr/bin/env bash
#
# verify-plan-story-blocks.sh — drift/coverage gate for the lumina-story-blocks
# plugin's Skill()-dispatch contract (CONVENTIONS §l.4 — now a tombstone for the
# retired inline-replication workaround; the chained runners dispatch via Skill()).
#
# Asserts four invariants and exits non-zero (listing EVERY failure) if any break:
#
#   1. COVERAGE  — every block named in the CONVENTIONS §l.0 six-phase table has
#                  a skills/<name>/SKILL.md (catches a STALE cite to a removed or
#                  renamed block), AND every story-phase skill dir is documented
#                  in §l.0 — modulo the explicit non-phase allowlist below
#                  (catches a FUTURE block added to skills/ but left undocumented).
#   2. CITATION  — plan-story and create-project both cite CONVENTIONS §l.4.
#   3. DISPATCH-DOCUMENTED — both runners document Skill()-dispatch: each carries
#                  at least one `Skill(` dispatch directive (the new canonical
#                  path now that inline-replication is retired).
#   4. NO-FLAG   — `disable-model-invocation:` must not reappear in any skill
#                  SKILL.md (the flag was removed plugin-wide; see CONVENTIONS §a).
#
# Bash + awk (the repo standard is GNU awk / gawk — see the CLAUDE.md Windows
# note: the default Git-Bash mawk is steered around by preferring gawk below;
# the awk used here is POSIX-clean either way). Wired into .githooks/pre-commit.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
PLUGIN="$ROOT/claude/plugins/lumina-story-blocks"
CONV="$PLUGIN/CONVENTIONS.md"
SKILLS="$PLUGIN/skills"
PLAN_STORY="$SKILLS/plan-story/SKILL.md"
CREATE_PROJECT="$SKILLS/create-project/SKILL.md"

# Skills that legitimately sit OUTSIDE the §l.0 six-phase STORY table:
# the chained runners themselves (plan-story walks the phases; create-project),
# the other orchestration skills (§n), the read-only advisors, the mcp
# catalogue, the §m epic/focus writers, and research-notes (the round-1/2
# auxiliary lens writer — §l.0 Phase 2 uses research-explore, not this).
# Adding a skill here is a deliberate, reviewed act.
NON_PHASE="plan-story compose-sprint create-project run-sprint lifecycle \
next-block mcp epic-outcome epic-close-criteria focus-shape focus-framing \
research-notes"

AWK="$(command -v gawk || command -v awk)"

fail=0
err() { printf 'FAIL: %s\n' "$1" >&2; fail=1; }
rel() { printf '%s' "${1#"$ROOT"/}"; }

# --- preconditions -----------------------------------------------------------
for f in "$CONV" "$PLAN_STORY" "$CREATE_PROJECT"; do
  [ -f "$f" ] || err "missing required file: $(rel "$f")"
done
[ -d "$SKILLS" ] || err "missing skills dir: $(rel "$SKILLS")"
if [ "$fail" -ne 0 ]; then
  printf '\nverify-plan-story-blocks: aborting (missing inputs)\n' >&2
  exit 1
fi

# --- 1. COVERAGE -------------------------------------------------------------
# Extract block names from the §l.0 phase table's Blocks column (column 3,
# backtick-quoted). The section is bounded by "The phase table" and the next
# "### "/"## " heading (matched without the multibyte § for locale safety).
l0_blocks="$(
  "$AWK" -F'|' '
    /The phase table/         { inl0=1; next }
    inl0 && /^#{2,3} /         { inl0=0 }
    inl0 && /^\| *[0-9]+\./ {
      col=$3
      while (match(col, /`[^`]+`/)) {
        print substr(col, RSTART+1, RLENGTH-2)
        col=substr(col, RSTART+RLENGTH)
      }
    }
  ' "$CONV" | sort -u
)"

[ -n "$l0_blocks" ] || err "could not parse any blocks from the CONVENTIONS §l.0 phase table"

# 1a. every §l.0 block resolves to a real skills/<name>/SKILL.md.
while IFS= read -r b; do
  [ -n "$b" ] || continue
  [ -f "$SKILLS/$b/SKILL.md" ] \
    || err "§l.0 cites block '$b' but $(rel "$SKILLS")/$b/SKILL.md does not exist (stale cite)"
done < <(printf '%s\n' "$l0_blocks")

# 1b. every skill dir is in §l.0 OR in the documented non-phase allowlist.
for d in "$SKILLS"/*/; do
  name="$(basename "$d")"
  [ -f "$d/SKILL.md" ] || continue
  printf '%s\n' "$l0_blocks" | grep -qx "$name" && continue
  case " $NON_PHASE " in *" $name "*) continue ;; esac
  err "skill '$name' is in neither the §l.0 phase table nor the non-phase allowlist (undocumented — add it to §l.0 or to NON_PHASE)"
done

# --- 2. CITATION -------------------------------------------------------------
for f in "$PLAN_STORY" "$CREATE_PROJECT"; do
  grep -q '§l\.4' "$f" || err "$(rel "$f") does not cite CONVENTIONS §l.4"
done

# --- 3. DISPATCH-DOCUMENTED --------------------------------------------------
# Each runner MUST carry at least one `Skill(` dispatch directive — the new
# canonical path now that inline-replication (§l.4) is retired.
for f in "$PLAN_STORY" "$CREATE_PROJECT"; do
  grep -q 'Skill(' "$f" \
    || err "$(rel "$f") has no Skill( dispatch directive — Skill()-dispatch (§l.4) is now the documented path"
done

# --- 4. NO-FLAG --------------------------------------------------------------
# The flag was removed plugin-wide (see CONVENTIONS §a); guard against any
# SKILL.md reintroducing `disable-model-invocation:` (at any value).
for d in "$SKILLS"/*/; do
  name="$(basename "$d")"
  [ -f "$d/SKILL.md" ] || continue
  grep -q '^disable-model-invocation:' "$d/SKILL.md" \
    && err "skill '$name' reintroduces 'disable-model-invocation:' (removed plugin-wide; see CONVENTIONS §a)"
done

# --- verdict -----------------------------------------------------------------
if [ "$fail" -ne 0 ]; then
  printf '\nverify-plan-story-blocks: FAILED — fix the drift above; do NOT skip the gate.\n' >&2
  exit 1
fi
printf 'verify-plan-story-blocks: OK (§l.0 coverage, §l.4 citations, Skill()-dispatch documented, no disable-model-invocation flag).\n'
