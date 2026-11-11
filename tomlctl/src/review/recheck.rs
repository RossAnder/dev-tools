//! `review recheck-targets` — the plan targets a re-check of one run covers:
//! each `applied.target` a tool or user edit wrote and each `addressed` entry
//! an author's hand edit recorded, over the findings the run minted.

use std::collections::BTreeSet;

use anyhow::Result;
use clap::Args;
use serde_json::json;
use toml::Value as TomlValue;

use super::store::{self, Target};
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
    let mut targets: BTreeSet<&str> = BTreeSet::new();
    for finding in stored.findings.iter().filter(|f| f.run == run) {
        let applied = finding
            .rest
            .get("applied")
            .and_then(TomlValue::as_table)
            .and_then(|a| a.get("target"))
            .and_then(TomlValue::as_str);
        let addressed = finding
            .rest
            .get("addressed")
            .and_then(TomlValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(TomlValue::as_str);
        targets.extend(
            applied
                .into_iter()
                .chain(addressed)
                .filter(|t| !t.is_empty()),
        );
    }
    print_json_line(&json!({"run": run, "targets": targets}))
}
