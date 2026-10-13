//! `review schema` — JSON Schemas built from the field catalogue.
//!
//! `finding` is the row an agent submits to `review add`: the input fields
//! of the catalogue, closed, with each kind's required set and an `if`/`then`
//! per conditional field. A plan-family field is listed but never required,
//! since a row does not carry its family. `cell-return` and `verdict-return`
//! are the objects agents echo back to the orchestrator.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::fields::{
    self, FIELDS, FieldSpec, Kind, Need, Origin, PathForm, Record, Route, Severity, Ty,
};
use super::invalid;
use crate::cli::ReviewSchemaArgs;
use crate::output::print_json;

const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
const TARGETS: &[&str] = &["finding", "cell-return", "verdict-return"];

pub(crate) fn run(args: ReviewSchemaArgs) -> Result<()> {
    let kind = match args.kind.as_deref() {
        None => None,
        Some(text) => Some(Kind::parse(text).ok_or_else(|| {
            invalid(format!(
                "--kind {text:?} is not one of {}",
                Kind::VALUES.join(", ")
            ))
        })?),
    };
    let schema = match (args.target.as_str(), kind) {
        ("finding", kind) => finding(kind),
        ("cell-return" | "verdict-return", Some(_)) => {
            return Err(invalid("--kind applies only to `review schema finding`"));
        }
        ("cell-return", None) => cell_return(),
        ("verdict-return", None) => verdict_return(),
        (other, _) => {
            return Err(invalid(format!(
                "schema target {other:?} is not one of {}",
                TARGETS.join(", ")
            )));
        }
    };
    print_json(&schema)
}

fn input_fields(kind: Option<Kind>) -> impl Iterator<Item = &'static FieldSpec> {
    FIELDS.iter().filter(move |s| {
        s.record == Record::Finding
            && s.origin == Origin::Input
            && kind.is_none_or(|k| s.kinds.contains(k))
    })
}

/// The fields every row of `kind` needs, whatever its family or other values.
fn required(kind: Kind) -> Vec<&'static str> {
    input_fields(Some(kind))
        .filter(|s| s.family.is_none())
        .filter(|s| match s.need {
            Need::Required => true,
            Need::RequiredFor(kinds) => kinds.contains(kind),
            _ => false,
        })
        .map(|s| s.name)
        .collect()
}

fn typed(ty: &str) -> Map<String, JsonValue> {
    let mut out = Map::new();
    out.insert("type".into(), json!(ty));
    out
}

fn property(spec: &FieldSpec) -> JsonValue {
    let mut text = typed("string");
    if !spec.values.is_empty() {
        text.insert("enum".into(), json!(spec.values));
    }
    if let Some(cap) = spec.cap {
        text.insert("maxLength".into(), json!(cap));
    }
    let mut out = match spec.ty {
        Ty::Str => text,
        Ty::Date => {
            text.insert("format".into(), json!("date"));
            text
        }
        Ty::StrList => {
            let mut list = typed("array");
            list.insert("items".into(), JsonValue::Object(text));
            if let Some(max) = spec.max_items {
                list.insert("maxItems".into(), json!(max));
            }
            list
        }
        Ty::Int => {
            let mut int = typed("integer");
            if let Some((lo, hi)) = spec.range {
                int.insert("minimum".into(), json!(lo));
                if hi != i64::MAX {
                    int.insert("maximum".into(), json!(hi));
                }
            }
            int
        }
        Ty::Bool => typed("boolean"),
        Ty::Table => typed("object"),
        Ty::TableList => {
            let mut list = typed("array");
            list.insert("items".into(), json!({"type": "object"}));
            list
        }
    };
    let mut notes = Vec::new();
    if spec.path == PathForm::Path {
        notes.push("repo-relative, `/`-separated, no `..`".to_string());
    }
    if let Some(family) = spec.family {
        let need = match spec.need {
            Need::RequiredFor(_) => "; required for defect and cleanup there",
            _ => "",
        };
        notes.push(format!("{} family only{need}", family.as_str()));
    }
    if spec.need == Need::OldText {
        notes.push(
            "required for direction change or remove, and for add unless target_file names a new file"
                .to_string(),
        );
    }
    if !notes.is_empty() {
        out.insert("description".into(), json!(notes.join("; ")));
    }
    JsonValue::Object(out)
}

