//! `plan check`: the task store's own findings plus the classes only a v2 flow
//! can raise, which read `plan.toml`, the task store's side tables, `brief.md`
//! and the `design/` documents.
//!
//! Warnings are `plan/milestone-no-gate` and `plan/probe-absent`; every other
//! class raised here is an error.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use serde_json::json;

use super::schema::{Bind, Contract, Decision, Handover, PlanStore};
use super::store::{self, BRIEF_FILE, brief_digest};
use crate::cli::{PlanCheckArgs, ReadIntegrityArgs};
use crate::io::repo_or_cwd_root;
use crate::output::{Rows, print_report};
use crate::tasks::backlog_refs::linked_backlog_findings;
use crate::tasks::check::{check, exit_code};
use crate::tasks::finding::{
    ERROR, Finding, NO_TASK, WARNING, finding_json, quoted_list, task_list,
};
use crate::tasks::markdown::FenceState;
use crate::tasks::schema::{
    BINDING_REQUIRED, Premise, Status, Store, TaskRow, TaskSpec, VERDICT_HEALTHY, normal_path,
};
use crate::tasks::slug::{dedupe_refs, derive_ref};
use crate::tasks::{load_store, resolve_store_path};

const DESIGN_DIR: &str = "design";
pub(crate) const REQUIRED_SECTIONS: [&str; 4] =
    ["Intent", "Non-goals", "Scope budget", "Success criteria"];

/// An approved brief differs from its stamp exactly when `--amend` is needed.
pub(super) const CLASS_BRIEF_CHANGED: &str = "plan/brief-changed";

pub(super) fn run(args: PlanCheckArgs) -> Result<()> {
    let findings = findings(&args.target.slug, &args.integrity)?;
    let code = exit_code(&findings);
    let rendered: Vec<_> = findings.iter().map(finding_json).collect();
    // `print_report` flushes before returning, so the exit below loses nothing.
    print_report(
        json!({ "ok": code == 0, "findings": rendered }),
        Rows::Field("findings"),
    )?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
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
                findings.extend(row_spec_findings(spec, row, elaborated, &gates, authored));
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

/// `elaborated` is false for a task in a deferred milestone, whose `check`
/// arrives when that milestone is elaborated. An unrecognised `elaboration`
/// counts as elaborated, which is the stricter reading. `plan/probe-absent` needs
/// a pending row because `plan probes` covers no other, so no verb could clear it.
/// `gates` are the gates of the row's own milestone, which a `covered_by` must name.
fn row_spec_findings(
    spec: &TaskSpec,
    row: &TaskRow,
    elaborated: bool,
    gates: &[&str],
    authored: &Path,
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
        let has_check = !accept.check.trim().is_empty();
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
        let probeable =
            super::probes::skip_reason(accept).is_none() && row.status == Status::Pending;
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
    }
    findings
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
