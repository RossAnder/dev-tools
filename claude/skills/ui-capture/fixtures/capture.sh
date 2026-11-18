#!/usr/bin/env bash
# The ui-capture contract's command over the dev-tools fixtures.
# Usage: capture.sh [--out <dir>] [--only <state>]... [--list]
# Exit 0 when every asked state was captured, else the highest code any state failed with.
set -uo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
WEB_DIR="$SCRIPT_DIR/web"
TUI_DIR="$SCRIPT_DIR/tui"
RENDER="$SCRIPT_DIR/../scripts/render.sh"

out=.ui-capture
list=0
only=()
while [ $# -gt 0 ]; do
  case "$1" in
    --out) [ $# -ge 2 ] || { echo "capture.sh: --out needs a value" >&2; exit 2; }; out=$2; shift 2 ;;
    --only) [ $# -ge 2 ] || { echo "capture.sh: --only needs a value" >&2; exit 2; }; only+=("$2"); shift 2 ;;
    --list) list=1; shift ;;
    *) echo "capture.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

# Each state is "<kind> <name> <size>"; the size is empty for a terminal state.
states=()
while IFS= read -r line || [ -n "$line" ]; do
  line=${line%$'\r'}
  [ -n "$line" ] || continue
  name=${line%% *}
  size=${line#* }
  states+=("web $name $size")
done < "$WEB_DIR/states.txt"
for f in "$TUI_DIR"/*.txt; do
  [ -e "$f" ] || continue
  b=$(basename "$f" .txt)
  states+=("tui $b")
done

if [ "$list" -eq 1 ]; then
  for s in "${states[@]}"; do
    set -- $s
    echo "$2"
  done
  exit 0
fi

wanted=()
if [ "${#only[@]}" -eq 0 ]; then
  for s in "${states[@]}"; do
    set -- $s
    wanted+=("$2")
  done
else
  unknown=0
  for o in "${only[@]}"; do
    found=0
    for s in "${states[@]}"; do
      set -- $s
      if [ "$2" = "$o" ]; then found=1; break; fi
    done
    if [ "$found" -eq 0 ]; then
      echo "capture.sh: unknown state: $o" >&2
      unknown=1
    fi
  done
  [ "$unknown" -eq 0 ] || exit 1
  wanted=("${only[@]}")
fi

mkdir -p "$out" || exit 1
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

worst=0
for w in "${wanted[@]}"; do
  for s in "${states[@]}"; do
    set -- $s
    [ "$2" = "$w" ] || continue
    kind=$1
    name=$2
    size=${3:-}
    rc=0
    if [ "$kind" = web ]; then
      sed "s/data-state=\"web-01\"/data-state=\"$name\"/" "$WEB_DIR/panel.html" > "$tmp/$name.html" || rc=1
      if [ "$rc" -eq 0 ]; then
        bash "$RENDER" --size "$size" "$tmp/$name.html" "$out" "$name" || rc=$?
      fi
    else
      cp "$TUI_DIR/$name.txt" "$out/$name.txt" || rc=1
    fi
    if [ "$rc" -ne 0 ]; then
      echo "capture.sh: state $name failed with exit code $rc" >&2
      [ "$rc" -gt "$worst" ] && worst=$rc
    fi
    break
  done
done
exit "$worst"
