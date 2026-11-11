Status: done — review digest prints duplicate_candidates and ruled_out_hits as counts unless --with names them

Changes:
- tomlctl/src/cli/types/review/read.rs: ReviewDigestArgs gains `--with` (comma-delimited, values `duplicates` and `ruled-out`).
- tomlctl/src/review/digest.rs: the two keys hold the array length by default and the full array when named.
- tomlctl/tests/review_digest.rs: existing array-reading tests pass `--with`; added digest_candidates_are_counts_by_default and digest_with_prints_named_arrays.

Deviations: none.

Checks: cargo clippy (tomlctl) clean; rustfmt --check on the test file clean; cargo nextest -E 'test(digest)' 42 passed; both accept greps pass.

Handover: none. Note that read.rs also carries another worker's uncommitted ReviewRecheckTargetsArgs edit, untouched.

TANGENTIAL: none
