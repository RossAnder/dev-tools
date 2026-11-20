//! The `plan run visual --accept` leaf: records the user's acceptance of a
//! failed visual round as a `[[visual_accept]]`, keyed on `(milestone, round)`.
//! The verb checks that each state is one of the milestone's `ui_states` and
//! each look one of its rows' `run_by = "look"` accept entries; it cannot know
//! which states failed and does not try. A second call for the same round
//! replaces that round's record.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};

use super::emit::load_milestone_store;
use super::schema::{VisualAccept, WaivedDefect, WaivedLook};
use super::store;
use crate::cli::PlanRunVisualArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_json_value_from_arg;
use crate::output::print_json_compact;
use crate::tasks::schema::{RUN_BY_LOOK, Store};

type Row = Map<String, JsonValue>;

const PAYLOAD_KEYS: &[&str] = &["why", "states", "looks", "defects", "backlog"];
const LOOK_KEYS: &[&str] = &["task", "accept"];
const DEFECT_KEYS: &[&str] = &["state", "class", "location", "detail", "note"];

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

/// The payload, checked for shape before the stores are read.
struct Payload {
    why: String,
    states: Vec<String>,
    looks: Vec<WaivedLook>,
    defects: Vec<WaivedDefect>,
    backlog: String,
}

pub(super) fn run(args: PlanRunVisualArgs) -> Result<()> {
    let PlanRunVisualArgs {
        target,
        milestone,
        round,
        accept: _,
        json,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    let payload = parse_payload(&read_json_value_from_arg(&json).context("parsing --json")?)?;
    let tasks = load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    check_states(&tasks, &milestone, &payload.states)?;
    check_looks(&tasks, &milestone, &payload.looks)?;
    let at = crate::time::now_toml_datetime()?;
    let record = VisualAccept {
        milestone: milestone.clone(),
        round,
        at,
        why: payload.why,
        states: payload.states,
        looks: payload.looks,
        defects: payload.defects,
        backlog: payload.backlog,
    };

    store::mutate(&store::resolve(slug)?, &integrity, |plan| {
        match plan
            .visual_accepts
            .iter_mut()
            .find(|a| a.milestone == record.milestone && a.round == round)
        {
            Some(earlier) => *earlier = record,
            None => plan.visual_accepts.push(record),
        }
        Ok(())
    })?;

    print_json_compact(&json!({"ok": true, "milestone": milestone, "round": round}))
}

fn parse_payload(value: &JsonValue) -> Result<Payload> {
    let object = value.as_object().ok_or_else(|| {
        refusal(format!(
            "`--json` must be a JSON object with {}",
            PAYLOAD_KEYS.join(", ")
        ))
    })?;
    check_keys(object, PAYLOAD_KEYS, "the payload")?;

    let why = text(object, "why", "the payload")?;
    let backlog = text(object, "backlog", "the payload")?;

    let raw_states = object
        .get("states")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| refusal("`states` must be an array of state names"))?;
    let mut states: Vec<String> = Vec::with_capacity(raw_states.len());
    for (index, state) in raw_states.iter().enumerate() {
        let state = state
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| refusal(format!("states[{index}] must be a non-empty string")))?;
        if states.iter().any(|s| s == state) {
            return Err(refusal(format!(
                "states[{index}]: `{state}` is named twice; name each state once"
            )));
        }
        states.push(state.to_string());
    }
    if states.is_empty() {
        return Err(refusal("`states` must name at least one accepted state"));
    }

    let mut looks: Vec<WaivedLook> = Vec::new();
    for (index, entry) in entries(object, "looks")?.into_iter().enumerate() {
        let at = format!("looks[{index}]");
        check_keys(entry, LOOK_KEYS, &at)?;
        let look = WaivedLook {
            task: index_of(entry, "task", &at)?,
            accept: index_of(entry, "accept", &at)?,
        };
        if looks.contains(&look) {
            return Err(refusal(format!(
                "{at}: task {} accept {} is named twice",
                look.task, look.accept
            )));
        }
        looks.push(look);
    }

    let mut defects = Vec::new();
    for (index, entry) in entries(object, "defects")?.into_iter().enumerate() {
        let at = format!("defects[{index}]");
        check_keys(entry, DEFECT_KEYS, &at)?;
        let state = text(entry, "state", &at)?;
        if !states.contains(&state) {
            return Err(refusal(format!(
                "{at}: state `{state}` is not one of this acceptance's states ({})",
                states.join(", ")
            )));
        }
        let class = optional_text(entry, "class", &at)?;
        defects.push(WaivedDefect {
            state,
            class,
            location: optional_text(entry, "location", &at)?,
            detail: optional_text(entry, "detail", &at)?,
            note: optional_text(entry, "note", &at)?,
        });
    }

    Ok(Payload {
        why,
        states,
        looks,
        defects,
        backlog,
    })
}

