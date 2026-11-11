//! The `plan` verb group over a flow's `plan.toml`, `docs/flows/<slug>/brief.md`
//! and task store.
//!
//! Companion rules that span flags (`--defer` needs `--reason`, `--for worker`
//! needs `--task`, a skip needs `--reason`) and the vocabularies of `--for`,
//! `--review` and `--outcome` are left to the verbs, so a breach is a
//! `kind=validation` refusal through `--error-format` rather than clap usage
//! prose.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::{ReadIntegrityArgs, WriteIntegrityArgs};

/// `plan` subcommand cluster.
#[derive(Subcommand)]
pub(crate) enum PlanOp {
    /// Write the `docs/flows/<slug>/brief.md` skeleton, bootstrap the flow and
    /// hold its binding in `plan.toml`, leaving the draft out of the active-flow
    /// registry.
    Open(PlanOpenArgs),

    /// Add or update decisions, minting ids under the lock.
    Decide(PlanDecideArgs),

    /// Add or update cross-task contracts, minting ids under the lock.
    Contract(PlanContractArgs),

    /// Add or update handovers, or mark one done.
    Handover {
        #[command(subcommand)]
        op: PlanHandoverOp,
    },

    /// Record a step's run starting, finishing or being skipped.
    Run {
        #[command(subcommand)]
        op: PlanRunOp,
    },

    /// Add or update a milestone: its `[[checkpoints]]` and `[[milestones]]`
    /// rows in the task store, written in one lock.
    Milestone {
        #[command(subcommand)]
        op: PlanMilestoneOp,
    },

    /// Run the task store's checks plus the plan classes. Exits 1 on any error.
    Check(PlanCheckArgs),

    /// Print the plan through a projection; nothing is written to disk.
    View(PlanViewArgs),

    /// Print the acceptance probe script, or record its captured results.
    Probes(PlanProbesArgs),

    /// Compute the plan's stage, next verb and blockers from its artefacts.
    Stage(PlanStageArgs),

    /// The approval screen's data, one object with an array per item.
    Gate(PlanGateArgs),

    /// Check the preconditions, then stamp the approval and activate the flow.
    Approve(PlanApproveArgs),

    /// Files changed since approval, minus this plan's commits, per affected
    /// task. Warns and exits 0.
    Drift(PlanDriftArgs),

    /// Print what the execution engine passes on, built from the stores; each
    /// leaf prints one JSON object and writes nothing.
    Emit {
        #[command(subcommand)]
        op: PlanEmitOp,
    },
}

/// `plan emit` leaves.
#[derive(Subcommand)]
pub(crate) enum PlanEmitOp {
    /// The milestone Workflow's launch args, with the rows left out and why and
    /// the `after:` checks to read before launch.
    LaunchArgs(PlanEmitLaunchArgs),
    /// The milestone gate's command list: its gates, then the accept checks a
    /// gate runs, with each check left out and why.
    GateList(PlanEmitGateListArgs),
    /// The reproduce check's launch args, one per review store the
    /// milestone's finding links name.
    ReproduceUnits(PlanEmitReproduceUnitsArgs),
    /// Whether the latest run of a milestone stayed in its lane: uncommitted
    /// paths no started row holds, and task-store writes beyond a worker's
    /// own `status` and `agent`.
    LaneCheck(PlanEmitLaneCheckArgs),
}

/// `plan handover` leaves.
#[derive(Subcommand)]
pub(crate) enum PlanHandoverOp {
    /// Add or update steps only a person can perform once a task lands,
    /// minting ids under the lock.
    Set(PlanHandoverSetArgs),
    /// Mark a handover done.
    Done(PlanHandoverDoneArgs),
}

/// `plan run` leaves.
#[derive(Subcommand)]
pub(crate) enum PlanRunOp {
    /// Record a step's run at launch.
    Start(PlanRunStartArgs),
    /// Close the newest unfinished run of a step.
    Finish(PlanRunFinishArgs),
    /// Close every unfinished run of a step, for a session that died.
    Abandon(PlanRunAbandonArgs),
    /// Record a step not run, and why.
    Skip(PlanRunSkipArgs),
}

/// `plan milestone` leaves.
#[derive(Subcommand)]
pub(crate) enum PlanMilestoneOp {
    /// Add a milestone.
    Add(PlanMilestoneAddArgs),
    /// Change a milestone; on an audited store this needs `--reason`.
    Update(PlanMilestoneUpdateArgs),
}

/// The flow a verb works on.
#[derive(Args, Clone)]
pub(crate) struct PlanTarget {
    #[arg(long = "slug", value_name = "SLUG", help = "Flow whose plan is used")]
    pub(crate) slug: String,
}

