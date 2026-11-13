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

# The regular-file test is the `-f` builtin, which treats a directory as
# missing and starts no process. Its result rides along as a third field so the
# missing-file message keeps its place in pair order among the checker's others.
checked=''
while IFS=$'\t' read -r bname bfile; do
  [[ -z $bname || -z $bfile ]] && continue
  if [[ -f $bfile ]]; then is_file=1; else is_file=0; fi
  checked+="${bname}"$'\t'"${bfile}"$'\t'"${is_file}"$'\n'
done <<< "$pairs"

# Canonical extraction, and the reason the mirror in tomlctl/src/blocks.rs is a
# mirror: this is what the pre-commit hook runs, so `tomlctl blocks verify` must
# never accept a tree this rejects. The in-crate
# `blocks_verify_agrees_with_shell_gate` holds the two to one verdict by running
# this script.
#
# One gawk process checks every pair, reading each carrier with `getline`, and
# decides parity by string equality; it starts the hasher only to print a drift.
#
# CR contract: carriers are pinned LF by .gitattributes. A marker matches the
# line exactly as gawk reads it — MSYS gawk's text-mode read drops one CR, as
# str::lines() does in the mirror — so a marker ending `\r\r\n` matches nothing;
# content lines lose one further CR. Marker presence is the one relaxation: it
# strips a trailing CR before comparing, so a marker keeping a CR passes the
# presence check and fails the empty-extraction check, which names line endings.
# A CR-preserving gawk (non-MSYS) extracts nothing from a CRLF carrier the mirror
# accepts, and fails that same check rather than passing.
fail=0
"$AWK" -v hasher="$HASHER" '
  # Sets has_start and has_end as a side effect; returns the concatenated text
  # of every span, each content line ending in a newline.
  function extract_block(file, name,    start, end, line, bare, in_block, block) {
    start = "<!-- SHARED-BLOCK:" name " START -->"
    end = "<!-- SHARED-BLOCK:" name " END -->"
    has_start = has_end = 0
    while ((getline line < file) > 0) {
      bare = line
      sub(/\r$/, "", bare)
      if (bare == start) has_start = 1
      if (bare == end) has_end = 1
      if (line == start) { in_block = 1; continue }
      if (line == end) { in_block = 0; continue }
      if (in_block) { sub(/\r$/, "", line); block = block line "\n" }
    }
    close(file)
    return block
  }
  function digest(s,    out, field) {
    printf "%s", s |& hasher
    close(hasher, "to")
    hasher |& getline out
    close(hasher)
    split(out, field, " ")
    return field[1]
  }
  BEGIN { FS = "\t"; q = "\047"; err = "/dev/stderr" }
  NF < 3 { next }
  {
    name = $1
    file = $2
    if ($3 != "1") {
      print "error: block " q name q " references missing file: " file > err
      fail = 1
      next
    }
    block = extract_block(file, name)
    if (!has_start) {
      print "error: " file " missing START marker for block " q name q > err
      fail = 1
      next
    }
    if (!has_end) {
      print "error: " file " missing END marker for block " q name q > err
      fail = 1
      next
    }
    if (block == "") {
      print "error: block " q name q " in " file " extracted to no content between its markers (empty span, or a marker line still ending in CR after awk" q "s read — see the CR contract at extract_block)" > err
      fail = 1
      next
    }
    if (!(name in first_block)) {
      first_block[name] = block
      first_file[name] = file
    } else if (first_block[name] != block) {
      if (!(name in first_digest)) first_digest[name] = digest(first_block[name])
      print "error: block " q name q " drift:" > err
      printf "  %s  hash=%s\n", first_file[name], first_digest[name] > err
      printf "  %s  hash=%s\n", file, digest(block) > err
      fail = 1
    }
  }
  END { exit fail }
' <<< "$checked" || fail=$?

if [[ $fail -eq 0 ]]; then
  echo "shared-block parity: OK"
fi
exit $fail
