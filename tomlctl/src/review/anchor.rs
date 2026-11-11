//! `review anchor` — whether a finding's anchor still holds in the working
//! tree, answered by exit code so a premise or accept check can ask it.
//!
//! A cleanup's `old` is matched exactly as `review add` matches it when it
//! accepts the row, so the verb and the write-time check cannot disagree. A
//! defect holds while its `file` exists and its symbol's last path segment
//! occurs in it, whole-word, as `items sweep` and `items orphans` test it.
//! `--gone` asks the opposite of a `remove` cleanup: that its `old` occurs
//! nowhere in the edit target file.

use std::path::Path;

use anyhow::Result;
use clap::Args;
use serde_json::json;

use super::add::{check_old, edit_target, occurrences, read_normalised};
use super::fields::{Direction, Kind};
use super::invalid;
use super::store::{self, Finding, Target};
use crate::cli::{ReadIntegrityArgs, ReviewStore};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::print_json_line;

#[derive(Args)]
pub(crate) struct ReviewAnchorArgs {
    #[arg(value_name = "ID", help = "Finding id (F<n>)")]
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "gone",
        help = "For a `remove` cleanup: exit 0 when `old` occurs nowhere in its edit target file"
    )]
    pub(crate) gone: bool,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

impl super::Verb for ReviewAnchorArgs {
    fn run(self) -> Result<()> {
        run(
            &super::store_target(&self.store)?,
            &self.id,
            self.gone,
            &self.integrity,
        )
    }
}

const VERB: &str = "review anchor";

pub(crate) fn run(
    target: &Target,
    id: &str,
    want_gone: bool,
    integrity: &ReadIntegrityArgs,
) -> Result<()> {
    let stored = store::load(&target.path, integrity)?;
    let id = id.trim();
    let finding = stored.findings.iter().find(|f| f.id == id).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("{VERB}: no finding {id} in this store"),
        )
    })?;
    let root = repo_or_cwd_root()?;
    let verdict = if want_gone {
        gone(&root, finding)?
    } else {
        holds(&root, finding)?
    };
    let (ok, reason) = match verdict {
        Ok(()) => (true, None),
        Err(reason) => (false, Some(reason)),
    };
    print_json_line(&json!({
        "ok": ok,
        "id": finding.id,
        "kind": finding.kind.as_str(),
        "gone": want_gone,
        "reason": reason,
    }))?;
    if !ok {
        // stdout is flushed by the print above, so skipping destructors loses nothing.
        std::process::exit(1);
    }
    Ok(())
}

/// The outer `Err` refuses the finding; the inner one says why its anchor
/// does not hold.
type Answer = Result<std::result::Result<(), String>>;

fn holds(root: &Path, finding: &Finding) -> Answer {
    match finding.kind {
        Kind::Cleanup => Ok(match finding.text("old").filter(|s| !s.is_empty()) {
            Some(old) => {
                let target = finding.text("target_file").filter(|s| !s.is_empty());
                let at = edit_target(root, &finding.file, target);
                check_old(root, at, old, finding.line)
            }
            None => file_exists(root, &finding.file),
        }),
        Kind::Defect => Ok(file_exists(root, &finding.file)
            .and_then(|()| symbol_occurs(root, &finding.file, &finding.symbol))),
        Kind::Advisory => Err(invalid(format!(
            "{VERB}: {} is an advisory, which has no anchor to check",
            finding.id
        ))),
    }
}

fn gone(root: &Path, finding: &Finding) -> Answer {
    let direction = finding.text("direction").and_then(Direction::parse);
    if finding.kind != Kind::Cleanup || direction != Some(Direction::Remove) {
        let what = match (finding.kind, direction) {
            (Kind::Cleanup, Some(d)) => format!("a {} cleanup", d.as_str()),
            (kind, _) => format!("a {}", kind.as_str()),
        };
        return Err(invalid(format!(
            "{VERB}: --gone applies only to a remove cleanup; {} is {what}, whose new text may keep `old`",
            finding.id
        )));
    }
    let Some(old) = finding.text("old").filter(|s| !s.is_empty()) else {
        return Err(invalid(format!(
            "{VERB}: --gone needs `old`; {} carries none",
            finding.id
        )));
    };
    let target = finding.text("target_file").filter(|s| !s.is_empty());
    let at = edit_target(root, &finding.file, target);
    let Some(text) = read_normalised(root, at) else {
        return Ok(Ok(()));
    };
    let count = occurrences(&text, &old.replace("\r\n", "\n")).len();
    Ok(if count == 0 {
        Ok(())
    } else {
        Err(format!("`old` still occurs {count} time(s) in {at}"))
    })
}

fn file_exists(root: &Path, rel: &str) -> std::result::Result<(), String> {
    if root.join(rel).is_file() {
        Ok(())
    } else {
        Err(format!("{rel} does not exist"))
    }
}

/// The text after the symbol's last `::`, `.` or `/`, or the whole symbol
/// when nothing follows that separator.
fn last_segment(symbol: &str) -> &str {
    let cut = [("::", 2), (".", 1), ("/", 1)]
        .into_iter()
        .filter_map(|(sep, len)| symbol.rfind(sep).map(|at| at + len))
        .max()
        .unwrap_or(0);
    let tail = &symbol[cut..];
    if tail.is_empty() { symbol } else { tail }
}

fn symbol_occurs(root: &Path, rel: &str, symbol: &str) -> std::result::Result<(), String> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return Ok(());
    }
    let segment = last_segment(symbol);
    let bytes = std::fs::read(root.join(rel)).map_err(|e| format!("{rel} cannot be read: {e}"))?;
    match crate::anchor::symbol_regex(segment) {
        Some(re) if re.is_match(&bytes) => Ok(()),
        _ => Err(format!("`{segment}` does not occur in {rel}")),
    }
}

#[cfg(test)]
mod tests {
    use super::last_segment;

    #[test]
    fn the_last_segment_follows_the_last_separator() {
        assert_eq!(last_segment("lib::limit"), "limit");
        assert_eq!(last_segment("Store.load"), "load");
        assert_eq!(last_segment("src/a::b.c"), "c");
        assert_eq!(last_segment("plain"), "plain");
        assert_eq!(last_segment("trailing::"), "trailing::");
    }
}
