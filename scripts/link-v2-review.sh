#!/bin/sh
# Links every v2 command, agent, skill and Workflow (review, plan, implement and apply) into the
# Claude config dir ($CLAUDE_CONFIG_DIR, else ~/.claude) with symbolic links; the POSIX sibling of
# link-v2-review.ps1, linking the same entries. Idempotent: an entry already linked to its
# source is skipped. Nothing under the config dir is ever deleted or overwritten: a real
# file or directory, or a link pointing elsewhere, in an entry's place is reported and
# left alone. --dry-run reports what would be made and writes nothing.
#
# Exit: 0 all linked; 1 any refusal or failure; 64 an unknown argument.

set -u

dry_run=0
case "${1-}" in
    '') ;;
    -n|--dry-run) dry_run=1 ;;
    *) echo "usage: $0 [--dry-run]" >&2; exit 64 ;;
esac

repo=$(cd -P -- "$(dirname -- "$0")/.." && pwd) || exit 1
config=${CLAUDE_CONFIG_DIR:-$HOME/.claude}

entries='
commands/review-v2.md
commands/optimise-v2.md
commands/review-plan-v2.md
commands/plan-v2.md
commands/implement-v2.md
commands/apply-v2.md
agents/review-surveyor.md
agents/review-sweeper.md
agents/review-checker.md
agents/review-finder.md
agents/review-finder-cf.md
agents/review-verifier.md
agents/plan-executor.md
agents/plan-review-lead.md
agents/plan-investigator.md
agents/plan-challenger.md
agents/plan-cold-reader.md
agents/plan-worker-lite.md
agents/plan-worker-deep.md
skills/review-v2-engine
skills/review-v2-method
skills/review-v2-lenses
skills/plan-v2-engine
skills/implement-v2-engine
skills/apply-v2-engine
workflows/review-v2.js
workflows/plan-v2-review.js
workflows/plan-v2-investigate.js
workflows/plan-v2-prove.js
workflows/implement-v2-milestone.js
'

# The physical path a path names, its directory components resolved; a directory is
# resolved whole. A file that is itself a link is not followed, so a chain of file links
# compares unequal and is refused.
resolve() {
    if [ -d "$1" ]; then
        (cd -P -- "$1" && pwd)
    else
        d=$(cd -P -- "$(dirname -- "$1")" && pwd) && printf '%s/%s\n' "$d" "$(basename -- "$1")"
    fi
}

echo "config dir: $config"
[ "$dry_run" -eq 1 ] && echo "dry run: nothing is written"

made=0 skipped=0 refused=0

for label in $entries; do
    source=$repo/claude/$label
    dest=$config/$label

    if [ ! -e "$source" ]; then
        echo "refused  $label  source missing: $source"
        refused=$((refused + 1))
        continue
    fi

    # -L before -e, so a dangling link is reported rather than retried.
    if [ -L "$dest" ]; then
        target=$(readlink -- "$dest")
        case $target in
            /*) abs=$target ;;
            *) abs=$(dirname -- "$dest")/$target ;;
        esac
        if [ -e "$abs" ] && [ "$(resolve "$abs")" = "$(resolve "$source")" ]; then
            echo "skipped  $label  already linked -> $target"
            skipped=$((skipped + 1))
        else
            echo "refused  $label  link points elsewhere: $target"
            refused=$((refused + 1))
        fi
        continue
    fi
    if [ -e "$dest" ]; then
        echo "refused  $label  a real file or directory is in its place"
        refused=$((refused + 1))
        continue
    fi

    if [ "$dry_run" -eq 1 ]; then
        echo "would    $label  -> $source"
        made=$((made + 1))
        continue
    fi

    # ln without -f fails rather than replace anything that appeared since the check.
    if mkdir -p -- "$(dirname -- "$dest")" && ln -s -- "$source" "$dest"; then
        echo "made     $label  -> $source"
        made=$((made + 1))
    else
        echo "refused  $label  ln -s failed"
        refused=$((refused + 1))
    fi
done

[ "$dry_run" -eq 1 ] && verb='would make' || verb=made
echo "$verb $made, skipped $skipped, refused $refused"
[ "$refused" -gt 0 ] && exit 1
exit 0
