Status: done — plan-review prose now applies accepted advisory-tier edits, records hand fixes as addressed, re-checks from `review recheck-targets`, and reads the digest arrays with `--with`

## What changed

- `claude/skills/review-v2-engine/references/plan-flow.md`
  - Section 1: `review_addressed` and `review_digest_with` added to the capability pre-flight list.
  - Section 7, Question tier: a Revise answer is recorded with `review triage <ids> --addressed-file <path>` (plus `--targets <targets>` when other records changed); Keep keeps `--resolve-file` with its own fenced command. The stats sentence names `author-addressed` beside `user-answered`.
  - Section 7, Advisory: an advisory-tier defect or cleanup row carrying `new` is offered as an edit and, once accepted, applied with `review apply <ids> --user-accepted` (dry run first). A finding the author fixed by hand is recorded with `review triage --addressed-file`, never dismissed. The old "never applied" sentence is gone.
  - Section 8: addressed targets on a `spec.accept` entry also trigger a re-probe.
  - Section 10: the re-check runs when a row was applied or addressed, and takes `targets` from `tomlctl review recheck-targets --run <run> --slug <slug>`, skipping when that array is empty. The launch's `targets` is that array copied as it stands.
  - Section 11: the close summary names advisory-tier edits accepted and the `author-addressed` count.
- `claude/skills/review-v2-engine/references/triage-and-close.md`: `--addressed-file` added to the file-twin list. New grammar row `addressed F4 — <what changed>` (plan family only), with `--targets` and the `--addressed` inline twin.
- `claude/skills/review-v2-engine/references/compose.md`: the Duplicates and Rule-out hits sections say the plain digest prints counts only, and read the arrays with `review digest ... --since-wave <wave> --with duplicates,ruled-out`.
- `claude/skills/review-v2-engine/SKILL.md`: the stage 0 halt list adds `review_digest_with`. Stage 5 now describes the two arrays as counts, and before the lists it adds one `--with duplicates,ruled-out` digest read for when either count is above zero. The depth envelope and verifier dispatch rules are untouched.
- `claude/agents/plan-review-lead.md`: settle step 3 reads both arrays in one `--with duplicates,ruled-out` call, and step 4 takes `ruled_out_hits` from that same read.

## Deviations

None from a `suggested` means. Both `required` means were followed: the addressed mode is written as `--addressed-file` with `--targets`, and `review_digest_with` sits in both pre-flight lists. The digest `--with` reads add `--since-wave <wave>` where the wave's digest was already read that way. The plan wants the arrays read for the wave being settled.

## Handover

None.

## Checks

- The task does no Rust edit, so no compile check was needed.
- Ran `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed.
- Accept checks: plan-flow `--addressed` and `recheck-targets` pass; lead `--with duplicates` passes; triage-and-close `--addressed` passes.

TANGENTIAL: none
