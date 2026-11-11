#!/usr/bin/env bash
# Records that the pre-commit hook's cargo steps passed on the working tree as it
# now is, so the hook can skip them on later commits until a file they read changes.
#
#   gate-stamp.sh run    run every cargo step, whatever is staged; stamp on a full pass
#   gate-stamp.sh check  exit 0 only when the stamp holds this working tree's digest
#
# The digest covers the path and raw bytes of every tracked or untracked, unignored
# file under the roots below; a tracked file missing from the tree counts by path.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
STAMP="$(git rev-parse --absolute-git-dir)/gate-stamp"

# scripts/ is a root because the dispatch tests read scripts/shared-blocks.toml
# and run scripts/verify-shared-blocks.sh.
ROOTS=(tomlctl glimpse claude .claude/review-lenses docs/plans scripts)

digest() {
  local -a paths=() present=() hashes=()
  local path i=0
  while IFS= read -r -d '' path; do
    paths+=("$path")
    if [ -f "$path" ]; then present+=("$path"); fi
  done < <(git ls-files -z --cached --others --exclude-standard -- "${ROOTS[@]}" | LC_ALL=C sort -z -u)

  # One process for every file. --no-filters hashes the bytes on disk, with no
  # line-ending conversion, so a CRLF rewrite voids the stamp.
  if [ "${#present[@]}" -gt 0 ]; then
    mapfile -t hashes < <(printf '%s\n' "${present[@]}" | git hash-object --no-filters --stdin-paths)
  fi

  for path in "${paths[@]}"; do
    if [ "$i" -lt "${#present[@]}" ] && [ "${present[$i]}" = "$path" ]; then
      printf '%s %s\0' "${hashes[$i]}" "$path"
      i=$((i + 1))
    else
      printf 'absent %s\0' "$path"
    fi
  done | sha256sum -b | {
    read -r sum _
    printf '%s\n' "$sum"
  }
}

step() {
  printf 'gate-stamp: %s\n' "$*"
  if ! "$@"; then
    rm -f "$STAMP"
    printf 'gate-stamp: failed, no stamp written. Rerun after fixing:\n  %s\n' "$*" >&2
    exit 1
  fi
}

case "${1:-}" in
  run)
    rm -f "$STAMP"
    before=$(digest)
    step cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
    step cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests
    step cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored
    step cargo fmt --manifest-path glimpse/Cargo.toml -- --check
    step cargo clippy --manifest-path glimpse/Cargo.toml --locked
    # A file edited while the steps ran was not what they tested.
    after=$(digest)
    if [ "$before" != "$after" ]; then
      printf 'gate-stamp: the working tree changed during the run, no stamp written. Rerun.\n' >&2
      exit 1
    fi
    printf '%s\n' "$after" >"$STAMP"
    printf 'gate-stamp: every cargo step passed, stamp written\n'
    ;;
  check)
    [ -f "$STAMP" ] || exit 1
    read -r stamped <"$STAMP" || exit 1
    [ "$stamped" = "$(digest)" ]
    ;;
  *)
    printf 'usage: %s run|check\n' "${0##*/}" >&2
    exit 2
    ;;
esac
