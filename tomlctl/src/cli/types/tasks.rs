//! The `tasks` verb group over a flow's task DAG store.

use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

use super::{QueryArgs, ReadIntegrityArgs, WriteIntegrityArgs};

/// Store target shared by every `tasks` verb. Not a `required` group:
/// `import-plan --plan <path> --dry-run` validates a plan with no store at
/// all, and every other verb refuses an empty target in post-parse
/// validation so the refusal is a `kind=validation` envelope, not usage prose.
#[derive(Args, Clone)]
#[command(next_help_heading = "Store target")]
#[group(multiple = false)]
pub(crate) struct TasksTarget {
    /// Flow slug whose task store is targeted
    /// (`<root>/.claude/flows/<slug>/tasks.toml`).
    #[arg(long = "slug", value_name = "SLUG")]
    pub(crate) slug: Option<String>,

    /// Explicit `tasks.toml` path, bypassing slug resolution.
    #[arg(long = "file", value_name = "PATH")]
    pub(crate) file: Option<PathBuf>,
}

/// Projection selector for `tasks show --with`. Comma-delimited and
/// repeatable; with the flag absent the output is `summary` alone. An unknown
/// part is a clap parse failure — exit 2, usage prose, no
/// `--error-format json` envelope — where `--effort` exits 1 `kind=validation`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ShowPart {
    /// id, ref, title, effort, status, checkpoint, files, needs, coupling.
    Summary,
    /// The `action` / `detail` / `acceptance` prose an executing agent needs.
    Body,
    /// The row's `files` list, then `file_notes`, `new_files` and `deleted_files`.
    Files,
    /// A summary per direct `needs` / `coupling` target of the row.
    Deps,
    /// Transitive dependents (successors) of the row.
    Dependents,
}

/// Edge selector for `tasks edges --kind`. `needs` and `coupling` are the
/// two stored edge sets; `overlap` is computed on read (a file shared by
/// two rows with no directed path either way) and never persisted. An unknown
/// kind fails as `--with` does: clap parse failure, exit 2, no JSON envelope.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum EdgeKind {
    /// Stored dependency edges.
    Needs,
    /// Stored acceptance-reachability edges.
    Coupling,
    /// Computed file-sharing pairs with no directed path either way.
    Overlap,
}

/// Commit granularity for `tasks train --granularity`, overriding the store's
/// `[policy] commit_granularity`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum TrainGranularity {
    /// One commit per row.
    PerTask,
    /// One commit per checkpoint group.
    PerCheckpoint,
    /// One commit for the whole window.
    SingleCommit,
}

impl TrainGranularity {
    /// The store's spelling of the same value.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PerTask => "per-task",
            Self::PerCheckpoint => "per-checkpoint",
            Self::SingleCommit => "single-commit",
        }
    }
}

