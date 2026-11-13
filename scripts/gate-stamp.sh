#!/usr/bin/env bash
# Records that each of the pre-commit hook's cargo steps passed on the working tree as
# it now is, so the hook can skip a step on later commits until a file it reads changes.
#
#   gate-stamp.sh run [<id>...]  run the named steps, or every step, whatever is staged
#   gate-stamp.sh check [<id>]   exit 0 only when the step's stamp, or every step's,
#                                holds the digest of its roots as they are now
#   gate-stamp.sh command <id>   print the step's command, one line of words
#
# A step's digest covers its id, its command, and the path and raw bytes of every
# tracked or untracked, unignored file under its roots; a tracked file missing from
# the tree counts by path. An unknown id is a usage error, exit 2.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
GIT_DIR_ABS="$(git rev-parse --absolute-git-dir)"
STAMP_DIR="$GIT_DIR_ABS/gate-stamp.d"
LEGACY_STAMP="$GIT_DIR_ABS/gate-stamp"

# The step table, in run order. Commands are repo-relative and have no space inside
# a word: the hook splits the printed command on whitespace.
STEPS=(fmt-tomlctl dispatch-tests plan-corpus fmt-glimpse workspace-clippy)
declare -A COMMAND ROOTS
COMMAND[fmt-tomlctl]='cargo fmt --manifest-path tomlctl/Cargo.toml -- --check'
ROOTS[fmt-tomlctl]='tomlctl'
# scripts/ is a root because the dispatch tests read scripts/shared-blocks.toml
# and run scripts/verify-shared-blocks.sh. Every step that compiles reads the
# workspace's root Cargo.toml and Cargo.lock.
COMMAND[dispatch-tests]='cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests'
ROOTS[dispatch-tests]='tomlctl claude .claude/review-lenses scripts Cargo.toml Cargo.lock'
COMMAND[plan-corpus]='cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored'
ROOTS[plan-corpus]='tomlctl docs/plans .claude/backlog.toml Cargo.toml Cargo.lock'
COMMAND[fmt-glimpse]='cargo fmt --manifest-path glimpse/Cargo.toml -- --check'
ROOTS[fmt-glimpse]='glimpse'
COMMAND[workspace-clippy]='cargo clippy --workspace --locked'
ROOTS[workspace-clippy]='glimpse tomlctl Cargo.toml Cargo.lock'

usage() {
  printf 'usage: %s run [<id>...] | check [<id>] | command <id>\n  ids: %s\n' \
    "${0##*/}" "${STEPS[*]}" >&2
  exit 2
}

known() {
  [ -n "${COMMAND[$1]+set}" ]
}

# Prints "<id> <digest>" for each id given. One file listing and one hash pass over
# the union of their roots; each step's digest folds only the entries under its roots.
digests() {
  local -a ids=("$@") roots=() paths=() present=() hashes=() entries=() mine=()
  local id path root i=0
  for id in "${ids[@]}"; do
    read -ra mine <<<"${ROOTS[$id]}"
    roots+=("${mine[@]}")
  done

  while IFS= read -r -d '' path; do
    paths+=("$path")
    if [ -f "$path" ]; then present+=("$path"); fi
  done < <(git ls-files -z --cached --others --exclude-standard -- "${roots[@]}" | LC_ALL=C sort -z -u)

  # One process for every file. --no-filters hashes the bytes on disk, with no
  # line-ending conversion, so a CRLF rewrite voids the stamp.
  if [ "${#present[@]}" -gt 0 ]; then
    mapfile -t hashes < <(printf '%s\n' "${present[@]}" | git hash-object --no-filters --stdin-paths)
  fi

  for path in "${paths[@]}"; do
    if [ "$i" -lt "${#present[@]}" ] && [ "${present[$i]}" = "$path" ]; then
      entries+=("${hashes[$i]} $path")
      i=$((i + 1))
    else
      entries+=("absent $path")
    fi
  done

  local entry sum
  for id in "${ids[@]}"; do
    read -ra mine <<<"${ROOTS[$id]}"
    sum=$(
      {
        printf 'step %s\0command %s\0' "$id" "${COMMAND[$id]}"
        for entry in "${entries[@]}"; do
          path=${entry#* }
          for root in "${mine[@]}"; do
            if [ "$path" = "$root" ] || [ "${path#"$root"/}" != "$path" ]; then
              printf '%s\0' "$entry"
              break
            fi
          done
        done
      } | sha256sum -b
    )
    printf '%s %s\n' "$id" "${sum%% *}"
  done
}

digest_of() {
  local line
  line=$(digests "$1")
  printf '%s\n' "${line#* }"
}

matches() {
  local stamped
  [ -f "$STAMP_DIR/$1" ] || return 1
  read -r stamped <"$STAMP_DIR/$1" || return 1
  [ "$stamped" = "$2" ]
}

case "${1:-}" in
  run)
    shift
    ids=("$@")
    if [ "${#ids[@]}" -eq 0 ]; then ids=("${STEPS[@]}"); fi
    for id in "${ids[@]}"; do known "$id" || usage; done
    rm -f "$LEGACY_STAMP"
    mkdir -p "$STAMP_DIR"
    changed=()
    for id in "${ids[@]}"; do
      rm -f "$STAMP_DIR/$id"
      read -ra cmd <<<"${COMMAND[$id]}"
      before=$(digest_of "$id")
      printf 'gate-stamp: %s: %s\n' "$id" "${COMMAND[$id]}"
      if ! "${cmd[@]}"; then
        printf 'gate-stamp: %s failed, no stamp written. Rerun after fixing:\n  %s\n' \
          "$id" "${COMMAND[$id]}" >&2
        exit 1
      fi
      # A file edited while the step ran was not what it tested.
      if [ "$before" = "$(digest_of "$id")" ]; then
        printf '%s\n' "$before" >"$STAMP_DIR/$id"
      else
        changed+=("$id")
      fi
    done
    if [ "${#changed[@]}" -gt 0 ]; then
      printf 'gate-stamp: files under the roots of %s changed during the run, no stamp written for them. Rerun.\n' \
        "${changed[*]}" >&2
      exit 1
    fi
    printf 'gate-stamp: %s passed, stamps written\n' "${ids[*]}"
    ;;
  check)
    [ "$#" -le 2 ] || usage
    if [ "$#" -eq 2 ]; then
      known "$2" || usage
      ids=("$2")
    else
      ids=("${STEPS[@]}")
    fi
    out=$(digests "${ids[@]}")
    seen=0
    while read -r id sum; do
      matches "$id" "$sum" || exit 1
      seen=$((seen + 1))
    done <<<"$out"
    [ "$seen" -eq "${#ids[@]}" ]
    ;;
  command)
    [ "$#" -eq 2 ] && known "$2" || usage
    printf '%s\n' "${COMMAND[$2]}"
    ;;
  *)
    usage
    ;;
esac
