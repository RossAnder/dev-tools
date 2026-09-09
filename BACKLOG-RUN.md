# Backlog run

| id | verdict | files | note |
|---|---|---|---|
| B-2fa4cf64 | needs-human | — | Adding CI or changing the deliberately fast pre-commit hook is a build-policy decision. |
| B-194cd19e | already-resolved | — | All three named Rust files contain zero CRLF sequences in this worktree. |
| B-15c11568 | needs-human | — | Enveloping the bare array is a breaking output change for callers. |
| B-e977ee2e | needs-human | — | Cross-platform CR handling requires a platform-contract decision. |
| B-14676dff | needs-human | — | Replacing the Windows Git Bash fallback needs environment-discovery policy. |
| B-4fdfea59 | needs-human | — | An intermittent linker permission failure needs root-cause diagnosis rather than a bounded change. |
| B-e129986c | needs-human | — | Unifying output-envelope documentation across thirteen verbs needs a documentation-format decision. |
| B-52825802 | fixer-failed | — | Required `codex exec --profile gpt` could not resolve a home directory before starting. |
| B-1cb6596a | fixer-failed | — | Required `codex exec --profile gpt` could not resolve a home directory before starting. |
| B-6b21fa60 | fixer-failed | — | Required `codex exec --profile gpt` reached the CLI but app-server access was denied before starting. |
| B-c49565c6 | fixer-failed | — | Required `codex exec --profile gpt` reached the CLI but app-server access was denied before starting. |
| B-92c6e467 | needs-human | — | Removing setup and teardown tools from the shell gate is a portability and failure-mode policy decision. |
| B-cf956199 | needs-human | — | Including shell files would trigger known gate failures and requires a scope-policy decision. |
