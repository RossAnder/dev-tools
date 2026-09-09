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

# Require GNU awk. Every rule below leans on GNU semantics — POSIX character
# classes inside the -v-passed patterns, and `gsub` on matched anchors in the
# added-line extractor. Under mawk the extractor yields no lines, no rule can
# match, and the script prints "doc-diff-gate: OK" having checked nothing. That
# silent all-clear is worse than no gate, and it is exactly what this script's
# own RECALL WARNING tells a reader not to over-trust.
#
# The resolver below is held byte-identical with scripts/verify-shared-blocks.sh
# by that script's parity check — see the gnu-awk-resolver entry in
# scripts/shared-blocks.toml. Its three per-caller inputs are set here, outside
# the block, because the override env var differs by script and so cannot be
# named inside a body that has to match byte for byte. This gate's warn/block
# knob governs what it does about FINDINGS; it is not a licence to run blind, so
# the block's exit 2 applies here too.
AWK_ENV_NAME='DOC_GATE_AWK'
AWK_ENV_VALUE="${DOC_GATE_AWK:-}"
AWK_CALLER='doc-diff-gate'

# The parity extractor matches a marker by whole-line equality and a bare HTML
# comment is not shell, so each marker sits alone inside a `:` no-op string. The
# two quote lines that wrap them are part of the hashed body and must stay.
: '
<!-- SHARED-BLOCK:gnu-awk-resolver START -->
'
# Candidates in preference order, probed by NAME rather than trusting whichever
# `awk` resolves first: an unvalidated plain-`awk` fallback makes coverage depend
# on the caller's PATH ordering, and .githooks/pre-commit runs these scripts
# directly. The env var named in AWK_ENV_NAME forces a specific binary.
AWK=''
for cand in "$AWK_ENV_VALUE" gawk /usr/bin/gawk /mingw64/bin/gawk /usr/local/bin/gawk awk; do
  [ -n "$cand" ] || continue
  cand_path=$(command -v "$cand" 2>/dev/null) || continue
  # Match the banner with `case`, not a pipe into grep. On a PATH without grep
  # that pipeline rejects every candidate — including a gawk named outright in
  # the env var — and the hard fail below then blames a missing gawk for a
  # missing grep. A shell pattern needs no external command at all, so the guard
  # depends on nothing beyond the interpreter already running it.
  cand_version=$("$cand_path" --version 2>&1) || cand_version=''
  case "$cand_version" in
    'GNU Awk'*) AWK=$cand_path; break ;;
  esac
done

# Hard fail when no candidate is GNU awk: mawk cannot run these scripts, and a
# gate that reports OK having checked nothing is worse than no gate. Exit 2 is
# "could not run", distinct from the exit 1 that means "the check found
# something".
if [ -z "$AWK" ]; then
  echo "$AWK_CALLER: requires GNU awk (gawk); none of \$$AWK_ENV_NAME, gawk, /usr/bin/gawk, /mingw64/bin/gawk, /usr/local/bin/gawk or awk is one" >&2
  exit 2
fi
: '
<!-- SHARED-BLOCK:gnu-awk-resolver END -->
'

# Source extensions carrying comments we gate. Markdown is included for G6 only.
#
# A literal dot is `[.]`, never `\.`, in every pattern this script passes to awk
# through -v: gawk processes escapes in a -v value, so `\.` arrives as a plain
# `.` — it warns on stderr and then matches ANY character, which would sweep
# `xyrs` into the gated set. The bracket form survives both -v and grep -E.
SRC_RE='[.](rs|ts|tsx|vue|cs|js|mjs|cjs|svelte|astro)$'
MD_RE='[.]md$'

