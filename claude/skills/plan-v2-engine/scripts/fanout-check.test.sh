#!/usr/bin/env bash
# Exercises fanout-check.sh in throwaway repositories; prints one line per case
# and exits non-zero when any case gets the wrong result.
set -u

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fanout-check.sh"
work=$(mktemp -d) || exit 3
trap 'rm -rf "$work"' EXIT

slug=s
note=notes/n.md
failures=0
case_no=0

g() {
    git -c user.name=fanout-test -c user.email=fanout-test@example.invalid -c commit.gpgsign=false \
        -c core.autocrlf=false "$@"
}

# Builds a fresh repository with one commit and sets $repo and $scratch.
fresh() {
    case_no=$((case_no + 1))
    repo="$work/repo-$case_no"
    scratch="$work/scratch-$case_no"
    mkdir -p "$repo/ex" "$repo/.claude/flows/$slug" || exit 3
    cd "$repo" || exit 3
    git init -q . || exit 3
    echo a > a.txt
    echo e > ex/e.txt
    echo b > .claude/backlog.toml
    echo f > ".claude/flows/$slug/x.toml"
    g add -A && g commit -q -m init || exit 3
}

# expect <name> <exit-code> <want-in-output|-> <check args>...
expect() {
    local name=$1 want=$2 grep_for=$3 out code
    shift 3
    out=$(bash "$script" check "$scratch" "$slug" "$@" 2>&1)
    code=$?
    if [ "$code" -eq "$want" ] && { [ "$grep_for" = - ] || printf '%s\n' "$out" | grep -qF -- "$grep_for"; }; then
        printf 'ok   %s (exit %s)\n' "$name" "$code"
    else
        printf 'FAIL %s: wanted exit %s%s, got %s\n%s\n' "$name" "$want" \
            "$([ "$grep_for" = - ] || printf " with '%s'" "$grep_for")" "$code" "$out"
        failures=$((failures + 1))
    fi
}

snap() {
    bash "$script" snapshot "$scratch" "$slug" "$@" > /dev/null || exit 3
}

fresh
snap --exclude ex/e.txt "$note"
echo changed > ex/e.txt
expect "1 edit to an excluded file passes" 0 - --exclude ex/e.txt "$note"

fresh
snap --exclude ex/e.txt "$note"
echo changed > ex/e.txt
g commit -q -am "excluded only"
expect "2 commit touching only an excluded file passes" 0 - --exclude ex/e.txt "$note"

fresh
snap --exclude ex/e.txt "$note"
echo changed > a.txt
expect "3 edit to another file exits 1" 1 "status changed" --exclude ex/e.txt "$note"

fresh
snap --exclude ex/e.txt "$note"
echo changed > a.txt
g commit -q -am "other file"
expect "4 commit touching another file exits 1" 1 "head changed" --exclude ex/e.txt "$note"

fresh
snap --exclude ex/e.txt "$note"
git config fanout.test changed
expect "5 config change exits 2" 2 "config changed" --exclude ex/e.txt "$note"

fresh
snap "$note"
echo changed > ex/e.txt
expect "6 no --exclude and an edit exits 1" 1 "status changed" "$note"

fresh
snap --exclude ex/e.txt "$note"
echo changed > .claude/backlog.toml
g commit -q -am "backlog only"
expect "7 commit touching only the backlog store passes" 0 - --exclude ex/e.txt "$note"

fresh
echo second > a.txt
g commit -q -am "second"
snap --exclude ex/e.txt "$note"
git reset -q --hard HEAD~1
expect "8 rewind past a commit to another file exits 1" 1 "head changed" --exclude ex/e.txt "$note"

fresh
snap "$note"
echo changed > ".claude/flows/$slug/x.toml"
g commit -q -am "flow store only"
expect "9 no --exclude and a flow-store commit exits 1" 1 "head changed" "$note"

fresh
snap --exclude ex/e.txt "$note"
expect "10 excludes differing from the snapshot's exit 3" 3 "differ" --exclude a.txt "$note"

[ "$failures" -eq 0 ]
