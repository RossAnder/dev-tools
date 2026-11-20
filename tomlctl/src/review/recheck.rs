//! `review recheck-targets` — the plan targets a re-check of one run covers:
//! each `applied.target` a tool or user edit wrote and each `addressed` entry
//! an author's hand edit recorded, over the findings the run minted.

use std::collections::BTreeSet;

use anyhow::Result;
use clap::Args;
use serde_json::json;
use toml::Value as TomlValue;

use super::store::{self, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewStore};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_line;

#[derive(Args)]
pub(crate) struct ReviewRecheckTargetsArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN", help = "The plan run re-checked")]
    pub(crate) run: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

impl super::Verb for ReviewRecheckTargetsArgs {
    fn run(self) -> Result<()> {
        run(
            &super::store_target(&self.store)?,
            &self.run,
            &self.integrity,
        )
    }
}

const VERB: &str = "review recheck-targets";

pub(crate) fn run(target: &Target, run: &str, integrity: &ReadIntegrityArgs) -> Result<()> {
    let stored = store::load(&target.path, integrity)?;
    if !stored.runs.iter().any(|r| r.id == run) {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("{VERB}: no run {run} in this store"),
        ));
    }
    let targets: BTreeSet<&str> = recheck_targets(&stored, run).collect();
    print_json_line(&json!({"run": run, "targets": targets}))
}

/// Each non-empty `applied.target` and `addressed` entry on the findings run
/// `run` minted, unsorted and possibly repeated. `review run start` digests
/// the same set for a re-check, so the two cannot drift.
pub(super) fn recheck_targets<'a>(store: &'a Store, run: &'a str) -> impl Iterator<Item = &'a str> {
    store
        .findings
        .iter()
        .filter(move |finding| finding.run == run)
        .flat_map(|finding| {
            let applied = finding
                .rest
                .get("applied")
                .and_then(TomlValue::as_table)
                .and_then(|applied| applied.get("target"))
                .and_then(TomlValue::as_str);
            let addressed = finding
                .rest
                .get("addressed")
                .and_then(TomlValue::as_array)
                .into_iter()
                .flatten()
                .filter_map(TomlValue::as_str);
            applied.into_iter().chain(addressed)
        })
        .filter(|target| !target.is_empty())
}