/// A control is captured to prove the capture can fail, never judged, so it
/// cannot be accepted as a state.
fn check_states(tasks: &Store, milestone: &str, states: &[String]) -> Result<()> {
    let spec = tasks.milestones.iter().find(|spec| spec.id == milestone);
    let declared: BTreeSet<&str> = spec
        .into_iter()
        .flat_map(|spec| spec.ui_states.iter().map(String::as_str))
        .collect();
    let controls: BTreeSet<&str> = spec
        .into_iter()
        .flat_map(|spec| spec.ui_controls.iter().map(String::as_str))
        .collect();
    for state in states {
        if declared.contains(state.as_str()) {
            continue;
        }
        let what = if controls.contains(state.as_str()) {
            "is one of its ui_controls, and a control is not a state"
        } else {
            "is not one of its ui_states"
        };
        return Err(refusal(format!(
            "state `{state}` {what}: milestone {milestone} declares {}",
            if declared.is_empty() {
                "no ui_states".to_string()
            } else {
                declared.into_iter().collect::<Vec<_>>().join(", ")
            }
        )));
    }
    Ok(())
}

/// Each look names, by the index `plan emit gate-list` prints as `accept`, a
/// `run_by = "look"` entry of a row whose checkpoint is the milestone.
fn check_looks(tasks: &Store, milestone: &str, looks: &[WaivedLook]) -> Result<()> {
    for WaivedLook { task, accept } in looks {
        let row = tasks
            .items
            .iter()
            .find(|row| row.id == *task && row.checkpoint == milestone)
            .ok_or_else(|| {
                refusal(format!(
                    "look task {task} accept {accept}: task {task} is not a row of milestone {milestone}"
                ))
            })?;
        let entry = tasks
            .specs
            .iter()
            .find(|spec| spec.r#ref == row.r#ref)
            .and_then(|spec| spec.accept.get(usize::try_from(*accept).ok()?));
        match entry {
            Some(entry) if entry.owner() == RUN_BY_LOOK => {}
            Some(_) => {
                return Err(refusal(format!(
                    "look task {task} accept {accept}: that accept entry is not run_by = \"look\""
                )));
            }
            None => {
                return Err(refusal(format!(
                    "look task {task} accept {accept}: task {task} has no accept entry {accept}"
                )));
            }
        }
    }
    Ok(())
}

fn check_keys(object: &Row, allowed: &[&str], at: &str) -> Result<()> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(refusal(format!(
            "{at}: unknown key `{key}` — expected one of {}",
            allowed.join(", ")
        ))),
        None => Ok(()),
    }
}

fn entries<'a>(object: &'a Row, key: &str) -> Result<Vec<&'a Row>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| refusal(format!("`{key}` must be an array of objects")))?;
    array
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry
                .as_object()
                .ok_or_else(|| refusal(format!("{key}[{index}] must be an object")))
        })
        .collect()
}

fn text(object: &Row, key: &str, at: &str) -> Result<String> {
    match object.get(key).and_then(JsonValue::as_str).map(str::trim) {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => Err(refusal(format!("{at}: `{key}` must be a non-empty string"))),
    }
}

/// Absent reads as `""`; a present non-string is refused.
fn optional_text(object: &Row, key: &str, at: &str) -> Result<String> {
    match object.get(key) {
        None => Ok(String::new()),
        Some(value) => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| refusal(format!("{at}: `{key}` must be a string"))),
    }
}

fn index_of(object: &Row, key: &str, at: &str) -> Result<u32> {
    object
        .get(key)
        .and_then(JsonValue::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| refusal(format!("{at}: `{key}` must be a non-negative integer")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<Payload> {
        parse_payload(&serde_json::from_str(raw).expect("valid JSON"))
    }

    #[test]
    fn a_payload_keeps_its_states_looks_and_defects() {
        let payload = parse(
            r#"{"why":"known","states":["full"],"looks":[{"task":2,"accept":0}],
               "defects":[{"state":"full","class":"clipping"}],"backlog":"B-1"}"#,
        )
        .expect("parses");
        assert_eq!(payload.states, ["full"]);
        assert_eq!(payload.looks, [WaivedLook { task: 2, accept: 0 }]);
        assert_eq!(payload.defects[0].class, "clipping");
        assert_eq!(payload.defects[0].note, "");
        assert_eq!(payload.backlog, "B-1");
    }

    #[test]
    fn a_bad_payload_is_refused_naming_what_is_wrong() {
        for (raw, needle) in [
            (r#"{"why":" ","states":["a"],"backlog":"B-1"}"#, "`why`"),
            (r#"{"why":"w","states":[],"backlog":"B-1"}"#, "at least one"),
            (r#"{"why":"w","states":["a"],"backlog":" "}"#, "`backlog`"),
            (r#"{"why":"w","states":["a"]}"#, "`backlog`"),
            (
                r#"{"why":"w","states":["a"],"backlog":"B-1","extra":1}"#,
                "why, states, looks, defects, backlog",
            ),
            (
                r#"{"why":"w","states":["a"],"backlog":"B-1","defects":[{"state":"b"}]}"#,
                "not one of this acceptance's states",
            ),
            (
                r#"{"why":"w","states":["a"],"backlog":"B-1","looks":[{"task":-1,"accept":0}]}"#,
                "`task`",
            ),
        ] {
            let message = parse(raw).err().expect(raw).to_string();
            assert!(message.contains(needle), "{raw}: {message}");
        }
    }
}
