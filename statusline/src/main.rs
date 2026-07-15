//! Native Claude Code statusline renderer — a faithful port of
//! `~/.claude/statusline.ps1`.
//!
//! Reads the statusline JSON payload on stdin and prints:
//!   Line 1: CWD@Branch (changes) | model | effort | tokens (pct)
//!   Line 2: 5h dots pct @reset | 7d dots pct @reset | agent-busy time
//!
//! Claude Code invokes the statusline command on every refresh, per session;
//! the pwsh script cost ~1s of cold-start CPU each time. This binary renders
//! in single-digit milliseconds. Output is raw UTF-8 with ANSI colour codes
//! and no trailing newline, exactly like the ps1's `Write-Host -NoNewline`.

use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";
// The ps1 defined a distinct $orange that was also ANSI 33; the alias is kept
// so the tier tables below read the same as the script they mirror.
const ORANGE: &str = "\x1b[33m";
const DIM: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

const DOT_FILL: char = '\u{25CF}'; // ●
const DOT_EMPTY: char = '\u{25CB}'; // ○

// ===== Payload (every field optional — render whatever is present) =====

#[derive(Deserialize, Default)]
#[serde(default)]
struct Payload {
    cwd: Option<String>,
    model: Option<Model>,
    effort: Option<Effort>,
    context_window: Option<ContextWindow>,
    exceeds_200k_tokens: Option<bool>,
    rate_limits: Option<RateLimits>,
    cost: Option<Cost>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Effort {
    level: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContextWindow {
    context_window_size: Option<i64>,
    used_percentage: Option<f64>,
    current_usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Usage {
    input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RateLimits {
    five_hour: Option<LimitWindow>,
    seven_day: Option<LimitWindow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LimitWindow {
    used_percentage: Option<f64>,
    // Number or string upstream; coerced in format_epoch.
    resets_at: Option<Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Cost {
    total_api_duration_ms: Option<f64>,
}

// ===== Git =====

/// Resolve the branch by reading `.git/HEAD` directly — no `git` spawn. Walks
/// up parents (subdir launches), follows the `gitdir:` pointer file
/// (worktrees/submodules; relative pointers resolve against the pointer's
/// directory), and falls back to a short sha for detached HEAD.
fn git_branch(dir: &Path) -> Option<String> {
    let mut d = dir;
    let git_dir: PathBuf = loop {
        let g = d.join(".git");
        if g.is_file() {
            let line = std::fs::read_to_string(&g).ok()?;
            let rest = line.lines().next()?.strip_prefix("gitdir:")?.trim();
            let p = PathBuf::from(rest);
            break if p.is_absolute() { p } else { d.join(p) };
        }
        if g.is_dir() {
            break g;
        }
        d = d.parent()?;
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    parse_head(head.lines().next()?.trim())
}

fn parse_head(head: &str) -> Option<String> {
    if let Some(r) = head.strip_prefix("ref:") {
        return r.trim().strip_prefix("refs/heads/").map(str::to_string);
    }
    let is_sha = (7..=40).contains(&head.len())
        && head
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    is_sha.then(|| head[..7].to_string())
}

/// Staged + unstaged line counts via `git diff HEAD --numstat`. Returns None
/// when git fails or there are no changes (the ps1 renders nothing for 0+0).
fn diff_stats(cwd: &str) -> Option<(u64, u64)> {
    let out = Command::new("git")
        .args(["-C", cwd, "diff", "HEAD", "--numstat"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let (added, deleted) = sum_numstat(&String::from_utf8_lossy(&out.stdout));
    (added + deleted > 0).then_some((added, deleted))
}

fn sum_numstat(text: &str) -> (u64, u64) {
    let (mut added, mut deleted) = (0u64, 0u64);
    for line in text.lines() {
        let mut cols = line.split_whitespace();
        // Binary files report "-" per column; skip whatever doesn't parse.
        if let Some(a) = cols.next().and_then(|c| c.parse::<u64>().ok()) {
            added += a;
        }
        if let Some(d) = cols.next().and_then(|c| c.parse::<u64>().ok()) {
            deleted += d;
        }
    }
    (added, deleted)
}

// ===== Formatting =====

fn format_tokens(num: i64) -> String {
    if num >= 1_000_000 {
        format!("{:.1}m", num as f64 / 1_000_000.0)
    } else if num >= 1000 {
        format!("{:.0}k", num as f64 / 1000.0)
    } else {
        num.to_string()
    }
}

fn usage_color(pct: i64) -> &'static str {
    match pct {
        90.. => RED,
        70.. => ORANGE,
        50.. => YELLOW,
        _ => GREEN,
    }
}

fn usage_dots(pct: i64) -> String {
    let filled = ((pct as f64 / 20.0).ceil() as i64).clamp(0, 5);
    const COLORS: [&str; 5] = [GREEN, GREEN, YELLOW, ORANGE, RED];
    let mut s = String::new();
    for (i, color) in COLORS.iter().enumerate() {
        if (i as i64) < filled {
            s.push_str(color);
            s.push(DOT_FILL);
        } else {
            s.push_str(DIM);
            s.push(DOT_EMPTY);
        }
        s.push_str(RESET);
    }
    s
}

fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return "0s".into();
    }
    let s = ms / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    if s < 3600 {
        return format!("{}m{}s", s / 60, s % 60);
    }
    format!("{}h{}m", s / 3600, (s % 3600) / 60)
}

enum EpochStyle {
    Time,     // "2:30pm", ":00" stripped → "2pm"
    DateTime, // "15/7, 2:30pm"
}

fn format_epoch(val: Option<&Value>, style: EpochStyle) -> Option<String> {
    let secs = match val? {
        Value::Number(n) => n.as_i64()?,
        Value::String(s) => s.parse::<i64>().ok()?,
        _ => return None,
    };
    if secs == 0 {
        return None;
    }
    let dt = chrono::DateTime::from_timestamp(secs, 0)?.with_timezone(&chrono::Local);
    let s = match style {
        EpochStyle::Time => dt.format("%-I:%M%P").to_string(),
        EpochStyle::DateTime => dt.format("%-d/%-m, %-I:%M%P").to_string(),
    };
    Some(s.replace(":00", ""))
}

/// "Claude Fable 5 (extra)" → "Fable 5": strip the leading "Claude " and a
/// trailing parenthesised qualifier, mirroring the ps1 regexes.
fn clean_model_name(name: &str) -> String {
    let name = name.strip_prefix("Claude ").unwrap_or(name).trim_end();
    if name.ends_with(')')
        && let Some(i) = name.rfind('(') {
            return name[..i].trim_end().to_string();
        }
    name.to_string()
}

fn path_leaf(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}

fn limit_segment(label: &str, w: Option<&LimitWindow>, style: EpochStyle, show_resets: bool) -> String {
    let pct = w
        .and_then(|w| w.used_percentage)
        .unwrap_or(0.0)
        .round() as i64;
    let mut seg = format!(
        "{WHITE}{label}{RESET} {} {}{pct}%{RESET}",
        usage_dots(pct),
        usage_color(pct)
    );
    if show_resets
        && let Some(r) = format_epoch(w.and_then(|w| w.resets_at.as_ref()), style) {
            seg.push_str(&format!(" {DIM}@{r}{RESET}"));
        }
    seg
}

// ===== Main =====

fn main() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    if input.trim().is_empty() {
        print!("Claude");
        return;
    }
    let data: Payload = match serde_json::from_str(&input) {
        Ok(d) => d,
        // The ps1 would die mid-render on bad JSON; degrade to the bare label.
        Err(_) => {
            print!("Claude");
            return;
        }
    };

    let cw = data.context_window.unwrap_or_default();
    let size = cw.context_window_size.unwrap_or(0).max(200_000);
    let pct_used = cw.used_percentage.unwrap_or(0.0).round() as i64;
    let usage = cw.current_usage.unwrap_or_default();
    let current = usage.input_tokens.unwrap_or(0)
        + usage.cache_creation_input_tokens.unwrap_or(0)
        + usage.cache_read_input_tokens.unwrap_or(0);

    // Width tiers. COLUMNS is set by Claude Code; querying the console handle
    // is unreliable in piped contexts (same rationale as the ps1), so fall
    // back to a wide default rather than probing.
    let cols: i64 = env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(120);
    let show_changes = cols >= 100;
    let show_model = cols >= 70;
    let show_resets = cols >= 70;
    let show_line2 = cols >= 50;
    let compact = cols < 50;

    let sep = format!(" {DIM}|{RESET} ");
    let mut parts1: Vec<String> = Vec::new();

    // Project@branch (changes)
    let cwd = data.cwd.as_deref().filter(|c| !c.is_empty());
    if let Some(cwd) = cwd {
        let mut seg = format!("{CYAN}{}{RESET}", path_leaf(cwd));
        if let Some(branch) = git_branch(Path::new(cwd)) {
            seg.push_str(&format!("{DIM}@{RESET}{GREEN}{branch}{RESET}"));
            if show_changes
                && let Some((added, deleted)) = diff_stats(cwd) {
                    seg.push_str(&format!(
                        " {DIM}({RESET}{GREEN}+{added}{RESET} {RED}-{deleted}{RESET}{DIM}){RESET}"
                    ));
                }
        }
        parts1.push(seg);
    }

    // Model
    let model_name = data
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_deref())
        .map(clean_model_name)
        .unwrap_or_default();
    if !model_name.is_empty() && show_model {
        parts1.push(format!("{DIM}{model_name}{RESET}"));
    }

    // Effort level — absent when the current model lacks the effort parameter.
    if show_model
        && let Some(level) = data
            .effort
            .as_ref()
            .and_then(|e| e.level.as_deref())
            .filter(|l| !l.is_empty())
        {
            let color = match level {
                "low" => GREEN,
                "medium" => YELLOW,
                "high" => ORANGE,
                "xhigh" => CYAN,
                "max" => RED,
                _ => WHITE,
            };
            parts1.push(format!("{color}{level}{RESET}"));
        }

    // Tokens — force red when past 200k regardless of proportional %
    let token_color = if data.exceeds_200k_tokens == Some(true) {
        RED
    } else {
        usage_color(pct_used)
    };
    parts1.push(format!(
        "{}/{} {DIM}({RESET}{token_color}{pct_used}%{RESET}{DIM}){RESET}",
        format_tokens(current),
        format_tokens(size)
    ));

    let line1 = parts1.join(&sep);

    // Line 2: rate limits
    let mut line2 = String::new();
    if let Some(rl) = data.rate_limits.as_ref().filter(|_| show_line2) {
        line2 = [
            limit_segment("5h", rl.five_hour.as_ref(), EpochStyle::Time, show_resets),
            limit_segment("7d", rl.seven_day.as_ref(), EpochStyle::DateTime, show_resets),
        ]
        .join(&sep);
    }

    // Agent-busy time (API wait). Appended to line 2, or shown standalone.
    let api_ms = data
        .cost
        .as_ref()
        .and_then(|c| c.total_api_duration_ms)
        .unwrap_or(0.0) as i64;
    if api_ms > 0 {
        let busy = format!("{DIM}busy{RESET} {WHITE}{}{RESET}", format_duration(api_ms));
        line2 = if line2.is_empty() {
            busy
        } else {
            format!("{line2}{sep}{busy}")
        };
    }

    // Output
    if compact {
        let mut cline = String::new();
        if let Some(cwd) = cwd {
            cline = format!("{CYAN}{}{RESET}", path_leaf(cwd));
        }
        cline.push_str(&format!(" {token_color}{pct_used}%{RESET}"));
        if let Some(rl) = &data.rate_limits {
            let p5 = rl
                .five_hour
                .as_ref()
                .and_then(|w| w.used_percentage)
                .unwrap_or(0.0)
                .round() as i64;
            let p7 = rl
                .seven_day
                .as_ref()
                .and_then(|w| w.used_percentage)
                .unwrap_or(0.0)
                .round() as i64;
            cline.push_str(&format!(
                " {DIM}|{RESET} {}5h:{p5}%{RESET} {}7d:{p7}%{RESET}",
                usage_color(p5),
                usage_color(p7)
            ));
        }
        print!("{cline}");
    } else if !line2.is_empty() {
        print!("{line1}\n{line2}");
    } else {
        print!("{line1}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_format_tiers() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1k");
        assert_eq!(format_tokens(85_000), "85k");
        assert_eq!(format_tokens(200_000), "200k");
        assert_eq!(format_tokens(1_234_567), "1.2m");
        assert_eq!(format_tokens(2_960_000), "3.0m");
    }

    #[test]
    fn duration_format_tiers() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(-5), "0s");
        assert_eq!(format_duration(59_000), "59s");
        assert_eq!(format_duration(60_000), "1m0s");
        assert_eq!(format_duration(754_321), "12m34s");
        assert_eq!(format_duration(3_600_000), "1h0m");
        assert_eq!(format_duration(5_430_000), "1h30m");
    }

    #[test]
    fn head_parsing() {
        assert_eq!(parse_head("ref: refs/heads/main"), Some("main".into()));
        assert_eq!(
            parse_head("ref: refs/heads/feature/x-1"),
            Some("feature/x-1".into())
        );
        assert_eq!(
            parse_head("5a715f7deadbeef5a715f7deadbeef5a715f7dea"),
            Some("5a715f7".into())
        );
        // Uppercase hex and non-hex are rejected, matching the ps1 regex.
        assert_eq!(parse_head("5A715F7DEADBEEF"), None);
        assert_eq!(parse_head("not-a-head"), None);
        assert_eq!(parse_head("ref: refs/tags/v1"), None);
    }

    #[test]
    fn model_name_cleaning() {
        assert_eq!(clean_model_name("Claude Fable 5"), "Fable 5");
        assert_eq!(clean_model_name("Claude Sonnet 4.5 (thinking)"), "Sonnet 4.5");
        assert_eq!(clean_model_name("Fable 5"), "Fable 5");
        assert_eq!(clean_model_name("Claude "), "");
    }

    #[test]
    fn numstat_summing() {
        let text = "10\t2\tsrc/main.rs\n-\t-\tassets/logo.png\n3\t0\tREADME.md\n";
        assert_eq!(sum_numstat(text), (13, 2));
        assert_eq!(sum_numstat(""), (0, 0));
    }

    #[test]
    fn dots_fill_counts() {
        let filled = |pct| usage_dots(pct).matches(DOT_FILL).count();
        assert_eq!(filled(0), 0);
        assert_eq!(filled(1), 1);
        assert_eq!(filled(63), 4);
        assert_eq!(filled(100), 5);
    }

    #[test]
    fn usage_color_boundaries() {
        assert_eq!(usage_color(49), GREEN);
        assert_eq!(usage_color(50), YELLOW);
        assert_eq!(usage_color(70), ORANGE);
        assert_eq!(usage_color(90), RED);
    }

    #[test]
    fn epoch_rejects_null_forms() {
        assert!(format_epoch(None, EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&Value::Null), EpochStyle::Time).is_none());
        assert!(
            format_epoch(Some(&Value::String("null".into())), EpochStyle::Time).is_none()
        );
        assert!(format_epoch(Some(&serde_json::json!(0)), EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&serde_json::json!(1752571800)), EpochStyle::Time).is_some());
    }
}
