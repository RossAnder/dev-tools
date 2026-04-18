#!/usr/bin/env bash
set -euo pipefail

# Require GNU awk: the manifest-parse pipeline below relies on GNU extensions
# (gsub behaviour on matched anchors, regex semantics). Non-GNU awks (mawk,
# BusyBox awk, some Windows Git Bash installs) silently emit wrong (block, file)
# pairs and the script would report "shared-block parity: OK" without actually
# validating anything.
awk --version 2>&1 | grep -qi '^GNU Awk' || {
  echo "verify-shared-blocks.sh requires GNU awk (gawk)" >&2
  exit 2
}

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

hash_block() {
  local file=$1 name=$2
  awk -v start="<!-- SHARED-BLOCK:${name} START -->" \
      -v end="<!-- SHARED-BLOCK:${name} END -->" '
    $0 == start { in_block=1; next }
    $0 == end   { in_block=0; next }
    in_block    { print }
  ' "$file" | $HASHER | awk '{print $1}'
}

pairs=$(awk '
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

  if ! grep -qF "<!-- SHARED-BLOCK:${bname} START -->" "$bfile"; then
    echo "error: $bfile missing START marker for block '$bname'" >&2
    fail=1
    continue
  fi
  if ! grep -qF "<!-- SHARED-BLOCK:${bname} END -->" "$bfile"; then
    echo "error: $bfile missing END marker for block '$bname'" >&2
    fail=1
    continue
  fi

  h=$(hash_block "$bfile" "$bname")

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
