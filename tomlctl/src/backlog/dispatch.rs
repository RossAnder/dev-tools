//! `BacklogOp` fan-out to the per-verb leaf modules.
//!
//! Every variant is destructured field-by-field with no `..` rest pattern, so
//! a flag added to a variant fails to compile here rather than being silently
//! dropped on the way to its leaf.

use anyhow::Result;

use crate::cli::{BacklogOp, EvidenceOp};

pub(crate) fn dispatch(op: BacklogOp) -> Result<()> {
    match op {
        BacklogOp::Add {
            summary,
            kind,
            area,
            tag,
            evidence,
            related,
            context,
            origin,
            flow,
            on_duplicate,
            json,
            dry_run,
            integrity,
        } => crate::backlog::add::dispatch(
            summary,
            kind,
            area,
            tag,
            evidence,
            related,
            context,
            origin,
            flow,
            on_duplicate,
            json,
            dry_run,
            integrity,
        ),
        BacklogOp::Check {
            summary,
            area,
            kind,
            tag,
            limit,
            similarity_strong,
            similarity_related,
            integrity,
        } => crate::backlog::check::dispatch(
            summary,
            area,
            kind,
            tag,
            limit,
            similarity_strong,
            similarity_related,
            integrity,
        ),
        BacklogOp::List {
            status,
            kind,
            tag,
            open,
            area_prefix,
            has_evidence,
            count,
            query,
            integrity,
        } => crate::backlog::query::dispatch_list(
            status,
            kind,
            tag,
            open,
            area_prefix,
            has_evidence,
            count,
            query,
            integrity,
        ),
        BacklogOp::Show { id, integrity } => crate::backlog::query::dispatch_show(id, integrity),
        BacklogOp::Relate {
            a,
            to,
            relation,
            integrity,
        } => crate::backlog::relate::dispatch(a, to, relation, integrity),
        BacklogOp::Triage {
            ids,
            mode,
            to,
            reason,
            resolution,
            rationale,
            integrity,
        } => crate::backlog::triage::dispatch(ids, mode, to, reason, resolution, rationale, integrity),
        BacklogOp::Cluster {
            by,
            min_size,
            min_shared_tags,
            all_statuses,
            integrity,
        } => crate::backlog::cluster::dispatch(by, min_size, min_shared_tags, all_statuses, integrity),
        BacklogOp::Compact {
            older_than,
            dry_run,
            integrity,
        } => crate::backlog::compact::dispatch(older_than, dry_run, integrity),
        BacklogOp::Evidence { op } => match op {
            EvidenceOp::Dir { id, no_create } => {
                crate::backlog::evidence_ops::dispatch_dir(id, no_create)
            }
            EvidenceOp::Audit {
                strict,
                max_bytes,
                integrity,
            } => crate::backlog::evidence_ops::dispatch_audit(strict, max_bytes, integrity),
        },
    }
}
