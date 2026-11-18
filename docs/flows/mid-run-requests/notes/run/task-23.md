Status: done — request paths canonicalised, item refs keyed by parsed index, generated-map errors no longer refuse add, coupling dropped from the holder closure, one missing-outputs rule

```toml run-note
files_touched = [
  "tomlctl/src/generated.rs",
  "tomlctl/src/plan/holders.rs",
  "tomlctl/src/plan/request.rs",
  "tomlctl/src/plan/check/generated_outputs.rs",
  "tomlctl/src/plan/view.rs",
]

[[deviations]]
what = "The path comparison key now lives in generated.rs as `path_key` (over a new `canonical_path`), and holders::fold_key delegates to it, instead of generated.rs reusing fold_key."
why = "holders is a private module of plan (plan/mod.rs declares `mod holders;`) and plan/mod.rs is not this task's file, so the crate-level generated module cannot reach fold_key. Defining the key once in generated.rs keeps one definition for every comparison site; the alternative, a second copy of the folding rule, is what the finding complains about."
means = "missing_outputs compares by fold_key's definition"

[[deviations]]
what = "The duplicate-path canonicalisation was applied both ways: parse_payload stores the canonical form and refuses an empty one, and fold_key itself collapses empty and `.` segments."
why = "Rows' own `files` and earlier grants are compared through fold_key too, so canonicalising only the request side would still miss a row whose files say `src/./a.rs`. A leading `/` is kept so forbidden() still reads `//etc/passwd` as absolute."
means = "canonicalise in parse_payload, or alternatively in fold_key"

[[deviations]]
what = "The holder closure walks `needs` only; spec `after:` targets are not added."
why = "The finding names them as optional. Leaving them out can only treat more rows as possible holders, which denies more and never grants a concurrent path."
means = "walk needs plus, optionally, after: premise targets"

[[deviations]]
what = "`plan request add` loads the generated-file map only when the payload has paths, and on a load error falls back to no generators and adds a `warnings` array to its summary; the key is absent when there is no warning."
why = "Keeps the summary's shape unchanged for every request with a well-formed config."
means = "fall back to an empty generator list and surface a warnings entry"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/generated.rs tomlctl/src/plan/holders.rs tomlctl/src/plan/request.rs tomlctl/src/plan/check/generated_outputs.rs tomlctl/src/plan/view.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- generated:: plan::holders:: plan::request::"
result = "pass"

[[checks]]
command = "tomlctl review anchor F66 --slug mid-run-requests --gone"
result = "pass"
```

## What changed

- `tomlctl/src/generated.rs`: removed the module-wide `#![allow(dead_code)]` and the never-read `Generator.input_globs` field. Added `canonical_path` (slashes normalised, empty and `.` segments dropped, a leading `/` kept, `..` kept), `path_key` (canonical, lowercased) and `missing_outputs(generator, files)`, which compares by `path_key`. New tests cover canonicalisation and a case-variant output counting as held.
- `tomlctl/src/plan/holders.rs`: `fold_key` delegates to `generated::path_key`, so `src/./a.rs`, `src//a.rs` and `docs/./flows/<slug>/x.md` compare equal to their plain forms in holder() and forbidden(). `closure` walks `needs` only, and the module header says why coupling is excluded. New tests: a coupled row still holds, dot and doubled-slash variants are held, and dot or doubled-slash variants of forbidden paths are refused.
- `tomlctl/src/plan/request.rs`: parse_payload stores `canonical_path` and refuses a path that canonicalises to nothing (`./`, `.`). `rule` keys its "ruled twice" check on the parsed (kind, index), and `item_ref` accepts only plain digits with no leading zero, so `p01` and `p+1` are refused. `add` no longer refuses on a malformed `.claude/generated.toml`; see the deviations. Request add's held/output sets already folded through fold_key and now share `path_key` with check and view.
- `tomlctl/src/plan/check/generated_outputs.rs` and `tomlctl/src/plan/view.rs`: the missing-output computation is now `generated::missing_outputs`, so plan check, the worker view and request add agree on which output is unheld.

The F48 change alters observable behaviour: a malformed generated map used to make `plan request add` fail and now gives a success with `warnings`. If the sibling's `tomlctl/tests/plan_request.rs` asserts the old refusal, the gate will flag it there.
