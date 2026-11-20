//! `plan check`: the task store's own findings plus the classes only a v2 flow
//! can raise, which read `plan.toml`, the task store's side tables, `brief.md`
//! and the `design/` documents.
//!
//! Warnings are `plan/file-case-collision`, `plan/generated-output-unheld`,
//! `plan/milestone-no-gate`,
//! `plan/milestone-order-unqualified`, `plan/needs-gate-effect`, `plan/probe-absent`,
//! `plan/ui-accept-structural` and `plan/ui-milestone-no-states`, and `plan/file-only-chain` and `plan/milestone-width`
//! are info; every other class raised here is an error.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use serde_json::json;

use super::schema::{Bind, Contract, Decision, Handover, PlanStore};
use super::store::{self, BRIEF_FILE, brief_digest};
use crate::cli::{PlanCheckArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::{Rows, print_report};
use crate::tasks::backlog_refs::linked_backlog_findings;
use crate::tasks::check::{check, exit_code};
use crate::tasks::finding::{
    ERROR, Finding, INFO, NO_TASK, WARNING, finding_json, quoted_list, task_list,
};
use crate::tasks::markdown::FenceState;
use crate::tasks::milestones;
use crate::tasks::schema::{
    BINDING_REQUIRED, EFFECT_EXTERNAL, MilestoneSpec, Premise, RUN_BY_GATE, RUN_BY_LOOK, Status,
    Store, TaskRow, TaskSpec, VERDICT_HEALTHY, normal_path,
};
use crate::tasks::slug::{dedupe_refs, derive_ref};
use crate::tasks::{load_store, resolve_store_path};
use crate::ui_capture::{self, CONFIG as UI_CAPTURE_CONFIG};

mod case_collision;
mod generated_outputs;

const DESIGN_DIR: &str = "design";
pub(crate) const REQUIRED_SECTIONS: [&str; 4] =
    ["Intent", "Non-goals", "Scope budget", "Success criteria"];

/// An approved brief differs from its stamp exactly when `--amend` is needed.
pub(super) const CLASS_BRIEF_CHANGED: &str = "plan/brief-changed";

pub(super) fn run(args: PlanCheckArgs) -> Result<()> {
    let scope = match &args.milestone {
        Some(milestone) => Some(milestone_scope(
            &args.target.slug,
            milestone,
            &args.integrity,
        )?),
        None => None,
    };
    let mut findings = findings(&args.target.slug, &args.integrity)?;
    if let Some(scope) = &scope {
        findings.retain(|finding| scope.keeps(finding));
    }
    let code = exit_code(&findings);
    let rendered: Vec<_> = findings.iter().map(finding_json).collect();
    print_report(
        json!({ "ok": code == 0, "findings": rendered }),
        Rows::Field("findings"),
    )?;
    if code != 0 {
        return Err(crate::invocation::ExitWith(code).into());
    }
    Ok(())
}

/// The rows `plan check --milestone` reports on: the milestone's own and those of
/// every milestone upstream of it, at any distance, beside every row id the
/// store carries.
struct MilestoneScope {
    rows: BTreeSet<u32>,
    stored: BTreeSet<u32>,
    /// Set when the milestone graph cannot be formed, so no finding is dropped
    /// and the classes reporting why stay in view.
    unscoped: bool,
}

impl MilestoneScope {
    /// A finding naming no task, or a task no row carries, belongs to no other
    /// milestone. A broken graph, a bad cut or an unfinished handover blocks
    /// every milestone, so those classes are kept whatever they name.
    fn keeps(&self, finding: &Finding) -> bool {
        self.unscoped
            || finding.ids.is_empty()
            || finding.class.starts_with("dag/")
            || matches!(
                finding.class,
                "checkpoint/invalid-cut"
                    | "plan/handover-incomplete"
                    | "plan/handover-task-unknown"
            )
            || finding
                .ids
                .iter()
                .any(|id| self.rows.contains(id) || !self.stored.contains(id))
    }
}

/// Refuses a `--milestone` naming no checkpoint.
fn milestone_scope(
    slug: &str,
    milestone: &str,
    integrity: &ReadIntegrityArgs,
) -> Result<MilestoneScope> {
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, integrity)?;
    if !tasks.checkpoints.iter().any(|c| c.id == milestone) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("`--milestone {milestone}` names no checkpoint in the store"),
        ));
    }
    let stored: BTreeSet<u32> = tasks.items.iter().map(|row| row.id).collect();
    let Ok(direct) = milestones::upstream(&tasks) else {
        return Ok(MilestoneScope {
            rows: BTreeSet::new(),
            stored,
            unscoped: true,
        });
    };
    let mut reached: BTreeSet<&str> = BTreeSet::from([milestone]);
    let mut frontier = vec![milestone];
    while let Some(id) = frontier.pop() {
        for up in direct.get(id).into_iter().flatten() {
            if reached.insert(up.as_str()) {
                frontier.push(up.as_str());
            }
        }
    }
    let rows = tasks
        .items
        .iter()
        .filter(|row| reached.contains(row.checkpoint.as_str()))
        .map(|row| row.id)
        .collect();
    Ok(MilestoneScope {
        rows,
        stored,
        unscoped: false,
    })
}

