//! `plan/generated-output-unheld`: a pending row of a milestone that is not
//! deferred whose files include an input of a declared generator but not every
//! output that generator writes. A row may leave regeneration to a dependent
//! that holds the output, so this is a warning.

use std::collections::BTreeSet;
use std::path::Path;

use crate::generated::{self, CONFIG};
use crate::tasks::finding::{Finding, WARNING};
use crate::tasks::schema::{Status, Store, normal_path};

pub(super) fn generated_output_findings(tasks: &Store, root: &Path) -> Vec<Finding> {
    let generators = match generated::load(root) {
        Ok(generators) => generators,
        Err(problem) => {
            return vec![Finding {
                class: "plan/generated-output-unheld",
                severity: WARNING,
                ids: Vec::new(),
                detail: format!("`{CONFIG}` is malformed: {problem}"),
            }];
        }
    };
    if generators.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for row in tasks
        .items
        .iter()
        .filter(|row| row.status == Status::Pending)
    {
        let deferred = tasks
            .milestones
            .iter()
            .any(|m| m.id == row.checkpoint && m.is_deferred());
        if deferred {
            continue;
        }
        let held: BTreeSet<String> = row.files.iter().map(|f| normal_path(f)).collect();
        for generator in generated::touched(&generators, &row.files) {
            let missing = generated::missing_outputs(generator, &row.files);
            if missing.is_empty() {
                continue;
            }
            let inputs: Vec<&str> = held
                .iter()
                .filter(|file| generator.inputs.is_match(file))
                .map(String::as_str)
                .collect();
            findings.push(Finding {
                class: "plan/generated-output-unheld",
                severity: WARNING,
                ids: vec![row.id],
                detail: format!(
                    "row {} edits {}, an input of generator `{}`, but does not hold its output {}",
                    row.id,
                    inputs.join(", "),
                    generator.id,
                    missing.join(", ")
                ),
            });
        }
    }
    findings
}
