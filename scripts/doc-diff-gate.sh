#!/usr/bin/env bash
set -euo pipefail

# Documentation gate over STAGED ADDED LINES ONLY.
#
# Scope is deliberate: the regression this catches is in newly-added code, not in
# the existing corpus. A repo-wide cap would fail on day one against a backlog
# nobody is going to clear, get bypassed with --no-verify, and take the whole
# hook with it. Added lines start clean, so the retrofit cost is zero.
#
# Pairs with the `documentation-conventions` skill: the skill carries the
# judgement (the redundancy test, where a fact belongs, what a comment must
# earn), this carries the seven things a machine can actually decide.
#
# DELIBERATELY OUT OF SCOPE (NOT gated here — the skill's prose carries these
# alone, and a clean run is NOT evidence they hold):
#   (a) the redundancy test — whether a comment restates its signature. No
#       linter in any of these ecosystems expresses it.
#   (b) whether a long block is EARNED. G1 counts lines; a 189-line module
#       header may be entirely legitimate.
#   (c) same-altitude duplication — the same fact asserted in six files.
#       Detecting it needs semantic matching, which is not a gate.
#   (d) whether a rejected alternative was RELOCATED or merely deleted.
#   (e) `file.ts:NN` citation drift — valid when written, silently wrong after
#       any edit above it. A gate here would be a nag, not a check.
#
# RECALL WARNING, and it is the important one: G2/G3 flag a specific IDIOM, not
# the disease. They were calibrated at ~5% false positives and correspondingly
# low recall. G1 is the volume instrument. A clean G2 run means "the idiom did
# not appear", never "the comments are fine".
#
# Modes: warn (default — report, exit 0) | block (exit 1 on any BLOCK finding).
# Set DOC_GATE_MODE=block, or pass --block, once the FP rate is proven in anger.

MODE="${DOC_GATE_MODE:-warn}"
SELFTEST=0
for arg in "$@"; do
  case "$arg" in
    --block) MODE=block ;;
    --warn) MODE=warn ;;
    --self-test) SELFTEST=1 ;;
    *) echo "doc-diff-gate: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "doc-diff-gate: not inside a git repository" >&2; exit 2; }

AWK="$(command -v gawk || command -v awk)"

# Source extensions carrying comments we gate. Markdown is included for G6 only.
SRC_RE='\.(rs|ts|tsx|vue|cs|js|mjs|cjs|svelte|astro)$'
MD_RE='\.md$'

# Paths exempt from every check. A reviewed constant: adding a line here is a
# deliberate act, not a convenience.
#   - generated trees: nothing here is hand-written, so no rule applies
#   - docs/plans + .claude/flows: harness flow output, governed by its own
#     retention rules rather than by comment discipline
EXCLUDE_RE='(^|/)(node_modules|target|dist|build|\.lumina)/|\.d\.ts$|(^|/)docs/plans/|(^|/)\.claude/flows/'

# Ledger-id prefixes that are NOT findings in this repo. `R<n>` is a durable
# lumina requirement id and is cited deliberately; `E<n>` is an execution-record
# entry. The gated prefixes below are flow-local and unresolvable once reaped.
# Per-repo: narrow or widen this, do not delete the check.
LEDGER_DENY='[OWTP]'

findings=0
blockers=0
report() { # severity, file:line, message
  printf '  [%s] %s\n    %s\n' "$1" "$2" "$3"
  findings=$((findings + 1))
  [ "$1" = "BLOCK" ] && blockers=$((blockers + 1))
  return 0
}

# --- added-line extraction ---------------------------------------------------
# Emits "path<TAB>lineno<TAB>content" for every added line in the staged diff.
added_lines() {
  git diff --cached -U0 --no-color --diff-filter=ACM -- "$@" |
    "$AWK" '
      /^\+\+\+ b\// { file = substr($0, 7); next }
      /^@@ / {
        # @@ -a,b +c,d @@
        match($0, /\+[0-9]+/); n = substr($0, RSTART + 1, RLENGTH - 1) + 0
        next
      }
      /^\+/ && !/^\+\+\+/ {
        printf "%s\t%d\t%s\n", file, n, substr($0, 2); n++
      }
    '
}

