#!/usr/bin/env bash
set -euo pipefail

# Require GNU awk: the manifest-parse pipeline below relies on GNU extensions
# (gsub behaviour on matched anchors, regex semantics). Non-GNU awks (mawk,
# BusyBox awk, some Windows Git Bash installs) silently emit wrong (block, file)
# pairs and the script would report "shared-block parity: OK" without actually
# validating anything.
#
# The resolver below is held byte-identical with scripts/doc-diff-gate.sh by this
# script's own parity check — see the gnu-awk-resolver entry in
# scripts/shared-blocks.toml. Its three per-caller inputs are set here, outside
# the block, because the override env var differs by script and so cannot be
# named inside a body that has to match byte for byte.
AWK_ENV_NAME='SHARED_BLOCKS_AWK'
AWK_ENV_VALUE="${SHARED_BLOCKS_AWK:-}"
AWK_CALLER='verify-shared-blocks.sh'

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

cd "$(git rev-parse --show-toplevel 2>/dev/null)"

MANIFEST="${MANIFEST:-scripts/shared-blocks.toml}"

if [[ ! -f $MANIFEST ]]; then
  echo "error: manifest not found at $MANIFEST" >&2
  exit 2
fi

if command -v sha256sum >/dev/null 2>&1; then
  HASHER='sha256sum'
elif command -v shasum >/dev/null 2>&1; then
  HASHER='shasum -a 256'
else
  echo "error: need sha256sum or shasum -a 256 on PATH" >&2
  exit 2
fi

# Canonical extraction, and the reason the mirror in tomlctl/src/blocks.rs is a
# mirror: this is what the pre-commit hook runs, so `tomlctl blocks verify` must
# never accept a tree this rejects. The in-crate
# `blocks_verify_agrees_with_shell_gate` holds the two to one verdict by running
# this script.
extract_block() {
  local file=$1 name=$2
  "$AWK" -v start="<!-- SHARED-BLOCK:${name} START -->" \
      -v end="<!-- SHARED-BLOCK:${name} END -->" '
    $0 == start { in_block=1; next }
    $0 == end   { in_block=0; next }
    in_block    { print }
  ' "$file"
}

# Marker presence, in the already-resolved awk rather than `grep -qF`. A guard
# that shells out cannot name its own failure: on a PATH without grep the
# invocation dies 127 and every carrier is reported as missing its START marker,
# blaming the files for an absent binary. The test is whole-line equality, the
# extractor's own semantics, with a single deliberate relaxation — a trailing CR
# is stripped here and NOT in extract_block, so a CRLF carrier clears this guard
# and trips the empty-extraction guard below, which names line endings instead.
has_marker() {
  local file=$1 marker=$2
  "$AWK" -v m="$marker" '
    { line = $0; sub(/\r$/, "", line); if (line == m) { found = 1; exit } }
    END { exit(found ? 0 : 1) }
  ' "$file"
}

pairs=$("$AWK" '
  /^\[\[block\]\]/ { name=""; in_files=0; next }
  /^name = "/ {
    gsub(/^name = "|"$/, "")
    name=$0
    next
  }
  /^files = \[/ { in_files=1; next }
  in_files && /^\]/ { in_files=0; name=""; next }
  in_files && /^[[:space:]]*"[^"]+"/ {
    gsub(/^[[:space:]]*"|",?$|"$/, "")
    if (name != "") print name "\t" $0
  }
' "$MANIFEST")

if [[ -z $pairs ]]; then
  echo "error: manifest yielded no (block, file) pairs — check $MANIFEST syntax" >&2
  exit 2
fi

fail=0
declare -A first_hash first_file

while IFS=$'\t' read -r bname bfile; do
  [[ -z $bname || -z $bfile ]] && continue

  if [[ ! -f $bfile ]]; then
    echo "error: block '$bname' references missing file: $bfile" >&2
    fail=1
    continue
  fi

  if ! has_marker "$bfile" "<!-- SHARED-BLOCK:${bname} START -->"; then
    echo "error: $bfile missing START marker for block '$bname'" >&2
    fail=1
    continue
  fi
  if ! has_marker "$bfile" "<!-- SHARED-BLOCK:${bname} END -->"; then
    echo "error: $bfile missing END marker for block '$bname'" >&2
    fail=1
    continue
  fi

  # Capture the block before hashing: the marker guards above tolerate a trailing
  # CR, which the extraction awk does not — it compares whole lines for equality
  # against the marker verbatim. Under a CR-preserving awk every block
  # extracts to zero lines, every side hashes to the empty-input digest, and the
  # comparison below would report parity OK without having compared anything.
  # The trailing 'x' preserves the block's own trailing newlines through the
  # command substitution so the hashed bytes are unchanged.
  block=$(extract_block "$bfile" "$bname"; printf 'x')
  block=${block%x}

  if [[ -z $block ]]; then
    echo "error: block '$bname' in $bfile extracted to no content between its markers" >&2
    fail=1
    continue
  fi

  h=$(printf '%s' "$block" | $HASHER | "$AWK" '{print $1}')

  if [[ -z ${first_hash[$bname]:-} ]]; then
    first_hash[$bname]=$h
    first_file[$bname]=$bfile
  elif [[ ${first_hash[$bname]} != "$h" ]]; then
    echo "error: block '$bname' drift:" >&2
    printf '  %s  hash=%s\n' "${first_file[$bname]}" "${first_hash[$bname]}" >&2
    printf '  %s  hash=%s\n' "$bfile" "$h" >&2
    fail=1
  fi
done <<< "$pairs"

if [[ $fail -eq 0 ]]; then
  echo "shared-block parity: OK"
fi
exit $fail