/// `if`/`then` rules for the fields whose need depends on another field.
fn conditionals(kind: Kind) -> Vec<JsonValue> {
    input_fields(Some(kind))
        .filter_map(|s| match s.need {
            Need::OldText => Some(json!({
                "if": {"properties": {"direction": {"enum": ["change", "remove"]}}, "required": ["direction"]},
                "then": {"required": [s.name]},
            })),
            Need::WhenChallenge => Some(json!({
                "if": {"properties": {"angle": {"const": "challenge"}}, "required": ["angle"]},
                "then": {"required": [s.name]},
            })),
            _ => None,
        })
        .collect()
}

fn properties(kind: Option<Kind>) -> Map<String, JsonValue> {
    let mut out = Map::new();
    for spec in input_fields(kind) {
        out.entry(spec.name.to_string())
            .or_insert_with(|| property(spec));
    }
    if let Some(kind) = kind {
        out.insert(
            "kind".into(),
            json!({"type": "string", "const": kind.as_str()}),
        );
    }
    out
}

fn finding(kind: Option<Kind>) -> JsonValue {
    let mut schema = Map::new();
    schema.insert("$schema".into(), json!(DIALECT));
    schema.insert("type".into(), json!("object"));
    schema.insert("additionalProperties".into(), json!(false));
    match kind {
        Some(kind) => {
            schema.insert(
                "title".into(),
                json!(format!("review finding ({})", kind.as_str())),
            );
            schema.insert(
                "properties".into(),
                JsonValue::Object(properties(Some(kind))),
            );
            schema.insert("required".into(), json!(required(kind)));
            let rules = conditionals(kind);
            if !rules.is_empty() {
                schema.insert("allOf".into(), json!(rules));
            }
        }
        None => {
            schema.insert("title".into(), json!("review finding"));
            schema.insert("properties".into(), JsonValue::Object(properties(None)));
            let common: Vec<&str> = required(Kind::Defect)
                .into_iter()
                .filter(|name| Kind::ALL.iter().all(|&k| required(k).contains(name)))
                .collect();
            schema.insert("required".into(), json!(common));
            let branches: Vec<JsonValue> = Kind::ALL.iter().map(|&k| kind_branch(k)).collect();
            schema.insert("allOf".into(), json!(branches));
        }
    }
    JsonValue::Object(schema)
}

/// The `if kind = k` branch of the kind-free schema: k's required fields and
/// conditionals, and a refusal of every field that does not apply to k.
fn kind_branch(kind: Kind) -> JsonValue {
    let own: BTreeSet<&str> = input_fields(Some(kind)).map(|s| s.name).collect();
    let foreign: BTreeSet<&str> = input_fields(None)
        .map(|s| s.name)
        .filter(|name| !own.contains(name))
        .collect();
    let mut then = Map::new();
    then.insert("required".into(), json!(required(kind)));
    let rules = conditionals(kind);
    if !rules.is_empty() {
        then.insert("allOf".into(), json!(rules));
    }
    if !foreign.is_empty() {
        let any: Vec<JsonValue> = foreign.iter().map(|f| json!({"required": [f]})).collect();
        then.insert("not".into(), json!({"anyOf": any}));
    }
    json!({
        "if": {"properties": {"kind": {"const": kind.as_str()}}, "required": ["kind"]},
        "then": then,
    })
}

fn cell_return() -> JsonValue {
    json!({
        "title": "review cell-return",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "cell": {"type": "string", "pattern": "^C[1-9][0-9]*$"},
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "id": {"type": "string", "pattern": "^F[1-9][0-9]*$"},
                        "kind": {"type": "string", "enum": Kind::VALUES},
                        "severity": {"type": ["string", "null"], "enum": severity_or_null()},
                        "route": {"type": "string", "enum": Route::VALUES},
                    },
                    "required": ["id", "kind", "route"],
                },
            },
            "status": {"type": "string", "enum": sealed_statuses()},
            "escalate": {"type": "array", "items": {"type": "string"}},
        },
        "required": ["cell", "findings", "status"],
    })
}

fn severity_or_null() -> Vec<JsonValue> {
    Severity::VALUES
        .iter()
        .map(|s| json!(s))
        .chain(std::iter::once(JsonValue::Null))
        .collect()
}

/// The statuses `review cover` seals a cell with.
fn sealed_statuses() -> Vec<&'static str> {
    fields::CELL_STATUSES
        .iter()
        .copied()
        .filter(|s| *s != "assigned")
        .collect()
}