# --- patterns ----------------------------------------------------------------
# Defined ONCE, above both the self-test and the checks, so the test can never
# assert against a different pattern than the one that ships. An earlier version
# defined them twice and the two copies diverged: the self-test's were lowercase,
# the real check's carried capitals, and G5 silently never fired while its test
# reported OK.
#
# All patterns except the case-sensitive ones (G4 ledger ids, G8 ALL-CAPS) are
# matched against tolower(line) and MUST therefore be written lowercase.
ARG_RE='(two|three|four) (independent|separate|distinct) (reasons|arguments|grounds)|for (two|three|four) reasons|(either|neither) alone (would|is enough|settles|suffices)|would (settle|suffice|be enough)|is not the (answer|fix|way)|(the temptation to|it is tempting to)|we (considered|rejected|chose not)|one might (be tempted|expect|assume)'
# "previously this ran ..." — an intervening subject is the common form, so the
# verb cannot be anchored directly to the adverb.
HIST_RE='(used to (be|have|live|run)|previously[[:space:]]+([a-z]+[[:space:]]+)?(was|were|ran|did|lived|had)|originally[[:space:]]+([a-z]+[[:space:]]+)?(was|were|ran|lived))'
PHASE_RE='(phase [0-9]+[.][0-9]+|user decision [0-9]+|adr-[0-9]+ d[0-9]+)'
CHAT_RE="(your (breakdown|approach|implementation) is|you're effectively|let me know if you|i hope this helps|great question)"
MEAS_RE='(measured|benchmark|benchmarked|profiled|regressed|speedup|[0-9]+x (faster|slower)|[0-9]+% (faster|slower))'
DATE_RE='(19|20)[0-9]{2}-[0-9]{2}-[0-9]{2}'
LEDGER_RE="^[[:space:]]*(//|[*]|#)[[:space:]]*$LEDGER_DENY[0-9]{1,3}([.][0-9]+)?[-:]"
CAPS_RE='[A-Z]{2,}[[:space:]]+[A-Z]{2,}[[:space:]]+[A-Z]{2,}'
BANNER_RE='(=|-){12,}'

# --- self-test ---------------------------------------------------------------
# A checker that cannot demonstrate it fires is a checker that reports OK on a
# broken tree. Every pattern below is asserted against a known-bad specimen.
if [ "$SELFTEST" -eq 1 ]; then
  st_fail=0
  # Runs each pattern through the SAME awk + tolower() path as the real check.
  # An earlier version used `grep -i`, which is more permissive than the real
  # matcher and silently passed a pattern carrying capitals that could never fire
  # against tolower(line). A self-test looser than the check it guards is worse
  # than none. Pass a 4th arg to assert case-sensitively.
  st() { # label, specimen, pattern, [any-4th-arg = case-sensitive]
    if printf '%s\n' "$2" |
       "$AWK" -v pat="$3" -v cs="${4:-}" \
         '{ s = (cs == "" ? tolower($0) : $0); if (s ~ pat) { found = 1 } }
          END { exit(found ? 0 : 1) }'; then
      printf 'selftest ok:   %s\n' "$1"
    else
      printf 'selftest FAIL: %s (pattern did not fire on its own specimen)\n' "$1" >&2
      st_fail=1
    fi
  }
  st "G2 argument-closing"   '// Two independent reasons, and either alone would settle it:' "$ARG_RE"
  st "G2 not-the-answer"     '// The top-level test.api is not the answer either.'            "$ARG_RE"
  st "G3 history"            '// O22: previously this ran synchronously at module import'     "$HIST_RE"
  st "G4 ledger id"          '// O22: coalesce the in-flight run'                             "$LEDGER_RE" cs
  st "G5 plan phase"         '// (Phase 0.2 & 6.1 fix)'                                       "$PHASE_RE"
  st "G5 user decision"      '/// ADR-0015 d10, User Decision 3'                              "$PHASE_RE"
  st "G6 chat transcript"    "Your breakdown is already pointing in the right direction"       "$CHAT_RE"
  st "G7 measurement"        '// ~38x slower than the native path'                            "$MEAS_RE"
  st "G8 caps clause"        '// IDENTITY IS PART OF THIS CONTRACT, not an optimisation'       "$CAPS_RE" cs
  st "G8 banner"             '// ============================='                               "$BANNER_RE" cs

  # Negative controls: the patterns that were REMOVED as net-negative, plus the
  # subjunctive, which matches the GOOD falsifier-naming pattern ~15 times in 15.
  # If any of these fire, a future edit has reintroduced a rejected pattern.
  for neg in '// a px literal here would mean the clamp had run' \
             '// the naive partition is fine below 1k rows'; do
    if printf '%s\n' "$neg" | grep -qE "$ARG_RE"; then
      printf 'selftest FAIL: negative control matched G2: %s\n' "$neg" >&2
      st_fail=1
    fi
  done
  [ "$st_fail" -eq 0 ] && printf 'doc-diff-gate self-test: OK (10 patterns fire, 2 negative controls clean)\n'
  exit "$st_fail"
fi

# --- collect -----------------------------------------------------------------
STAGED_SRC=$(git diff --cached --name-only --diff-filter=ACM | grep -E "$SRC_RE" | grep -vE "$EXCLUDE_RE" || true)
STAGED_MD=$(git diff --cached --name-only --diff-filter=ACM | grep -E "$MD_RE" | grep -vE "$EXCLUDE_RE" || true)

if [ -z "$STAGED_SRC" ] && [ -z "$STAGED_MD" ]; then
  exit 0
fi

