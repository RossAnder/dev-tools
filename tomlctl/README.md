# tomlctl

Small TOML read/write CLI for Claude Code flow and ledger files.

Built because `python3 -c "import tomllib"` is unreliable on Windows Git Bash, and the canonical flow/ledger schemas (`.claude/flows/*/context.toml`, `review-ledger.toml`, `optimise-findings.toml`) require parse-rewrite operations rather than line-level edits.

## Install

```bash
cargo install --path .
```

Requires Rust 1.85+.

## Usage

See [`claude/skills/tomlctl/SKILL.md`](../claude/skills/tomlctl/SKILL.md) for the full reference.

Quick tour:

```bash
tomlctl get         <file> [path]                     # JSON of value (or whole file)
tomlctl set         <file> <path> <value> [--type T]  # scalar
tomlctl set-json    <file> <path> --json <json>       # array / object / scalar
tomlctl validate    <file>                            # parse-check
tomlctl items list  <file> [--status X] [--category Y] [--newer-than YYYY-MM-DD] [--file PATH] [--count]
tomlctl items get   <file> <id>
tomlctl items add   <file> --json '{"id":"R7",...}'
tomlctl items update <file> <id> --json '{"status":"fixed"}' [--unset key]...
tomlctl items remove <file> <id>
tomlctl items next-id <file> [--prefix R|O]
tomlctl items apply  <file> --ops '[{"op":"add|update|remove", ...}, ...]' [--array NAME]
tomlctl items find-duplicates <file> [--tier A|B|C]    # dedup hygiene, read-only JSON array
tomlctl items orphans  <file>                          # missing-file / symbol-missing / dangling-dep
tomlctl blocks verify  <file>... [--block <marker-name>]...  # cross-file shared-block parity

# Global flags (accepted before or after the subcommand):
#   --allow-outside           bypass the best-effort .claude/ containment guard (not a sandbox)
#   --no-write-integrity      suppress the <file>.sha256 sidecar on write
#   --verify-integrity        verify <file> against <file>.sha256 before any read
```

**Stdin input** (`-` sentinel on `--json` / `--ops`): see [SKILL.md stdin section](../claude/skills/tomlctl/SKILL.md#stdin-input-for-large-json-payloads) for the full reference.

All commands print JSON on stdout, exit non-zero on failure.

## Design

- Uses [`toml 1.1.2+spec-1.1.0`](https://crates.io/crates/toml) with `preserve_order` for stable key layout.
- Whole-file parse → mutate → re-serialise. No format preservation (flow/ledger schemas forbid inline comments).
- Dates round-trip as TOML date literals; JSON strings matching `YYYY-MM-DD` are promoted to dates on write.
- **Integrity sidecar.** Every write emits `<file>.sha256` alongside the target, in standard `sha256sum` format (`<64-hex>  <basename>\n`), written atomically after the primary rename so an interleaved reader cannot see a torn pair. Pass `--no-write-integrity` to opt out. Pass `--verify-integrity` on any invocation to verify the target against its sidecar before every read — a missing sidecar or digest mismatch aborts with expected/actual hashes named in the error. `tomlctl` never auto-repairs; a mismatch means either an out-of-band edit or a corrupted sidecar, and a human should decide which. **Threat model.** The sidecar is a consistency check against accidental corruption and collaborative out-of-band edits — it is **not** a MAC or tamper-proof signature. An attacker with ledger write access can trivially rewrite the sidecar; hostile-actor threat models still require auditing the ledger's git history.
