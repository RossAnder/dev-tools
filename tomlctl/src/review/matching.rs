//! Anchor candidates: the stored findings a new row might restate. Within a
//! family, a row with a symbol matches findings on the same `(file, symbol)`;
//! one without matches findings in the same file within [`LINE_WINDOW`]
//! lines. `lens` and `kind` are not part of the key. A candidate never
//! decides identity — only a `same_as` naming one does.

use super::fields::{Family, FindingStatus};
use super::store::Finding;

/// How far apart two symbol-less rows' lines may be and still match.
pub(crate) const LINE_WINDOW: u32 = 10;

/// The location a row is matched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Site<'a> {
    pub(crate) family: Family,
    pub(crate) file: &'a str,
    pub(crate) symbol: &'a str,
    pub(crate) line: Option<u32>,
}

/// The stored `anchor` key: `<family>|<file>|<symbol>`, or `@<line>` in the
/// symbol's place when the row has none.
pub(crate) fn anchor(site: &Site<'_>) -> String {
    let family = site.family.as_str();
    let file = site.file;
    match (site.symbol, site.line) {
        ("", Some(line)) => format!("{family}|{file}|@{line}"),
        (symbol, _) => format!("{family}|{file}|{symbol}"),
    }
}

/// The findings `site` matches, in store order. Superseded and merged rows
/// are skipped: the first is withdrawn, and the second's identity already
/// lives in its `merged_into` target.
pub(crate) fn candidates<'s>(findings: &'s [Finding], site: &Site<'_>) -> Vec<&'s Finding> {
    findings
        .iter()
        .filter(|f| {
            !matches!(
                f.status(),
                FindingStatus::Superseded | FindingStatus::Merged
            )
        })
        .filter(|f| f.family == site.family && f.file == site.file)
        .filter(|f| matches_at(f, site))
        .collect()
}

fn matches_at(finding: &Finding, site: &Site<'_>) -> bool {
    if !site.symbol.is_empty() {
        return finding.symbol == site.symbol;
    }
    match (site.line, finding.line) {
        (Some(want), Some(have)) => want.abs_diff(have) <= LINE_WINDOW,
        // A row with neither symbol nor line can only restate another such row.
        (None, None) => finding.symbol.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::fields::Kind;

    fn finding(id: &str, family: Family, file: &str, symbol: &str, line: Option<u32>) -> Finding {
        Finding {
            id: id.to_string(),
            run: "U1".to_string(),
            cell: "C1".to_string(),
            seen_in: vec!["U1".to_string()],
            family,
            kind: Kind::Defect,
            lens: "correctness".to_string(),
            route: None,
            file: file.to_string(),
            line,
            symbol: symbol.to_string(),
            summary: String::new(),
            same_as: String::new(),
            depends_on: Vec::new(),
            related: Vec::new(),
            anchor: String::new(),
            outcome: String::new(),
            status: FindingStatus::Open,
            first_seen: None,
            rest: Default::default(),
        }
    }

    fn site<'a>(family: Family, file: &'a str, symbol: &'a str, line: Option<u32>) -> Site<'a> {
        Site {
            family,
            file,
            symbol,
            line,
        }
    }

    fn ids(found: Vec<&Finding>) -> Vec<&str> {
        found.into_iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn candidates_prefer_symbol_over_line_window() {
        let store = [
            finding("F1", Family::Code, "src/a.rs", "a::run", Some(500)),
            finding("F2", Family::Code, "src/a.rs", "a::other", Some(101)),
            finding("F3", Family::Code, "src/a.rs", "", Some(100)),
        ];
        let probe = site(Family::Code, "src/a.rs", "a::run", Some(100));
        assert_eq!(ids(candidates(&store, &probe)), vec!["F1"]);
        assert_eq!(anchor(&probe), "code|src/a.rs|a::run");

        let symbol_less = site(Family::Code, "src/a.rs", "", Some(100));
        assert_eq!(ids(candidates(&store, &symbol_less)), vec!["F2", "F3"]);
        assert_eq!(anchor(&symbol_less), "code|src/a.rs|@100");
    }

    #[test]
    fn candidates_within_ten_lines() {
        let store = [
            finding("F1", Family::Code, "src/a.rs", "", Some(40)),
            finding("F2", Family::Code, "src/a.rs", "", Some(60)),
            finding("F3", Family::Code, "src/a.rs", "", Some(39)),
            finding("F4", Family::Code, "src/a.rs", "", Some(61)),
            finding("F5", Family::Code, "src/b.rs", "", Some(50)),
            finding("F6", Family::Code, "src/a.rs", "", None),
        ];
        let probe = site(Family::Code, "src/a.rs", "", Some(50));
        assert_eq!(ids(candidates(&store, &probe)), vec!["F1", "F2"]);

        let line_less = site(Family::Code, "src/a.rs", "", None);
        assert_eq!(ids(candidates(&store, &line_less)), vec!["F6"]);
    }

    #[test]
    fn candidates_partition_by_family() {
        let mut withdrawn = finding("F4", Family::Code, "src/a.rs", "a::run", None);
        withdrawn.set_status(FindingStatus::Superseded, &[]);
        let mut merged = finding("F5", Family::Code, "src/a.rs", "a::run", None);
        merged.set_status(FindingStatus::Merged, &[]);
        let store = [
            finding("F1", Family::Code, "src/a.rs", "a::run", None),
            finding("F2", Family::Perf, "src/a.rs", "a::run", None),
            finding("F3", Family::Plan, "src/a.rs", "a::run", None),
            withdrawn,
            merged,
        ];
        for (family, want) in [
            (Family::Code, "F1"),
            (Family::Perf, "F2"),
            (Family::Plan, "F3"),
        ] {
            let probe = site(family, "src/a.rs", "a::run", None);
            assert_eq!(ids(candidates(&store, &probe)), vec![want], "{family:?}");
        }
    }
}
