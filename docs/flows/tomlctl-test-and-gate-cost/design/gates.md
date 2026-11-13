# Gate stamp and verifier

## Step table

`scripts/gate-stamp.sh` holds the one list of cargo steps. Each has an id, a command and the roots it reads. A root is a directory or a single file, repo-relative.

| Id | Command | Roots |
|---|---|---|
| `fmt-tomlctl` | `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` | `tomlctl` |
| `dispatch-tests` | `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` | `tomlctl`, `claude`, `.claude/review-lenses`, `scripts` |
| `plan-corpus` | `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored` | `tomlctl`, `docs/plans`, `.claude/backlog.toml` |
| `fmt-glimpse` | `cargo fmt --manifest-path glimpse/Cargo.toml -- --check` | `glimpse` |
| `glimpse-clippy` | `cargo clippy --manifest-path glimpse/Cargo.toml --locked` | `glimpse`, `tomlctl` |

The workspace milestone renames the last step to `workspace-clippy` with the command `cargo clippy --workspace --locked`, and adds the root `Cargo.toml` and `Cargo.lock` to the roots of every step but the two format checks.

## Stamps

- `gate-stamp.sh run` runs every step in table order. It stops at the first failure, as today, and removes that step's stamp. A step that passes gets its stamp only when its digest is the same before and after the step ran.
- `gate-stamp.sh run <id>...` runs only the named steps.
- `gate-stamp.sh check <id>` exits 0 only when that step's stamp holds the digest of its roots as they are now. `check` with no id exits 0 only when every step's stamp matches.
- An unknown id is a usage error, exit 2.
- A stamp is one file per step under `gate-stamp.d/` in the git directory, named by the step id. The old single `gate-stamp` file is ignored and removed by `run`.
- The digest of a step covers the path and raw bytes of every tracked or untracked, unignored file under its roots, as `digest()` computes it today; a tracked file missing from the tree counts by path. The digest also folds in the step's id and its command line, so editing a step's command voids its stamp.

## Hook

`gate` in `.githooks/pre-commit` takes the step id first, then nothing else: it reads the command from `gate-stamp.sh`, through `gate-stamp.sh command <id>`, so the command is written once. For each call it runs `gate-stamp.sh check <id>`; on a match it prints `pre-commit: <id> skipped, its gate stamp matches this working tree` and returns, and otherwise it runs the command and blocks the commit on a failure, naming the command to rerun. The staged-path triggers that decide which gates are called stay as they are. The hook never writes a stamp.

## One-pass verifier

`scripts/verify-shared-blocks.sh` keeps, unchanged: the GNU awk resolver block it shares with `scripts/doc-diff-gate.sh`, the `git rev-parse` call, the hasher presence check, every message text, the order errors are reported in, and exit codes 0, 1 and 2.

The loop over (block, file) pairs becomes one gawk program that reads the manifest and each carrier with `getline < file`:

- presence of a marker is a whole-line match after stripping one trailing CR;
- extraction starts and ends on an exact match of the marker line with no CR stripped, and each content line has one trailing CR removed and a newline appended;
- two carriers agree when their extracted text is equal as strings;
- a hash is computed only to print a `drift:` message, through a gawk coprocess to the hasher;
- a carrier that is not a regular file is reported as missing, checked with the bash `-f` test before gawk runs.
