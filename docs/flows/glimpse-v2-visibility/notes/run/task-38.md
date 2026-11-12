Status: done — the output-options harness normalises a clock case's call-time stamp instead of retrying across second boundaries

## Cause

`plan emit launch-args` prints `digest_at`, the whole-second UTC time of the call (`tomlctl/src/plan/emit.rs`, `window_open` in `tomlctl/src/plan/lane.rs`). It is the only call-time value in that report: `args`, `args_sha256` and `store_digest` derive from the fixture's store, whose trail rows all predate the call. The harness compares many separate invocations of one case (`--lines`, `--get '*'`, `--header`, `--rows … --lines`, `--max-chars`). Its only guard was to rerun a `clock` case up to three times. The milestone's three `plan run` cases make the plan group longer, so every attempt could straddle a second boundary.

## What changed

`tomlctl/tests/output_options.rs`:
- `run` takes the second before spawning the command. For a `clock` case it replaces each RFC 3339 stamp in stdout no earlier than that second with the constant `CALL_TIME` (`2000-01-01T00:00:00Z`). It reuses the file's existing `datetime()` regex, which `snapshot` already uses to mask write stamps. Older stamps, which are fixture data, are left as they are, so only the call time is exempted.
- `CALL_TIME` keeps a real stamp's length and first character. The `--max-chars 1` run prints the real stamp trimmed to `2…(+19)`, which no mask can recognise, and that still equals the trim of the masked report.
- The retry loop in `check` is removed and `check_once` folded back into `check`, since the cause it worked around is gone. Every assertion is as strict as before.
- The `clock` field's comment now says what the harness does with it.

No behaviour of `plan emit launch-args` changed.

## Deviations

None from the suggested means. It said to normalise in the harness's comparison and reuse an existing volatile-field mechanism; this reuses the `datetime()` mask. Two other approaches were weighed:
- Masking the `digest_at` key in parsed JSON would miss the `--get '*'` and trimmed text forms.
- Lengthening the retry would still be timing-dependent.

## Checks

- `rustfmt --edition 2024 --check tomlctl/tests/output_options.rs`: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --test output_options`: clean.
- Accept check (`cargo nextest … every_case_honours_the_output_options::plan`): `Run by: gate`, so the worker did not run it.

## Handover

None.

TANGENTIAL: none