# Paths exempt from every check. A reviewed constant: adding a line here is a
# deliberate act, not a convenience.
#   - generated trees: nothing here is hand-written, so no rule applies
#   - docs/plans + .claude/flows: harness flow output, governed by its own
#     retention rules rather than by comment discipline
EXCLUDE_RE='(^|/)(node_modules|target|dist|build|[.]lumina)/|[.]d[.]ts$|(^|/)docs/plans/|(^|/)[.]claude/flows/'

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
# The content is verbatim and may itself contain tabs, so a consumer must split
# on the FIRST TWO tabs only — see the content extraction in the checks below.
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
# LEDGER_RE requires the id to OPEN a comment, so it cannot see the other place a
# ledger id leaks: a string literal, almost always an assertion message. Scoping
# to the macro instead was measured at zero recall — the gate reads a -U0 diff one
# physical line at a time, and in every real site the id is on the message
# continuation line while `assert!(` is two or three lines above, outside the
# hunk. So the rule is wording-shaped: one double-quoted string carrying BOTH a
# denied id and message wording, in either order.
#
# Two blind spots, both deliberate, neither an oversight:
#   - it catches four of the five leak classes. `R<n>` stays invisible because it
#     is outside LEDGER_DENY above; widening to it costs 12 false positives.
#   - it is staged-diff-only like every other rule here, so it prevents the next
#     leak and finds none of today's. A clean run still means "the idiom did not
#     appear", and a hand sweep of the existing corpus is still a separate job.
MSG_WORD_RE='(regression|must|expected|got)'
LEDGER_MSG_RE="\"[^\"]*($LEDGER_DENY[0-9]{1,3}[^\"]*$MSG_WORD_RE|$MSG_WORD_RE[^\"]*$LEDGER_DENY[0-9]{1,3})[^\"]*\""
# Pre-stripped before that match: an ISO datetime's `T00:00:00Z` reads as a `T<n>`
# id, and it is the rule's only measured false positive.
ISO_RE="$DATE_RE|T[0-9]{2}:[0-9]{2}:[0-9]{2}Z"
CAPS_RE='[A-Z]{2,}[[:space:]]+[A-Z]{2,}[[:space:]]+[A-Z]{2,}'
BANNER_RE='(=|-){12,}'

