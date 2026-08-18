//! The `subagentStatusLine` stdin payload.
//!
//! Claude Code passes every visible agent-panel row as one JSON object and
//! expects NDJSON back: one `{"id":…,"content":…}` per row to override, the id
//! omitted to keep a row's default rendering, an empty `content` to hide it.
//!
//! `columns` is already the *usable* body width — Claude Code subtracts the
//! pointer/bullet/tree gutter before handing it over — and it renders `content`
//! with `wrap: "truncate"`. So the width budget is ours to spend, and spending
//! it deliberately is the whole point of overriding the row.
//!
//! **Observed against Claude Code 2.1.234, 2026-08-18.** The field set below is
//! documented upstream; the *behaviour* recorded in these doc comments is not.
//! `label` being `progressSummary || description`, `tokenSamples` capping at 16,
//! `columns` arriving already net of the gutter, and the `type`/`status`
//! vocabularies were all read off that build. Every one of them degrades
//! silently: a renamed or retired field simply stops binding and reads `None`,
//! and a changed cap or fallback changes what rows say with nothing anywhere to
//! announce it. This line exists to give a future reader a diff anchor — the
//! build to compare against when a row starts showing less than it used to.

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SubagentPayload {
    pub columns: Option<usize>,
    pub tasks: Vec<Task>,
    /// Base hook field, snake_case unlike the camelCase task entries. Locates
    /// the sibling `subagents/` directory holding per-teammate metadata.
    pub transcript_path: Option<String>,
}

// The subagent wire uses camelCase throughout (`startTime`, `tokenCount`,
// `contextWindowSize`) unlike the snake_case main statusline payload.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// Teammate call-sign from the agent-name registry. Absent for plain
    /// subagents, bash tasks and workflows.
    pub name: Option<String>,
    /// `local_agent` | `local_bash` | `local_workflow` | `remote_agent` |
    /// `in_process_teammate`.
    pub r#type: Option<String>,
    /// `pending` | `running` | `paused` | `completed` | `failed` | `killed`.
    pub status: Option<String>,
    pub description: Option<String>,
    /// Live progress summary / command line / workflow name — whatever the row
    /// is currently *doing*. When the task has a `name` it renders as the
    /// ACTIVITY segment at `P_ACTIVITY`, the lowest shed priority: clipped first
    /// when space is short, then shed, while `name` (`P_NAME`) is never shed at
    /// all. With no `name` — bash tasks, workflows — it is the row's only
    /// identifier and takes `P_NAME` itself.
    pub label: Option<String>,
    /// Epoch milliseconds.
    pub start_time: Option<Value>,
    pub model: Option<String>,
    /// Effort level string, or a numeric token budget.
    pub effort: Option<Value>,
    pub context_window_size: Option<i64>,
    pub token_count: Option<i64>,
    /// Rolling history of `token_count`, newest last, capped upstream at 16.
    /// Flat across the whole window means the task is producing nothing — the
    /// wire fact the renderer's stall window is sized against.
    pub token_samples: Vec<i64>,
    /// Working directory the task is running in.
    ///
    /// Deliberately modelled but not yet rendered. It is the field that tells
    /// two rows apart when they share a task-name prefix and differ only by git
    /// worktree — the shape of this repo's sprint-in-a-worktree runs — but a
    /// path costs more columns than the tail it disambiguates, so it has no
    /// segment until there is a way to render just the distinguishing leaf.
    /// Carried here so the wire type stays a complete mirror of the documented
    /// payload and that future segment does not start by rediscovering the
    /// field; if it is still unread when the next reader passes through, that
    /// is the moment to drop it rather than to widen the row.
    pub cwd: Option<String>,
}

impl Task {
    /// Epoch milliseconds, or `None`. A zero or negative stamp is treated as
    /// absent rather than as 1970 — otherwise a task whose start time has not
    /// been populated yet renders a runtime of half a million hours.
    pub fn started_at_ms(&self) -> Option<i64> {
        let ms = match self.start_time.as_ref()? {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }?;
        (ms > 0).then_some(ms)
    }

    pub fn effort_label(&self) -> Option<String> {
        match self.effort.as_ref()? {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(crate::fmt::format_tokens(n.as_i64()?)),
            _ => None,
        }
    }

    /// Claude Code sends the resolved model *id*; trim the vendor prefix and
    /// the date suffix so `claude-haiku-4-5-20251001` reads as `haiku-4-5`.
    pub fn model_label(&self) -> Option<String> {
        let id = self.model.as_deref().filter(|m| !m.is_empty())?;
        let base = id.rsplit('.').next().unwrap_or(id);
        let base = base.strip_prefix("claude-").unwrap_or(base);
        let trimmed = match base.rsplit_once('-') {
            // Trailing 8-digit release date carries no information in a row.
            Some((head, tail)) if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) => head,
            _ => base,
        };
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(json: &str) -> Task {
        serde_json::from_str(json).expect("task parses")
    }

    #[test]
    fn start_time_accepts_number_or_string() {
        assert_eq!(task(r#"{"startTime":1738425600000}"#).started_at_ms(), Some(1738425600000));
        assert_eq!(task(r#"{"startTime":"1738425600000"}"#).started_at_ms(), Some(1738425600000));
        assert_eq!(task(r#"{"startTime":null}"#).started_at_ms(), None);
        // Not-yet-populated stamps must not render as a runtime since 1970.
        assert_eq!(task(r#"{"startTime":0}"#).started_at_ms(), None);
        assert_eq!(task(r#"{"startTime":-1}"#).started_at_ms(), None);
        // snake_case is *not* the wire shape and must not silently bind.
        assert_eq!(task(r#"{"start_time":1738425600000}"#).started_at_ms(), None);
    }

    #[test]
    fn model_label_strips_vendor_and_date() {
        assert_eq!(task(r#"{"model":"claude-haiku-4-5-20251001"}"#).model_label().as_deref(), Some("haiku-4-5"));
        assert_eq!(task(r#"{"model":"claude-opus-5"}"#).model_label().as_deref(), Some("opus-5"));
        assert_eq!(task(r#"{"model":"us.anthropic.claude-opus-5"}"#).model_label().as_deref(), Some("opus-5"));
        assert_eq!(task(r#"{"model":""}"#).model_label(), None);
        assert_eq!(task("{}").model_label(), None);
    }

    #[test]
    fn cwd_binds_from_the_camel_case_wire_and_stays_absent_otherwise() {
        assert_eq!(task(r#"{"cwd":"C:/dev/wt/sprint-1"}"#).cwd.as_deref(), Some("C:/dev/wt/sprint-1"));
        assert_eq!(task("{}").cwd, None);
    }

    #[test]
    fn effort_label_accepts_level_or_budget() {
        assert_eq!(task(r#"{"effort":"xhigh"}"#).effort_label().as_deref(), Some("xhigh"));
        assert_eq!(task(r#"{"effort":32000}"#).effort_label().as_deref(), Some("32k"));
        assert_eq!(task("{}").effort_label(), None);
    }
}
