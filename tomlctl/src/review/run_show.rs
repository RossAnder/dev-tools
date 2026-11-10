//! `review run show` — one run's bounds, signals, baselines, waves, agents
//! and audit, read back from the store.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::catalogue::meets;
use super::store::{self, Run, Signal, Target};
use crate::cli::ReviewRunShowArgs;
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::output::{Rows, print_report};

pub(crate) fn show(target: &Target, args: ReviewRunShowArgs) -> Result<()> {
    let stored = store::load(&target.path, &args.integrity)?;
    let run = stored
        .runs
        .iter()
        .find(|r| r.id == args.run)
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::NotFound,
                Some(target.path.clone()),
                format!("review run show: no run {} in this store", args.run),
            )
        })?;
    let rest = |key: &str| run.rest.get(key).map(toml_to_json);
    let signals: Vec<JsonValue> = run.signals.iter().map(|s| signal(s, run)).collect();
    let report = json!({
        "ok": true,
        "run": run.id,
        "base": (!run.base.is_empty()).then_some(&run.base),
        "head": run.head,
        "scope": run.scope,
        "depth": run.depth.as_str(),
        "baselines": rest("baselines").unwrap_or_else(|| json!({})),
        "waves": super::stats::run_waves(&stored.cells, &run.id),
        "agents": run.agents,
        "audit": rest("audit"),
        "signals": signals,
    });
    print_report(report, Rows::Field("signals"))
}

fn signal(signal: &Signal, run: &Run) -> JsonValue {
    let (kept, outside): (Vec<&String>, Vec<&String>) = signal
        .roots
        .iter()
        .partition(|root| run.scope.is_empty() || run.scope.iter().any(|e| meets(e, root)));
    json!({
        "name": signal.name,
        "source": signal.source,
        "roots": kept,
        "roots_outside_scope": outside.len(),
        "accepted": signal.accepted,
        "evidence": signal.evidence,
    })
}
