//! Claude Code and Codex hook handler: records agent events and opens the glimpse pane on
//! subagent start.
//!
//! Neither harness waits for nor reads an async hook, so a failure here would vanish
//! silently. Every error is appended instead to `<claude_dir>/glimpse/hook.log`, and nothing
//! is ever written to stdout or stderr. The I/O lives in `run_hook` and the helpers it
//! names; `decide`, `parse_record_result` and `append_log` are what the tests cover.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::cli::Harness;
use crate::config::{Config, claude_dir};
use crate::herdr::Herdr;

/// Matches the cap `tomlctl` applies to a stdin payload.
const MAX_PAYLOAD: u64 = 32 * 1024 * 1024;
const LOG_CAP: u64 = 1024 * 1024;
const OLD_TOMLCTL: &str = "tomlctl ≥0.12.0 required — cargo install --path tomlctl";

/// The one JSON line `tomlctl agents record` prints. Unknown keys are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct RecordResult {
    pub(crate) recorded: bool,
    pub(crate) slug: Option<String>,
    pub(crate) event: Option<String>,
    /// Kept as raw JSON: the record's id shape is tomlctl's to choose.
    pub(crate) id: Option<Value>,
    pub(crate) task_ids: Vec<u64>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookAction {
    Nothing,
    EnsurePane {
        origin_pane: String,
        slug: Option<String>,
        cwd: String,
    },
}

/// Only a recorded `start` inside herdr opens a pane; an empty `HERDR_PANE_ID` counts as unset.
pub(crate) fn decide(result: &RecordResult, herdr_pane_id: Option<&str>, cwd: &str) -> HookAction {
    let Some(origin) = herdr_pane_id.filter(|p| !p.is_empty()) else {
        return HookAction::Nothing;
    };
    if !result.recorded || result.event.as_deref() != Some("start") {
        return HookAction::Nothing;
    }
    HookAction::EnsurePane {
        origin_pane: origin.to_string(),
        slug: result.slug.clone().filter(|s| !s.is_empty()),
        cwd: cwd.to_string(),
    }
}

/// Reads the last non-empty line, so a stray line printed before the result does not hide it.
pub(crate) fn parse_record_result(stdout: &str) -> Result<RecordResult, String> {
    let line = stdout
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .ok_or("tomlctl agents record printed nothing")?;
    serde_json::from_str(line).map_err(|e| format!("invalid tomlctl agents record output: {e}"))
}

/// Turns a failed `tomlctl` run into the line logged for it.
pub(crate) fn failure_message(status: &str, stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("unrecognized subcommand") || lower.contains("unknown subcommand") {
        return OLD_TOMLCTL.to_string();
    }
    let first = stderr.lines().map(str::trim).find(|l| !l.is_empty());
    match first {
        Some(line) => format!("tomlctl agents record failed ({status}): {line}"),
        None => format!("tomlctl agents record failed ({status})"),
    }
}

/// The payload's `cwd`, or `None` when it is absent, not a string, or not valid JSON.
pub(crate) fn payload_cwd(payload: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    v.get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Appends one timestamped line, first truncating the log to empty when it is over `cap`.
pub(crate) fn append_log(
    path: &Path,
    cap: u64,
    timestamp: &str,
    message: &str,
) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let over = std::fs::metadata(path).is_ok_and(|m| m.len() > cap);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(!over)
        .write(true)
        .truncate(over)
        .open(path)?;
    let message = message.replace(['\r', '\n'], " ");
    writeln!(file, "{timestamp} {message}")
}

