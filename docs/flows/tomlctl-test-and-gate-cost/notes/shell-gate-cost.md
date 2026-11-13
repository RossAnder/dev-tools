Verdict: the 24 s is about 500 process starts, eight per (block, file) pair over 63 pairs in the loop at scripts/verify-shared-blocks.sh:135-177; one gawk pass over the manifest and every carrier removes all of them, keeps the resolver, and measured 0.84 s with the same verdict on the repo and 8 fixtures

## Answer

The manifest lists 63 (block, file) pairs (18 blocks). For each pair the loop starts:

- two gawk processes in `has_marker` (lines 144, 149), one per marker;
- one subshell plus one gawk for `block=$(extract_block ...; printf 'x')` (line 157);
- one subshell, a forked `printf` pipeline stage, `sha256sum` and a gawk for `h=$(printf | $HASHER | "$AWK" '{print $1}')` (line 166).

An xtrace of one run counted 254 gawk execs (126 `has_marker`, 63 extract, 63 hash-field, 1 manifest parse, 1 `--version`) and 63 `sha256sum`, plus about 190 forked subshells and pipeline stages. Measured on this machine: a bare gawk start costs about 73 ms (20 in 1.45 s), an empty `$(:)` subshell about 32 ms (20 in 0.64 s), and the hash line about 200 ms (20 in 4.0 s). That puts a pair at about 450 ms and 63 pairs at about 28 s, against 25.8 s for the whole script. The script does almost no work of its own; the time is all process starts.

Every per-pair start can go. One gawk invocation can parse the manifest, read each carrier with `getline < file`, apply the `has_marker` test (whole-line match after stripping a trailing CR) and the `extract_block` rule (exact marker match, content `sub(/\r$/,"")`) in the same read, and compare each block's extracted string with the first carrier's. Since the hash only feeds equality, string equality decides the same verdict. The `drift:` message still needs hashes, so compute them through a gawk coprocess (`printf "%s", s |& hasher`) only on the drift path. The resolver block (lines 22-54, shared byte-for-byte with scripts/doc-diff-gate.sh), `git rev-parse` (line 56) and the hasher presence check (lines 65-72, builtins only) stay as they are. That keeps the GNU awk requirement and exit 2 for "could not run". What remains is about 4 starts: the resolver's `command -v` subshell, `gawk --version`, `git rev-parse` and the one gawk.

A prototype at that shape (scratch only, not in the tree) ran in 0.84 s on the repo and printed `shared-block parity: OK`. On eight fixtures (ok, `\r\r\n` marker, empty span, drift, missing START, CRLF carrier, two spans against one, missing file) its exit code matched the current script's every time. Its stderr matched too, the drift hashes byte for byte, except that the prototype left out the empty-extraction message's parenthetical, which can be kept verbatim. The success criterion's bar is "under half of 24 s", and this clears it by a factor of about 30. It would also take blocks_verify_agrees_with_shell_gate from about 24 s to about 1 s.

## Findings

- **Library/API or file**: scripts/verify-shared-blocks.sh:135-177, the per-pair `while read` loop (`has_marker`, `extract_block`, the `h=$(...)` hash line)
- **Source**: `time bash scripts/verify-shared-blocks.sh` gave `real 0m25.784s`. `bash -x` with xtrace to a scratch file counted `126 + has_marker`, `63 ++ extract_block`, `63 ++ sha256sum` and 254 gawk lines. Start costs came from 20-iteration loops: gawk 1.453 s, `$(:)` 0.642 s, the hash pipeline 4.035 s.
- **Evidence-grade**: high
- **Finding**: The script's 24-26 s is about 8 process starts for each of 63 pairs; nothing else in it costs measurable time.
- **Details**: Each pair costs 2 `has_marker` gawks, a subshell plus gawk for `extract_block`, and a subshell, `printf` stage, `sha256sum` and gawk for the hash. At about 73 ms per gawk exec and about 32 ms per fork under MSYS on this machine, that comes to about 450 ms a pair. The loop scales with the pair count, so every carrier added to scripts/shared-blocks.toml costs about 0.4 s more per commit and per test run.
- **Counter**: A run where the 63 pairs together take well under 20 s, while the script stays above 20 s, would mean the time sits outside the loop. The xtrace showed nothing outside the loop beyond one manifest gawk and the resolver.
- **Impact on plan / report**: The task is to restructure this loop. Do not tune the hasher or the resolver.

