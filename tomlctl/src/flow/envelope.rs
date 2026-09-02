//! `tomlctl flow envelope build` — emit the canonical `flow-bootstrap`
//! input envelope as JSON on stdout, in place of the inline carrier prose
//! each flow command's Step-0 dispatch would otherwise hand-roll.
//!
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

    // SECURITY: each `path_args` entry is emitted verbatim into the
    // JSON envelope that the flow-bootstrap sub-agent consumes as its
    // prompt, so untrusted `$ARGUMENTS` path tokens must be sanitised
    // before they cross that boundary. Reject `..` traversal segments,
    // absolute paths, and absurdly long tokens (length cap 512).
    const MAX_PATH_ARG_LEN: usize = 512;
    for p in &path_args {
        if p.len() > MAX_PATH_ARG_LEN {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "invalid --path-arg value (len {}); must be at most {} chars",
                    p.len(),
                    MAX_PATH_ARG_LEN
                ),
            ));
        }
        // Path-traversal: reject any `..` component. Check both `/` and `\`
        // separators so a Windows-style token can't sneak a parent ref past
        // a Unix-only split.
        let has_dotdot = p.split(['/', '\\']).any(|seg| seg == "..");
        if has_dotdot {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "invalid --path-arg value '{}'; must not contain '..' path-traversal segments",
                    p
                ),
            ));
        }
        // Absolute paths: Unix `/foo`, Windows `\foo` / `C:\foo` / `C:/foo`.
        let is_absolute = p.starts_with('/')
            || p.starts_with('\\')
            || p.as_bytes().get(1).is_some_and(|&b| b == b':');
        if is_absolute {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "invalid --path-arg value '{}'; must be a repo-relative path, not absolute",
                    p
                ),
            ));
        }
    }

    // SECURITY: `staleness_threshold` is echoed verbatim into the
    // envelope; pin it to the documented grammar `<n>{s|m|h|d|w}` so a
    // freeform token can't ride through. Manual char scan rather than a
    // `regex::Regex` build — the grammar is trivial and this avoids
    // pulling the dependency into a hot, single-shot path.
    {
        let bytes = staleness_threshold.as_bytes();
        let valid = bytes.len() >= 2
            && bytes[..bytes.len() - 1].iter().all(u8::is_ascii_digit)
            && matches!(bytes[bytes.len() - 1], b's' | b'm' | b'h' | b'd' | b'w');
        if !valid {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "invalid --staleness-threshold value '{}'; must match <n>{{s|m|h|d|w}} (e.g. 7d)",
                    staleness_threshold
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
        // `cwd` is a canonical schema field here, but
        // `claude/agents/flow-bootstrap.md` treats it as a tolerated-and-
        // ignored legacy extra. The two contracts disagree: the field is
        // emitted and the consumer ignores it.
        "cwd": cwd,
        "require_artifacts": require_artifacts,
        "staleness_threshold": staleness_threshold,
    });
    print_json_compact(&envelope)
}
