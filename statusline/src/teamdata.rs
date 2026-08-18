//! Per-teammate facts that the `subagentStatusLine` payload does not carry,
//! read from Claude Code's own on-disk state.
//!
//! Two sources, both deliberately cheap — this runs on every refresh tick:
//!
//! * `<transcript dir>/<session id>/subagents/*.meta.json` — ~330 bytes each,
//!   carrying `customAgentType` (`implement-deep`, `verification`, …), the
//!   teammate's assigned `color`, and its `teamName`. None of that is in the
//!   payload, and the agent type is the field that tells teammate roles apart.
//! * `<claude dir>/teams/<teamName>/inboxes/<name>.json` — a JSON array of
//!   pending inbound messages. Length is the queue depth.
//!
//! Deliberately NOT read: `<claude dir>/teams/<teamName>/config.json`. It holds
//! the same agent types, but every member embeds its full dispatch prompt, so it
//! runs to ~110 KB for a team of eleven — a bad thing to parse on a timer.
//!
//! * Deliberately NOT cached, while the source stays a handful of small files.
//!   The binary is a fresh process per tick, so there is no in-process state to
//!   cache into; a cross-process cache would itself be a file opened, read and
//!   mtime-checked every tick — more I/O than the ~330-byte reads it replaces —
//!   and it would serve a stale inbox depth, the one field whose whole value is
//!   being current. A genuinely bigger source would deserve the question again.
//!
//! Everything degrades to "absent" rather than erroring. A row that cannot find
//! its metadata simply renders without it.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Bound the work per tick regardless of how large a team grows.
const MAX_MEMBERS: usize = 64;
/// Bound the `read_dir` walk itself, which happens before the iteration budget
/// below can be applied in a deterministic order. Set far above any real
/// subagents directory — it is a guard against a pathological one, not a
/// second budget: the sort that follows is what makes the walk reproducible,
/// and it needs the listing in hand first.
const MAX_SCAN_ENTRIES: usize = 4096;
/// A meta file is ~330 bytes; anything far larger is not one.
const MAX_META_BYTES: u64 = 16 * 1024;
/// An inbox holds pending prose messages, so it is legitimately much larger
/// than a meta file — but a queue this deep is not one anybody is waiting on.
/// Set well clear of any real queue: past the cap the depth reads as 0, and a
/// count that vanishes exactly when it matters most would be worse than the
/// occasional slower tick this allows.
const MAX_INBOX_BYTES: u64 = 256 * 1024;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Member {
    /// `implement-deep`, `research-lite`, `verification`, …
    pub agent_type: Option<String>,
    /// Claude Code's assigned teammate colour: one of green, blue, yellow,
    /// cyan, purple, orange, red, pink.
    pub color: Option<String>,
    /// Pending inbound messages.
    pub inbox: usize,
}

pub type Team = HashMap<String, Member>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    name: Option<String>,
    custom_agent_type: Option<String>,
    color: Option<String>,
    team_name: Option<String>,
}

/// `<dir>/<session>.jsonl` → `<dir>/<session>/subagents`.
pub(crate) fn subagents_dir(transcript_path: &str) -> Option<PathBuf> {
    let p = Path::new(transcript_path);
    let stem = p.file_stem()?;
    Some(p.parent()?.join(stem).join("subagents"))
}

/// Where Claude Code keeps its state, resolved from three environment values.
///
/// The env READ stays at the call site and the precedence lives here because
/// this crate is edition 2024: `std::env::set_var` is an `unsafe fn` there, so
/// a test that drove the real environment would need `unsafe` plus a
/// process-wide lock against the multi-threaded test harness. A pure function
/// over three `Option<&str>` needs neither.
///
/// Three arguments rather than one pre-selected `home`, because the
/// `USERPROFILE`-then-`HOME` order is itself behaviour worth pinning — and it
/// carries a quirk a two-argument form would hide. The selection is
/// `Result::or_else`, so a `USERPROFILE` that is *set but empty* still wins
/// over `HOME`; only `CLAUDE_CONFIG_DIR` treats empty as unset.
fn claude_dir_from(
    config: Option<&str>,
    user_profile: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(d) = config.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(user_profile.or(home)?).join(".claude"))
}

