#!/usr/bin/env bash
# Usage: fanout-check.sh snapshot|check <scratch-dir> <slug> <note-path>...
set -u

die() {
    printf 'fanout-check: %s\n' "$1" >&2
    exit 3
}

[ "$#" -ge 4 ] || die "usage: fanout-check.sh snapshot|check <scratch-dir> <slug> <note-path>..."
mode=$1
scratch=$2
slug=$3
shift 3

case "$mode" in
    snapshot | check) ;;
    *) die "mode must be snapshot or check, not '$mode'" ;;
esac
[[ "$slug" =~ ^[a-z0-9][a-z0-9-]{0,63}$ ]] || die "slug '$slug' does not match ^[a-z0-9][a-z0-9-]{0,63}\$"

specs=(-- .)
for note in "$@"; do
    case "$note" in
        '' | /* | \\* | [A-Za-z]:* | *..*) die "refusing note path '$note': it must be repo-relative with no '..'" ;;
    esac
    specs+=(":(exclude)$note")
done
specs+=(":(exclude).claude/flows/$slug")

mkdir -p "$scratch" || die "cannot create '$scratch'"
scratch=$(cd "$scratch" && pwd) || die "cannot enter '$scratch'"
top=$(git rev-parse --show-toplevel) || die "not inside a git work tree"
cd "$top" || die "cannot enter '$top'"

capture() {
    git rev-parse HEAD > "$scratch/head-$1.txt"
    git status --porcelain=v1 -uall "${specs[@]}" > "$scratch/status-$1.txt"
    git diff HEAD --binary "${specs[@]}" > "$scratch/diff-$1.txt"
    git ls-files -z --others --exclude-standard "${specs[@]}" | xargs -0 -r sha256sum > "$scratch/untracked-$1.txt"
}

if [ "$mode" = snapshot ]; then
    git config --list --show-origin --show-scope > "$scratch/config-before.txt"
    capture before
    exit 0
fi

for f in head config status diff untracked; do
    [ -f "$scratch/$f-before.txt" ] || die "no snapshot in '$scratch': $f-before.txt is missing"
done

# The config is compared before anything else runs, because git status and
# git diff execute whatever core.fsmonitor or diff driver the config names.
git config --list --show-origin --show-scope > "$scratch/config-after.txt"
if ! cmp -s "$scratch/config-before.txt" "$scratch/config-after.txt"; then
    echo "config changed; no other git command was run:"
    diff "$scratch/config-before.txt" "$scratch/config-after.txt"
    exit 2
fi

capture after
changed=0
for f in head status diff untracked; do
    if ! cmp -s "$scratch/$f-before.txt" "$scratch/$f-after.txt"; then
        changed=1
        echo "$f changed"
        case "$f" in
            status | untracked) diff "$scratch/$f-before.txt" "$scratch/$f-after.txt" ;;
        esac
    fi
done
exit "$changed"
