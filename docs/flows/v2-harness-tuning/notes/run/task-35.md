Status: done — findings.md's Reproduce check now builds its launches from `plan emit reproduce-units`, relaunches through `--relaunch`, and reads verdicts and gates on the milestone-wide reproduce-status call

## What changed

`claude/skills/implement-v2-engine/references/findings.md`:

- **The units.** The task and finding_links listings, the highest-id-owner rule, the per-store split, the round computation and the per-finding later-pass loop are replaced by one call, `tomlctl plan emit reproduce-units --slug <slug> --milestone <id>`, with `--since-launch` on a later pass. The text says what the verb settles (owner row, settles links excluded, round, run_dir) so the orchestrator does not recompute it. An empty `launches` goes straight to the gate.
- **Routes.** A `checker` unit is settled by pooled `review-checker` agents; a finding they leave `unsettled` reaches a `review-verifier` in the same launch. A `verifier` unit goes to a verifier directly.
- **Round and launch.** One `review-v2-pipeline` launch per `launches` entry, with that entry as args, written verbatim to `<scratch>/<run>-args.json` (the `run_dir`'s last segment) for the run hash. The old hand-built args example and the round query are gone; the old step 1 (round) was folded into the verb, so the steps are renumbered 1 to 4. The index shape now names `agents`. `plan run finish` moved into step 3, so each run is closed before the next launch, which the "started and finished before the next launch" rule and the relaunch's "after the first run is finished" already required.
- **Relaunch.** A relaunch calls the verb with `--relaunch <n> --round <r> --task <refs>`: `<r>` is the round of the launch it repeats, `--task` the refs of the units that did not settle, `n` counting from 1. Its `run_dir` ends `-r<n>`, so it never reads an earlier launch's payloads. Still once per round.
- **Read the store.** The per-finding `reproduce-status <F> --expect gone` and `--expect reproduces` calls are replaced by one `tomlctl review reproduce-status --flow <slug> --milestone <id> <store-flag> --expect gone` per store. Its output carries each finding's verdict, and the note says that exit 1 is an answer, not a failure.
- **Reproduce fix tasks.** The minted row's accept is the milestone-wide gate-owned entry from the shapes, byte-identical to the one `review schedule` writes, so gate-list deduplicates it. The guarantee paragraph now rests on that one check per store.
- **Placeholder.** `<store flag>` is renamed `<store-flag>` in its definition (Apply rows) and in the Anchor premises prose, so the fenced commands that now use it take a single-token placeholder.

The constraints hold: the two-round cap, Settle drain, Failure on an apply row and Reconcile are untouched, and the file keeps its name.

## Deviations

- The args file's name is `<scratch>/<run>-args.json` (the `run_dir`'s last segment) rather than the old `reproduce-<id>-<round>-args.json`. A relaunch shares its round, and with two stores both launches share it, so the old name would collide.
- The relaunch now covers the failed units of a launch together in one call, not one launch per unit. The required means asks for `--task` naming the refs of the units that did not settle, and the verb takes a list.

## Checks

- Accept: `grep -q 'plan emit reproduce-units'`: pass.
- Accept: `grep -q -- '--relaunch'`: pass.
- Accept: `grep -q 'reproduce-status --flow'`: pass.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed, 0 failed.
- No compile check was needed, since the edit is markdown only. The flags were checked against `tomlctl plan emit reproduce-units --help`, `tomlctl review reproduce-status --help`, `tomlctl/src/plan/emit.rs` `reproduce_units`, and the index shape in `claude/workflows/review-v2.js`.

## Handover

None.

TANGENTIAL: none
