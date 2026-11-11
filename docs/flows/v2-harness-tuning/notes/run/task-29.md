Status: done — the plan review wrapper passes `batch_defects: true` to the pipeline and re-verifies a wave's reverify ids in one batch, with `same_as` rows kept single

## What changed

- `claude/workflows/plan-v2-review.js`
  - The `review-v2-pipeline` launch args gain `batch_defects: true`.
  - The settle schema gains an optional `reverify_single` id list. `required` is unchanged, so a settle return without it still validates.
  - `reverifyPrompt` takes a list of ids and an optional batch name. With a batch it adds a `batch: reverify-<wave>` line, which the verifier uses as its NDJSON payload name, and asks for one verdict-return object per finding under `verdicts`.
  - New `reverifyBatch(ids, wave)` dispatches one `review-verifier`. Its schema is `{verdicts: [verdict_return]}`, the same shape as the pipeline's `batchSchema`.
  - Reverify step: ids in `reverify_single` (intersected with `reverify`) each get a single verifier, as before. Every other id goes into one batch, so a return without `reverify_single` batches every id. If the batch agent fails, all its ids go to `unverified`; if it returns but omits an id, that id goes to `unverified`. A `reverify_single` id outside `reverify` is reported in `errors` and not dispatched.
  - `SCRIPT_VERSION` goes from `'2'` to `'3'`. The index keys are unchanged, and so are the compose and audit phases.
- `claude/agents/plan-review-lead.md`, settle phase
  - Step 4 now says the Workflow re-verifies `reverify` ids in one batch. It tells the lead to read each id's `same_as` from the same `--for audit` read it already makes for rule-out hits, and to list the ids that carry one under `reverify_single`.
  - The return example and its key description now include `reverify_single`.

## Deviations

None. Choices made inside the spec:
- The batch is used even when only one non-single id remains. The detail says "every other reverify id in one dispatch", and a one-id batch is valid for the verifier.
- The batch payload is named `reverify-<wave>`. That is unique within a run because each wave settles once.

## Handover

None.

## Checks

- Accept: `grep -q 'batch_defects: true' claude/workflows/plan-v2-review.js` passes.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed.
- Parse check: compiled the script body as an async function under node, with no errors.
- Smoke run with stubbed `agent`/`workflow`/`parallel` in a scratch dir:
  - The pipeline received `batch_defects: true`.
  - With `reverify: [F1, F2, F3]` and `reverify_single: [F2]`, the script dispatched `reverify F2` and `reverify x2`.
  - The batch returned only F1, so F3 was reported unverified.
  - The index keys were intact.
- There is no Rust edit, so no clippy run.

TANGENTIAL: none
