Status: done — the shared-block verifier checks every pair in one gawk pass, 0.62 s against about 24 s, with identical verdicts and stderr

```toml run-note
files_touched = ["scripts/verify-shared-blocks.sh", "tomlctl/src/cli/dispatch/tests/skills.rs"]

[[deviations]]
what = "The manifest parse stays a separate gawk run and the per-pair bash loop stays, but it now only runs the `-f` builtin and builds the checker's input. So the script starts two gawk processes, not one"
why = "The detail asks for each bash `-f` result to be passed into the gawk program so the missing-file message keeps its place in pair order. That needs the pair list in bash before the checker runs. The loop is builtins only and starts no process"
means = "design/gates.md#one-pass-verifier: one gawk program that reads the manifest and each carrier"

[[deviations]]
what = "The extraction lives in a gawk function named extract_block, and the CR-contract comment moved above the gawk call"
why = "The empty-extraction message, which must stay verbatim, says 'see the CR contract at extract_block'. Keeping that name means the message still points at real code"
means = "design/gates.md#one-pass-verifier"

[[deviations]]
what = "The existing both_gates_reject_a_cr_marker_and_an_empty_span body was moved into a shared helper, assert_both_gates_reject, which the new drift and missing-START test also uses"
why = "The new test needs the same fixture-writing, git-init and two-gate assertion. One helper is clearer than a second copy. The old test's cases and assertions are unchanged"
means = "detail: add a test beside both_gates_reject_a_cr_marker_and_an_empty_span"

[[tangential]]
kind = "debt"
area = "scripts/verify-shared-blocks.sh"
summary = "gawk strings keep NUL bytes, but bash variables dropped them. A carrier block that differs only in NUL bytes now counts as drift where the old loop passed it"
why = "This is a narrow edge that shell-gate-cost.md already flagged. The mirror compares bytes, so the new behaviour is closer to blocks_verify, but no test pins it"

[[checks]]
command = "! grep -q 'has_marker \"' scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass (shared-block parity: OK, real 0.62 s)"

[[checks]]
command = "s=$(date +%s%N); bash scripts/verify-shared-blocks.sh >/dev/null && test $(( ($(date +%s%N) - s) / 1000000 )) -lt 12000"
result = "pass"

[[checks]]
command = "scratch fixture comparison: HEAD script vs edited script, exit code + stdout + stderr over 11 git-init fixtures"
result = "pass (byte-identical output)"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/cli/dispatch/tests/skills.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --lib --tests"
result = "pass (no diagnostics in skills.rs; the dead-code warnings name only the sibling tomlctl/src/invocation.rs)"
```

## What changed

**scripts/verify-shared-blocks.sh.** The gnu-awk-resolver block, `git rev-parse`, the manifest check, the hasher check and the manifest-parse gawk are unchanged. The per-pair loop and the `has_marker` and `extract_block` shell functions are gone, and they started about eight processes per pair. Now a builtin-only bash loop records each carrier's `-f` result as a third tab-separated field. One gawk program then reads those records and checks each pair in order:

- missing file, using the `-f` result;
- START presence, then END presence, each a whole-line match after stripping one trailing CR;
- extraction by an exact marker match with no CR stripped, where each content line loses one CR and gains `\n`, and every span is concatenated;
- the empty-extraction check;
- comparison as strings with the block's first good carrier.

Hashes come from a `|&` coprocess to `$HASHER`, and only on drift. The first carrier's hash is memoised. Every message text, its order, and exit codes 0, 1 and 2 are kept. A gawk failure propagates its own exit code, so a broken program still exits 2 (could not run).

The fixtures were built in a temp directory outside the repo, each a `git init` with `MANIFEST=manifest.toml`: ok, a `\r\r\n` marker, an empty span, drift across four carriers, a missing START, a CRLF carrier, two spans against one, a missing file, a mixed case, no pairs and no manifest. The mixed case covers two blocks, a directory as a carrier, a missing END, an empty span between other errors, and missing files placed between drift messages. All eleven gave byte-identical exit code, stdout and stderr against the HEAD script, drift hashes included.

**tomlctl/src/cli/dispatch/tests/skills.rs.** I added `both_gates_reject_drift_and_a_missing_start`. Its drift case is two carriers with different spans, and its missing-START case is a carrier with only an END marker. It asserts that both `blocks_verify` and `run_shell_gate` reject each case. The fixture setup and the two-gate assertion moved into `assert_both_gates_reject`, which the existing CR-marker and empty-span test now calls too. Its behaviour is unchanged. I did not run the nextest accept check, because the gate runs it.