/// Everything `plan check` reports for `slug`, sorted by class and ids. A store
/// that cannot be read is a refusal rather than a finding.
pub(crate) fn findings(slug: &str, integrity: &ReadIntegrityArgs) -> Result<Vec<Finding>> {
    let plan_path = store::resolve(slug)?;
    let plan = store::load(&plan_path, integrity)?;
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, integrity)?;
    let authored = store::authored_dir(slug)?;

    let mut found = check(&tasks, &[]);
    found.extend(linked_backlog_findings(&tasks, Some(slug))?);
    found.extend(brief_findings(&authored, &plan)?);
    found.extend(
        plan.contracts
            .iter()
            .flat_map(|c| contract_findings(c, &tasks)),
    );
    found.extend(
        plan.handovers
            .iter()
            .flat_map(|h| handover_findings(h, &tasks)),
    );
    found.extend(tasks.specs.iter().flat_map(|spec| {
        spec.premises
            .iter()
            .filter_map(|p| premise_finding(p, &spec.r#ref, &tasks))
    }));
    let root = repo_or_cwd_root()?;
    found.extend(
        plan.decisions
            .iter()
            .filter_map(|d| decision_finding(d, &tasks, &root)),
    );
    found.extend(
        tasks
            .items
            .iter()
            .filter_map(|row| no_spec_finding(row, &tasks)),
    );
    found.extend(spec_findings(&tasks, &authored));
    found.extend(milestone_findings(&tasks));
    found.extend(needs_gate_effect_findings(&tasks));
    found.extend(ui_findings(
        &tasks,
        ui_capture::load_paths(&root).ok().as_ref(),
    ));
    found.extend(ui_scope_findings(&tasks, &root));
    found.extend(generated_outputs::generated_output_findings(&tasks, &root));
    found.extend(case_collision::case_collision_findings(&tasks, &root));
    if plan.approvals.is_empty() {
        found.extend(file_chain_findings(&tasks, &plan));
        found.extend(milestone_width_findings(&tasks));
        found.extend(milestone_order_findings(&tasks));
    }
    found.sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
    Ok(found)
}

/// The section a `design/x.md#anchor` citation names, LF-joined with its heading
/// line, up to the next heading no deeper than its own. `root` is the flow's
/// authored directory, `store::authored_dir`. A citation is file-controlled input,
/// so one that leaves `design/` or names no anchor reads as absent.
pub(crate) fn design_section(root: &Path, citation: &str) -> Option<String> {
    let (relative, anchor) = citation.split_once('#')?;
    if anchor.is_empty() {
        return None;
    }
    let path = design_path(root, relative)?;
    let source = fs::read_to_string(path).ok()?;
    design_section_in(&source, anchor).map(|(_, body)| body)
}

/// The section `anchor` names in `source`, as `design_section` reads it, with
/// the byte offset of its heading line. The offset is meaningful only when
/// `source` has LF line endings.
pub(crate) fn design_section_in(source: &str, anchor: &str) -> Option<(usize, String)> {
    if anchor.is_empty() {
        return None;
    }
    let outline = Outline::of(source);
    let (first, section) = outline.section(anchor)?;
    let end = section
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(0, |last| last + 1);
    let offset = outline.lines[..first]
        .iter()
        .map(|line| line.len() + 1)
        .sum();
    Some((offset, section[..end].join("\n")))
}

/// Containment is checked after resolving symlinks, because a link at or inside
/// `design/` could otherwise inline any file on the machine into a worker prompt.
pub(crate) fn design_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    let mut parts = relative.components();
    match parts.next()? {
        Component::Normal(first) if first == DESIGN_DIR => {}
        _ => return None,
    }
    if !parts.all(|part| matches!(part, Component::Normal(_))) {
        return None;
    }
    if relative.extension().is_none_or(|ext| ext != "md") {
        return None;
    }
    let contained = store::authored_anchor(root).ok()?.join(DESIGN_DIR);
    let resolved = root.join(relative).canonicalize().ok()?;
    resolved.starts_with(contained).then_some(resolved)
}

struct Heading {
    line: usize,
    depth: usize,
    slug: String,
}

/// A document's headings outside fenced blocks, each slugged by the task-ref
/// rule, with repeats numbered in document order the way refs are.
struct Outline<'a> {
    lines: Vec<&'a str>,
    headings: Vec<Heading>,
}

impl<'a> Outline<'a> {
    fn of(source: &'a str) -> Self {
        let lines: Vec<&str> = source.lines().collect();
        let mut fence = FenceState::default();
        let mut found: Vec<(usize, usize, String)> = Vec::new();
        for (line, text) in lines.iter().enumerate() {
            if fence.consume(text) {
                continue;
            }
            if let Some((depth, title)) = atx_heading(text) {
                found.push((line, depth, derive_ref(title)));
            }
        }
        let mut slugs: Vec<String> = found.iter().map(|(_, _, slug)| slug.clone()).collect();
        dedupe_refs(&mut slugs);
        let headings = found
            .into_iter()
            .zip(slugs)
            .map(|((line, depth, _), slug)| Heading { line, depth, slug })
            .collect();
        Self { lines, headings }
    }

    /// The section's lines with the index of its heading line.
    fn section(&self, slug: &str) -> Option<(usize, &[&'a str])> {
        let at = self.headings.iter().position(|h| h.slug == slug)?;
        let heading = &self.headings[at];
        let end = self.headings[at + 1..]
            .iter()
            .find(|next| next.depth <= heading.depth)
            .map_or(self.lines.len(), |next| next.line);
        Some((heading.line, &self.lines[heading.line..end]))
    }
}

/// The depth and title of an ATX heading. A closing `#` run needs no stripping,
/// since the slug rule drops it.
pub(super) fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let depth = rest.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&depth) {
        return None;
    }
    let title = &rest[depth..];
    (title.is_empty() || title.starts_with([' ', '\t'])).then_some((depth, title))
}

