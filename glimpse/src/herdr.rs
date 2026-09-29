//! Wrapper over the herdr CLI's pane commands.
//!
//! Every command prints `{"id":…,"result":…}` on stdout; failures print JSON on
//! stderr and exit 1. Parsing is pure and separate from process spawning.

use std::ffi::OsString;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub label: Option<String>,
    pub cwd: String,
    /// `pane list` and `pane get` carry no rect; it is filled from `pane layout`.
    pub rect: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    Right,
    Down,
}

impl SplitDir {
    fn as_arg(self) -> &'static str {
        match self {
            SplitDir::Right => "right",
            SplitDir::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDir {
    Left,
    Right,
    Up,
    Down,
}

impl FocusDir {
    fn as_arg(self) -> &'static str {
        match self {
            FocusDir::Left => "left",
            FocusDir::Right => "right",
            FocusDir::Up => "up",
            FocusDir::Down => "down",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Herdr {
    bin: OsString,
}

impl Herdr {
    /// Uses `$HERDR_BIN_PATH` when set and non-empty, else `herdr` from `PATH`.
    pub fn from_env() -> Self {
        let bin = std::env::var_os("HERDR_BIN_PATH")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| OsString::from("herdr"));
        Herdr { bin }
    }

    pub fn pane_list(&self) -> Result<Vec<PaneInfo>, String> {
        parse_pane_list(&self.exec(&["pane".into(), "list".into()])?)
    }

    pub fn pane_get(&self, pane_id: &str) -> Result<PaneInfo, String> {
        parse_pane_get(&self.exec(&["pane".into(), "get".into(), pane_id.into()])?)
    }

    /// Rect of `pane_id` within its tab's layout.
    pub fn layout(&self, pane_id: &str) -> Result<Rect, String> {
        let out = self.exec(&[
            "pane".into(),
            "layout".into(),
            "--pane".into(),
            pane_id.into(),
        ])?;
        parse_layout(&out, pane_id)
    }

    /// Splits `origin` without moving focus and returns the new pane id.
    pub fn split(
        &self,
        origin: &str,
        dir: SplitDir,
        ratio: f32,
        cwd: &str,
    ) -> Result<String, String> {
        let out = self.exec(&[
            "pane".into(),
            "split".into(),
            "--pane".into(),
            origin.into(),
            "--direction".into(),
            dir.as_arg().into(),
            "--ratio".into(),
            ratio.to_string(),
            "--cwd".into(),
            cwd.into(),
            "--no-focus".into(),
        ])?;
        parse_split(&out)
    }

    /// Focuses the neighbour of `pane_id` in direction `dir`.
    pub fn focus(&self, pane_id: &str, dir: FocusDir) -> Result<(), String> {
        self.exec(&[
            "pane".into(),
            "focus".into(),
            "--pane".into(),
            pane_id.into(),
            "--direction".into(),
            dir.as_arg().into(),
        ])
        .map(drop)
    }

    pub fn rename(&self, pane_id: &str, label: &str) -> Result<(), String> {
        self.exec(&["pane".into(), "rename".into(), pane_id.into(), label.into()])
            .map(drop)
    }

    /// Types `command` plus Enter into the pane's shell; it spawns no process itself.
    pub fn run(&self, pane_id: &str, command: &str) -> Result<(), String> {
        self.exec(&["pane".into(), "run".into(), pane_id.into(), command.into()])
            .map(drop)
    }

    fn exec(&self, args: &[String]) -> Result<String, String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|e| format!("cannot run {}: {e}", self.bin.to_string_lossy()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(error_message(&String::from_utf8_lossy(&output.stderr)))
        }
    }
}

/// Extracts a readable message from herdr's JSON stderr, falling back to the raw text.
fn error_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let err = v.get("error").unwrap_or(&v);
        if let Some(m) = err.get("message").and_then(Value::as_str) {
            return m.to_string();
        }
        if let Some(m) = err.as_str() {
            return m.to_string();
        }
    }
    trimmed.to_string()
}

fn result_of(stdout: &str) -> Result<Value, String> {
    let mut v: Value =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("invalid herdr output: {e}"))?;
    match v.get_mut("result") {
        Some(r) => Ok(r.take()),
        None => Err("herdr output has no `result`".to_string()),
    }
}

