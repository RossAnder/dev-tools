#!/usr/bin/env bash
# Shared by post-merge and post-checkout. Runs only in the primary checkout: the
# links in ~/.claude/ point there, so every linked worktree would report all of
# them as wrong-target.
[ "$(git rev-parse --git-dir)" = "$(git rev-parse --git-common-dir)" ] || exit 0
command -v pwsh >/dev/null 2>&1 || exit 0
pwsh -NoProfile -File "$(git rev-parse --show-toplevel)/scripts/deploy-claude.ps1" -Check >&2 || true
exit 0
