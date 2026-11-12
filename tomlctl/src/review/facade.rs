//! Library reads of a v2 review store, for consumers that link tomlctl rather
//! than spawn it. Each read goes through the projections the verbs print.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use serde_json::Value as JsonValue;

use super::fields::Role;
use super::invalid;
use super::store::{self, View};
use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};

/// Finding `id` of the review store at the repo-relative `store` under
/// `root`, as `review show --for report` prints it, read without integrity
/// checks. A store path that is absolute, holds `..` or resolves outside
/// `<root>/.claude` is refused.
pub(crate) fn finding(root: &Path, store: &str, id: &str) -> Result<JsonValue> {
    let path = resolve_store(root, store)?;
    let read_opts = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    let stored = store::load(&path, &read_opts)?;
    let index = stored
        .findings
        .iter()
        .position(|f| f.id == id)
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::NotFound,
                Some(path.clone()),
                format!("review store {store}: no finding {id}"),
            )
        })?;
    let row = View::new(&stored).project(Role::Report, index, true);
    Ok(JsonValue::Object(row))
}

/// The real path of `store` under `root`, refused unless it names a file
/// below `<root>/.claude`; an absent store is the review group's not-found
/// error.
fn resolve_store(root: &Path, store: &str) -> Result<PathBuf> {
    let relative = Path::new(store);
    let escapes = relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if store.is_empty() || escapes {
        return Err(invalid(format!(
            "review store {store:?}: expected a relative path under .claude without `..`"
        )));
    }
    let joined = root.join(relative);
    let resolved = match joined.canonicalize() {
        Ok(resolved) => resolved,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(store::not_found(&joined));
        }
        Err(err) => return Err(err.into()),
    };
    let claude = root
        .join(".claude")
        .canonicalize()
        .map_err(|_| store::not_found(&joined))?;
    if resolved == claude || !resolved.starts_with(&claude) {
        return Err(invalid(format!(
            "review store {store:?} resolves outside .claude"
        )));
    }
    Ok(resolved)
}
