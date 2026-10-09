//! Where a flow's execution record lives: the path its `context.toml` names
//! under `[artifacts].execution_record`, else `execution-record.toml` beside
//! that context. Every reader and writer of the record resolves it here, so
//! the containment rule on a recorded path is stated once. The one exception
//! is the task snapshot, which fingerprints the flow dir's fixed file set and
//! so reads the sibling file whatever the context names.

use std::path::{Path, PathBuf};

use anyhow::Result;
use toml::Value as TomlValue;

use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_toml, recorded_under_root};

const RECORD_FILE: &str = "execution-record.toml";
const CONTEXT_FILE: &str = "context.toml";

/// The record of the flow at `flow_dir`, read from that dir's `context.toml`.
/// An absent or unparseable context resolves as one naming no record, so a
/// reader still lands on the sibling file; an escaping recorded path is
/// refused as [`execution_record_path`] refuses it.
pub(crate) fn flow_execution_record_path(root: &Path, flow_dir: &Path) -> Result<PathBuf> {
    let context_path = flow_dir.join(CONTEXT_FILE);
    let context = read_toml(&context_path).ok();
    execution_record_path(root, flow_dir, &context_path, context.as_ref())
}

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

/// The skeleton a missing execution record is seeded with, chosen by kind
/// rather than by the file's name: a record the context names under another
/// basename still starts as the two-line skeleton `flow init` writes.
pub(crate) fn execution_record_seed() -> Result<TomlValue> {
    crate::io::schema_seed()
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
    fn the_flow_dir_form_reads_the_record_its_context_names() {
        with_root(|root| {
            let dir = root.join(".claude").join("flows").join(SLUG);
            std::fs::create_dir_all(&dir).unwrap();
            assert_eq!(
                flow_execution_record_path(root, &dir).expect("no context"),
                dir.join(RECORD_FILE)
            );
            std::fs::write(dir.join(CONTEXT_FILE), "not = [valid").unwrap();
            assert_eq!(
                flow_execution_record_path(root, &dir).expect("unparseable context"),
                dir.join(RECORD_FILE)
            );
            std::fs::write(
                dir.join(CONTEXT_FILE),
                "[artifacts]\nexecution_record = '.claude/flows/x/record-2.toml'\n",
            )
            .unwrap();
            assert_eq!(
                flow_execution_record_path(root, &dir).expect("a named record"),
                root.join(".claude/flows/x/record-2.toml")
            );
        });
    }

    #[test]
    fn the_seed_is_the_schema_skeleton() {
        let seed = execution_record_seed().expect("the seed builds");
        let keys: Vec<&str> = seed
            .as_table()
            .expect("a table")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["schema_version", "last_updated"]);
        assert_eq!(seed["schema_version"].as_integer(), Some(1));
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
