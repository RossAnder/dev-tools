//! Refusal of generic writes (`set`, `set-json`, `array-append`, the `items`
//! writers) aimed at a store that a dedicated verb group owns and validates.
//! The target is classified by the trailing components of its resolved path,
//! so `..` segments, a junctioned or symlinked directory and an 8.3 short name
//! resolve to the store they reach. The file and its parents need not exist:
//! an auto-create target is classified too.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use crate::errors::{ErrorKind, tagged_err};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnedStore {
    Backlog,
    Inputs,
    Tasks,
    ExecutionRecord,
    Review,
}

impl OwnedStore {
    fn replacement(self) -> &'static str {
        match self {
            Self::Backlog => {
                "the `backlog` verbs own it: use `backlog edit` for context, tags, evidence or \
                 origin, and `backlog triage` for status"
            }
            Self::Inputs => {
                "the `inputs` verbs own it: use `inputs add`, `inputs ack`, `inputs handle`, \
                 `inputs answer` or `inputs withdraw`"
            }
            Self::Tasks => {
                "the `tasks` verbs own it: use `tasks update` or `tasks add` for rows, and \
                 `tasks import-plan` for `[[checkpoints]]`, which no other verb writes"
            }
            Self::ExecutionRecord => "`flow record` owns it: append entries with `flow record`",
            Self::Review => {
                "the `review` verbs own it: write it with `review add`, `review verdict`, \
                 `review resolve` and the other `review` verbs"
            }
        }
    }
}

/// Refuses a generic write to one of the verb-owned stores unless
/// `raw_store` is set. Never fails for any other path, including one that
/// cannot be resolved.
pub(crate) fn refuse_owned_store(path: &Path, raw_store: bool) -> Result<()> {
    if raw_store {
        return Ok(());
    }
    let Some(store) = classify(&resolve(path)) else {
        return Ok(());
    };
    Err(tagged_err(
        ErrorKind::Validation,
        Some(path.to_path_buf()),
        format!(
            "{} is a verb-owned store — {}; pass --raw-store to write it generically",
            path.display(),
            store.replacement()
        ),
    ))
}

/// The path a write to `path` reaches: the deepest existing ancestor
/// canonicalised, with the components below it folded lexically.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut below: Vec<Component<'_>> = Vec::new();
    let mut ancestor = absolute.as_path();
    loop {
        if let Ok(base) = ancestor.canonicalize() {
            return fold(base, below.iter().rev().copied());
        }
        let (Some(parent), Some(last)) = (ancestor.parent(), ancestor.components().next_back())
        else {
            break;
        };
        below.push(last);
        ancestor = parent;
    }
    fold(PathBuf::new(), absolute.components())
}

/// Appends `components` to `base`, dropping `.` and letting `..` pop the
/// component before it.
fn fold<'a>(mut base: PathBuf, components: impl Iterator<Item = Component<'a>>) -> PathBuf {
    for component in components {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !base.pop() {
                    base.push(component);
                }
            }
            other => base.push(other),
        }
    }
    base
}

fn classify(resolved: &Path) -> Option<OwnedStore> {
    let names: Vec<String> = resolved
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(normalise(name)),
            _ => None,
        })
        .collect();
    let tail = |n: usize| names.len().checked_sub(n).map(|start| &names[start..]);
    if let Some([claude, file]) = tail(2) {
        if same(claude, ".claude") && same(file, "backlog.toml") {
            return Some(OwnedStore::Backlog);
        }
        if same(claude, ".claude") && same(file, "inputs.toml") {
            return Some(OwnedStore::Inputs);
        }
    }
    if let Some([claude, dir, file]) = tail(3)
        && same(claude, ".claude")
        && same(dir, "review-v2")
        && Path::new(file)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
    {
        return Some(OwnedStore::Review);
    }
    if let Some([claude, flows, _slug, file]) = tail(4)
        && same(claude, ".claude")
        && same(flows, "flows")
    {
        if same(file, "tasks.toml") {
            return Some(OwnedStore::Tasks);
        }
        if same(file, "execution-record.toml") {
            return Some(OwnedStore::ExecutionRecord);
        }
        if same(file, "review-v2.toml") {
            return Some(OwnedStore::Review);
        }
    }
    None
}