/// RFC 3339 UTC to the second, from seconds since the Unix epoch.
pub(crate) fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days over 400-year eras of 146 097 days, with March as month 0.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Seconds since the Unix epoch of an RFC 3339 timestamp, with optional
/// fractional seconds (dropped) and a `Z` or `±HH:MM` offset; `None` for
/// anything else, or for an instant before the epoch.
pub(crate) fn parse_utc(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    let digits = |from: usize, to: usize| -> Option<i64> {
        let part = bytes.get(from..to)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if separators
        .iter()
        .any(|(at, sep)| bytes.get(*at) != Some(sep))
        || !matches!(bytes.get(10), Some(b'T' | b't' | b' '))
    {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 7)?, digits(8, 10)?);
    let (hour, minute, second) = (digits(11, 13)?, digits(14, 16)?, digits(17, 19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let mut rest = &s[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            let tail = rest.as_bytes();
            if tail.len() != 6 || tail[3] != b':' {
                return None;
            }
            let at = s.len() - 6;
            sign * (digits(at + 1, at + 3)? * 3_600 + digits(at + 4, at + 6)? * 60)
        }
    };

    let secs =
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    u64::try_from(secs).ok()
}

/// Days since 1970-01-01 of a proleptic Gregorian date, over 400-year eras
/// with March as month 0; the inverse of the civil-from-days in [`format_utc`].
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn log_path() -> Option<PathBuf> {
    Some(claude_dir()?.join("glimpse").join("hook.log"))
}

pub(crate) fn log_error(message: &str) {
    let Some(path) = log_path() else {
        return;
    };
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = append_log(&path, LOG_CAP, &format_utc(secs), message);
}

/// Handles one hook payload. Never fails and never prints; errors go to the hook log.
pub(crate) fn run_hook(stdin: impl Read, harness: Harness) {
    if let Err(e) = handle(stdin, harness) {
        log_error(&e);
    }
}

fn handle(stdin: impl Read, harness: Harness) -> Result<(), String> {
    let payload = read_capped(stdin)?;
    let (config, warning) = Config::load();
    if let Some(w) = warning {
        log_error(&w);
    }
    let cwd = payload_cwd(&payload);
    let result = record(&config, harness, &payload, cwd.as_deref())?;
    let herdr_pane = std::env::var("HERDR_PANE_ID").ok();
    let cwd = cwd.unwrap_or_else(|| ".".to_string());
    match decide(&result, herdr_pane.as_deref(), &cwd) {
        HookAction::Nothing => Ok(()),
        HookAction::EnsurePane {
            origin_pane,
            slug,
            cwd,
        } => crate::pane::ensure(
            &Herdr::from_env(),
            &origin_pane,
            slug.as_deref(),
            &cwd,
            false,
            &config,
        )
        .map(drop)
        .map_err(|e| format!("cannot open the glimpse pane: {e}")),
    }
}

fn read_capped(stdin: impl Read) -> Result<Vec<u8>, String> {
    let mut payload = Vec::new();
    stdin
        .take(MAX_PAYLOAD + 1)
        .read_to_end(&mut payload)
        .map_err(|e| format!("cannot read the hook payload: {e}"))?;
    if payload.len() as u64 > MAX_PAYLOAD {
        return Err(format!("hook payload exceeds {MAX_PAYLOAD} bytes"));
    }
    Ok(payload)
}

pub(crate) fn record_args(harness: Harness) -> [&'static str; 5] {
    ["agents", "record", "--harness", harness.as_str(), "-"]
}

/// Runs `tomlctl agents record` in the payload's `cwd` when that directory exists, so
/// tomlctl resolves the worktree the agent ran in.
fn record(
    config: &Config,
    harness: Harness,
    payload: &[u8],
    cwd: Option<&str>,
) -> Result<RecordResult, String> {
    let mut cmd = Command::new(&config.tomlctl);
    cmd.args(record_args(harness))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(dir);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", config.tomlctl))?;
    let mut input = child.stdin.take().ok_or("tomlctl stdin was not piped")?;
    let payload = payload.to_vec();
    // Written from a thread so a child that fills its output pipe before draining stdin
    // cannot deadlock against us.
    let writer = std::thread::spawn(move || input.write_all(&payload));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("tomlctl agents record: {e}"))?;
    let written = writer.join();
    if !output.status.success() {
        return Err(failure_message(
            &output.status.to_string(),
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    if let Ok(Err(e)) = written {
        return Err(format!("cannot send the hook payload to tomlctl: {e}"));
    }
    parse_record_result(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(slug: &str) -> RecordResult {
        parse_record_result(&format!(
            r#"{{"recorded":true,"slug":"{slug}","event":"start","id":"s1:a1","task_ids":[16,17]}}"#
        ))
        .expect("parses")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glimpse-hook-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn decide_opens_a_pane_on_a_recorded_start_inside_herdr() {
        assert_eq!(
            decide(&start("a-b"), Some("w1:p2"), "C:/dev/x"),
            HookAction::EnsurePane {
                origin_pane: "w1:p2".to_string(),
                slug: Some("a-b".to_string()),
                cwd: "C:/dev/x".to_string(),
            }
        );
    }

    #[test]
    fn decide_does_nothing_outside_herdr() {
        assert_eq!(decide(&start("a-b"), None, "."), HookAction::Nothing);
        assert_eq!(decide(&start("a-b"), Some(""), "."), HookAction::Nothing);
    }

    #[test]
    fn decide_does_nothing_on_stop_or_idle() {
        for event in ["stop", "idle"] {
            let mut result = start("a-b");
            result.event = Some(event.to_string());
            assert_eq!(
                decide(&result, Some("w1:p2"), "."),
                HookAction::Nothing,
                "{event}"
            );
        }
    }

    #[test]
    fn decide_does_nothing_when_not_recorded() {
        let result =
            parse_record_result(r#"{"recorded":false,"reason":"unknown-flow"}"#).expect("parses");
        assert_eq!(result.reason.as_deref(), Some("unknown-flow"));
        assert_eq!(decide(&result, Some("w1:p2"), "."), HookAction::Nothing);
    }

    #[test]
    fn the_result_parse_ignores_unknown_keys_and_leading_noise() {
        let result = parse_record_result(
            "warning: something\n{\"recorded\":true,\"slug\":\"s\",\"event\":\"start\",\"extra\":1}\n\n",
        )
        .expect("parses");
        assert!(result.recorded);
        assert_eq!(result.slug.as_deref(), Some("s"));
        assert!(result.task_ids.is_empty());
        assert!(parse_record_result("").is_err());
        assert!(parse_record_result("not json").is_err());
    }

    #[test]
    fn an_old_tomlctl_is_named_in_the_failure() {
        assert_eq!(
            failure_message(
                "exit code: 2",
                "error: unrecognized subcommand 'agents'\n\nUsage: …"
            ),
            OLD_TOMLCTL
        );
        assert_eq!(
            failure_message("exit code: 1", "\nerror: lock held\nmore"),
            "tomlctl agents record failed (exit code: 1): error: lock held"
        );
    }

    #[test]
    fn the_harness_reaches_the_tomlctl_arguments() {
        assert_eq!(
            record_args(Harness::ClaudeCode),
            ["agents", "record", "--harness", "claude-code", "-"]
        );
        assert_eq!(
            record_args(Harness::Codex),
            ["agents", "record", "--harness", "codex", "-"]
        );
    }

    #[test]
    fn payload_cwd_is_read_tolerantly() {
        assert_eq!(
            payload_cwd(br#"{"cwd":"C:/dev/x","other":1}"#).as_deref(),
            Some("C:/dev/x")
        );
        assert_eq!(payload_cwd(br#"{"cwd":""}"#), None);
        assert_eq!(payload_cwd(b"{}"), None);
        assert_eq!(payload_cwd(b"not json"), None);
    }

    #[test]
    fn read_capped_rejects_an_oversized_payload() {
        assert_eq!(read_capped(&b"{}"[..]).expect("small"), b"{}");
        let big = std::io::repeat(b' ').take(MAX_PAYLOAD + 1);
        assert!(read_capped(big).is_err());
    }

    #[test]
    fn the_log_appends_and_is_truncated_when_over_its_cap() {
        let dir = temp_dir("log");
        let path = dir.join("nested").join("hook.log");
        append_log(&path, 64, "T1", "first").expect("append");
        append_log(&path, 64, "T2", "second\r\nline").expect("append");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "T1 first\nT2 second  line\n"
        );

        std::fs::write(&path, "x".repeat(65)).expect("fill past the cap");
        append_log(&path, 64, "T3", "after").expect("append");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "T3 after\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_utc_gives_rfc3339() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_790_000_000), "2026-09-21T14:13:20Z");
    }

    #[test]
    fn parse_utc_inverts_format_utc() {
        for secs in [0, 951_782_400, 1_790_000_000, 1_790_000_059, 4_102_444_799] {
            assert_eq!(parse_utc(&format_utc(secs)), Some(secs));
        }
        assert_eq!(
            parse_utc("2026-09-28T11:04:22.517Z"),
            parse_utc("2026-09-28T11:04:22Z")
        );
        assert_eq!(
            parse_utc("2026-09-28T13:04:22.5+02:00"),
            parse_utc("2026-09-28T11:04:22Z")
        );
        assert_eq!(parse_utc(""), None);
        assert_eq!(parse_utc("2026-09-28"), None);
        assert_eq!(parse_utc("2026-13-28T11:04:22Z"), None);
        assert_eq!(parse_utc("2026-09-28T11:04:22+0200"), None);
    }
}
