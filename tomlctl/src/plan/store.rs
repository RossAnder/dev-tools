//! Store path resolution and locked load/mutate over a flow's `plan.toml`,
//! and the digest that decides whether a brief has changed.
//!
//! `mutate` converts through `schema::from_toml` / `schema::to_toml` inside
//! the `io::mutate_doc` closure, so the exclusive lock, the sidecar write and
//! the auto-create all apply unchanged. Every mutation restamps `last_updated`;
//! nothing else writes it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use sha2::{Digest, Sha256};

use super::schema::{self, PlanStore};
use crate::cli::{
    ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::integrity::hex_lower;
use crate::io::{
    mutate_doc, on_missing_for, read_doc, repo_or_cwd_root, strict_read_check, warn_if_created,
};

const STORE_FILE: &str = "plan.toml";

/// `<root>/.claude/flows/<slug>/plan.toml`. The flow-init slug rule admits
/// neither a separator nor a `..`, which is the only thing keeping `--slug`
/// from reaching outside `.claude/flows/`.
pub(crate) fn resolve(slug: &str) -> Result<PathBuf> {
    crate::flow::validate_slug(slug)?;
    Ok(repo_or_cwd_root()?
        .join(".claude")
        .join("flows")
        .join(slug)
        .join(STORE_FILE))
}

/// Read the store at `path`. Under `--verify-integrity` the sidecar check
/// runs first, under a shared lock, so a write mid-swap cannot be read as a
/// mismatch.
pub(crate) fn load(path: &Path, integrity: &ReadIntegrityArgs) -> Result<PlanStore> {
    strict_read_check(path, integrity.strict_read)?;
    read_doc(path, read_integrity_opts(integrity), schema::from_toml)
}

/// Run `f` over the parsed store inside the write pipeline's exclusive lock
/// and write the result back with `last_updated` restamped. `f` returning
/// `Err` persists nothing.
pub(crate) fn mutate<T>(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    f: impl FnOnce(&mut PlanStore) -> Result<T>,
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

/// SHA-256 of a brief with CRLF folded to LF, so that a checkout's line endings
/// never read as a change to intent. Lone CRs are content and stay.
pub(crate) fn brief_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    let mut rest = bytes;
    while let Some(at) = rest.windows(2).position(|pair| pair == b"\r\n") {
        hasher.update(&rest[..at]);
        hasher.update(b"\n");
        rest = &rest[at + 2..];
    }
    hasher.update(rest);
    hex_lower(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::super::schema::tests::fixture;
    use super::*;
    use crate::integrity::{sidecar_path, verify_integrity};
    use crate::test_support::with_root;

    const SLUG: &str = "whimsical-hugging-puppy";

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
            .join(SLUG)
            .join(STORE_FILE)
    }

    #[test]
    fn load_reads_back_every_record_mutate_wrote() {
        with_root(|root| {
            let path = store_path(root);
            let written = fixture();
            mutate(&path, &write_args(), |store| {
                *store = written.clone();
                Ok(())
            })
            .expect("the missing store is created");

            assert!(
                sidecar_path(&path).exists(),
                "the same write must leave a .sha256 sidecar"
            );
            verify_integrity(&path).expect("the sidecar matches the written bytes");

            let read = load(&path, &read_args(true)).expect("the store loads");
            let stamped = read.last_updated;
            assert_ne!(
                stamped, written.last_updated,
                "every mutation restamps last_updated"
            );
            assert_eq!(
                read,
                schema::PlanStore {
                    last_updated: stamped,
                    ..written
                }
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
    fn a_newer_store_is_left_untouched_by_a_mutation() {
        with_root(|root| {
            let path = store_path(root);
            std::fs::create_dir_all(path.parent().expect("a flow dir")).unwrap();
            let body = format!(
                "schema_version = {}\nfuture = true\n",
                schema::SCHEMA_VERSION + 1
            );
            std::fs::write(&path, &body).unwrap();

            let err = mutate(&path, &write_args(), |_store| Ok(()))
                .expect_err("a newer store is refused");
            assert!(format!("{err:#}").contains("upgrade tomlctl"), "{err:#}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        });
    }

    #[test]
    fn resolve_joins_the_flow_dir_and_refuses_traversal() {
        with_root(|root| {
            assert_eq!(resolve(SLUG).expect("slug resolves"), store_path(root));
            for bad in ["..", "../escape", "a/b", "a\\b", "", "Upper", "-lead"] {
                assert!(resolve(bad).is_err(), "slug `{bad}` must be refused");
            }
        });
    }

    #[test]
    fn line_endings_alone_do_not_change_a_brief_digest() {
        let lf = b"# Intent\n\nStop the retry storm.\n";
        let crlf = b"# Intent\r\n\r\nStop the retry storm.\r\n";
        assert_eq!(brief_digest(lf), brief_digest(crlf));
        assert_eq!(brief_digest(lf), hex_lower(&Sha256::digest(lf)));
    }

    #[test]
    fn a_changed_word_changes_a_brief_digest() {
        assert_ne!(
            brief_digest(b"# Intent\r\nStop the retry storm.\r\n"),
            brief_digest(b"# Intent\r\nStart the retry storm.\r\n")
        );
        assert_ne!(brief_digest(b"a\rb"), brief_digest(b"a\nb"));
    }
}