- **Library/API or file**: scripts/verify-shared-blocks.sh:86-177 (`extract_block`, `has_marker`, the pair loop), to be replaced by one gawk program; the resolver at lines 22-54 is untouched, so scripts/doc-diff-gate.sh is not touched
- **Source**: a scratch prototype (one gawk pass with `getline < file`, string equality per block, and a `|&` coprocess to `$HASHER` only on drift) run against the repo and 8 git-initialised fixtures, each with `MANIFEST=manifest.toml`. Repo: `real 0m0.836s`, exit 0. Fixtures: `ok 0/0, crmarker 1/1, empty 1/1, drift 1/1, nostart 1/1, crlf 0/0, twospan 0/0, missing 1/1` (current/prototype), with identical drift hashes.
- **Evidence-grade**: high
- **Finding**: Fold manifest parse, marker checks, extraction and comparison into one gawk invocation and hash only on drift. The script goes from about 500 starts to about 4 and from 25.8 s to 0.84 s, with the same exit codes.
- **Details**: The CR contract survives because the in-gawk `getline < file` reads in the same text mode as gawk's main input. The `\r\r\n` marker fixture still fails as an empty extraction and the CRLF carrier still passes. Rules to keep exactly: `$0 == start` / `$0 == end` with no CR strip for extraction, a CR-stripped match for presence, and content lines with one more `sub(/\r$/,"")` plus `"\n"`. Error order and texts carry over unchanged, as do exit 2 for no pairs, a missing manifest, no hasher or no gawk. Two edge differences to decide on. First, bash variables drop NUL bytes and gawk strings keep them, so a block differing only in NULs would newly count as drift. Second, the current `[[ ! -f $bfile ]]` treats a directory as missing, while `getline` on a directory returns -1. A bash `-f` pre-check is a builtin and costs nothing, so it can stay.
- **Counter**: A carrier read differently by `getline < file` and by gawk's file-argument input under some gawk build would break the CR contract. The `crmarker` and `crlf` fixtures above agreed under the MSYS gawk the hook uses, but no non-MSYS gawk was tried.
- **Impact on plan / report**: One task, `lite`-sized, touching only scripts/verify-shared-blocks.sh. Its gate is `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (blocks_verify_agrees_with_shell_gate and both_gates_reject_a_cr_marker_and_an_empty_span compare only exit codes, tomlctl/src/cli/dispatch/tests/skills.rs:174-176) plus the 8-fixture comparison above. The extract_block CR-contract comment (lines 80-85) and the "tomlctl/src/blocks.rs is a mirror" comment (lines 74-79) move with the code.

Outside this lens: the stated reason the brief gives for blocks_verify_agrees_with_shell_gate's 203 test-seconds is this script. No separate batching of its shell calls is needed once the script is fast.

Coverage: read in full scripts/verify-shared-blocks.sh, scripts/shared-blocks.toml, docs/flows/tomlctl-test-and-gate-cost/brief.md, and tomlctl/src/cli/dispatch/tests/skills.rs:130-375. Checks run: a timed run, an xtrace count, the start-cost microbenchmarks, and a prototype against the repo and 8 fixtures. Ruled out: swapping `sha256sum` for a faster hasher (it is 63 of about 500 starts, and the shape is the cost); caching hashes across runs (it adds state, and one pass is already under 1 s); trimming the resolver (shared with doc-diff-gate.sh, 2-3 starts, and it carries the GNU awk requirement).

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = ["docs/flows/tomlctl-test-and-gate-cost/brief.md"]

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 135
claim = "per-pair loop starting has_marker, extract_block and hash pipeline processes for each of 63 pairs"

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 166
claim = "hash line: subshell plus printf | sha256sum | gawk per pair"

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 22
claim = "gnu-awk-resolver shared block, kept unchanged"

[[source]]
path = "scripts/shared-blocks.toml"
line = 24
claim = "18 blocks listing 63 (block, file) pairs"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/skills.rs"
line = 174
claim = "the dispatch tests read only the script's exit code (0 ok, 1 drift, other = not runnable)"

[[source]]
path = "docs/flows/tomlctl-test-and-gate-cost/brief.md"
line = 35
claim = "success criterion: same verdict on the dispatch fixtures in under half of 24 s"
```
