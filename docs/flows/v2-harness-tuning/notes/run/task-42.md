Status: done — checker reproduce prompt now reads rows with `--for checker` and writes lines only for `gone` findings

- claude/workflows/review-v2.js: in `checkerReproducePrompt`, `--for implementer` became `--for checker`, and "Write one JSON line per finding" became "per `gone` finding and none for an `unsettled` one". The return line is unchanged and still lists every finding. The verifier prompt is untouched.
- Edited with a scripted exact-string replace (asserted one match each, line endings preserved) rather than the Edit tool.

Checks:
- `grep -q -- '--for checker' claude/workflows/review-v2.js`: pass
- `node --test claude/workflows/tests/harness.test.mjs`: 17 pass
- `cargo test --lib -- cli::dispatch::tests`: 55 pass

TANGENTIAL: none