TMP=$(mktemp); trap 'rm -f "$TMP" "$TMP.md" "$TMP.out"' EXIT
: > "$TMP"; : > "$TMP.md"
[ -n "$STAGED_SRC" ] && added_lines $STAGED_SRC > "$TMP"
[ -n "$STAGED_MD" ] && added_lines $STAGED_MD > "$TMP.md"

printf 'doc-diff-gate: %s added source lines, %s added markdown lines\n' \
  "$(wc -l < "$TMP")" "$(wc -l < "$TMP.md")"

# --- per-line checks + G1 block length (single awk pass) ---------------------
# One pass, not a grep per line per pattern: on Windows/MSYS a subprocess spawn
# costs ~150ms under on-access AV, so the obvious shell loop takes minutes on a
# few hundred lines and gets disabled. Everything below runs in-process.
{
  "$AWK" -F'\t' \
    -v arg_re="$ARG_RE" -v hist_re="$HIST_RE" -v phase_re="$PHASE_RE" \
    -v meas_re="$MEAS_RE" -v date_re="$DATE_RE" -v ledger_re="$LEDGER_RE" \
    -v caps_re="$CAPS_RE" -v banner_re="$BANNER_RE" '
    function emit(sev, f, n, msg) { printf "%s\t%s:%d\t%s\n", sev, f, n, msg }
    function flush() {
      if (run > 20)
        emit("BLOCK", pf, pstart, "added comment block of " run " lines — past 20 it is a relocation defect; move it to a design doc or decision record and leave a one-line pointer")
      run = 0
    }
    {
      c = $3; lc = tolower(c)
      is_c = (c ~ /^[[:space:]]*(\/\/|\/\*|\*[^\/]|\*$|#[^!]|--)/)

      # G1 — contiguous added comment run
      if (is_c && $1 == pf && $2 == pline + 1) { run++; pline = $2 }
      else { flush(); if (is_c) { run = 1; pf = $1; pstart = $2; pline = $2 } else pf = "" }

      if (!is_c) next

      if (lc ~ arg_re)
        emit("BLOCK", $1, $2, "argues a rejected alternative — state what the code does and one falsifier; the argument belongs in a decision record")
      if (lc ~ hist_re)
        emit("BLOCK", $1, $2, "narrates history — git owns the previous shape; describe the code as it is now")
      # Case-SENSITIVE by design: a ledger prefix is uppercase, and folding case
      # here would match ordinary prose like "p12" or "the o3 path".
      if (c ~ ledger_re)
        emit("BLOCK", $1, $2, "ledger id in source — it resolves to nothing once the ledger is reaped; state the invariant instead")
      if (lc ~ phase_re)
        emit("BLOCK", $1, $2, "plan-phase or decision ref in source — unresolvable once the plan is reorganised")
      if (lc ~ meas_re && c !~ date_re)
        emit("BLOCK", $1, $2, "measurement without a date — carry value + date + the command that produced it, or delete it")
      if (c ~ caps_re)
        emit("WARN", $1, $2, "multi-word ALL-CAPS clause — one contrastive word may be capitalised, a clause may not")
      if (c ~ banner_re)
        emit("WARN", $1, $2, "decorative banner rule")
    }
    END { flush() }
  ' "$TMP"

  # G6 — chat transcript pasted into markdown. Zero FPs in calibration.
  "$AWK" -F'\t' -v chat_re="$CHAT_RE" '
    tolower($3) ~ chat_re {
      printf "BLOCK\t%s:%d\t%s\n", $1, $2,
        "reads as a pasted chat response — documentation describes the system, not a conversation"
    }
  ' "$TMP.md"
} | while IFS=$'\t' read -r sev loc msg; do
  report "$sev" "$loc" "$msg"
done > "$TMP.out"
cat "$TMP.out"
# `grep -c` prints 0 AND exits 1 on no-match, so `|| echo 0` would emit a second
# zero and every arithmetic test downstream would fail on "0\n0".
findings=$(grep -c '^  \[' "$TMP.out" 2>/dev/null || true)
blockers=$(grep -c '^  \[BLOCK\]' "$TMP.out" 2>/dev/null || true)
findings=${findings:-0}
blockers=${blockers:-0}

# --- verdict -----------------------------------------------------------------
if [ "$findings" -eq 0 ]; then
  printf 'doc-diff-gate: OK (%s checks, added lines only — see the RECALL WARNING in this script before reading a clean run as an all-clear)\n' 8
  exit 0
fi

printf '\ndoc-diff-gate: %d finding(s), %d of them blocking-class.\n' "$findings" "$blockers"
if [ "$MODE" = block ] && [ "$blockers" -gt 0 ]; then
  printf 'Fix the blocking findings above; do NOT skip the gate.\n' >&2
  exit 1
fi
printf 'Mode is "%s" — not failing the commit. Set DOC_GATE_MODE=block once the FP rate is proven.\n' "$MODE"
exit 0