/// `tasks` subcommand cluster. Every op resolves its store through the
/// shared `TasksTarget` group and emits JSON. The comma-delimited list
/// flags carry no `num_args(1..)` — a greedy multi-value arg swallows
/// whitespace-separated tokens, including the next subcommand name.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum TasksOp {
    /// Import a plan's `## Tasks`, `## Execution Policy` and
    /// `## Dependency Graph` sections into the store. An upsert keyed on
    /// each row's `ref`: existing rows keep `status`, `agent` and `commit`,
    /// new rows arrive `pending`, and nothing is deleted.
    ImportPlan {
        #[command(flatten)]
        target: TasksTarget,
        /// Plan markdown to import. Under `--slug`, defaults to the flow's
        /// recorded plan path; may be passed alone (no store target) to
        /// validate a plan with `--dry-run` before a flow exists.
        #[arg(long = "plan", value_name = "PATH")]
        plan: Option<PathBuf>,
        /// Also read the flow's `execution-record.toml` task-completions,
        /// mark matched rows `done`, and adopt the record's `ref` on a
        /// unique normalised match.
        #[arg(long = "reconcile-record")]
        reconcile_record: bool,
        /// Preview the import without writing. Emits the same envelope plus
        /// any findings and leaves the file + sidecar byte-identical.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Append one task row. Its `ref` is derived from the title, and
    /// dangling dependency targets or a cycle are refused before writing.
    Add {
        #[command(flatten)]
        target: TasksTarget,
        /// Row title; the `ref` slug is derived from it.
        #[arg(long, value_name = "TEXT")]
        title: String,
        /// Effort tag: `S`, `M` or `L`. Validated against the store schema
        /// after parsing, so an unrecognised value surfaces as a
        /// `kind=validation` error rather than clap usage prose.
        #[arg(long, value_name = "S|M|L")]
        effort: String,
        /// Repo-relative paths this task edits.
        #[arg(long, value_delimiter = ',', value_name = "F1,F2,...")]
        files: Vec<String>,
        /// Task ids this row depends on.
        #[arg(long, value_delimiter = ',', value_name = "N1,N2,...")]
        needs: Vec<u32>,
        /// Acceptance-reachability edges — ids whose work this row's
        /// acceptance command exercises. Counted in the in-degree exactly
        /// like `--needs`.
        #[arg(long, value_delimiter = ',', value_name = "N1,N2,...")]
        coupling: Vec<u32>,
        /// Prose remainder of the plan's `Depends on` line, stored verbatim.
        #[arg(long = "deps-note", value_name = "TEXT")]
        deps_note: Option<String>,
        /// Checkpoint group id this row belongs to. Omit for a row committed
        /// only by the final train.
        #[arg(long, value_name = "ID")]
        checkpoint: Option<String>,
        #[arg(
            long,
            conflicts_with = "action_file",
            value_name = "TEXT",
            help = "Action body as a literal string"
        )]
        action: Option<String>,
        #[arg(
            long = "action-file",
            conflicts_with = "action",
            value_name = "PATH",
            help = "Read the action body from a file"
        )]
        action_file: Option<PathBuf>,
        #[arg(
            long,
            conflicts_with = "detail_file",
            value_name = "TEXT",
            help = "Detail body as a literal string"
        )]
        detail: Option<String>,
        #[arg(
            long = "detail-file",
            conflicts_with = "detail",
            value_name = "PATH",
            help = "Read the detail body from a file"
        )]
        detail_file: Option<PathBuf>,
        #[arg(
            long,
            conflicts_with = "acceptance_file",
            value_name = "TEXT",
            help = "Acceptance body as a literal string"
        )]
        acceptance: Option<String>,
        #[arg(
            long = "acceptance-file",
            conflicts_with = "acceptance",
            value_name = "PATH",
            help = "Read the acceptance body from a file"
        )]
        acceptance_file: Option<PathBuf>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Append many rows in one batch from NDJSON, one row object per line.
    /// Validation matches `add` and the batch is all-or-nothing: a
    /// malformed line, a dangling target or a cycle aborts before the
    /// file is mutated.
    AddMany {
        #[command(flatten)]
        target: TasksTarget,
        #[arg(
            long = "ndjson",
            value_name = "SRC",
            help = "NDJSON source: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Patch the mutable fields of one or more rows in one write. Every id is
    /// resolved before anything changes, so an unknown id aborts the whole
    /// call. `ref` is immutable unless `--ref` is given explicitly — renaming
    /// it orphans the execution record's `task_ref` and the last import's ref
    /// set — and `--ref` takes a single id.
    Update {
        /// Task ids to patch, comma- or space-separated.
        #[arg(
            required = true,
            num_args = 1..,
            value_delimiter = ',',
            value_name = "ID,..."
        )]
        ids: Vec<u32>,
        #[command(flatten)]
        target: TasksTarget,
        /// Lifecycle status: `pending`, `in-progress`, `done`, `failed` or
        /// `deferred`. Validated against the store schema after parsing.
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Agent the row was dispatched to, recorded at dispatch time.
        #[arg(long, value_name = "NAME")]
        agent: Option<String>,
        /// Commit SHA the row landed in, recorded by the commit train.
        #[arg(long, value_name = "SHA")]
        commit: Option<String>,
        /// Checkpoint group id. Pass an empty value to clear it.
        #[arg(long, value_name = "ID")]
        checkpoint: Option<String>,
        /// Rewrite the row's `ref`. Never inferred from a retitle.
        #[arg(long = "ref", value_name = "SLUG")]
        task_ref: Option<String>,
        /// Permit `--set files=` and `--set needs=`, and stamp the row with
        /// the plan values the patch replaces. The next import keeps the
        /// hand-patched values while the plan still states that base.
        #[arg(long = "unlock-import-fields", conflicts_with = "relock")]
        unlock: bool,
        /// Drop that stamp, handing `files` and `needs` back to the plan: the
        /// next import restores whatever it states.
        #[arg(long = "relock-import-fields")]
        relock: bool,
        #[arg(
            long = "set",
            value_name = "KEY=VAL",
            help = "Set any other row field (repeatable)"
        )]
        set: Vec<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Delete one row. `import-plan` keeps a row the plan stopped producing,
    /// so a plan-deleted task is retired here or not at all. A row past
    /// `pending`, or one other rows depend on, is refused unless `--force`.
    Remove {
        /// Task id to remove.
        id: u32,
        #[command(flatten)]
        target: TasksTarget,
        /// Remove a settled row, or one other rows depend on. Each dependent's
        /// edges are re-pointed at the removed row's own dependencies, so the
        /// ordering it stood for survives it.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Print one row's object, or an array of rows in the order given; an
    /// unknown id fails the whole call. Without `--with` each is the summary
    /// shape; `--with body,files,deps` is the fetch-by-id form a dispatching
    /// orchestrator hands an implementing agent in place of pasted prose.
    Show {
        /// Task ids to print, comma- or space-separated.
        #[arg(
            required = true,
            num_args = 1..,
            value_delimiter = ',',
            value_name = "ID,..."
        )]
        ids: Vec<u32>,
        #[command(flatten)]
        target: TasksTarget,
        #[arg(
            long = "with",
            value_enum,
            value_delimiter = ',',
            value_name = "PART,...",
            help = "Sections to include (summary,body,files,deps,dependents)"
        )]
        with: Vec<ShowPart>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// List rows as a JSON array. The full `items list` predicate,
    /// projection and aggregation surface applies.
    ///
    /// `--count` joins `--count-by`, `--group-by`, `--pluck` and
    /// `--count-distinct` in a mutually-exclusive `ArgGroup`, so a
    /// mismatched pair is a parse-time error rather than a silent collapse
    /// to one shape. `--count` is declared here because it lives on the
    /// variant in `items list` too, not in `QueryArgs`.
    #[command(group(clap::ArgGroup::new("shape").multiple(false).args(["count", "count_by", "group_by", "pluck", "count_distinct"])))]
    List {
        #[command(flatten)]
        target: TasksTarget,
        #[arg(
            long,
            help = "Print `{\"count\": N}` of matching rows instead of the array"
        )]
        count: bool,
        #[command(flatten)]
        query: QueryArgs,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print the graph's edge list.
    Edges {
        #[command(flatten)]
        target: TasksTarget,
        /// Restrict to one edge kind. Omit for all three.
        #[arg(long = "kind", value_enum, value_name = "KIND")]
        kind: Option<EdgeKind>,
        /// Emit Graphviz DOT source instead of a JSON edge list.
        #[arg(long = "dot")]
        dot: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print the dispatchable frontier: rows whose dependencies are all
    /// `done`, which of those are held by a file claim, what becomes ready
    /// once the current round lands, and which rows are blocked behind a
    /// dependency no later wave can clear — each naming the predecessor
    /// stranding it and that predecessor's status.
    Ready {
        #[command(flatten)]
        target: TasksTarget,
        /// Ids currently dispatched. A ready row sharing a file with one of
        /// them is reported as held, naming the file and the holder.
        #[arg(long = "in-flight", value_delimiter = ',', value_name = "N1,N2,...")]
        in_flight: Vec<u32>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print the Kahn layers of the whole graph, each sorted ascending.
    Batches {
        #[command(flatten)]
        target: TasksTarget,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print the commit groups for the `done` rows with no `commit`, in
    /// commit order. Groups follow the commit granularity, then merge on any
    /// shared file and on any dependency running both ways between them.
    /// `shared_with_pending` names a group file a row not yet `done` claims.
    Train {
        #[command(flatten)]
        target: TasksTarget,
        /// Narrow the window to these checkpoint groups' members.
        #[arg(
            long = "checkpoint",
            value_delimiter = ',',
            conflicts_with = "ids",
            value_name = "ID,..."
        )]
        checkpoint: Vec<String>,
        /// Narrow the window to these task ids.
        #[arg(long = "ids", value_delimiter = ',', value_name = "N1,N2,...")]
        ids: Vec<u32>,
        /// Override the store's `[policy] commit_granularity`.
        #[arg(long = "granularity", value_enum, value_name = "GRANULARITY")]
        granularity: Option<TrainGranularity>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print a checkpoint group's task set, or one task's transitive
    /// dependencies (`--up`) or dependents (`--down`), plus the set's
    /// maximal elements.
    Closure {
        #[command(flatten)]
        target: TasksTarget,
        /// Checkpoint group id whose closure is printed.
        #[arg(long = "checkpoint", conflicts_with = "task", value_name = "ID")]
        checkpoint: Option<String>,
        /// Task id whose closure is printed.
        #[arg(long = "task", conflicts_with = "checkpoint", value_name = "N")]
        task: Option<u32>,
        /// Walk to transitive dependencies (ancestors), inclusive.
        #[arg(long = "up", conflicts_with = "down")]
        up: bool,
        /// Walk to transitive dependents (successors), inclusive.
        #[arg(long = "down", conflicts_with = "up")]
        down: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Run the store's invariant checks. Each finding carries a `class`, a
    /// `severity` and the ids it names; any `error`-class finding exits 1.
    Check {
        #[command(flatten)]
        target: TasksTarget,
        /// Also compare the plan markdown against the render output and
        /// report a `render/drift` warning when they differ.
        #[arg(long = "plan")]
        plan: bool,
        /// Ids currently dispatched. A row waiting on one of them is not
        /// reported as a stalled dependency.
        #[arg(long = "in-flight", value_delimiter = ',', value_name = "N1,N2,...")]
        in_flight: Vec<u32>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Rewrite the plan's `## Execution Policy`, `## Tasks` and
    /// `## Dependency Graph` sections from the store.
    ///
    /// A derived write, like `flow render-progress-log`: the output carries
    /// no integrity sidecar, and the target is resolved from the flow
    /// context or the store's recorded plan path — never from a free
    /// argument — so the write-side containment flags have no hook here.
    Render {
        #[command(flatten)]
        target: TasksTarget,
        /// Print the rendered plan to stdout instead of writing it.
        #[arg(long = "stdout")]
        stdout: bool,
        /// Report drift and exit 1 without writing.
        #[arg(long = "check")]
        check: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print one consistent read of the flow for a viewer: task rows, graph
    /// products, the execution record joined to the rows, and the agent
    /// records. Sibling files are optional and read as empty when absent;
    /// `revision` fingerprints the raw input bytes.
    Snapshot {
        #[command(flatten)]
        target: TasksTarget,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
}