fn pane_from_value(v: &Value) -> Result<PaneInfo, String> {
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    Ok(PaneInfo {
        pane_id: text("pane_id").ok_or("pane has no `pane_id`")?,
        tab_id: text("tab_id").unwrap_or_default(),
        label: text("label"),
        cwd: text("cwd").unwrap_or_default(),
        rect: None,
    })
}

pub fn parse_pane_list(stdout: &str) -> Result<Vec<PaneInfo>, String> {
    let result = result_of(stdout)?;
    result
        .get("panes")
        .and_then(Value::as_array)
        .ok_or("`result.panes` missing")?
        .iter()
        .map(pane_from_value)
        .collect()
}

pub fn parse_pane_get(stdout: &str) -> Result<PaneInfo, String> {
    let result = result_of(stdout)?;
    pane_from_value(result.get("pane").ok_or("`result.pane` missing")?)
}

pub fn parse_split(stdout: &str) -> Result<String, String> {
    let result = result_of(stdout)?;
    result
        .pointer("/pane/pane_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "`result.pane.pane_id` missing".to_string())
}

pub fn parse_layout(stdout: &str, pane_id: &str) -> Result<Rect, String> {
    let result = result_of(stdout)?;
    let panes = result
        .pointer("/layout/panes")
        .and_then(Value::as_array)
        .ok_or("`result.layout.panes` missing")?;
    let entry = panes
        .iter()
        .find(|p| p.get("pane_id").and_then(Value::as_str) == Some(pane_id))
        .ok_or_else(|| format!("pane {pane_id} not in layout"))?;
    let rect = entry.get("rect").ok_or("layout entry has no `rect`")?;
    serde_json::from_value(rect.clone()).map_err(|e| format!("invalid rect: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"id":"cli:pane:list","result":{"panes":[{"agent_status":"unknown","cwd":"C:\\dev\\a","focused":false,"pane_id":"w11:p2","revision":1,"tab_id":"w11:t2","workspace_id":"w11"},{"cwd":"C:\\dev\\b","label":"glimpse","pane_id":"w11:p4","tab_id":"w11:t4"}]}}"#;
    const GET: &str = r#"{"id":"cli:pane:get","result":{"pane":{"cwd":"C:\\dev\\a","pane_id":"w11:p2","tab_id":"w11:t2"},"type":"pane_info"}}"#;
    const SPLIT: &str = r#"{"id":"cli:pane:split","result":{"pane":{"pane_id":"w11:p9","tab_id":"w11:t2"},"type":"pane_info"}}"#;
    const LAYOUT: &str = r#"{"id":"cli:pane:layout","result":{"layout":{"area":{"height":76,"width":146,"x":0,"y":0},"panes":[{"focused":true,"pane_id":"w11:p2","rect":{"height":76,"width":73,"x":0,"y":0}},{"focused":false,"pane_id":"w11:p9","rect":{"height":76,"width":73,"x":73,"y":0}}]},"type":"pane_layout"}}"#;

    #[test]
    fn parses_pane_list() {
        let panes = parse_pane_list(LIST).unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pane_id, "w11:p2");
        assert_eq!(panes[0].label, None);
        assert_eq!(panes[1].label.as_deref(), Some("glimpse"));
        assert_eq!(panes[1].tab_id, "w11:t4");
    }

    #[test]
    fn parses_pane_get_and_split() {
        let pane = parse_pane_get(GET).unwrap();
        assert_eq!(pane.cwd, "C:\\dev\\a");
        assert_eq!(parse_split(SPLIT).unwrap(), "w11:p9");
    }

    #[test]
    fn parses_layout_rect_for_named_pane() {
        let rect = parse_layout(LAYOUT, "w11:p9").unwrap();
        assert_eq!(
            rect,
            Rect {
                x: 73,
                y: 0,
                width: 73,
                height: 76
            }
        );
        assert!(parse_layout(LAYOUT, "w11:p1").is_err());
    }

    #[test]
    fn rejects_malformed_output() {
        assert!(parse_pane_list("not json").is_err());
        assert!(parse_pane_list(r#"{"id":"x"}"#).is_err());
        assert!(parse_split(r#"{"result":{"pane":{}}}"#).is_err());
    }

    #[test]
    fn extracts_error_message() {
        assert_eq!(
            error_message(r#"{"error":{"code":"x","message":"no such pane"}}"#),
            "no such pane"
        );
        assert_eq!(error_message("plain failure\n"), "plain failure");
    }
}
