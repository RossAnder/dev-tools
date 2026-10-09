//! Where a flow's execution record lives: the path its `context.toml` names
//! under `[artifacts].execution_record`, else `execution-record.toml` beside
//! that context. Every reader and writer of the record resolves it here, so
//! the containment rule on a recorded path is stated once.

use std::path::{Path, PathBuf};

use anyhow::Result;
use toml::Value as TomlValue;

use crate::errors::{ErrorKind, tagged_err};
use crate::io::recorded_under_root;

const RECORD_FILE: &str = "execution-record.toml";

/// The record `context` names, or `flow_dir`'s sibling file when it names
/// none (an absent table, key, or empty string). A recorded path is
/// file-controlled input, so one that does not anchor under `root` is a
/// `Validation` error rather than a path: resolving it would let the context
/// aim a read or a write anywhere.
pub(crate) fn execution_record_path(
    root: &Path,
    flow_dir: &Path,
    context_path: &Path,
    context: Option<&TomlValue>,
) -> Result<PathBuf> {
    let recorded = context
        .and_then(|context| context.get("artifacts"))
        .and_then(TomlValue::as_table)
        .and_then(|artifacts| artifacts.get("execution_record"))
        .and_then(TomlValue::as_str)
        .filter(|path| !path.is_empty());
    match recorded {
        Some(recorded) if !recorded_under_root(root, Path::new(recorded)) => Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "`execution_record` in `{}` must be repo-relative and stay under the repo \
                 root, got `{recorded}`",
                context_path.display()
            ),
        )),
        Some(recorded) => Ok(root.join(recorded)),
        None => Ok(flow_dir.join(RECORD_FILE)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use crate::test_support::with_root;

    const SLUG: &str = "whimsical-hugging-puppy";

    fn context(recorded: &str) -> TomlValue {
        toml::from_str(&format!("[artifacts]\nexecution_record = '{recorded}'\n"))
            .expect("the fixture context parses")
    }

    fn resolve(root: &Path, context: Option<&TomlValue>) -> Result<PathBuf> {
        let dir = root.join(".claude").join("flows").join(SLUG);
        execution_record_path(root, &dir, &dir.join("context.toml"), context)
    }

    fn refusal(root: &Path, recorded: &str) -> String {
        let err = resolve(root, Some(&context(recorded)))
            .expect_err("an unanchored record path is refused");
        let tagged = err
            .downcast_ref::<TaggedError>()
            .expect("the refusal is tagged");
        assert!(matches!(tagged.kind, ErrorKind::Validation), "{err}");
        let message = err.to_string();
        assert!(message.contains("execution_record"), "{message}");
        assert!(message.contains(recorded), "{message}");
        message
    }

    #[test]
    fn a_recorded_repo_relative_path_resolves_under_the_root() {
        with_root(|root| {
            let path = resolve(root, Some(&context(".claude/flows/x/record-2.toml")))
                .expect("a contained path resolves");
            assert_eq!(path, root.join(".claude/flows/x/record-2.toml"));
        });
    }

    #[test]
    fn an_absent_key_falls_back_to_the_sibling_record() {
        with_root(|root| {
            let sibling = root
                .join(".claude")
                .join("flows")
                .join(SLUG)
                .join(RECORD_FILE);
            let no_table: TomlValue = toml::from_str("plan_path = 'p.md'\n").unwrap();
            let no_key: TomlValue = toml::from_str("[artifacts]\nreview_ledger = 'r'\n").unwrap();
            for context in [None, Some(&no_table), Some(&no_key), Some(&context(""))] {
                assert_eq!(resolve(root, context).expect("fallback"), sibling);
            }
        });
    }

    #[test]
    fn a_traversing_path_is_refused() {
        with_root(|root| {
            refusal(root, "../x.toml");
            refusal(root, ".claude/../../x.toml");
        });
    }

    #[test]
    fn an_absolute_path_is_refused() {
        with_root(|root| {
            let absolute = std::env::temp_dir().join("x.toml");
            refusal(root, &absolute.to_string_lossy());
            refusal(root, "/etc/passwd");
        });
    }

    #[test]
    fn an_absolute_path_inside_the_root_is_still_refused() {
        with_root(|root| {
            refusal(root, &root.join("x.toml").to_string_lossy());
        });
    }
}