/// The heading level is not part of the contract, only the title.
fn section_has_content(outline: &Outline<'_>, title: &str) -> bool {
    let Some((_, section)) = outline.section(&derive_ref(title)) else {
        return false;
    };
    body_is_filled(&section[1..].join("\n"))
}

/// A section body counts as filled by anything but blank lines, sub-headings and
/// HTML comments.
pub(super) fn body_is_filled(body: &str) -> bool {
    strip_comments(body)
        .lines()
        .any(|line| !line.trim().is_empty() && atx_heading(line).is_none())
}

fn strip_comments(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<!--") {
        kept.push_str(&rest[..open]);
        rest = match rest[open..].find("-->") {
            Some(close) => &rest[open + close + 3..],
            None => "",
        };
    }
    kept.push_str(rest);
    kept
}

fn brief_findings(authored: &Path, plan: &PlanStore) -> Result<Vec<Finding>> {
    let Some(bytes) = store::read_brief(authored)? else {
        return Ok(vec![error(
            "plan/brief-missing",
            Vec::new(),
            format!("`{BRIEF_FILE}` does not exist in `{}`", authored.display()),
        )]);
    };

    let text = String::from_utf8_lossy(&bytes);
    let outline = Outline::of(&text);
    let mut findings: Vec<Finding> = REQUIRED_SECTIONS
        .iter()
        .filter(|title| !section_has_content(&outline, title))
        .map(|title| {
            error(
                "plan/brief-section-empty",
                Vec::new(),
                format!("`{BRIEF_FILE}` has no content under `{title}`"),
            )
        })
        .collect();
    // `--amend` appends, so the newest stamp is the one in force.
    if let Some(approval) = plan.approvals.last()
        && brief_digest(&bytes) != approval.brief_sha256
    {
        // The finding-class gate reads a literal here; keep it equal to `CLASS_BRIEF_CHANGED`.
        findings.push(error(
            "plan/brief-changed",
            Vec::new(),
            format!(
                "`{BRIEF_FILE}` differs from the brief approved at `{}`",
                approval.commit
            ),
        ));
    }
    Ok(findings)
}

fn contract_findings(contract: &Contract, tasks: &Store) -> Vec<Finding> {
    let mut findings = Vec::new();
    if contract.check.trim().is_empty() {
        findings.push(error(
            "plan/contract-no-check",
            contract.tasks.clone(),
            format!("contract {} has no `check`", contract.id),
        ));
    }
    let unknown: Vec<u32> = contract
        .tasks
        .iter()
        .copied()
        .filter(|id| tasks.find(*id).is_none())
        .collect();
    if !unknown.is_empty() {
        findings.push(error(
            "plan/contract-task-unknown",
            unknown.clone(),
            format!(
                "contract {} names {}, which no row carries",
                contract.id,
                task_list(&unknown, NO_TASK)
            ),
        ));
    }
    findings
}

/// A done handover has nothing left to run, so the task it came from no longer
/// matters and must not block `plan approve --amend`.
fn handover_findings(handover: &Handover, tasks: &Store) -> Vec<Finding> {
    let mut findings = Vec::new();
    if handover.done {
        return findings;
    }
    let empty: Vec<&str> = [
        ("reason", handover.reason.is_none()),
        ("action", handover.action.trim().is_empty()),
        ("when", handover.when.trim().is_empty()),
        ("done_when", handover.done_when.trim().is_empty()),
    ]
    .into_iter()
    .filter_map(|(key, missing)| missing.then_some(key))
    .collect();
    if !empty.is_empty() {
        findings.push(error(
            "plan/handover-incomplete",
            vec![handover.source_task],
            format!("handover {} has no {}", handover.id, empty.join(", ")),
        ));
    }
    if tasks.find(handover.source_task).is_none() {
        findings.push(error(
            "plan/handover-task-unknown",
            vec![handover.source_task],
            format!(
                "handover {} names task {}, which no row carries",
                handover.id, handover.source_task
            ),
        ));
    }
    findings
}

/// A premise at `after:<id>` waits on that row, so one naming no row can never be
/// scheduled. A non-canonical id such as `after:007` names none, as at write time.
fn premise_finding(premise: &Premise, spec_ref: &str, tasks: &Store) -> Option<Finding> {
    let target = premise.at.strip_prefix("after:")?;
    let id = target
        .parse::<u32>()
        .ok()
        .filter(|id| id.to_string() == target);
    if id.is_some_and(|id| tasks.find(id).is_some()) {
        return None;
    }
    Some(error(
        "plan/premise-task-unknown",
        id.into_iter().collect(),
        format!(
            "a premise of `{spec_ref}` is at `{}`, which no row carries",
            premise.at
        ),
    ))
}

fn decision_finding(decision: &Decision, tasks: &Store, root: &Path) -> Option<Finding> {
    let claimed: BTreeSet<String> = tasks
        .items
        .iter()
        .flat_map(|row| row.files.iter())
        .map(|file| normal_path(file))
        .collect();
    let unknown: Vec<&str> = decision
        .binds
        .iter()
        .map(String::as_str)
        .filter(|entry| !bind_known(entry, tasks, &claimed, root))
        .collect();
    if unknown.is_empty() {
        return None;
    }
    let ids: BTreeSet<u32> = unknown
        .iter()
        .filter_map(|entry| match Bind::parse(entry) {
            Some(Bind::Task(id)) => Some(id),
            _ => None,
        })
        .collect();
    Some(error(
        "plan/decision-bind-unknown",
        ids.into_iter().collect(),
        format!(
            "decision {} binds {}, each neither a task row nor a path on a row's `files` or \
             under the repo root",
            decision.id,
            quoted_list(&unknown, "nothing")
        ),
    ))
}

