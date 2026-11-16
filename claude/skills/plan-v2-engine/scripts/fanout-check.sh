#!/usr/bin/env bash
# Usage: fanout-check.sh snapshot|check <scratch-dir> <slug> [--exclude <path>]... <note-path>...
# With any --exclude, the backlog store is excluded too, and HEAD may move
# through commits that touch only excluded paths, note paths, the flow's
# store and run-note directories or the backlog store.
set -u

usage="usage: fanout-check.sh snapshot|check <scratch-dir> <slug> [--exclude <path>]... <note-path>..."

die() {
    printf 'fanout-check: %s\n' "$1" >&2
    exit 3
}

[ "$#" -ge 4 ] || die "$usage"
mode=$1
scratch=$2
slug=$3
shift 3

case "$mode" in
    snapshot | check) ;;
    *) die "mode must be snapshot or check, not '$mode'" ;;
esac
[[ "$slug" =~ ^[a-z0-9][a-z0-9-]{0,63}$ ]] || die "slug '$slug' does not match ^[a-z0-9][a-z0-9-]{0,63}\$"

refuse_unsafe() {
    case "$2" in
        '' | /* | \\* | [A-Za-z]:* | *..*) die "refusing $1 '$2': it must be repo-relative with no '..'" ;;
    esac
}

excludes=()
while [ "$#" -gt 0 ] && [ "$1" = --exclude ]; do
    [ "$#" -ge 2 ] || die "$usage"
    refuse_unsafe "exclude path" "$2"
    excludes+=("$2")
    shift 2
done
[ "$#" -ge 1 ] || die "$usage"
notes=("$@")

specs=(-- .)
for ex in "${excludes[@]}"; do
    specs+=(":(exclude)$ex")
done
for note in "${notes[@]}"; do
    refuse_unsafe "note path" "$note"
    specs+=(":(exclude)$note")
done
specs+=(":(exclude).claude/flows/$slug")
if [ "${#excludes[@]}" -gt 0 ]; then
    specs+=(":(exclude).claude/backlog.toml" ":(exclude).claude/backlog.toml.sha256")
fi

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

write_excludes() {
    if [ "${#excludes[@]}" -gt 0 ]; then
        printf '%s\n' "${excludes[@]}"
    fi
}

if [ "$mode" = snapshot ]; then
    write_excludes > "$scratch/excludes.txt"
    git config --list --show-origin --show-scope > "$scratch/config-before.txt"
    capture before
    exit 0
fi

for f in head config status diff untracked; do
    [ -f "$scratch/$f-before.txt" ] || die "no snapshot in '$scratch': $f-before.txt is missing"
done
# A snapshot without excludes.txt predates --exclude and took none.
if [ -f "$scratch/excludes.txt" ]; then
    snapshot_excludes=$(cat "$scratch/excludes.txt")
else
    snapshot_excludes=
fi
[ "$snapshot_excludes" = "$(write_excludes)" ] || die "the --exclude paths differ from the snapshot's"

# A path is tolerated in a HEAD move when it is, or lies under, one of these.
tolerated=("${excludes[@]}" "${notes[@]}" ".claude/flows/$slug" "docs/flows/$slug/notes/run"
    ".claude/backlog.toml" ".claude/backlog.toml.sha256")

path_tolerated() {
    local root
    for root in "${tolerated[@]}"; do
        root=${root%/}
        case "$1" in
            "$root" | "$root"/*) return 0 ;;
        esac
    done
    return 1
}

head_moved_through_tolerated() {
    local before path
    before=$(cat "$scratch/head-before.txt")
    git merge-base --is-ancestor "$before" HEAD 2> /dev/null || return 1
    git diff --name-only --no-renames -z "$before" HEAD > "$scratch/head-move.txt" || return 1
    while IFS= read -r -d '' path; do
        path_tolerated "$path" || return 1
    done < "$scratch/head-move.txt"
    return 0
}

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
        if [ "$f" = head ] && [ "${#excludes[@]}" -gt 0 ] && head_moved_through_tolerated; then
            continue
        fi
        changed=1
        echo "$f changed"
        case "$f" in
            status | untracked) diff "$scratch/$f-before.txt" "$scratch/$f-after.txt" ;;
        esac
    fi
done
exit "$changed"
