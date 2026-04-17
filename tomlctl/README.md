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
tomlctl items list  <file> [--status X]
tomlctl items get   <file> <id>
tomlctl items add   <file> --json '{"id":"R7",...}'
tomlctl items update <file> <id> --json '{"status":"fixed"}'
tomlctl items remove <file> <id>
tomlctl items next-id <file> [--prefix R|O]
tomlctl items apply  <file> --ops '[{"op":"add|update|remove", ...}, ...]'
```

All commands print JSON on stdout, exit non-zero on failure.

## Design

- Uses [`toml 1.1.2+spec-1.1.0`](https://crates.io/crates/toml) with `preserve_order` for stable key layout.
- Whole-file parse → mutate → re-serialise. No format preservation (flow/ledger schemas forbid inline comments).
- Dates round-trip as TOML date literals; JSON strings matching `YYYY-MM-DD` are promoted to dates on write.