pub(crate) fn claude_dir() -> Option<PathBuf> {
    let config = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let user_profile = std::env::var("USERPROFILE").ok();
    let home = std::env::var("HOME").ok();
    claude_dir_from(config.as_deref(), user_profile.as_deref(), home.as_deref())
}

/// Is `dir` inside `root`? The transcript path arrives in the payload, so the
/// directory derived from it is only ever enumerated after this says yes.
///
/// Both sides are canonicalised first. A plain `Path::starts_with` is a
/// component comparison, and `..` is just another component to it, so
/// `<root>/../elsewhere` would pass one; canonicalising resolves the traversal
/// (and any symlink) before the prefix is tested. It also settles the Windows
/// `\\?\` prefix consistently, being applied to both operands.
fn contained_in(dir: &Path, root: &Path) -> bool {
    match (std::fs::canonicalize(dir), std::fs::canonicalize(root)) {
        (Ok(dir), Ok(root)) => dir.starts_with(root),
        _ => false,
    }
}

/// Read a file that is expected to be small, or nothing at all.
///
/// The cap binds the bytes actually read rather than a stat taken beforehand:
/// the two can describe different inodes — `DirEntry::metadata` does not follow
/// symlinks while the read does — so a link can report its own length, clear
/// the cap, and then hand over the whole of whatever it points at.
fn read_capped(path: &Path, cap: u64) -> Option<String> {
    let mut text = String::new();
    // One byte past the cap: enough to tell "at the limit" from "over it".
    std::fs::File::open(path)
        .ok()?
        .take(cap + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= cap).then_some(text)
}

/// Claude Code sanitises both the team and agent name before using them as path
/// components. Mirror that conservatively: anything outside a safe set becomes
/// `_`, so a mismatch costs only the inbox count. Rewriting is not enough for
/// the three strings that are not a name at all — `.`, `..` and the empty one
/// pass the safe set unchanged and would move the path out of the directory it
/// is pinned to, so they are rejected instead.
fn sanitize(s: &str) -> Option<String> {
    if matches!(s, "" | "." | "..") {
        return None;
    }
    Some(
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect(),
    )
}

/// Where one teammate's inbox lives, or `None` when either component is one of
/// the three strings `sanitize` refuses. Both components are sanitised, and the
/// `.json` suffix is appended afterwards, so no input can name a path outside
/// `<claude>/teams/<team>/inboxes/`.
fn inbox_path(claude: &Path, team: &str, name: &str) -> Option<PathBuf> {
    let (team, name) = (sanitize(team)?, sanitize(name)?);
    Some(
        claude
            .join("teams")
            .join(team)
            .join("inboxes")
            .join(format!("{name}.json")),
    )
}

/// Top level is an array of message entries; anything else means the file is
/// mid-write or a shape we do not know, and zero is the safe reading. Only the
/// length is ever wanted, so the bodies are discarded as they are parsed:
/// `IgnoredAny` is zero-sized, making this `Vec` a counter that allocates
/// nothing rather than a copy of every message on the queue.
fn count_messages(text: &str) -> usize {
    match serde_json::from_str::<Vec<serde::de::IgnoredAny>>(text) {
        Ok(v) => v.len(),
        Err(_) => 0,
    }
}

fn inbox_count(claude: &Path, team: &str, name: &str) -> usize {
    let Some(path) = inbox_path(claude, team, name) else {
        return 0;
    };
    let Some(text) = read_capped(&path, MAX_INBOX_BYTES) else {
        return 0;
    };
    count_messages(&text)
}

/// Build the name → facts map for one refresh. Returns an empty map when there
/// is no transcript path, no subagents directory, nothing readable in it, or
/// the directory the transcript path derives is not inside the Claude state
/// tree this reads from.
///
/// `wanted` is the set of names the panel is actually about to draw. Every
/// `*.meta.json` still has to be read — the member's name lives *inside* the
/// file, so there is no way to know whose it is without opening it — but the
/// inbox is a *second* file per member, and only the rows in `wanted` will ever
/// have their depth looked at. Gating it roughly halves the syscalls per tick
/// whenever the panel is a strict subset of the team. A skipped member still
/// lands in the map with its type and colour; only its depth is left at 0,
/// which is what a member with an empty queue already reports.
pub fn load(transcript_path: Option<&str>, wanted: &HashSet<&str>) -> Team {
    let Some(dir) = transcript_path.and_then(subagents_dir) else {
        return Team::new();
    };
    let claude = claude_dir();
    // Nothing here is trusted enough to walk an arbitrary directory for: the
    // transcript path is payload-supplied, and every `*.meta.json` found ends
    // up rendered as a row. The guard stays on this entry point — the one that
    // takes the untrusted path — so `load_from` below only ever sees a
    // directory that has already been proven inside the Claude state tree.
    if !claude.as_deref().is_some_and(|c| contained_in(&dir, c)) {
        return Team::new();
    }
    load_from(&dir, claude.as_deref(), wanted)
}