/// Windows opens `name:stream` on the file `name` and drops trailing dots and
/// spaces from a component, so `tasks.toml.` and `tasks.toml::$DATA` reach
/// `tasks.toml`; both are cut back to the name the write lands on.
fn normalise(name: &OsStr) -> String {
    let name = name.to_string_lossy();
    if !cfg!(windows) {
        return name.into_owned();
    }
    let name = name.split(':').next().unwrap_or_default();
    name.trim_end_matches(['.', ' ']).to_string()
}

fn same(name: &str, expected: &str) -> bool {
    if cfg!(windows) {
        name.eq_ignore_ascii_case(expected)
    } else {
        name == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(path: &str) -> bool {
        refuse_owned_store(Path::new(path), false).is_err()
    }

    #[test]
    fn store_guard_refuses_each_owned_store() {
        for path in [
            ".claude/backlog.toml",
            ".claude/inputs.toml",
            ".claude/flows/x/tasks.toml",
            ".claude/flows/x/execution-record.toml",
            ".claude/flows/x/review-v2.toml",
            ".claude/review-v2/scope.toml",
        ] {
            assert!(refused(path), "{path} should be refused");
        }
    }

    #[test]
    fn store_guard_refuses_through_parent_segments() {
        assert!(refused(".claude/flows/x/../x/tasks.toml"));
        assert!(refused(".claude/flows/x/./tasks.toml"));
        assert!(refused("unrelated/../.claude/backlog.toml"));
    }

    #[test]
    fn store_guard_refuses_a_target_whose_parents_exist() {
        let dir = tempfile::tempdir().unwrap();
        let flow = dir.path().join(".claude").join("flows").join("x");
        std::fs::create_dir_all(&flow).unwrap();
        assert!(refuse_owned_store(&flow.join("tasks.toml"), false).is_err());
        let existing = flow.join("execution-record.toml");
        std::fs::write(&existing, "").unwrap();
        assert!(refuse_owned_store(&existing, false).is_err());
        let detour = flow.join("..").join("x").join("execution-record.toml");
        assert!(refuse_owned_store(&detour, false).is_err());
    }

    #[test]
    fn store_guard_fold_pops_parent_segments() {
        let folded = fold(
            PathBuf::from("base"),
            Path::new("flows/x/../y/./tasks.toml").components(),
        );
        assert_eq!(folded, Path::new("base/flows/y/tasks.toml"));
        let above = fold(PathBuf::new(), Path::new("../tasks.toml").components());
        assert_eq!(above, Path::new("../tasks.toml"));
    }

    #[cfg(windows)]
    #[test]
    fn store_guard_refuses_windows_aliases() {
        assert!(refused(".claude/FLOWS/x/Tasks.TOML"));
        assert!(refused(".claude/backlog.toml."));
        assert!(refused(".claude/backlog.toml::$DATA"));
    }

    #[test]
    fn store_guard_raw_store_allows() {
        for path in [".claude/backlog.toml", ".claude/flows/x/tasks.toml"] {
            assert!(refuse_owned_store(Path::new(path), true).is_ok());
        }
    }

    #[test]
    fn store_guard_allows_unowned_paths() {
        for path in [
            ".claude/flows/x/context.toml",
            ".claude/flows/x/review-ledger.toml",
            ".claude/execution-record.toml",
            ".claude/tasks.toml",
            ".claude/flows/tasks.toml",
            ".claude/review-v2/scope.json",
            "backlog.toml",
        ] {
            assert!(!refused(path), "{path} should be allowed");
        }
    }

    #[test]
    fn store_guard_message_names_the_replacement_and_bypass() {
        let err = refuse_owned_store(Path::new(".claude/flows/x/tasks.toml"), false).unwrap_err();
        let tagged = err
            .downcast_ref::<crate::errors::TaggedError>()
            .expect("tagged");
        assert!(matches!(tagged.kind, ErrorKind::Validation));
        let message = format!("{err:#}");
        assert!(message.contains("tasks update"), "{message}");
        assert!(message.contains("tasks import-plan"), "{message}");
        assert!(
            message.contains("pass --raw-store to write it generically"),
            "{message}"
        );
        let err = refuse_owned_store(Path::new(".claude/backlog.toml"), false).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("backlog edit"), "{message}");
        assert!(message.contains("backlog triage"), "{message}");
    }
}