# --- self-test ---------------------------------------------------------------
# A checker that cannot demonstrate it fires is a checker that reports OK on a
# broken tree. Every pattern below is asserted against a known-bad specimen.
if [ "$SELFTEST" -eq 1 ]; then
  st_fail=0
  # Counted, never hardcoded. The summary line at the bottom used to carry its
  # own literals while the assertions were hand-listed above it, so adding or
  # removing an assertion left the summary overstating what ran — a self-test
  # claiming coverage it does not have is the same silent all-clear this script
  # exists to catch, one level up. Both counters are incremented by the helpers
  # themselves, so the only way to inflate them is to write another assertion.
  st_count=0
  st_neg_count=0
  # Runs each pattern through the SAME awk + tolower() path as the real check.
  # An earlier version used `grep -i`, which is more permissive than the real
  # matcher and silently passed a pattern carrying capitals that could never fire
  # against tolower(line). A self-test looser than the check it guards is worse
  # than none. Pass a 4th arg to assert case-sensitively.
  st() { # label, specimen, pattern, [any-4th-arg = case-sensitive]
    st_count=$((st_count + 1))
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
  # The inverse assertion, through that same matcher. A control that shells out
  # to grep is a control that passes on a PATH without grep: the invocation dies
  # 127, the `if` reads false, and the run prints its OK naming controls it never
  # ran — the silent all-clear this whole self-test exists to prevent. Every
  # assertion below therefore depends on nothing beyond the awk the resolver has
  # already validated.
  st_neg() { # label, specimen, pattern
    st_neg_count=$((st_neg_count + 1))
    if printf '%s\n' "$2" |
       "$AWK" -v pat="$3" \
         '{ if (tolower($0) ~ pat) { found = 1 } }
          END { exit(found ? 0 : 1) }'; then
      printf 'selftest FAIL: negative control matched %s: %s\n' "$1" "$2" >&2
      st_fail=1
    fi
  }
  st "G2 argument-closing"   '// Two independent reasons, and either alone would settle it:' "$ARG_RE"
  st "G2 not-the-answer"     '// The top-level test.api is not the answer either.'            "$ARG_RE"
  st "G3 history"            '// O22: previously this ran synchronously at module import'     "$HIST_RE"
  st "G4 ledger id"          '// O22: coalesce the in-flight run'                             "$LEDGER_RE" cs
  st "G4b message ledger id" 'assert!(x, "T7 regression: must hold");'                        "$LEDGER_MSG_RE" cs
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
    st_neg G2 "$neg" "$ARG_RE"
  done
  # G4b's pre-strip is a suppression, so it fails silently: without this control a
  # deleted ISO_RE leaves the self-test green and the false positive back. It is
  # a negative control like the two above, but cannot route through st_neg —
  # that helper has no pre-strip stage — so it counts itself in.
  st_neg_count=$((st_neg_count + 1))
  if printf '%s\n' '        "last_activity must be `<today>T00:00:00Z`, got: {last}"' |
     "$AWK" -v pat="$LEDGER_MSG_RE" -v iso="$ISO_RE" \
       '{ s = $0; gsub(iso, " ", s); if (s ~ pat) { found = 1 } }
        END { exit(found ? 0 : 1) }'; then
    printf 'selftest FAIL: G4b matched an ISO timestamp — the pre-strip is not firing\n' >&2
    st_fail=1
  fi
  # The checks split the content off by hand rather than reading $3, because a
  # tab-indented line's content starts with a tab. Asserted here because the
  # failure is silent in exactly one direction: reading $3 again would leave
  # every pattern above still firing on its specimen while the gate went blind
  # to every tab-indented line in the tree.
  if ! printf 'f.rs\t12\t\t// O22: coalesce the in-flight run\n' |
     "$AWK" -F'\t' -v pat="$LEDGER_RE" \
       '{ c = $0; sub(/^[^\t]*\t[^\t]*\t/, "", c); if (c ~ pat) { found = 1 } }
        END { exit(found ? 0 : 1) }'; then
    printf 'selftest FAIL: a tab-indented line did not reach the rules — content extraction is truncating at the tab\n' >&2
    st_fail=1
  fi
  [ "$st_fail" -eq 0 ] && printf 'doc-diff-gate self-test: OK (%d patterns fire, %d negative controls clean, tab-indented content reaches the rules)\n' \
    "$st_count" "$st_neg_count"
  exit "$st_fail"
fi

# --- collect -----------------------------------------------------------------
# Filtered by the resolved awk, not by `grep -E … | grep -vE …`. On a PATH
# without grep both stages of that pipeline die 127, both lists come back empty,
# and the early exit below reports a clean tree HAVING READ NO LINES — the same
# silent all-clear the resolver above exists to prevent, but on the main path
# rather than the self-test. Awk is the one external the resolver has already
# validated, so routing through it makes the collect stage depend on nothing
# else. It also drops the `|| true`: awk exits 0 on no match, so a non-zero
# status here now means git or awk actually failed and set -e should see it.
staged_paths() { # include-pattern
  git diff --cached --name-only --diff-filter=ACM |
    "$AWK" -v inc="$1" -v exc="$EXCLUDE_RE" '$0 ~ inc && $0 !~ exc'
}
STAGED_SRC=$(staged_paths "$SRC_RE")
STAGED_MD=$(staged_paths "$MD_RE")

if [ -z "$STAGED_SRC" ] && [ -z "$STAGED_MD" ]; then
  exit 0
fi

TMP=$(mktemp)
trap 'rm -f "$TMP" "$TMP.md" "$TMP.out" "$TMP.rules" "$TMP.md.rules"' EXIT
: > "$TMP"; : > "$TMP.md"
[ -n "$STAGED_SRC" ] && added_lines $STAGED_SRC > "$TMP"
[ -n "$STAGED_MD" ] && added_lines $STAGED_MD > "$TMP.md"

# Counted by the resolved awk, not `wc -l`. With wc absent the substitutions come
# back empty and this line prints blanks where two figures belong, so the summary
# a reader skims says nothing about how much was read. Awk is the one external
# the resolver has already validated. Every extracted line carries its own
# newline, so NR and a newline count agree on both files, empty ones included.
printf 'doc-diff-gate: %s added source lines, %s added markdown lines\n' \
  "$("$AWK" 'END { print NR }' "$TMP")" "$("$AWK" 'END { print NR }' "$TMP.md")"

# --- per-line checks + G1 block length (single awk pass) ---------------------
# One pass, not a grep per line per pattern: on Windows/MSYS a subprocess spawn
# costs ~150ms under on-access AV, so the obvious shell loop takes minutes on a
# few hundred lines and gets disabled. Everything below runs in-process.
#
# Each rule declares itself once, in the registry the BEGIN block builds, and the
# check count in the verdict is derived from those declarations. A literal there
# keeps announcing its old figure once a rule is added or dropped, which is the
# gate overstating its own coverage on the one line every clean run prints — the
# same defect the self-test summary above avoids by counting its assertions. The
# declarations run in BEGIN, so the count is the size of the rule set and not the
# subset some particular diff happened to reach.
{
  DOC_GATE_RULES_FILE="$TMP.rules" "$AWK" -F'\t' \
    -v arg_re="$ARG_RE" -v hist_re="$HIST_RE" -v phase_re="$PHASE_RE" \
    -v meas_re="$MEAS_RE" -v date_re="$DATE_RE" -v ledger_re="$LEDGER_RE" \
    -v ledger_msg_re="$LEDGER_MSG_RE" -v iso_re="$ISO_RE" \
    -v caps_re="$CAPS_RE" -v banner_re="$BANNER_RE" '
    function emit(sev, f, n, msg) { printf "%s\t%s:%d\t%s\n", sev, f, n, msg }
    # One registry row per rule, in the order the rules fire. An empty pattern
    # marks a rule the table loop cannot express — it is applied by hand below
    # and keeps its row so the registry stays the whole rule set. `except`
    # suppresses a match; the returned index is how a hand-applied rule reaches
    # its own severity and message. `id` may repeat across rows (G4 has two
    # forms) and the count that reads this file dedupes on it.
    function rule(id, pat, cs, sev, msg, except) {
      n_rules++
      r_pat[n_rules] = pat; r_cs[n_rules] = cs
      r_sev[n_rules] = sev; r_msg[n_rules] = msg; r_except[n_rules] = except
      print id > rules_file
      return n_rules
    }
    BEGIN {
      # The registry path arrives through the environment rather than -v: a
      # Windows-style TMPDIR puts backslashes in it, gawk processes escapes in a
      # -v value, and the `\r` of a user directory would arrive as a carriage
      # return — the registry written somewhere else, and the count that reads it
      # left with nothing. Same trap the `[.]` patterns above avoid, on a path
      # rather than a pattern. ENVIRON hands the value over verbatim.
      rules_file = ENVIRON["DOC_GATE_RULES_FILE"]
      g1 = rule("G1", "", "", "BLOCK", "")
      rule("G2", arg_re, "", "BLOCK", "argues a rejected alternative — state what the code does and one falsifier; the argument belongs in a decision record")
      rule("G3", hist_re, "", "BLOCK", "narrates history — git owns the previous shape; describe the code as it is now")
      # Case-sensitive by design: a ledger prefix is uppercase, and folding case
      # here would match ordinary prose like "p12" or "the o3 path".
      rule("G4", ledger_re, "cs", "BLOCK", "ledger id in source — it resolves to nothing once the ledger is reaped; state the invariant instead")
      g4b = rule("G4", "", "cs", "BLOCK", "ledger id in a message string — it resolves to nothing once the ledger is reaped; name the invariant the assertion protects instead")
      rule("G5", phase_re, "", "BLOCK", "plan-phase or decision ref in source — unresolvable once the plan is reorganised")
      rule("G7", meas_re, "", "BLOCK", "measurement without a date — carry value + date + the command that produced it, or delete it", date_re)
      rule("G8", caps_re, "cs", "WARN", "multi-word ALL-CAPS clause — one contrastive word may be capitalised, a clause may not")
      rule("G8", banner_re, "cs", "WARN", "decorative banner rule")
    }
    function flush() {
      if (run > 20)
        emit(r_sev[g1], pf, pstart, "added comment block of " run " lines — past 20 it is a relocation defect; move it to a design doc or decision record and leave a one-line pointer")
      run = 0
    }
    {
      # NOT $3: a tab inside the content splits it further, and every rule then
      # sees only the text before that tab — for a tab-indented line, nothing at
      # all. Path and lineno are tab-free by construction, so dropping exactly
      # those two fields leaves the content verbatim, tabs included.
      c = $0; sub(/^[^\t]*\t[^\t]*\t/, "", c)
      lc = tolower(c)
      is_c = (c ~ /^[[:space:]]*(\/\/|\/\*|\*[^\/]|\*$|#[^!]|--)/)

      # G1 — contiguous added comment run
      if (is_c && $1 == pf && $2 == pline + 1) { run++; pline = $2 }
      else { flush(); if (is_c) { run = 1; pf = $1; pstart = $2; pline = $2 } else pf = "" }

      # G4b — ledger id in a message string. Must sit BEFORE the comment
      # short-circuit below: the leak lives in a string literal, which never
      # opens a comment, so a branch placed after it could not fire at all.
      s = c; gsub(iso_re, " ", s)
      if (s ~ ledger_msg_re)
        emit(r_sev[g4b], $1, $2, r_msg[g4b])

      if (!is_c) next

      # The registry, applied in declaration order. A case-sensitive rule reads
      # the content verbatim; every other one reads it folded, which is why the
      # folded patterns are written lowercase where they are defined.
      for (i = 1; i <= n_rules; i++) {
        if (r_pat[i] == "") continue
        subj = (r_cs[i] == "" ? lc : c)
        if (subj ~ r_pat[i] && (r_except[i] == "" || subj !~ r_except[i]))
          emit(r_sev[i], $1, $2, r_msg[i])
      }
    }
    END { flush() }
  ' "$TMP"

  # G6 — chat transcript pasted into markdown. Zero FPs in calibration.
  DOC_GATE_RULES_FILE="$TMP.md.rules" "$AWK" -F'\t' -v chat_re="$CHAT_RE" '
    # This pass carries one rule, declared the way the source pass declares each
    # of its own — and through the same environment hand-off — so the check count
    # below covers both passes rather than one.
    BEGIN { print "G6" > ENVIRON["DOC_GATE_RULES_FILE"] }
    {
      c = $0; sub(/^[^\t]*\t[^\t]*\t/, "", c)   # content verbatim, not $3 — see above
      if (tolower(c) ~ chat_re)
        printf "BLOCK\t%s:%d\t%s\n", $1, $2,
          "reads as a pasted chat response — documentation describes the system, not a conversation"
    }
  ' "$TMP.md"
} | while IFS=$'\t' read -r sev loc msg; do
  report "$sev" "$loc" "$msg"
done > "$TMP.out"
# Displayed by the resolved awk, not `cat`. Every finding has already been
# computed and written by this point, so with cat absent the run dies 127 here
# and the operator loses the whole report to a binary the gate never needed.
"$AWK" '{ print }' "$TMP.out"
# Recounted from the report file because `report` ran inside a pipeline, so its
# increments landed in a subshell.
#
# Counted by the resolved awk, not `grep -c … || true`: with grep absent that
# form leaves BOTH counters empty, the `:-0` defaults take over, and the verdict
# below announces OK over a $TMP.out that already holds BLOCK findings it just
# printed — and DOC_GATE_MODE=block then fails to block. One awk pass emits both
# counts, always exits 0, and always prints two integers, so no default can be
# mistaken for a real zero.
counts=$("$AWK" '
  /^  \[/ { f++ }
  /^  \[BLOCK\]/ { b++ }
  END { printf "%d %d\n", f + 0, b + 0 }
' "$TMP.out")
findings=${counts% *}
blockers=${counts#* }

# The figure the OK line quotes, taken from what the two passes declared. Both
# registries are written from a BEGIN block, so this is the size of the rule set
# rather than the number of rules the diff reached, and it moves the moment a
# `rule(...)` row is added or dropped. `G4` declares two rows, for its comment
# and message-string forms, and dedupes to the one check a reader is told about.
rule_count=$("$AWK" '!seen[$0]++ { n++ } END { printf "%d\n", n + 0 }' \
  "$TMP.rules" "$TMP.md.rules")

# --- verdict -----------------------------------------------------------------
if [ "$findings" -eq 0 ]; then
  printf 'doc-diff-gate: OK (%s checks, added lines only — see the RECALL WARNING in this script before reading a clean run as an all-clear)\n' "$rule_count"
  exit 0
fi

printf '\ndoc-diff-gate: %d finding(s), %d of them blocking-class.\n' "$findings" "$blockers"
if [ "$MODE" = block ] && [ "$blockers" -gt 0 ]; then
  printf 'Fix the blocking findings above; do NOT skip the gate.\n' >&2
  exit 1
fi
printf 'Mode is "%s" — not failing the commit. Set DOC_GATE_MODE=block once the FP rate is proven.\n' "$MODE"
exit 0
