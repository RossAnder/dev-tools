# Direction

## Overarching

A worker's request is a store record, not a message. The worker posts it through `tomlctl plan request add`, which writes a `[[request]]` row in the flow's `plan.toml` and settles, in the same call, everything a command can settle: the path is forbidden, another row that could run beside the worker holds it, an earlier grant in the same run already gave it to such a row, or it is a declared generated output of an input the worker holds. Only what needs judgement waits for the orchestrator, who rules through `tomlctl plan request rule`. The `REQUEST <request-id> task <n>` message only wakes the orchestrator; it carries no content anyone acts on. A ruling a worker writes itself is no ruling: before any grant is applied, the drain audits the run's transcripts for `plan request rule` and `plan request applied`, and treats every request a worker ruled as denied.

The direction optimises for three things:

- **A worker finishes in one attempt** whenever the rules allow, without the user.
- **Every rule that a command can decide is decided by one.** The case-folded holder test, the forbidden paths and the generator lookup live in tomlctl, never in an orchestrator's reading of prose.
- **Nothing changes for the user's own questions.** `question` records, the question loop and the rule that only the user answers a question stay exactly as they are.

What it gives up: a `plan.toml` schema tier (version 5), several new tomlctl leaves with their tests, and one orchestrator turn per non-mechanical request.

Expectations every task stays consistent with:

- **No `tasks.toml` write while a milestone run is live.** Grants reach a row's `files` and `spec` only at the drain, through the audited `tasks update`. Until then the lane check and the pre-flight read granted paths from the request records. A `files` change that renames or adds a file carries its new path as a path item of the same request too, so the holder test, the lane check and the pre-flight see it; the orchestrator denies a `files` change whose new path is not granted as a path item.
- **One path comparison.** Paths are compared by the folded key: slashes normalised, a leading `./` dropped, letter case folded. The milestone Workflow's `fileKey` normalises slashes and folds case but keeps a leading `./`, so this key is the stricter of the two. `normal_path` itself does not change.
- **The holder rule.** A path is held when any other row of the requester's milestone holds it, or was granted it earlier in a still-unapplied request, and that row is `pending`, `in-progress`, `failed` or `parked`. A row in the requester's `needs` closure, up or down, is excluded, since the Workflow never runs it at the same time as the requester. A `done` row is excluded too; the drain orders it before the requester.
- **The orchestrator never decides what the work is for.** A request that would change a task's `outcome`, a success criterion or a decision is ruled `needs-user`. The worker then asks it through its own one question, or parks.
- **A denial is today's path.** A denied or unanswered scope request means `failed` with `needs_files`. A denied amend means the tier rules that already exist. The one change is D2's: a denied `/apply-v2` fix row that returns `failed` with `needs_files` is widened by drain.md's `needs_files` rules instead of moving up a tier.

## Milestone A

The work splits in two because the engine and worker prose is read live by the very run that implements it: a drain or a worker reading `plan request` steps mid-run would call verbs the installed 0.24 binary lacks. Milestone A builds and installs everything those steps need; milestone B then changes the steps. Milestone A holds three strands:

- **tomlctl.** The `generated.rs` config module, the `[[request]]` record and schema tier, the holder computation, the `plan request` leaves, the lane check reading grants, the gate list carrying generator checks, the worker view naming each held generator, and the two new `plan check` warnings. Each piece is its own module where it can be, so tasks stay file-disjoint. The gate ends by installing tomlctl and glimpse, since glimpse parses `plan.toml` through the tomlctl it links and would refuse a schema-5 store.
- **The drain.md split.** Its `needs_human` report section moves to its own reference, a move that changes no step, so milestone B has room under the line ceiling.
- **Planning.** The Prove step's handling of the two new warnings, and the Write step's rule that a row editing a declared generator input holds that generator's outputs.

A task that adds a verb or a record field writes its tests in the same task, or in the task its `needs` names. Prose names a verb only once the task adding it is in the row's `needs`, because the dispatch tests lint every fenced `tomlctl` command against the clap tree.

## Milestone B

The implement-v2 engine and the two workers start using `plan request`, against the binary milestone A installed: the worker's new **Asking the orchestrator** section, the orchestrator's `references/requests.md`, the drain's new "Granted requests" step, the apply-row rules, the question loop's note on requests, and the writer-rule exception in the task-store contract. Every command these documents name must match the verbs as milestone A built them; read `claude/skills/tomlctl/references/plan-request.md` for the flags rather than the task text.