/// Enumerate an already-vetted subagents directory. `claude` is the root the
/// inbox counts are resolved against; `None` leaves every depth at zero, and so
/// does a name absent from `wanted`.
fn load_from(dir: &Path, claude: Option<&Path>, wanted: &HashSet<&str>) -> Team {
    let mut out = Team::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };

    // Sorted, because `read_dir` order is filesystem-defined: NTFS hands back
    // entries sorted, ext4/btrfs hand back hash order. The iteration budget
    // below truncates the walk, so an unsorted listing makes *which* members
    // survive it depend on the filesystem — and on Linux that is not even
    // stable across ticks, so a badge could appear and vanish between two
    // refreshes of the same panel. Sorting first makes the truncation a
    // deterministic prefix everywhere.
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .take(MAX_SCAN_ENTRIES)
        .map(|e| e.path())
        .collect();
    paths.sort_unstable();

    for path in paths.into_iter().take(MAX_MEMBERS * 2) {
        if !path.to_string_lossy().ends_with(".meta.json") {
            continue;
        }
        let Some(text) = read_capped(&path, MAX_META_BYTES) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<Meta>(&text) else {
            continue;
        };
        let Some(name) = meta.name.filter(|n| !n.is_empty()) else {
            continue;
        };

        // The second file per member, and the one nothing will read unless the
        // panel is about to draw this row.
        let inbox = match (claude, meta.team_name.as_deref()) {
            (Some(c), Some(t)) if !t.is_empty() && wanted.contains(name.as_str()) => {
                inbox_count(c, t, &name)
            }
            _ => 0,
        };
        out.insert(
            name,
            Member {
                agent_type: meta.custom_agent_type.filter(|s| !s.is_empty()),
                color: meta.color.filter(|s| !s.is_empty()),
                inbox,
            },
        );
        if out.len() >= MAX_MEMBERS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names the panel is about to draw. Most tests here are about member
    /// *enumeration*, which `wanted` does not gate, so they pass the empty set
    /// — which doubles as the proof that membership is ungated.
    fn wanted(names: &[&'static str]) -> HashSet<&'static str> {
        names.iter().copied().collect()
    }

    #[test]
    fn subagents_dir_is_derived_from_the_transcript_path() {
        assert_eq!(
            subagents_dir("/p/proj/88530c49-c6e9.jsonl"),
            Some(PathBuf::from("/p/proj/88530c49-c6e9/subagents"))
        );
        assert_eq!(subagents_dir(""), None);
    }

    #[test]
    fn sanitize_keeps_real_teammate_names_intact() {
        // The names in play are already path-safe; the guard is for the rest.
        assert_eq!(
            sanitize("task-14-react19-residue").as_deref(),
            Some("task-14-react19-residue")
        );
        assert_eq!(
            sanitize("session-88530c49").as_deref(),
            Some("session-88530c49")
        );
        assert_eq!(sanitize("a/b\\c:d").as_deref(), Some("a_b_c_d"));
    }

    #[test]
    fn sanitize_rejects_a_name_that_is_not_a_component() {
        // `.` survives the safe set, so rewriting alone would leave the path
        // free to climb out of `teams/<team>/inboxes/`.
        assert_eq!(sanitize(".."), None);
        assert_eq!(sanitize("."), None);
        assert_eq!(sanitize(""), None);
        // Dots inside a name are legitimate and stay.
        assert_eq!(sanitize("v1.2.3").as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn containment_sees_through_a_traversal() {
        // Cargo runs unit tests from the package root, so both paths exist.
        let root = std::env::current_dir().expect("cwd");
        assert!(contained_in(&root, &root));

        let climb = root.join("..");
        // Lexically the traversal is still inside the root — the canonical
        // comparison is the only one that disagrees.
        assert!(climb.starts_with(&root));
        assert!(!contained_in(&climb, &root));
    }

    #[test]
    fn a_capped_read_stops_at_the_cap() {
        let this = Path::new("src/teamdata.rs");
        assert!(read_capped(this, MAX_INBOX_BYTES).is_some());
        assert_eq!(read_capped(this, 16), None);
        assert_eq!(
            read_capped(Path::new("src/not-a-file"), MAX_META_BYTES),
            None
        );
    }

    #[test]
    fn a_missing_transcript_path_is_not_an_error() {
        assert!(load(None, &wanted(&[])).is_empty());
        assert!(load(Some("/definitely/not/here.jsonl"), &wanted(&[])).is_empty());
    }

    #[test]
    fn meta_parses_the_real_on_disk_shape() {
        let m: Meta = serde_json::from_str(
            r#"{"agentType":"task-14-react19-residue","description":"Task 14",
                "name":"task-14-react19-residue","spawnDepth":0,"model":"opus",
                "taskKind":"in_process_teammate","teamName":"session-88530c49",
                "color":"green","planModeRequired":false,
                "customAgentType":"implement-deep","permissionMode":"bypassPermissions"}"#,
        )
        .expect("meta parses");
        assert_eq!(m.name.as_deref(), Some("task-14-react19-residue"));
        assert_eq!(m.custom_agent_type.as_deref(), Some("implement-deep"));
        assert_eq!(m.color.as_deref(), Some("green"));
        assert_eq!(m.team_name.as_deref(), Some("session-88530c49"));
    }

    /// A fixture tree of this test's own. Unit tests share one process across
    /// threads, so the path is keyed by both the pid and a per-test tag — a
    /// fixed one would have the tests racing each other's directories.
    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "statusline-teamdata-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        dir
    }

    fn meta_json(name: &str) -> String {
        format!(r#"{{"name":"{name}","customAgentType":"implement-deep","color":"green"}}"#)
    }

    #[test]
    fn a_valid_meta_file_becomes_a_member_carrying_its_type_and_colour() {
        let dir = fixture_dir("valid");
        std::fs::write(dir.join("a.meta.json"), meta_json("planner")).expect("write");

        let team = load_from(&dir, None, &wanted(&[]));
        assert_eq!(
            team.get("planner"),
            Some(&Member {
                agent_type: Some("implement-deep".to_string()),
                color: Some("green".to_string()),
                inbox: 0,
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_meta_json_is_skipped() {
        let dir = fixture_dir("not-meta");
        std::fs::write(dir.join("a.meta.json"), meta_json("kept")).expect("write");
        // Each of these parses fine; only the suffix disqualifies it.
        std::fs::write(dir.join("b.json"), meta_json("plain-json")).expect("write");
        std::fs::write(dir.join("c.meta.json.bak"), meta_json("backup")).expect("write");
        std::fs::write(dir.join("notes.txt"), "not json at all").expect("write");

        let team = load_from(&dir, None, &wanted(&[]));
        assert_eq!(team.len(), 1);
        assert!(team.contains_key("kept"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_meta_file_without_a_usable_name_is_skipped() {
        let dir = fixture_dir("no-name");
        std::fs::write(dir.join("empty.meta.json"), r#"{"name":"","color":"green"}"#)
            .expect("write");
        std::fs::write(dir.join("absent.meta.json"), r#"{"color":"green"}"#).expect("write");
        std::fs::write(dir.join("ok.meta.json"), meta_json("kept")).expect("write");

        let team = load_from(&dir, None, &wanted(&[]));
        assert_eq!(team.len(), 1);
        assert!(team.contains_key("kept"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_team_name_pairs_with_the_claude_dir_to_produce_the_inbox_depth() {
        let root = fixture_dir("inbox-pairing");
        let dir = root.join("subagents");
        std::fs::create_dir_all(&dir).expect("subagents dir");
        let claude = root.join("claude");
        let inboxes = claude.join("teams").join("session-1").join("inboxes");
        std::fs::create_dir_all(&inboxes).expect("inboxes dir");
        std::fs::write(inboxes.join("worker.json"), r#"[{"a":1},{"b":2},{"c":3}]"#)
            .expect("write");
        std::fs::write(
            dir.join("worker.meta.json"),
            r#"{"name":"worker","teamName":"session-1","color":"blue"}"#,
        )
        .expect("write");

        assert_eq!(load_from(&dir, Some(&claude), &wanted(&["worker"]))["worker"].inbox, 3);
        // Without a Claude root the depth is simply absent, not an error.
        assert_eq!(load_from(&dir, None, &wanted(&[]))["worker"].inbox, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_member_the_panel_is_not_drawing_keeps_its_facts_but_costs_no_inbox_read() {
        let root = fixture_dir("inbox-gating");
        let dir = root.join("subagents");
        std::fs::create_dir_all(&dir).expect("subagents dir");
        let claude = root.join("claude");
        let inboxes = claude.join("teams").join("session-1").join("inboxes");
        std::fs::create_dir_all(&inboxes).expect("inboxes dir");
        // Both members have a readable, non-empty queue on disk.
        for who in ["drawn", "offscreen"] {
            std::fs::write(inboxes.join(format!("{who}.json")), r#"[{"a":1},{"b":2}]"#)
                .expect("write");
            std::fs::write(
                dir.join(format!("{who}.meta.json")),
                format!(
                    r#"{{"name":"{who}","teamName":"session-1","color":"blue",
                        "customAgentType":"implement-deep"}}"#
                ),
            )
            .expect("write");
        }

        let team = load_from(&dir, Some(&claude), &wanted(&["drawn"]));
        assert_eq!(team["drawn"].inbox, 2, "the requested row still counts its queue");
        // The unrequested member is still a member — only the second file read
        // is skipped, and a skipped depth reads exactly like an empty queue.
        assert_eq!(
            team.get("offscreen"),
            Some(&Member {
                agent_type: Some("implement-deep".to_string()),
                color: Some("blue".to_string()),
                inbox: 0,
            }),
            "membership is not gated, only the inbox read is"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_claude_dir_prefers_its_own_variable_and_treats_empty_as_unset() {
        let home = Some("/h");
        assert_eq!(
            claude_dir_from(Some("/cfg"), home, None),
            Some(PathBuf::from("/cfg")),
            "CLAUDE_CONFIG_DIR wins outright and is used verbatim, no `.claude` suffix"
        );
        // Set-but-empty is not a directory; it falls through as if unset.
        assert_eq!(
            claude_dir_from(Some(""), home, None),
            Some(PathBuf::from("/h").join(".claude"))
        );
        assert_eq!(
            claude_dir_from(None, home, None),
            Some(PathBuf::from("/h").join(".claude"))
        );
    }

    #[test]
    fn the_home_fallback_is_userprofile_then_home() {
        assert_eq!(
            claude_dir_from(None, Some("/up"), Some("/home")),
            Some(PathBuf::from("/up").join(".claude"))
        );
        assert_eq!(
            claude_dir_from(None, None, Some("/home")),
            Some(PathBuf::from("/home").join(".claude"))
        );
        // The selection is `Result::or_else` on the real thing, so a set-but-
        // empty USERPROFILE still wins — unlike CLAUDE_CONFIG_DIR above.
        assert_eq!(
            claude_dir_from(None, Some(""), Some("/home")),
            Some(PathBuf::from(".claude"))
        );
        // Nothing to resolve against at all.
        assert_eq!(claude_dir_from(None, None, None), None);
    }

    #[test]
    fn the_member_cap_bounds_insertion_at_sixty_four() {
        let dir = fixture_dir("member-cap");
        for i in 0..100 {
            let name = format!("m{i:03}");
            std::fs::write(dir.join(format!("{name}.meta.json")), meta_json(&name))
                .expect("write");
        }

        // 100 readable members, all of them within the iteration budget; the
        // `break` is what stops the map at the cap.
        assert_eq!(load_from(&dir, None, &wanted(&[])).len(), MAX_MEMBERS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_meta_file_over_the_size_cap_is_skipped() {
        let dir = fixture_dir("size-cap");
        // Valid JSON throughout, so the skip is attributable to the cap alone
        // and not to a parse failure.
        let pad = "a".repeat(MAX_META_BYTES as usize);
        std::fs::write(
            dir.join("big.meta.json"),
            format!(r#"{{"name":"big","color":"green","pad":"{pad}"}}"#),
        )
        .expect("write");
        std::fs::write(dir.join("small.meta.json"), meta_json("small")).expect("write");

        let team = load_from(&dir, None, &wanted(&[]));
        assert_eq!(team.len(), 1);
        assert!(team.contains_key("small"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_meta_entries_spend_the_iteration_budget_the_member_cap_never_sees() {
        let dir = fixture_dir("iteration-budget");
        // `.take(MAX_MEMBERS * 2)` caps ITERATION while the `break` caps
        // INSERTION, so entries that are skipped still cost budget. Zero-padded
        // names put every filler entry ahead of the two real meta files once
        // the listing is sorted — which is the only reason this is assertable
        // at all. Unsorted, it passed on NTFS and failed on ext4/btrfs.
        for i in 0..MAX_MEMBERS * 2 {
            std::fs::write(dir.join(format!("{i:03}.txt")), "filler").expect("write");
        }
        for i in MAX_MEMBERS * 2..MAX_MEMBERS * 2 + 2 {
            let name = format!("m{i:03}");
            std::fs::write(dir.join(format!("{i:03}.meta.json")), meta_json(&name))
                .expect("write");
        }

        // Two members were readable and available; the budget was gone by the
        // time the walk reached them.
        assert!(load_from(&dir, None, &wanted(&[])).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The companion to the test above: when the budget truncates, *which*
    /// members survive must be the sorted prefix, not whatever order the
    /// filesystem happened to hand back. Written so it can only pass by
    /// sorting — the meta files straddle the budget by name, and the creation
    /// order is deliberately the reverse of the name order.
    #[test]
    fn the_surviving_members_are_the_sorted_prefix_not_the_readdir_order() {
        let dir = fixture_dir("sorted-prefix");
        // Created last-to-first, so a filesystem that returns creation order
        // (or its reverse) disagrees with the sort.
        for i in (0..MAX_MEMBERS * 2 + 10).rev() {
            let name = format!("m{i:03}");
            std::fs::write(dir.join(format!("{i:03}.meta.json")), meta_json(&name))
                .expect("write");
        }

        let team = load_from(&dir, None, &wanted(&[]));
        // Insertion stops at MAX_MEMBERS, and every entry here is a meta file,
        // so the budget never binds — the member cap does.
        assert_eq!(team.len(), MAX_MEMBERS);
        for i in 0..MAX_MEMBERS {
            assert!(team.contains_key(&format!("m{i:03}")), "missing m{i:03}");
        }
        assert!(!team.contains_key(&format!("m{:03}", MAX_MEMBERS)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_inbox_path_sanitises_both_components_before_the_suffix_is_appended() {
        let claude = Path::new("/c");
        // Neither component is one of the three rejected strings, so both are
        // rewritten rather than refused — and the rewrite is what keeps the
        // result inside `inboxes/`.
        assert_eq!(
            inbox_path(claude, "team/../x", "../../etc/passwd"),
            Some(PathBuf::from("/c/teams/team_.._x/inboxes/.._.._etc_passwd.json"))
        );
        // `.json` lands after sanitisation, so a name cannot slip a separator
        // in ahead of the suffix.
        assert_eq!(
            inbox_path(claude, "t", "a\\b"),
            Some(PathBuf::from("/c/teams/t/inboxes/a_b.json"))
        );
        assert_eq!(
            inbox_path(claude, "session-1", "task-14"),
            Some(PathBuf::from("/c/teams/session-1/inboxes/task-14.json"))
        );
    }

    #[test]
    fn an_inbox_path_is_refused_for_a_component_that_is_not_a_name() {
        let claude = Path::new("/c");
        for bad in ["", ".", ".."] {
            assert_eq!(inbox_path(claude, bad, "n"), None, "team {bad:?}");
            assert_eq!(inbox_path(claude, "t", bad), None, "name {bad:?}");
        }
        assert!(inbox_path(claude, "t", "n").is_some());
    }

    #[test]
    fn only_a_json_array_reads_as_a_queue_depth() {
        assert_eq!(count_messages(r#"[{"a":1},{"b":2}]"#), 2);
        assert_eq!(count_messages("[]"), 0);
        // A shape we do not know, and a file caught mid-write.
        assert_eq!(count_messages(r#"{"a":1}"#), 0);
        assert_eq!(count_messages(r#"[{"a":1},"#), 0);
        assert_eq!(count_messages(""), 0);
    }
}
