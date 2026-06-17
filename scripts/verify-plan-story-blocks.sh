#!/usr/bin/env bash
#
# verify-plan-story-blocks.sh — drift/coverage gate for the lumina-story-blocks
# plugin's inline-replication contract (story 1A-F5; CONVENTIONS §l.4).
#
# Asserts four invariants and exits non-zero (listing EVERY failure) if any break:
#
#   1. COVERAGE  — every block named in the CONVENTIONS §l.0 six-phase table has
#                  a skills/<name>/SKILL.md (catches a STALE cite to a removed or
#                  renamed block), AND every story-phase skill dir is documented
#                  in §l.0 — modulo the explicit non-phase allowlist below
#                  (catches a FUTURE block added to skills/ but left undocumented).
#   2. CITATION  — plan-story and create-project both cite CONVENTIONS §l.4.
#   3. NO-SOLE-DISPATCH — both runners document inline-replication, and any
#                  `Skill("lumina:...)` mention in a runner is NEGATED (never an
#                  imperative "dispatch via Skill" directive), so Skill()-dispatch
#                  is never presented as THE path to "run" a block.
#   4. FLAG      — every DB-mutating writer SKILL.md retains
#                  `disable-model-invocation: true` (the §a/§n.3 no-auto-fire
#                  guarantee); only the read-only skills in NO_FLAG_OK omit it.
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

# Read-only skills that legitimately OMIT disable-model-invocation (the §a
# read-only exception + the §n.3 lifecycle advisor + the read-only next-block).
NO_FLAG_OK="mcp lifecycle next-block"

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

# --- 3. NO-SOLE-DISPATCH + inline-replication documented ---------------------
for f in "$PLAN_STORY" "$CREATE_PROJECT"; do
  grep -qi 'inline-replicat' "$f" \
    || err "$(rel "$f") does not document inline-replication"
  # Every line mentioning Skill( MUST carry a negation cue; a bare imperative
  # dispatch directive (no cue) is the regression this catches.
  if grep -n 'Skill(' "$f" \
       | grep -vi 'not\|never\|refus\|instead\|rather\|disable-model' \
       >/dev/null; then
    err "$(rel "$f") has a non-negated Skill( dispatch directive — inline-replication (§l.4) must be the documented path, not Skill()-dispatch"
  fi
done

# --- 4. FLAG -----------------------------------------------------------------
for d in "$SKILLS"/*/; do
  name="$(basename "$d")"
  [ -f "$d/SKILL.md" ] || continue
  if case " $NO_FLAG_OK " in *" $name "*) true ;; *) false ;; esac; then
    # read-only skill: assert it does NOT carry the flag (keeps NO_FLAG_OK honest)
    grep -q '^disable-model-invocation: true' "$d/SKILL.md" \
      && err "read-only skill '$name' unexpectedly carries disable-model-invocation: true (remove it from NO_FLAG_OK or drop the flag)"
    continue
  fi
  grep -q '^disable-model-invocation: true' "$d/SKILL.md" \
    || err "DB-mutating skill '$name' is missing 'disable-model-invocation: true' (the §a/§n.3 no-auto-fire guarantee)"
done

# --- verdict -----------------------------------------------------------------
if [ "$fail" -ne 0 ]; then
  printf '\nverify-plan-story-blocks: FAILED — fix the drift above; do NOT skip the gate.\n' >&2
  exit 1
fi
printf 'verify-plan-story-blocks: OK (§l.0 coverage, §l.4 citations, no sole Skill-dispatch, disable-model-invocation flags intact).\n'
