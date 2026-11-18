Status: done — `plan request add|rule|list|applied` added in tomlctl/src/plan/request.rs, declared and dispatched

```toml run-note
files_touched = ["tomlctl/src/plan/request.rs", "tomlctl/src/cli/types/plan.rs", "tomlctl/src/plan/dispatch.rs", "tomlctl/src/plan/mod.rs"]

[[deviations]]
what = "add and rule read tasks.toml once before taking the plan.toml write lock; every ruling, the open-run check and the one-request-per-run check run inside the lock"
why = "Reading the task store under the plan lock would nest a tasks.toml lock inside the plan.toml lock. `plan run notes` reads tasks.toml before its plan mutate for the same reason. The race the constraint guards, two posts both granted one path, is between requests in plan.toml, and the lock covers it. A row status a worker changes between the read and the lock can at worst make the holder test count a row that has just finished."
means = "detail: inside the plan.toml write lock"

[[deviations]]
what = "add also refuses a payload that names one path twice by folded key, a blank path, why, what or evidence, and a change field outside the request vocabulary; rule also refuses an item named twice in one batch; applied refuses an empty id list"
why = "The detail does not cover these cases. Refusing them is easy to undo and nothing a user would see changes."
means = "detail: add, rule and applied refusal list"

[[deviations]]
what = "add stores each path normalised (backslashes to slashes, leading ./ dropped, case kept); list prints at and ruled_at as RFC 3339 strings, with ruled_at empty while unset"
why = "The drain copies granted paths into a row's files, and the task store holds paths in the same normal form. An empty string keeps `--where ruled_at=` usable."
means = "detail: list row fields"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "cargo clippy --all-targets (isolated copy of tomlctl in the scratchpad, own target dir, sibling's missing module removed)"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/request.rs tomlctl/src/plan/dispatch.rs tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "cargo test --lib -- plan::request (isolated copy)"
result = "pass"

[[checks]]
command = "cargo test --lib -- cli::dispatch::tests (isolated copy)"
result = "fail"

[[checks]]
command = "grep -q 'PlanRequestOp' tomlctl/src/cli/types/plan.rs && grep -q 'PlanOp::Request' tomlctl/src/plan/dispatch.rs && test -f tomlctl/src/plan/request.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/request.rs` (new) holds the four leaves:
  - `add --task N --json SRC` refuses a task that no open `implement:<m>` run lists, and a task that already has a request in that run. Inside the plan.toml lock it mints `Q<n>` with `mint_request_id` and rules each path in this order: forbidden (`holders::forbidden`), already in the task's files ("already held by this task"), held (`holders::holder`, naming the task and status, plus the request id for a grant), a declared output of an input the task holds (`generated::load` at the repo root, then `outputs_of` over the row's files, compared by `fold_key`) is granted, and anything else stays open. Change items stay open. When nothing is left open the request is ruled and `ruled_at` is stamped. It prints `{ok,id,status,items:[{item,ruling,by,reason}]}`, numbering items p1.. and c1...
  - `rule ID --ndjson SRC` validates every row before it writes anything. It refuses an unknown key, a ruling other than granted, denied or needs-user, a blank reason, an unknown or repeated item, an item already ruled, a request that is not open, and a grant of a path `holders::holder` finds held at the moment of the write. It writes `by = "orchestrator"` and prints the same shape as `add`.
  - `list` prints every request as a row report through `print_report(.., Rows::Top)`, oldest first, with `paths` and `changes` as nested arrays. The global `--where`/`--get` options apply to it.
  - `applied ID[,ID...]` sets `applied = true`. It refuses an unknown or still-open request and writes nothing if any id is refused.
  - There are five unit tests on payload and rule-row parsing.
- `tomlctl/src/cli/types/plan.rs`: adds `PlanOp::Request { op: PlanRequestOp }` and four arg structs. `add`, `rule` and `applied` take `WriteIntegrityArgs`; `list` takes `ReadIntegrityArgs`. The doc comments on the variants are the help text.
- `tomlctl/src/plan/dispatch.rs`: one `PlanOp::Request` arm.
- `tomlctl/src/plan/mod.rs`: `mod request;`.

## Checks

Clippy on the shared tree fails only with `E0583: file not found for module generated_outputs` at `tomlctl/src/plan/check.rs`. That comes from a sibling's in-flight edit to a file this task does not hold, and a second try gave the same result. To still type-check this task's code, I copied `tomlctl/` into the session scratchpad and dropped that one `mod` line and its call there. In that copy clippy `--all-targets` is clean, and the five unit tests pass.

I also ran the copy's binary against a throwaway git repo holding a copy of this flow's stores and a test `.claude/generated.toml`. It showed:

- A declared output was granted by tomlctl, matching by folded key (`Docs/Generated/plan.md`).
- `.claude/x.toml` was denied as forbidden.
- The task's own file was denied as "already held by this task".
- An unrelated pending row's file was denied as "held by task 20 (pending)", after setting that row pending in the copy.
- Ordinary paths and change items stayed open.
- A second request in the same run was refused.
- An unknown key was refused, naming the key.
- `rule` refused an unknown item and an item already ruled, then set the status to ruled once every item had a ruling.
- `applied` refused an open request and an unknown id.
- `list --where id=Q1 --where status=ruled --get id` printed `Q1`.
- The store was stamped `schema_version = 5`.

In the copy, `cli::dispatch::tests` passed 55 of 56. The one failure, `catalogue_validates`, reads the review-lens catalogue from the repo, which the scratch copy is outside. Whether the doc-lint dispatch tests want a reference entry for the new leaves is for the gate to see once task 13's docs land.

Not run: the `plan_request::` nextest accept check. It is marked for the gate, and its test file belongs to task 5.
