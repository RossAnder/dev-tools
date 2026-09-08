//! Store path resolution and locked load/mutate over a flow's `tasks.toml`.
//!
//! `mutate` converts through `schema::from_toml` / `schema::to_toml` inside
//! the `io::mutate_doc` closure, so the exclusive lock, the sidecar write and
//! the auto-create-from-seed all apply to the task store unchanged. Every
//! mutation restamps `last_updated`; nothing else writes it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;

use super::schema::{self, Store};
use crate::cli::{
    ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{mutate_doc, on_missing_for, read_doc, repo_or_cwd_root, warn_if_created};

/// Basename of the per-flow store, and the key `io::seed_doc_for` matches on.
const STORE_FILE: &str = "tasks.toml";

/// Anchored, ASCII-only, and identical to the flow-init slug rule: a slug
/// that satisfies it can contribute neither a separator nor a `..` to the
/// join below, which is the only thing keeping `--slug` from reaching outside
/// `.claude/flows/`.
fn slug_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").expect("slug regex compiles"))
}

/// Resolve the store a `tasks` verb targets. `--file` passes through
/// verbatim — the write guard still holds it under `.claude/` unless
/// `--allow-outside` — and a slug resolves to
/// `<root>/.claude/flows/<slug>/tasks.toml` only after the slug check.
pub(crate) fn resolve_store_path(slug: Option<&str>, file: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = file {
        return Ok(path.to_path_buf());
    }
    let Some(slug) = slug else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "no task store target: pass --slug <SLUG> or --file <PATH>".to_string(),
        ));
    };
    if !slug_regex().is_match(slug) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid slug: {slug} (must match ^[a-z0-9][a-z0-9-]{{0,63}}$)"),
        ));
    }
    Ok(repo_or_cwd_root()?
        .join(".claude")
        .join("flows")
        .join(slug)
        .join(STORE_FILE))
}

/// Read the store at `path`. Under `--verify-integrity` the sidecar check
/// runs first, under a shared lock, so a write mid-swap cannot be read as a
/// mismatch.
pub(crate) fn load(path: &Path, integrity: &ReadIntegrityArgs) -> Result<Store> {
    read_doc(path, read_integrity_opts(integrity), schema::from_toml)
}

/// Run `f` over the parsed store inside the write pipeline's exclusive lock
/// and write the result back with `last_updated` restamped. `f` returning
/// `Err` persists nothing, so a validation that fails mid-mutation leaves the
/// file and its sidecar untouched.
pub(crate) fn mutate<T>(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    f: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    let today = crate::time::today_toml_date()?;
    let on_missing = on_missing_for(path, integrity.no_create)?;
    let mut outcome = None;
    let created = mutate_doc(
        path,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        on_missing,
        |doc| {
            let mut store = schema::from_toml(doc)?;
            outcome = Some(f(&mut store)?);
            store.last_updated = Some(today);
            *doc = schema::to_toml(&store);
            Ok(())
        },
    )?;
    warn_if_created(path, created);
    Ok(outcome.expect("mutate_doc reports success only after running the closure"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{sidecar_path, verify_integrity};
    use crate::io::read_toml;
    use crate::test_support::with_root;
    use toml::Value as TomlValue;

    fn read_args(verify: bool) -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: verify,
            strict_read: false,
        }
    }

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude")
            .join("flows")
            .join("whimsical-hugging-puppy")
            .join(STORE_FILE)
    }

    #[test]
    fn mutate_seeds_a_missing_store_and_writes_its_sidecar() {
        with_root(|root| {
            let path = store_path(root);
            assert!(!path.exists(), "precondition: the store must not exist");

            // The seed only reaches the store because `tasks.toml` is in
            // `io`'s schema-seeded set; drop it there and this is a bare `{}`.
            let seed = crate::io::seed_doc_for(&path).expect("the seed builds");
            assert_eq!(
                seed.get("schema_version").and_then(TomlValue::as_integer),
                Some(1),
                "tasks.toml must seed schema_version = 1, got: {seed:?}"
            );
            assert!(
                seed.get("last_updated")
                    .is_some_and(|v| v.as_datetime().is_some()),
                "tasks.toml must seed a `last_updated` date, got: {seed:?}"
            );

            let plan = mutate(&path, &write_args(), |store| {
                store.plan_path = "docs/plans/whimsical-hugging-puppy.md".to_string();
                Ok(store.plan_path.clone())
            })
            .expect("the missing store is created");
            assert_eq!(plan, "docs/plans/whimsical-hugging-puppy.md");

            let on_disk = read_toml(&path).expect("the written store parses");
            assert_eq!(
                on_disk
                    .get("schema_version")
                    .and_then(TomlValue::as_integer),
                Some(1),
                "the seeded schema_version must survive the round-trip"
            );

            assert!(
                sidecar_path(&path).exists(),
                "the same write must leave a .sha256 sidecar"
            );
            verify_integrity(&path).expect("the sidecar matches the written bytes");
        });
    }

    #[test]
    fn load_reads_back_what_mutate_wrote() {
        with_root(|root| {
            let path = store_path(root);
            mutate(&path, &write_args(), |store| {
                store.last_import_refs = vec!["seed-the-store".to_string()];
                Ok(())
            })
            .expect("the store is written");

            let store = load(&path, &read_args(true)).expect("the store loads");
            assert_eq!(store.last_import_refs, vec!["seed-the-store".to_string()]);
            assert!(
                store.last_updated.is_some(),
                "every mutation restamps last_updated"
            );
        });
    }

    #[test]
    fn a_failing_closure_leaves_no_store_behind() {
        with_root(|root| {
            let path = store_path(root);
            let err = mutate(&path, &write_args(), |_store| {
                Err::<(), _>(anyhow::anyhow!("refused"))
            })
            .expect_err("the closure's error propagates");
            assert!(err.to_string().contains("refused"), "{err}");
            assert!(!path.exists(), "a refused mutation must persist nothing");
        });
    }

    #[test]
    fn resolve_store_path_joins_the_flow_dir_and_refuses_traversal() {
        with_root(|root| {
            assert_eq!(
                resolve_store_path(Some("whimsical-hugging-puppy"), None).expect("slug resolves"),
                store_path(root)
            );

            for bad in ["..", "../escape", "a/b", "a\\b", "", "Upper", "-lead"] {
                assert!(
                    resolve_store_path(Some(bad), None).is_err(),
                    "slug `{bad}` must be refused"
                );
            }

            let explicit = root.join("elsewhere").join("tasks.toml");
            assert_eq!(
                resolve_store_path(None, Some(&explicit)).expect("an explicit path passes through"),
                explicit
            );
            assert!(
                resolve_store_path(None, None).is_err(),
                "an empty target must be refused"
            );
        });
    }
}
