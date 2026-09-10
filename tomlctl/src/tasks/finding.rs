//! The shape every `tasks` diagnostic is reported in, and the severity
//! vocabulary `check::exit_code` grades it against.
//!
//! `check`, `import_plan` and `parse_tasks` each raise classes the others
//! cannot and `render` raises one of its own, so the type all four share lives
//! under none of them.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) class: &'static str,
    pub(crate) severity: &'static str,
    pub(crate) ids: Vec<u32>,
    pub(crate) detail: String,
}

pub(crate) const ERROR: &str = "error";
pub(crate) const WARNING: &str = "warning";
/// Below `WARNING` and outside `exit_code`'s test, so a class raised at this
/// severity can never move an exit status.
pub(crate) const INFO: &str = "info";

/// What an empty id list reads as mid-sentence, where a plan's `—` would
/// leave the clause around it truncated.
pub(crate) const NO_TASK: &str = "no task";

/// The three list formatters below take their empty marker rather than
/// choosing one: a plan bullet needs a marker the importer reads back as no
/// entries at all, and a diagnostic sentence one that reads as prose, so a
/// shared default would be wrong on one of the two surfaces.
pub(crate) fn join_ids(ids: &[u32], empty: &str) -> String {
    if ids.is_empty() {
        return empty.to_string();
    }
    ids.iter()
        .map(u32::to_string)
        .collect::<Vec<String>>()
        .join(", ")
}

/// `join_ids` under the noun, agreeing in number.
pub(crate) fn task_list(ids: &[u32], empty: &str) -> String {
    match ids.len() {
        0 => empty.to_string(),
        1 => format!("task {}", join_ids(ids, empty)),
        _ => format!("tasks {}", join_ids(ids, empty)),
    }
}

pub(crate) fn quoted_list<S: AsRef<str>>(values: &[S], empty: &str) -> String {
    if values.is_empty() {
        return empty.to_string();
    }
    values
        .iter()
        .map(|value| format!("`{}`", value.as_ref()))
        .collect::<Vec<String>>()
        .join(", ")
}