#[derive(Args)]
pub(crate) struct PlanOpenArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "from",
        value_name = "ID,...",
        value_delimiter = ',',
        help = "Backlog items whose summary, context and evidence seed the brief"
    )]
    pub(crate) from: Vec<String>,
    #[arg(
        long = "branch",
        value_name = "BRANCH",
        help = "Branch held in `[binding]` until approval"
    )]
    pub(crate) branch: Option<String>,
    #[arg(
        long = "scope",
        value_name = "GLOB",
        help = "Scope glob held in `[binding]` until approval (repeatable; the flags given replace the held list, not append)"
    )]
    pub(crate) scope: Vec<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanDecideArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "Decisions, one JSON object per line; a row with `id` updates: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanContractArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "Contracts, one JSON object per line; a row with `id` updates: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanHandoverSetArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "Handovers, one JSON object per line; a row with `id` updates: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanHandoverDoneArgs {
    #[arg(value_name = "ID")]
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanRunStartArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "step",
        value_name = "STEP",
        help = "The flow step being run; `review` is the one value read elsewhere, matched exactly by plan stage and plan gate"
    )]
    pub(crate) step: String,
    #[arg(
        long = "workflow",
        value_name = "WORKFLOW",
        help = "The saved Workflow launched; a one-agent step has none"
    )]
    pub(crate) workflow: Option<String>,
    #[arg(
        long = "args-hash",
        value_name = "HASH",
        help = "Digest of the Workflow's arguments"
    )]
    pub(crate) args_hash: Option<String>,
    #[arg(
        long = "run-id",
        value_name = "ID",
        help = "The run id from the Workflow's result; a one-agent step has none"
    )]
    pub(crate) run_id: Option<String>,
    #[arg(
        long = "session",
        value_name = "ID",
        help = "The session launching the step, from CLAUDE_CODE_SESSION_ID"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        long = "tasks",
        value_name = "N1,N2",
        value_delimiter = ',',
        help = "The task ids the launch dispatched, stored on the run row"
    )]
    pub(crate) tasks: Vec<u32>,
    #[arg(
        long = "store-digest",
        value_name = "DIGEST",
        requires = "digest_at",
        help = "The `store_digest` plan emit launch-args printed, stored on an implement: run in place of one taken now"
    )]
    pub(crate) store_digest: Option<String>,
    #[arg(
        long = "digest-at",
        value_name = "TIMESTAMP",
        requires = "store_digest",
        help = "The `digest_at` plan emit launch-args printed beside `--store-digest`; the lane check's window opens there"
    )]
    pub(crate) digest_at: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanRunFinishArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(long = "step", value_name = "STEP", help = "The flow step that ran")]
    pub(crate) step: String,
    #[arg(
        long = "run-id",
        value_name = "ID",
        help = "The Workflow's run id, when learned after launch"
    )]
    pub(crate) run_id: Option<String>,
    #[arg(
        long = "outcome",
        value_name = "OUTCOME",
        default_value = "ok",
        help = "ok|errors|stopped"
    )]
    pub(crate) outcome: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanRunAbandonArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "step",
        value_name = "STEP",
        help = "The flow step whose open runs are dead"
    )]
    pub(crate) step: String,
    #[arg(long = "reason", value_name = "TEXT", help = "Why they were abandoned")]
    pub(crate) reason: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanRunSkipArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "step",
        value_name = "STEP",
        help = "The flow step not run; `review` is the one value read elsewhere, matched exactly by plan stage and plan gate"
    )]
    pub(crate) step: String,
    #[arg(long = "reason", value_name = "TEXT", help = "Why it was not run")]
    pub(crate) reason: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanMilestoneAddArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(long = "id", value_name = "ID", help = "The milestone's id")]
    pub(crate) id: String,
    #[arg(
        long = "goal",
        value_name = "TEXT",
        help = "What the milestone delivers"
    )]
    pub(crate) goal: String,
    #[arg(
        long = "gate",
        value_name = "CMD",
        help = "Command that must pass at the milestone's drain (repeatable)"
    )]
    pub(crate) gate: Vec<String>,
    #[arg(
        long = "gate-probe",
        value_name = "GATE :: PROBE",
        help = "A cheap command that fails where <GATE>, one of the milestone's gates, cannot pass \
                here; split at the first ` :: ` (repeatable)"
    )]
    pub(crate) gate_probe: Vec<String>,
    #[arg(
        long = "defer",
        help = "Elaborate the milestone's tasks just before it runs; takes --reason"
    )]
    pub(crate) defer: bool,
    #[arg(
        long = "reason",
        value_name = "TEXT",
        help = "Why it is deferred, and why it was added on an audited store, which requires it"
    )]
    pub(crate) reason: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanMilestoneUpdateArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(long = "id", value_name = "ID", help = "The milestone's id")]
    pub(crate) id: String,
    #[arg(
        long = "goal",
        value_name = "TEXT",
        help = "What the milestone delivers"
    )]
    pub(crate) goal: Option<String>,
    #[arg(
        long = "gate",
        value_name = "CMD",
        help = "Command that must pass at the milestone's drain (repeatable; the flags given replace the whole gate list, unlike add)"
    )]
    pub(crate) gate: Vec<String>,
    #[arg(long = "clear-gate", help = "Empty the gate list; refused with --gate")]
    pub(crate) clear_gate: bool,
    #[arg(
        long = "gate-probe",
        value_name = "GATE :: PROBE",
        help = "A cheap command that fails where <GATE>, one of the milestone's gates, cannot pass \
                here; split at the first ` :: ` (repeatable; the flags given replace the whole \
                probe list, keeping the verdict of a probe given unchanged)"
    )]
    pub(crate) gate_probe: Vec<String>,
    #[arg(
        long = "defer",
        help = "Elaborate the milestone's tasks just before it runs; takes --reason"
    )]
    pub(crate) defer: bool,
    #[arg(
        long = "elaborated",
        help = "Record that a deferred milestone's tasks are now elaborated; takes --reason"
    )]
    pub(crate) elaborated: bool,
    #[arg(
        long = "reason",
        value_name = "TEXT",
        help = "Why it is deferred or elaborated, and why it changed on an audited store"
    )]
    pub(crate) reason: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanCheckArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanViewArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(long = "for", value_name = "ROLE", help = "human|digest|worker")]
    pub(crate) for_role: String,
    #[arg(
        long = "task",
        value_name = "ID",
        help = "The task a worker projection is for"
    )]
    pub(crate) task: Option<u32>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("probes_mode").required(true).multiple(false).args(["emit", "record"])))]
