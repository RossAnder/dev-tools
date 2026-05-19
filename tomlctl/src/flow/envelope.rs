//! T5 (harness progressive disclosure): `tomlctl flow envelope build` —
//! emit the canonical `flow-bootstrap` input envelope as JSON on stdout.
//!
//! This subcommand replaces the ~15 lines of inline carrier prose that
//! hand-rolled this envelope in every flow command's Step-0 dispatch.
//! The schema is the one documented in
//! `claude/agents/flow-bootstrap.md` (Contract section). The subcommand
//! is read-only and pure — no filesystem writes, no flow-state mutation.
//!
//! Field order in the emitted JSON matches the schema documentation
//! exactly: `command, flow_override, path_args, branch, worktree, cwd,
//! require_artifacts, staleness_threshold`. `serde_json` is built with
//! the `preserve_order` feature so this insertion order is the
//! on-wire order.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_compact;

/// Carrier commands accepted by `--command`. Matches the enumeration in
/// `claude/agents/flow-bootstrap.md`'s envelope schema.
const VALID_COMMANDS: &[&str] = &[
    "review",
    "optimise",
    "plan-new",
    "plan-update",
    "implement",
    "review-plan",
    "tdd",
    "review-apply",
    "optimise-apply",
    "test-bootstrap",
];

/// Artifact keys accepted by `--require-artifact`. Matches the canonical
/// artifact set on `flow-bootstrap`'s `require_artifacts` field.
const VALID_ARTIFACTS: &[&str] = &[
    "review_ledger",
    "optimise_findings",
    "execution_record",
    "plan_review_findings",
];

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    command: String,
    flow_override: Option<String>,
    path_args: Vec<String>,
    branch: Option<String>,
    worktree: Option<String>,
    cwd: Option<String>,
    require_artifacts: Vec<String>,
    staleness_threshold: String,
) -> Result<()> {
    if !VALID_COMMANDS.contains(&command.as_str()) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "invalid --command value '{}'; must be one of: {}",
                command,
                VALID_COMMANDS.join(", ")
            ),
        ));
    }
    for art in &require_artifacts {
        if !VALID_ARTIFACTS.contains(&art.as_str()) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "invalid --require-artifact value '{}'; must be one of: {}",
                    art,
                    VALID_ARTIFACTS.join(", ")
                ),
            ));
        }
    }

    // Build the envelope by hand (rather than via a #[derive(Serialize)]
    // struct) so the field order matches the schema documentation byte
    // for byte, and so `Option<String>` fields render as JSON `null`
    // rather than being omitted (which is the schema's contract).
    let envelope: JsonValue = json!({
        "command": command,
        "flow_override": flow_override,
        "path_args": path_args,
        "branch": branch,
        "worktree": worktree,
        "cwd": cwd,
        "require_artifacts": require_artifacts,
        "staleness_threshold": staleness_threshold,
    });
    print_json_compact(&envelope)
}
