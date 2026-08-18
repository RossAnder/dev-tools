//! The `statusLine` stdin payload. Every field is optional — Claude Code adds
//! fields over time and leaves others absent or null until the first API
//! response, so the renderers draw whatever happens to be present.

use serde::Deserialize;
use serde_json::Value;

use crate::fmt::path_leaf;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Payload {
    pub cwd: Option<String>,
    pub workspace: Option<Workspace>,
    pub model: Option<Model>,
    pub effort: Option<Effort>,
    pub context_window: Option<ContextWindow>,
    pub exceeds_200k_tokens: Option<bool>,
    pub rate_limits: Option<RateLimits>,
    pub cost: Option<Cost>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Workspace {
    pub current_dir: Option<String>,
    pub project_dir: Option<String>,
    pub repo: Option<Repo>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Repo {
    pub name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Model {
    pub display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Effort {
    pub level: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ContextWindow {
    pub context_window_size: Option<i64>,
    pub used_percentage: Option<f64>,
    pub total_input_tokens: Option<i64>,
    pub current_usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RateLimits {
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LimitWindow {
    pub used_percentage: Option<f64>,
    // Number or string upstream; coerced in fmt::format_epoch.
    pub resets_at: Option<Value>,
}

/// Deliberately models only the duration. The payload also carries
/// `total_lines_added` / `total_lines_removed` — the lines *Claude changed this
/// session* — and they are left unmodelled because `full`'s `(+N -N)` segment
/// measures something else: `git diff HEAD --numstat` reports the whole working
/// tree against HEAD, including edits the user made by hand and changes from
/// before the session started. Were the session-scoped semantics ever
/// acceptable, the two fields would give the same visual with no subprocess at
/// all.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Cost {
    pub total_api_duration_ms: Option<f64>,
}

impl ContextWindow {
    /// Tokens currently occupying the context window. Prefers the per-component
    /// breakdown (what the ps1 summed), but `current_usage` is null before the
    /// first API call and again after `/compact`, so fall back to the
    /// pre-summed total rather than reporting a bare 0.
    pub fn tokens(&self) -> i64 {
        let summed = self
            .current_usage
            .as_ref()
            .map(|u| {
                u.input_tokens.unwrap_or(0)
                    + u.cache_creation_input_tokens.unwrap_or(0)
                    + u.cache_read_input_tokens.unwrap_or(0)
            })
            .unwrap_or(0);
        if summed > 0 {
            summed
        } else {
            self.total_input_tokens.unwrap_or(0)
        }
    }
}

impl Payload {
    /// The repository this session belongs to: the `origin` remote's repo name
    /// when there is one, else the launch directory's leaf. `project_dir` beats
    /// `cwd` because `cwd` follows mid-session directory changes.
    pub fn repo_label(&self) -> String {
        let ws = self.workspace.as_ref();
        if let Some(name) = ws
            .and_then(|w| w.repo.as_ref())
            .and_then(|r| r.name.as_deref())
            .filter(|n| !n.is_empty())
        {
            return name.to_string();
        }
        ws.and_then(|w| w.project_dir.as_deref())
            .or(self.cwd.as_deref())
            .filter(|d| !d.is_empty())
            .map(path_leaf)
            .unwrap_or_else(|| "claude".into())
    }

    /// Working directory, preferring the payload's canonical field.
    pub fn dir(&self) -> Option<&str> {
        self.workspace
            .as_ref()
            .and_then(|w| w.current_dir.as_deref())
            .or(self.cwd.as_deref())
            .filter(|c| !c.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Payload {
        serde_json::from_str(s).expect("payload parses")
    }

    #[test]
    fn repo_label_prefers_origin_then_project_dir() {
        let p = parse(r#"{"workspace":{"repo":{"name":"dev-tools"},"project_dir":"/x/other"}}"#);
        assert_eq!(p.repo_label(), "dev-tools");

        let p = parse(r#"{"workspace":{"project_dir":"/x/other"},"cwd":"/x/other/sub"}"#);
        assert_eq!(p.repo_label(), "other");

        let p = parse(r#"{"cwd":"/x/only-cwd"}"#);
        assert_eq!(p.repo_label(), "only-cwd");

        assert_eq!(parse("{}").repo_label(), "claude");
        // An empty repo name is not an identity; fall through to the path.
        let p = parse(r#"{"workspace":{"repo":{"name":""}},"cwd":"/x/fallback"}"#);
        assert_eq!(p.repo_label(), "fallback");
    }

    #[test]
    fn context_tokens_fall_back_past_a_compact() {
        let cw: ContextWindow = serde_json::from_str(
            r#"{"current_usage":{"input_tokens":8500,"cache_creation_input_tokens":5000,
                 "cache_read_input_tokens":2000},"total_input_tokens":99}"#,
        )
        .unwrap();
        assert_eq!(cw.tokens(), 15_500);

        // `current_usage` is null right after /compact — use the pre-summed total.
        let cw: ContextWindow =
            serde_json::from_str(r#"{"current_usage":null,"total_input_tokens":31000}"#).unwrap();
        assert_eq!(cw.tokens(), 31_000);

        let cw: ContextWindow = serde_json::from_str("{}").unwrap();
        assert_eq!(cw.tokens(), 0);
    }
}
