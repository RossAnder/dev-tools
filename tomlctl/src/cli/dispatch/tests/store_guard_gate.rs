//! Every generic writer carries `--raw-store`, the bypass of the owned-store
//! guard; a writer without it was added without calling the guard at all.

use clap::{Command, CommandFactory as _};

use crate::cli::Cli;

/// `items` subcommands that take the write-integrity flags but are left
/// unguarded: `sweep --update` rewrites only the `instances` of a ledger's
/// open items.
const UNGUARDED_ITEMS: &[&str] = &["sweep"];

fn has_long(cmd: &Command, long: &str) -> bool {
    cmd.get_arguments().any(|a| a.get_long() == Some(long))
}

fn missing_raw_store(root: &Command) -> Vec<String> {
    let mut missing = Vec::new();
    for name in ["set", "set-json", "array-append"] {
        let cmd = root
            .find_subcommand(name)
            .unwrap_or_else(|| panic!("`{name}` is not a subcommand"));
        if !has_long(cmd, "raw-store") {
            missing.push(name.to_string());
        }
    }
    let items = root.find_subcommand("items").expect("`items` subcommand");
    for cmd in items.get_subcommands() {
        let name = cmd.get_name();
        if has_long(cmd, "no-write-integrity")
            && !UNGUARDED_ITEMS.contains(&name)
            && !has_long(cmd, "raw-store")
        {
            missing.push(format!("items {name}"));
        }
    }
    missing
}

#[test]
fn generic_writers_carry_the_owned_store_guard() {
    crate::test_support::on_cli_stack(|| {
        let root = Cli::command();
        let missing = missing_raw_store(&root);
        assert!(
            missing.is_empty(),
            "generic writers without `--raw-store` can write a verb-owned store unchecked — \
             flatten `RawStoreArg` into each and call `store_guard::refuse_owned_store` \
             before the write: {}",
            missing.join(", ")
        );
        let items = root.find_subcommand("items").expect("`items` subcommand");
        for name in UNGUARDED_ITEMS {
            assert!(
                items.find_subcommand(name).is_some(),
                "`items {name}` is allowlisted as unguarded but is no longer a subcommand"
            );
        }
    });
}
