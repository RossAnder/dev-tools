# Worked examples: verdicts

Illustrative verdicts, one per outcome plus a severity reassessment, a struck identity and the checker's rows. Paths are invented. In an NDJSON batch each verdict is one line of `payloads/<batch>.ndjson`; a single verdict takes the same values as flags, with the rationale staged as a file and passed with `--rationale-file`.

Every verdict carries a `citation` — a `file:line`, or the command and its decisive output — and a `rationale` of at most 600 characters.

## confirmed — verifier, reproduced

The verifier started from the `counter`, found no caller that pads, and reproduced the trigger with the narrowest test.

```json
{"finding": "F7", "verdict": "confirmed", "by": "verifier", "citation": "cargo test --test merge apply_64_rows: assertion failed, left 32 right 64", "rationale": "Counter checked first: the three callers (sync.rs:40, import.rs:88, cli.rs:212) pass batches unpadded. Built a 64-row batch with CHUNK = 32 in a scratch test; 32 rows landed and the call returned Ok, as the failure_scenario says."}
```

## plausible — verifier, decisive check out of reach

The claim holds as far as reading takes it, but only a whole-suite run or a production-size input would decide it. Say what would decide it.

```json
{"finding": "F9", "verdict": "plausible", "by": "verifier", "citation": "src/index/lookup.rs:58", "rationale": "find_all walks the Vec per call (lookup.rs:58) and sweep calls it per row (sweep.rs:120), so the cost is quadratic as claimed. The 9.2 s figure needs the 20k bench, which is a full build; not run. A bench run of sweep_20k would decide the magnitude."}
```

## refuted — verifier, documented intent

Behaviour pinned as deliberate is not a defect. The rationale says where the intent is written and that the point, if it still matters, is an advisory challenge.

```json
{"finding": "F11", "verdict": "refuted", "by": "verifier", "citation": "tests/report.rs:204", "rationale": "The test report_skips_empty_sections asserts empty sections are omitted, and the comment at render.rs:66 says why (the v1 reader rejects empty tables). Not a defect; if the omission still matters it is an advisory challenge to that comment."}
```

## confirmed with a reassessed severity

Pass `--severity` (or `severity` in the batch row) only when it changes. Here the finder said `minor`; the trigger turns out to be the default configuration.

```json
{"finding": "F14", "verdict": "confirmed", "by": "verifier", "severity": "major", "citation": "src/config.rs:31", "rationale": "Reproduced. The finder graded the trigger narrow, but config.rs:31 makes retries = 0 the default, so every fresh install hits the unguarded division at retry.rs:52. Major: a real defect with a common trigger."}
```

## struck identity

The finding's `same_as` names a prior row describing a different fault. Striking is a single-verdict flag, not a batch field:

```bash
tomlctl review verdict <finding> --verdict confirmed --by verifier --citation <path:line> --rationale-file <run-dir>/payloads/<finding>.rationale.md --strike-same-as --slug <slug>
```

Rationale: "same_as names F7, the chunk-bound off-by-one at merge.rs:142. This finding is the remainder pass missing after the bound changed, at merge.rs:150, which F7's fix did not touch. Different fault; struck."

## panel seat

At `deep` a critical defect gets three seats. Each seat verifies independently and records its own `seat`.

```json
{"finding": "F2", "verdict": "refuted", "by": "panel", "seat": 2, "citation": "src/auth/token.rs:97", "rationale": "The claimed bypass needs an unsigned token to reach verify(); token.rs:97 rejects alg = none before verify() is called, and the test token_rejects_none pins it."}
```

## test removal — verifier

For a cleanup row that removes a test, name the regression the test alone guards, or cite what else guards it.

```json
{"finding": "F20", "verdict": "confirmed", "by": "verifier", "citation": "tests/parse.rs:310", "rationale": "The test asserts the same CRLF case as parse_crlf_header at tests/parse.rs:310, through the same parse_header path. Removing it leaves that path guarded; no other behaviour is pinned only by it."}
```

## checker — confirmed

Every applicable predicate held, each with a citation from the checker's own Grep or Read. The rationale names the predicates and the search strings.

```json
{"finding": "F31", "verdict": "confirmed", "by": "checker", "citation": "src/cli/args.rs:77", "rationale": "Target exists: src/cli/args.rs. Old text present at args.rs:77. Edit names only args.rs. Search `--ledger` across the repo: no other help text names it."}
```

## checker — unsettled

Grep could not decide it, so the checker records `unsettled` with the line it checked last; tomlctl re-routes the row to the verifier.

```json
{"finding": "F33", "verdict": "unsettled", "by": "checker", "citation": "src/report/table.rs:3", "rationale": "Old text present at table.rs:3. Claimed unused: search `fmt::Write` finds only the import, but table.rs uses `write!(` at line 41, which needs the trait in scope. Whether the import is dead turns on that, which grep cannot settle."}
```

## checker — refuted, sweeper row only

Allowed only on a row a sweeper cell wrote, when a predicate is false and a grep shows it.

```json
{"finding": "F35", "verdict": "refuted", "by": "checker", "citation": "src/report/footer.rs:12", "rationale": "Claimed dead: `render_total`. Search `render_total` finds a call at footer.rs:12 outside the row's listed sites, so the symbol is referenced."}
```