fn verdict_return() -> JsonValue {
    json!({
        "title": "review verdict-return",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "finding": {"type": "string", "pattern": "^F[1-9][0-9]*$"},
            "verdict": {"type": "string", "enum": fields::VERDICTS},
            "reroute": {"type": "string", "enum": [Route::Verifier.as_str()]},
        },
        "required": ["finding", "verdict"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::fields::{Family, RowContext, validate_row};

    /// What `review add`'s validator demands of `row`: the fields it refuses
    /// as missing.
    fn validator_requires(row: JsonValue, family: Family) -> BTreeSet<String> {
        let row = row.as_object().expect("an object").clone();
        let ctx = RowContext {
            family,
            target_exists: &|_| true,
        };
        validate_row(Record::Finding, &row, &ctx)
            .err()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.reason.starts_with("is required"))
            .map(|r| r.field)
            .collect()
    }

    fn names(value: &JsonValue) -> BTreeSet<String> {
        value
            .as_array()
            .expect("a list of names")
            .iter()
            .map(|v| v.as_str().expect("a name").to_string())
            .collect()
    }

    fn then_requires(rules: &JsonValue, field: &str, value: &str) -> BTreeSet<String> {
        rules
            .as_array()
            .into_iter()
            .flatten()
            .filter(|rule| {
                let cond = &rule["if"]["properties"][field];
                cond["const"] == json!(value)
                    || cond["enum"]
                        .as_array()
                        .is_some_and(|values| values.contains(&json!(value)))
            })
            .flat_map(|rule| names(&rule["then"]["required"]))
            .collect()
    }

    #[test]
    fn schema_agrees_with_field_catalogue() {
        let whole = finding(None);
        for &kind in Kind::ALL {
            let k = kind.as_str();
            let schema = finding(Some(kind));
            let required = names(&schema["required"]);
            let mut expected = validator_requires(json!({"kind": k}), Family::Code);
            expected.insert("kind".to_string());
            assert_eq!(required, expected, "{k}: required set");

            let branch = whole["allOf"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["if"]["properties"]["kind"]["const"] == json!(k))
                .expect("a branch per kind");
            assert_eq!(names(&branch["then"]["required"]), expected, "{k}: branch");

            let plan_only = validator_requires(json!({"kind": k}), Family::Plan);
            for extra in plan_only.difference(&expected) {
                let note = schema["properties"][extra]["description"]
                    .as_str()
                    .unwrap_or_default();
                assert!(
                    note.contains("plan family only; required"),
                    "{k}: plan-family `{extra}` must say it is required there: {note:?}"
                );
            }

            let props: BTreeSet<String> = schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            let catalogued: BTreeSet<String> = FIELDS
                .iter()
                .filter(|s| {
                    s.record == Record::Finding
                        && s.origin == Origin::Input
                        && s.kinds.contains(kind)
                })
                .map(|s| s.name.to_string())
                .collect();
            assert_eq!(props, catalogued, "{k}: properties");
        }

        let cleanup = finding(Some(Kind::Cleanup));
        for direction in ["change", "remove"] {
            let demanded = validator_requires(
                json!({"kind": "cleanup", "direction": direction}),
                Family::Code,
            );
            assert!(demanded.contains("old"), "the validator demands old");
            assert_eq!(
                then_requires(&cleanup["allOf"], "direction", direction),
                BTreeSet::from(["old".to_string()]),
                "{direction}"
            );
        }
        let advisory = finding(Some(Kind::Advisory));
        assert!(
            validator_requires(
                json!({"kind": "advisory", "angle": "challenge"}),
                Family::Code
            )
            .contains("challenges")
        );
        assert_eq!(
            then_requires(&advisory["allOf"], "angle", "challenge"),
            BTreeSet::from(["challenges".to_string()])
        );
    }

    #[test]
    fn computed_fields_are_not_offered() {
        let schema = finding(None);
        let props = schema["properties"].as_object().unwrap();
        for computed in ["id", "status", "route", "anchor", "first_seen", "related"] {
            assert!(!props.contains_key(computed), "{computed} is computed");
        }
    }

    #[test]
    fn return_schemas_name_their_required_keys() {
        assert_eq!(
            names(&cell_return()["required"]),
            BTreeSet::from(["cell", "findings", "status"].map(String::from))
        );
        assert_eq!(
            names(&verdict_return()["required"]),
            BTreeSet::from(["finding", "verdict"].map(String::from))
        );
    }
}
