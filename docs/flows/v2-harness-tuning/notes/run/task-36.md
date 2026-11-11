Status: done — the carriers require the new features, and the engine skills and CLAUDE.md name the pool, the emit verbs, the gate probe, the audit and fix_files

## What changed

- `claude/commands/implement-v2.md`: pre-flight step 2 now also requires `plan_emit`, `accept_run_by`, `gate_probes` and `agents_audit`, and its stop reason names the emit and audit surface.
- `claude/commands/apply-v2.md`: pre-flight step 2 now also requires `plan_emit`, `accept_run_by`, `reproduce_status_milestone` and `review_fix_files`.
- `claude/skills/implement-v2-engine/SKILL.md`: step table rows name the verbs. Premises uses `plan probes --premises`, and the gate probes (`--gates`) run at the pick, in run-milestone section 2. Launch uses `plan emit launch-args`, Reproduce check uses `plan emit reproduce-units`, and Gate uses `plan emit gate-list` with the probe at the pick. Rule 3 says the Workflow runs a rolling pool that starts a ready file-disjoint task as a slot frees and releases an `after:` task once its target is done. The Vetting and harvest bullet adds the drain's `tomlctl agents audit` read and says the verb reads the transcripts and prints only matching commands, so the no-transcript rule still holds. The Worktree bullet no longer says "per wave".
- `claude/skills/apply-v2-engine/SKILL.md`: the Preview has a new bullet, "A fix's other files". A would-mint row's files already include `fix_files`. When a finding has no `fix_files` but its fix text names a file outside the row, the row is widened between step 6 and step 9. The widening uses the engine's audited update (drain.md step 6), with a reason opening `apply <store>: `.
- `CLAUDE.md`, Implement v2: the Workflow bullet says rolling pool. The Gate and commit bullet takes the gate commands from `tomlctl plan emit gate-list`. The Deploy bullet's refusal list gains the four features. The Developer setup section is untouched.

## Deviations

- apply-v2-engine: the detail asks for an "audited `tasks update`". I wrote no fenced command. Instead the bullet points at the engine's existing "The audited update" subsection (drain.md step 6), so the record-file form and reason rules are not restated. The alternative was a fenced `tasks update --set files=... --unlock-import-fields` line, which would duplicate drain.md's procedure and could drift from it.

## Checks

- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed, including skill_markdown_links_resolve and command_lint.
- All four worker accept greps pass.
- Feature names confirmed against the installed `tomlctl capabilities --get features` and `tomlctl/src/cli/types/mod.rs`.

## Notes

- The siblings list included `CLAUDE.md` and `implement-v2-engine/SKILL.md`. Their other holders, tasks 38 and 39, were still `pending` when I ran, so they made no concurrent edit.
- The `findings.md` links were left as they were.

TANGENTIAL: none