/// A path must be claimed or sit under the repo root, which a `..` or an absolute
/// path does not.
fn bind_known(entry: &str, tasks: &Store, claimed: &BTreeSet<String>, root: &Path) -> bool {
    match Bind::parse(entry) {
        None => false,
        Some(Bind::Task(id)) => tasks.find(id).is_some(),
        Some(Bind::Path(normal)) => {
            if claimed.contains(&normal) {
                return true;
            }
            let relative = Path::new(&normal);
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
                && root.join(relative).exists()
        }
    }
}

fn no_spec_finding(row: &TaskRow, tasks: &Store) -> Option<Finding> {
    if tasks.spec_for(&row.r#ref).is_some() {
        return None;
    }
    Some(error(
        "plan/task-no-spec",
        vec![row.id],
        format!("task {} (`{}`) has no spec", row.id, row.r#ref),
    ))
}

fn spec_findings(tasks: &Store, authored: &Path) -> Vec<Finding> {
    let deferred: BTreeSet<&str> = tasks
        .milestones
        .iter()
        .filter(|milestone| milestone.is_deferred())
        .map(|milestone| milestone.id.as_str())
        .collect();

    let mut findings = Vec::new();
    for spec in &tasks.specs {
        match tasks.find_ref(&spec.r#ref) {
            Some(row) => {
                let elaborated = !deferred.contains(row.checkpoint.as_str());
                let gates: Vec<&str> = tasks
                    .milestones
                    .iter()
                    .filter(|milestone| milestone.id == row.checkpoint)
                    .flat_map(|milestone| milestone.gates.iter().map(String::as_str))
                    .collect();
                findings.extend(row_spec_findings(
                    spec,
                    row,
                    elaborated,
                    &gates,
                    authored,
                    added_after_approval(tasks, &row.r#ref),
                ));
            }
            None => findings.push(error(
                "plan/spec-orphan",
                Vec::new(),
                format!("a spec is keyed on `{}`, which no row carries", spec.r#ref),
            )),
        }
    }
    findings
}

/// Approval turns `audit` on, and from then on every added row leaves a `*`
/// change with an empty `old`, so that trail entry marks a row the plan's Prove
/// step never saw.
fn added_after_approval(tasks: &Store, r#ref: &str) -> bool {
    tasks.audit
        && tasks
            .changes
            .iter()
            .any(|change| change.r#ref == r#ref && change.field == "*" && change.old.is_empty())
}

/// `elaborated` is false for a task in a deferred milestone, whose `check`
/// arrives when that milestone is elaborated. An unrecognised `elaboration`
/// counts as elaborated, which is the stricter reading. `plan/probe-absent` needs
/// a pending row because `plan probes` covers no other, so no verb could clear it,
/// and one written before approval, since probes run only at the plan's Prove step.
/// `gates` are the gates of the row's own milestone, which a `covered_by` must name.
fn row_spec_findings(
    spec: &TaskSpec,
    row: &TaskRow,
    elaborated: bool,
    gates: &[&str],
    authored: &Path,
    added_after_approval: bool,
) -> Vec<Finding> {
    let id = row.id;
    let mut findings = Vec::new();
    if spec.outcome.trim().is_empty() {
        findings.push(error(
            "plan/spec-no-outcome",
            vec![id],
            format!("task {id}'s spec has no `outcome`"),
        ));
    }
    if spec.accept.is_empty() {
        findings.push(error(
            "plan/spec-no-accept",
            vec![id],
            format!("task {id}'s spec has no `accept` entry"),
        ));
    }
    for (index, accept) in spec.accept.iter().enumerate() {
        // A `look` entry is judged by an inspector, so its empty check is its shape.
        let has_check = !accept.check.trim().is_empty() || accept.run_by == RUN_BY_LOOK;
        if !has_check && elaborated {
            findings.push(error(
                "plan/accept-no-check",
                vec![id],
                format!("task {id}'s `accept[{index}]` has no `check`"),
            ));
        }
        if let Some(problem) = accept.owner_problem() {
            findings.push(error(
                "plan/accept-owner-invalid",
                vec![id],
                format!("task {id}'s `accept[{index}]`: {problem}"),
            ));
        }
        if !accept.covered_by.is_empty() && !gates.contains(&accept.covered_by.as_str()) {
            findings.push(error(
                "plan/accept-cover-unknown",
                vec![id],
                format!(
                    "task {id}'s `accept[{index}]` is covered by `{}`, which is no gate of \
                     milestone `{}`",
                    accept.covered_by, row.checkpoint
                ),
            ));
        }
        let probeable = super::probes::skip_reason(accept).is_none()
            && row.status == Status::Pending
            && !added_after_approval;
        match &accept.probe {
            Some(probe) if probe.verdict != VERDICT_HEALTHY => findings.push(error(
                "plan/probe-unhealthy",
                vec![id],
                format!(
                    "task {id}'s `accept[{index}]` probed `{}` at `{}`",
                    probe.verdict, probe.at
                ),
            )),
            None if probeable => findings.push(Finding {
                class: "plan/probe-absent",
                severity: WARNING,
                ids: vec![id],
                detail: format!(
                    "task {id}'s `accept[{index}]` is the worker's, marked `writes_build = \
                     false`, and has no probe"
                ),
            }),
            _ => {}
        }
    }
    for (index, means) in spec.means.iter().enumerate() {
        if means.binding == BINDING_REQUIRED && means.detail.trim().is_empty() {
            findings.push(error(
                "plan/means-required-no-reason",
                vec![id],
                format!("task {id}'s `means[{index}]` is required and has no `detail`"),
            ));
        }
    }
    for citation in &spec.design {
        if design_section(authored, citation).is_none() {
            findings.push(error(
                "plan/design-anchor-dead",
                vec![id],
                format!("task {id} cites `{citation}`, which resolves to no design section"),
            ));
        }
    }
    findings
}

/// A checkpoint with no `[[milestones]]` row has no gate either, so both lists
/// are walked.
fn milestone_findings(tasks: &Store) -> Vec<Finding> {
    let members = |id: &str| -> Vec<u32> {
        let ids: BTreeSet<u32> = tasks
            .items
            .iter()
            .filter(|row| row.checkpoint == id)
            .map(|row| row.id)
            .collect();
        ids.into_iter().collect()
    };

    let mut findings = Vec::new();
    let declared: BTreeSet<&str> = tasks
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .chain(
            tasks
                .milestones
                .iter()
                .map(|milestone| milestone.id.as_str()),
        )
        .collect();
    let stored = checkpoint_order(tasks);
    for id in declared {
        let gated = tasks
            .milestones
            .iter()
            .filter(|milestone| milestone.id == id)
            .flat_map(|milestone| milestone.gates.iter())
            .any(|gate| !gate.trim().is_empty());
        if !gated {
            findings.push(Finding {
                class: "plan/milestone-no-gate",
                severity: WARNING,
                ids: members(id),
                detail: format!("milestone `{id}` has no gate"),
            });
        }
    }
    for milestone in &tasks.milestones {
        if milestone.is_deferred() && milestone.defer_reason.trim().is_empty() {
            findings.push(error(
                "plan/deferred-no-reason",
                members(&milestone.id),
                format!("milestone `{}` is deferred with no reason", milestone.id),
            ));
        }
        let own = stored.iter().position(|id| *id == milestone.id);
        for named in &milestone.after {
            match stored.iter().position(|id| *id == named.as_str()) {
                None => findings.push(error(
                    "plan/milestone-after-unknown",
                    members(&milestone.id),
                    format!(
                        "milestone `{}` is after `{named}`, which names no checkpoint",
                        milestone.id
                    ),
                )),
                Some(at) if *named == milestone.id || own.is_some_and(|own| at >= own) => {
                    findings.push(error(
                        "plan/milestone-after-forward",
                        members(&milestone.id),
                        format!(
                            "milestone `{}` is after `{named}`, which is not stored before it in \
                             `[[checkpoints]]`",
                            milestone.id
                        ),
                    ));
                }
                Some(_) => {}
            }
        }
        for (index, probe) in milestone.gate_probes.iter().enumerate() {
            if !milestone.gates.contains(&probe.gate) {
                findings.push(error(
                    "plan/gate-probe-unknown",
                    members(&milestone.id),
                    format!(
                        "milestone `{}`'s `gate_probes[{index}]` probes `{}`, which is no gate of \
                         the milestone",
                        milestone.id, probe.gate
                    ),
                ));
            }
        }
        for (index, external) in milestone.external_gates.iter().enumerate() {
            if !milestone.gates.contains(external) {
                findings.push(error(
                    "plan/external-gate-unknown",
                    members(&milestone.id),
                    format!(
                        "milestone `{}`'s `external_gates[{index}]` marks `{external}`, which is \
                         no gate of the milestone",
                        milestone.id
                    ),
                ));
            }
        }
    }
    findings
}

/// Per pending row, the first row its `needs` reach in its own milestone that
/// holds a gate-run `effect = "external"` accept. That effect exists only once
/// the milestone's gate has run, after every row of the milestone is done.
fn needs_gate_effect_findings(tasks: &Store) -> Vec<Finding> {
    let mut findings = Vec::new();
    for row in &tasks.items {
        if row.status != Status::Pending || row.checkpoint.is_empty() {
            continue;
        }
        let mut visited: BTreeSet<u32> = BTreeSet::new();
        let mut frontier: Vec<u32> = row.needs.clone();
        let mut hit: Option<(u32, &str)> = None;
        while let Some(id) = frontier.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(reached) = tasks.find(id) else {
                continue;
            };
            frontier.extend(reached.needs.iter().copied());
            if reached.id == row.id || reached.checkpoint != row.checkpoint {
                continue;
            }
            let external = tasks.spec_for(&reached.r#ref).and_then(|spec| {
                spec.accept.iter().find(|accept| {
                    accept.effect == EFFECT_EXTERNAL && accept.owner() == RUN_BY_GATE
                })
            });
            if let Some(accept) = external
                && hit.is_none_or(|(at, _)| reached.id < at)
            {
                hit = Some((reached.id, accept.check.as_str()));
            }
        }
        if let Some((effect_row, check)) = hit {
            findings.push(Finding {
                class: "plan/needs-gate-effect",
                severity: WARNING,
                ids: vec![row.id],
                detail: format!(
                    "task {} needs task {effect_row}, whose gate-run accept `{check}` has an \
                     external effect; that effect exists only after milestone `{}`'s gate, so \
                     task {} cannot observe it while it runs",
                    row.id, row.checkpoint, row.id
                ),
            });
        }
    }
    findings
}

fn milestone_members(tasks: &Store, id: &str) -> Vec<u32> {
    let ids: BTreeSet<u32> = tasks
        .items
        .iter()
        .filter(|row| row.checkpoint == id)
        .map(|row| row.id)
        .collect();
    ids.into_iter().collect()
}

/// The classes a milestone's declared capture states raise. A `look` entry is
/// judged on the milestone's `ui_states` only: a control state is one the
/// inspector must fail, so no behaviour is accepted on it.
fn ui_findings(tasks: &Store, capture_paths: Option<&globset::GlobSet>) -> Vec<Finding> {
    let capture_paths = capture_paths.filter(|globs| !globs.is_empty());
    let mut findings = Vec::new();
    for milestone in &tasks.milestones {
        if !milestone.ui_states.is_empty() && milestone.ui_controls.is_empty() {
            findings.push(error(
                "plan/ui-controls-missing",
                milestone_members(tasks, &milestone.id),
                format!(
                    "milestone `{}` declares `ui_states` and no `ui_controls`, so nothing shows \
                     its inspector fails a planted defect",
                    milestone.id
                ),
            ));
        }
    }
    for spec in &tasks.specs {
        let Some(row) = tasks.find_ref(&spec.r#ref) else {
            continue;
        };
        let id = row.id;
        let milestone = tasks.milestones.iter().find(|m| m.id == row.checkpoint);
        let states: &[String] = milestone.map_or(&[], |m| m.ui_states.as_slice());
        let controls: &[String] = milestone.map_or(&[], |m| m.ui_controls.as_slice());
        let mut looked = false;
        for (index, accept) in spec.accept.iter().enumerate() {
            if accept.run_by != RUN_BY_LOOK {
                continue;
            }
            looked = true;
            if states.is_empty() {
                findings.push(error(
                    "plan/look-without-states",
                    vec![id],
                    format!(
                        "task {id}'s `accept[{index}]` is a `look` entry, and milestone `{}` \
                         declares no `ui_states`",
                        row.checkpoint
                    ),
                ));
                continue;
            }
            let unknown: Vec<&str> = accept
                .states
                .iter()
                .filter(|state| !states.contains(state))
                .map(String::as_str)
                .collect();
            if unknown.is_empty() {
                continue;
            }
            let control = if unknown
                .iter()
                .any(|state| controls.iter().any(|c| c == state))
            {
                "; a control state is the inspector's to fail, never a `look` entry's"
            } else {
                ""
            };
            findings.push(error(
                "plan/look-state-unknown",
                vec![id],
                format!(
                    "task {id}'s `accept[{index}]` names {}, which milestone `{}` does not list \
                     in `ui_states`{control}",
                    quoted_list(&unknown, "no state"),
                    row.checkpoint
                ),
            ));
        }
        let structural =
            !spec.accept.is_empty() && spec.accept.iter().all(|accept| !accept.writes_build);
        if !states.is_empty()
            && !milestone.is_some_and(MilestoneSpec::is_deferred)
            && row.status == Status::Pending
            && !looked
            && structural
            && capture_paths.is_none_or(|globs| {
                row.files
                    .iter()
                    .any(|file| globs.is_match(normal_path(file)))
            })
        {
            findings.push(Finding {
                class: "plan/ui-accept-structural",
                severity: WARNING,
                ids: vec![id],
                detail: format!(
                    "task {id} sits in milestone `{}`, which declares `ui_states`, and every \
                     `accept` entry is `writes_build = false` with no `look` entry, so its \
                     behaviour is accepted on reads alone",
                    row.checkpoint
                ),
            });
        }
    }
    findings
}

/// Per milestone that is not deferred and declares no `ui_states`, the rows
/// whose files match a `paths` glob of the project's capture config. The config
/// belongs to the project, not the plan, so a malformed one is one warning.
fn ui_scope_findings(tasks: &Store, root: &Path) -> Vec<Finding> {
    let globs = match ui_capture::load_paths(root) {
        Ok(globs) => globs,
        Err(problem) => {
            return vec![Finding {
                class: "plan/ui-milestone-no-states",
                severity: WARNING,
                ids: Vec::new(),
                detail: format!("`{UI_CAPTURE_CONFIG}` is malformed: {problem}"),
            }];
        }
    };
    if globs.is_empty() {
        return Vec::new();
    }
    let declared: BTreeSet<&str> = tasks
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .chain(tasks.milestones.iter().map(|m| m.id.as_str()))
        .collect();
    let mut findings = Vec::new();
    for id in declared {
        let milestone = tasks.milestones.iter().find(|m| m.id == id);
        if milestone.is_some_and(|m| m.is_deferred() || !m.ui_states.is_empty()) {
            continue;
        }
        let mut rows: BTreeSet<u32> = BTreeSet::new();
        let mut files: BTreeSet<String> = BTreeSet::new();
        for row in tasks.items.iter().filter(|row| row.checkpoint == id) {
            for file in &row.files {
                let file = normal_path(file);
                if globs.is_match(&file) {
                    rows.insert(row.id);
                    files.insert(file);
                }
            }
        }
        if rows.is_empty() {
            continue;
        }
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        findings.push(Finding {
            class: "plan/ui-milestone-no-states",
            severity: WARNING,
            ids: rows.into_iter().collect(),
            detail: format!(
                "milestone `{id}` declares no `ui_states`, yet its rows edit {}, under the \
                 `paths` of `{UI_CAPTURE_CONFIG}`",
                quoted_list(&files, "no file")
            ),
        });
    }
    findings
}

/// Per milestone, the `needs` links inside it whose two rows share a file and
/// nothing else: no contract naming both, and no premise of the later row at
/// `after:<earlier>`. Such a link serialises the milestone only because one
/// file holds both edits, which splitting the file before approval would undo.
fn file_chain_findings(tasks: &Store, plan: &PlanStore) -> Vec<Finding> {
    let declared: BTreeSet<&str> = tasks
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .chain(
            tasks
                .milestones
                .iter()
                .map(|milestone| milestone.id.as_str()),
        )
        .collect();
    let mut findings = Vec::new();
    for milestone in declared {
        let rows: Vec<&TaskRow> = tasks
            .items
            .iter()
            .filter(|row| row.checkpoint == milestone)
            .collect();
        let member = |id: u32| rows.iter().find(|row| row.id == id).copied();
        let mut links: Vec<(u32, u32, Vec<&str>)> = Vec::new();
        for row in &rows {
            for &need in &row.needs {
                let Some(earlier) = member(need) else {
                    continue;
                };
                let theirs: BTreeSet<String> =
                    earlier.files.iter().map(|file| normal_path(file)).collect();
                let shared: Vec<&str> = row
                    .files
                    .iter()
                    .filter(|file| theirs.contains(&normal_path(file)))
                    .map(String::as_str)
                    .collect();
                if !shared.is_empty() && !link_has_reason(tasks, plan, earlier, row) {
                    links.push((earlier.id, row.id, shared));
                }
            }
        }
        if links.is_empty() {
            continue;
        }
        links.sort();
        let ids: BTreeSet<u32> = links.iter().flat_map(|(a, b, _)| [*a, *b]).collect();
        let described: Vec<String> = links
            .iter()
            .map(|(a, b, shared)| format!("{a} -> {b} over {}", quoted_list(shared, "no file")))
            .collect();
        findings.push(Finding {
            class: "plan/file-only-chain",
            severity: INFO,
            ids: ids.into_iter().collect(),
            detail: format!(
                "milestone `{milestone}`: {} share a file and no contract or premise; its \
                 longest needs chain is {} of {} rows",
                described.join(", "),
                longest_chain(&rows),
                rows.len()
            ),
        });
    }
    findings
}

/// Per milestone after the first, in `[[checkpoints]]` order, the rows that
/// could join an earlier one: the milestone after the latest one holding a
/// direct need, whose gate the row may wait on, or the first when it has no
/// needs. A deferred milestone is a boundary its plan chose, so it raises
/// nothing and is never a target: the row is offered the next milestone that is
/// not deferred instead. A row with a need in no listed milestone is left out.
fn milestone_width_findings(tasks: &Store) -> Vec<Finding> {
    let order = checkpoint_order(tasks);
    let index_of = |checkpoint: &str| order.iter().position(|id| *id == checkpoint);
    let row_index = |id: u32| {
        tasks
            .items
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| index_of(&row.checkpoint))
    };

    let deferred = |id: &str| {
        tasks
            .milestones
            .iter()
            .any(|milestone| milestone.id == id && milestone.is_deferred())
    };

    let mut findings = Vec::new();
    for (index, milestone) in order.iter().enumerate().skip(1) {
        if deferred(milestone) {
            continue;
        }
        let rows: Vec<&TaskRow> = tasks
            .items
            .iter()
            .filter(|row| row.checkpoint == *milestone)
            .collect();
        let mut waiting: Vec<(u32, usize)> = Vec::new();
        for row in &rows {
            let Some(placed) = row
                .needs
                .iter()
                .map(|&need| row_index(need))
                .collect::<Option<Vec<usize>>>()
            else {
                continue;
            };
            let earliest = placed.iter().copied().max().map_or(0, |latest| latest + 1);
            if let Some(target) = (earliest..index).find(|&at| !deferred(order[at])) {
                waiting.push((row.id, target));
            }
        }
        if waiting.is_empty() {
            continue;
        }
        waiting.sort();
        let described: Vec<String> = waiting
            .iter()
            .map(|(id, target)| format!("{id} could join `{}`", order[*target]))
            .collect();
        findings.push(Finding {
            class: "plan/milestone-width",
            severity: INFO,
            ids: waiting.iter().map(|(id, _)| *id).collect(),
            detail: format!(
                "milestone `{milestone}`: {}; {} of {} rows could start with an earlier \
                 milestone",
                described.join(", "),
                waiting.len(),
                rows.len()
            ),
        });
    }
    findings
}

/// Per milestone after the first, the earlier milestones it does not depend on
/// through `needs`, `coupling` or `after`, at any distance: the dependency pick
/// may run it before them. A store whose milestone graph cannot be formed is
/// left to the classes that report why.
fn milestone_order_findings(tasks: &Store) -> Vec<Finding> {
    let Ok(direct) = milestones::upstream(tasks) else {
        return Vec::new();
    };
    let reach = |start: &str| -> BTreeSet<&str> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut frontier = vec![start];
        while let Some(id) = frontier.pop() {
            for up in direct.get(id).into_iter().flatten() {
                if seen.insert(up.as_str()) {
                    frontier.push(up.as_str());
                }
            }
        }
        seen
    };

    let order = checkpoint_order(tasks);
    let mut findings = Vec::new();
    for (index, milestone) in order.iter().enumerate().skip(1) {
        let ups = reach(milestone);
        let unordered: Vec<&str> = order[..index]
            .iter()
            .copied()
            .filter(|earlier| !ups.contains(earlier))
            .collect();
        if unordered.is_empty() {
            continue;
        }
        findings.push(Finding {
            class: "plan/milestone-order-unqualified",
            severity: WARNING,
            ids: Vec::new(),
            detail: format!(
                "milestone `{milestone}` does not depend on {}, so /implement-v2 may run it \
                 first; add `--after <id>` to `{milestone}` where the earlier one must go first",
                quoted_list(&unordered, "no milestone")
            ),
        });
    }
    findings
}