pub(crate) struct PlanProbesArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(long = "emit", help = "Print the probe script")]
    pub(crate) emit: bool,
    #[arg(
        long = "record",
        value_name = "PATH",
        help = "Store the verdicts from the script's captured stdout"
    )]
    pub(crate) record: Option<PathBuf>,
    #[arg(
        long = "premises",
        help = "Work on each pending task's premises instead of its accept rows: `base` ones, \
                and `after:<id>` ones once task <id> is done. With --record, store each \
                premise's verdict: holds, fails or broken"
    )]
    pub(crate) premises: bool,
    #[arg(
        long = "gates",
        conflicts_with_all = ["premises", "task"],
        help = "Work on each milestone's stored gate probes instead of task rows. With --record, \
                store each probe's verdict: holds, fails or broken"
    )]
    pub(crate) gates: bool,
    #[arg(
        long = "milestone",
        value_name = "ID",
        help = "Cover only this checkpoint's pending rows, in either mode"
    )]
    pub(crate) milestone: Option<String>,
    #[arg(
        long = "task",
        value_name = "N1,N2",
        value_delimiter = ',',
        help = "Cover only these tasks' pending rows, in either mode"
    )]
    pub(crate) task: Vec<u32>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanStageArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanGateArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanApproveArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "review",
        value_name = "OUTCOME",
        help = "ran|skipped-low-risk|skipped-flag"
    )]
    pub(crate) review: String,
    #[arg(
        long = "reason",
        value_name = "TEXT",
        help = "Why review was skipped; both skips need it"
    )]
    pub(crate) reason: Option<String>,
    #[arg(
        long = "amend",
        help = "Append a new approval after a user-agreed change to intent"
    )]
    pub(crate) amend: bool,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanEmitLaunchArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "milestone",
        value_name = "ID",
        help = "The checkpoint whose rows are launched"
    )]
    pub(crate) milestone: String,
    #[arg(
        long = "max-parallel",
        value_name = "N",
        default_value_t = 4,
        help = "Workers running at once, from 1 to 8"
    )]
    pub(crate) max_parallel: u32,
    #[arg(
        long = "max-compile-parallel",
        value_name = "N",
        help = "Running tasks whose files hold a `.rs` path or a Cargo.toml, from 1 to 8; \
                absent, --max-parallel alone caps them"
    )]
    pub(crate) max_compile_parallel: Option<u32>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanEmitLaneCheckArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "milestone",
        value_name = "ID",
        help = "The checkpoint whose latest `implement:<id>` run is checked"
    )]
    pub(crate) milestone: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanEmitGateListArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "milestone",
        value_name = "ID",
        help = "The checkpoint whose gate commands are listed"
    )]
    pub(crate) milestone: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanEmitReproduceUnitsArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "milestone",
        value_name = "ID",
        help = "The checkpoint whose fixed findings are checked"
    )]
    pub(crate) milestone: String,
    #[arg(
        long = "since-launch",
        help = "Keep a unit only when one of its findings is not yet `gone` in this milestone, \
                or its files share a path with a row the latest `implement:<id>` run dispatched"
    )]
    pub(crate) since_launch: bool,
    #[arg(
        long = "relaunch",
        value_name = "N",
        help = "The Nth relaunch of a round cut short; takes --round and --task, and gives the \
                run directory its own `-r<N>` suffix"
    )]
    pub(crate) relaunch: Option<u32>,
    #[arg(
        long = "round",
        value_name = "R",
        help = "The round being relaunched; only with --relaunch"
    )]
    pub(crate) round: Option<u32>,
    #[arg(
        long = "task",
        value_name = "REF1,REF2",
        value_delimiter = ',',
        help = "The refs of the units relaunched; only with --relaunch"
    )]
    pub(crate) task: Vec<String>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct PlanDriftArgs {
    #[command(flatten)]
    pub(crate) target: PlanTarget,
    #[arg(
        long = "uncommitted",
        help = "Also report the working tree's modified and untracked paths"
    )]
    pub(crate) uncommitted: bool,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}