/// `[[checkpoints]]` ids in store order, each once.
fn checkpoint_order(tasks: &Store) -> Vec<&str> {
    let mut order: Vec<&str> = Vec::new();
    for checkpoint in &tasks.checkpoints {
        if !order.contains(&checkpoint.id.as_str()) {
            order.push(checkpoint.id.as_str());
        }
    }
    order
}

/// A contract naming both rows, or a premise of `later` waiting on `earlier`,
/// is a reason for the edge beyond the file the two rows share.
fn link_has_reason(tasks: &Store, plan: &PlanStore, earlier: &TaskRow, later: &TaskRow) -> bool {
    let contracted = plan
        .contracts
        .iter()
        .any(|contract| contract.tasks.contains(&earlier.id) && contract.tasks.contains(&later.id));
    let waited = format!("after:{}", earlier.id);
    let premised = tasks
        .spec_for(&later.r#ref)
        .is_some_and(|spec| spec.premises.iter().any(|premise| premise.at == waited));
    contracted || premised
}

/// The most rows on one `needs` path inside `rows`. A cycle, which `dag/cycle`
/// reports, is cut where the walk re-enters it.
fn longest_chain(rows: &[&TaskRow]) -> usize {
    fn depth(
        id: u32,
        rows: &[&TaskRow],
        memo: &mut std::collections::BTreeMap<u32, usize>,
        walking: &mut BTreeSet<u32>,
    ) -> usize {
        if let Some(known) = memo.get(&id) {
            return *known;
        }
        if !walking.insert(id) {
            return 0;
        }
        let needs = rows
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.needs.clone())
            .unwrap_or_default();
        let deepest = needs
            .into_iter()
            .filter(|need| rows.iter().any(|row| row.id == *need))
            .map(|need| depth(need, rows, memo, walking))
            .max()
            .unwrap_or(0);
        walking.remove(&id);
        memo.insert(id, deepest + 1);
        deepest + 1
    }
    let mut memo = std::collections::BTreeMap::new();
    let mut walking = BTreeSet::new();
    rows.iter()
        .map(|row| depth(row.id, rows, &mut memo, &mut walking))
        .max()
        .unwrap_or(0)
}

fn error(class: &'static str, ids: Vec<u32>, detail: String) -> Finding {
    Finding {
        class,
        severity: ERROR,
        ids,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow_with(document: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(DESIGN_DIR)).expect("design dir");
        fs::write(dir.path().join(DESIGN_DIR).join("store.md"), document).expect("document");
        dir
    }

    #[test]
    fn a_section_runs_to_the_next_heading_no_deeper_than_its_own() {
        let flow = flow_with(
            "# Store\n\n## Locking\nTake it.\n\n### Order\nOuter first.\n\n## Sidecars\nLater.\n",
        );
        assert_eq!(
            design_section(flow.path(), "design/store.md#locking").as_deref(),
            Some("## Locking\nTake it.\n\n### Order\nOuter first.")
        );
        assert_eq!(
            design_section(flow.path(), "design/store.md#order").as_deref(),
            Some("### Order\nOuter first.")
        );
    }

    #[test]
    fn crlf_reads_as_lf_and_a_heading_alone_is_a_live_anchor() {
        let flow = flow_with("## Locking\r\nTake it.\r\n\r\n## Empty\r\n");
        assert_eq!(
            design_section(flow.path(), "design/store.md#locking").as_deref(),
            Some("## Locking\nTake it.")
        );
        assert_eq!(
            design_section(flow.path(), "design/store.md#empty").as_deref(),
            Some("## Empty")
        );
    }

    #[test]
    fn a_heading_in_a_fence_is_no_anchor_and_ends_no_section() {
        let flow = flow_with("## Locking\n```\n## Fenced\n```\nAfter.\n");
        assert!(design_section(flow.path(), "design/store.md#fenced").is_none());
        assert_eq!(
            design_section(flow.path(), "design/store.md#locking").as_deref(),
            Some("## Locking\n```\n## Fenced\n```\nAfter.")
        );
    }

    #[test]
    fn headings_slug_by_the_ref_rule_and_repeats_are_numbered() {
        let flow = flow_with("## Setup & `Config`\nOne.\n\n## Notes\nTwo.\n\n## Notes\nThree.\n");
        assert!(design_section(flow.path(), "design/store.md#setup-config").is_some());
        assert_eq!(
            design_section(flow.path(), "design/store.md#notes-2").as_deref(),
            Some("## Notes\nThree.")
        );
    }

    #[test]
    fn a_citation_that_leaves_design_or_names_no_anchor_reads_as_absent() {
        let flow = flow_with("## Locking\nTake it.\n");
        fs::write(flow.path().join("brief.md"), "## Locking\nOutside.\n").expect("brief");
        for citation in [
            "design/store.md",
            "design/store.md#",
            "design/missing.md#locking",
            "design/store.md#unlocking",
            "brief.md#locking",
            "design/../brief.md#locking",
            "../brief.md#locking",
        ] {
            assert!(
                design_section(flow.path(), citation).is_none(),
                "`{citation}` must not resolve"
            );
        }
    }

    #[test]
    fn a_comment_or_a_bare_sub_heading_does_not_fill_a_section() {
        let source = "## Intent\n<!-- what\nmust hold -->\n\n### Later\n\n## Non-goals\nNone.\n";
        let outline = Outline::of(source);
        assert!(!section_has_content(&outline, "Intent"));
        assert!(section_has_content(&outline, "Non-goals"));
        assert!(!section_has_content(&outline, "Scope budget"));
    }
}
