//! Runtime clap-reflection helper for the `capabilities`
//! subcommand. `build_agent_context()` walks the live `<Cli as
//! CommandFactory>::command()` tree and emits a per-subcommand flag schema
//! suitable for agents that need to introspect `tomlctl`'s surface without
//! parsing `--help` prose.

use clap::{ArgAction, Command, CommandFactory};
use serde_json::{Map, Value as JsonValue};

use crate::cli::Cli;

/// Mutex groups that clap doesn't expose via `Command::get_groups()` —
/// e.g. shape-mutex constraints enforced at parse-time inside `query.rs`.
/// Format: `[(subcommand_path, &[&[flag_names_in_one_group]])]` where
/// `subcommand_path` is the space-separated chain ("items list" / "items get").
const MUTEX_GROUPS: &[(&str, &[&[&str]])] = &[(
    "items list",
    &[&["count", "count_by", "group_by", "pluck", "count_distinct"]],
)];

/// Fallback enum value sets for `ValueEnum` flags whose
/// `get_value_parser().possible_values()` returns `None` (a clap edge case
/// that affects some derive forms). Format: `(flag_name, &[allowed_values])`.
///
/// `flag_name` is matched against `Arg::get_id()` (snake_case rust field id),
/// not the user-facing long name. ScalarType has six variants
/// (Str/Int/Float/Bool/Date/Datetime); the kebab-cased ValueEnum names are
/// `str`/`int`/`float`/`bool`/`date`/`datetime`.
const ENUM_VALUES: &[(&str, &[&str])] = &[
    ("ty", &["str", "int", "float", "bool", "date", "datetime"]), // ScalarType (clap id is "ty" — see Cmd::Set)
    ("tier", &["A", "B", "C"]),                                   // DupTier
    ("error_format", &["text", "json"]),                          // ErrorFormat
];

/// Vocabularies of flags validated or coerced after parsing, which clap cannot
/// report. Keyed by subcommand path as well as clap id because one id names a
/// different vocabulary per subcommand — `status` on `backlog list` is not
/// `status` on `tasks update`. Format: `(subcommand_path, clap_id, values)`.
const SCOPED_ENUM_VALUES: &[(&str, &str, &[&str])] = &[
    (
        "flow envelope build",
        "command",
        &[
            "review",
            "optimise",
            "plan-new",
            "plan-update",
            "implement",
            "review-plan",
            "tdd",
            "review-apply",
            "optimise-apply",
            "test-bootstrap",
        ],
    ),
    (
        "flow envelope build",
        "require_artifact",
        &[
            "review_ledger",
            "optimise_findings",
            "execution_record",
            "plan_review_findings",
            "tasks",
        ],
    ),
    ("backlog add", "kind", BACKLOG_KINDS),
    ("backlog check", "kind", BACKLOG_KINDS),
    ("backlog list", "kind", BACKLOG_KINDS),
    (
        "backlog list",
        "status",
        &["open", "promoted", "dismissed", "resolved"],
    ),
    (
        "tasks update",
        "status",
        &["pending", "in-progress", "done", "failed", "deferred"],
    ),
    ("tasks add", "effort", &["S", "M", "L"]),
];

const BACKLOG_KINDS: &[&str] = &[
    "bug",
    "flaky-test",
    "debt",
    "direction",
    "annoyance",
    "question",
    "other",
];

pub(crate) fn build_agent_context() -> JsonValue {
    let cmd = <Cli as CommandFactory>::command();
    let mut commands_map = Map::new();
    walk_commands(&cmd, "", &mut commands_map);
    JsonValue::Object(commands_map)
}

/// The root's own flags, which apply to every subcommand. Read from the
/// unbuilt root, so clap's auto-generated `--help` / `--version` stay out, and
/// listed once here because the `.commands` walk never sees a global: clap
/// copies globals into subcommands only when the tree is built.
pub(crate) fn build_global_flags() -> JsonValue {
    let cmd = <Cli as CommandFactory>::command();
    describe_flags(&cmd, "")
}

fn walk_commands(cmd: &Command, parent_path: &str, out: &mut Map<String, JsonValue>) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        let sub_path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{} {}", parent_path, name)
        };

        let mut node = Map::new();
        // Container subcommands (items / blocks / integrity) carry no leaf
        // flags of their own. Suppress the key entirely when there are none,
        // so a consumer can rely on `flags` being present iff non-empty.
        let flags = describe_flags(sub, &sub_path);
        if flags.as_object().is_some_and(|m| !m.is_empty()) {
            node.insert("flags".to_string(), flags);
        }
        node.insert(
            "mutex_groups".to_string(),
            describe_mutex_groups(sub, &sub_path),
        );

        // Recurse — get_subcommands() is shallow; descend into each child's children.
        let has_children = sub.get_subcommands().next().is_some();
        if has_children {
            let mut child_map = Map::new();
            walk_commands(sub, &sub_path, &mut child_map);
            node.insert("subcommands".to_string(), JsonValue::Object(child_map));
        }

        out.insert(name, JsonValue::Object(node));
    }
}

fn describe_flags(cmd: &Command, sub_path: &str) -> JsonValue {
    let mut flags = Map::new();
    for arg in cmd.get_arguments() {
        // Hidden arguments are recovery aliases, not part of the documented surface.
        if arg.is_hide_set() {
            continue;
        }
        let id = arg.get_id().as_str();
        // Use the user-facing long name when available — this matches what
        // appears in `--help` output. clap stores ids as snake_case (e.g.
        // `where_eq`), but the long is the explicit `long = "where"` from
        // the derive attribute. Falling back to id-with-dashes for any arg
        // without an explicit long preserves coverage of edge cases.
        let key = if let Some(long) = arg.get_long() {
            format!("--{}", long)
        } else if arg.is_positional() {
            format!("<{}>", id)
        } else {
            // Unusual: short-only flag with no long. Tag with id for visibility.
            format!("-{}", id)
        };

        let values = describe_values(arg, sub_path);
        // A vocabulary validated after parsing is still an enum to the caller.
        let ty = match infer_type(arg) {
            "string" if values.is_some() => "enum",
            ty => ty,
        };
        let mut entry = Map::new();
        entry.insert("type".to_string(), JsonValue::String(ty.to_string()));
        entry.insert(
            "required".to_string(),
            JsonValue::Bool(arg.is_required_set()),
        );
        if let Some(default) = describe_default(arg) {
            entry.insert("default".to_string(), default);
        }
        if let Some(values) = values {
            entry.insert("values".to_string(), values);
        }
        entry.insert(
            "repeatable".to_string(),
            JsonValue::Bool(is_repeatable(arg)),
        );
        flags.insert(key, JsonValue::Object(entry));
    }
    JsonValue::Object(flags)
}

fn infer_type(arg: &clap::Arg) -> &'static str {
    // `Append` is treated as `"string"` because every Vec<_> repeatable in
    // the current CLI is `Vec<String>`. A future Vec<PathBuf> / Vec<u64> flag
    // would be misclassified — extend this match if that lands.
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => "bool",
        ArgAction::Count => "count",
        ArgAction::Append => "string", // Vec<String> repeatable — element type is string
        ArgAction::Set if arg.get_value_parser().possible_values().is_some() => "enum",
        ArgAction::Set => "string",
        _ => "string",
    }
}

fn describe_default(arg: &clap::Arg) -> Option<JsonValue> {
    let defaults = arg.get_default_values();
    if defaults.is_empty() {
        None
    } else if defaults.len() == 1 {
        Some(JsonValue::String(defaults[0].to_string_lossy().to_string()))
    } else {
        Some(JsonValue::Array(
            defaults
                .iter()
                .map(|v| JsonValue::String(v.to_string_lossy().to_string()))
                .collect(),
        ))
    }
}

fn describe_values(arg: &clap::Arg, sub_path: &str) -> Option<JsonValue> {
    if let Some(pv) = arg.get_value_parser().possible_values() {
        let vals: Vec<_> = pv
            .map(|p| JsonValue::String(p.get_name().to_string()))
            .collect();
        if !vals.is_empty() {
            return Some(JsonValue::Array(vals));
        }
    }
    let id = arg.get_id().as_str();
    let scoped = SCOPED_ENUM_VALUES
        .iter()
        .find(|(path, name, _)| *path == sub_path && *name == id)
        .map(|(_, _, vals)| *vals);
    let vals = scoped.or_else(|| {
        ENUM_VALUES
            .iter()
            .find(|(name, _)| *name == id)
            .map(|(_, vals)| *vals)
    })?;
    Some(JsonValue::Array(
        vals.iter()
            .map(|v| JsonValue::String(v.to_string()))
            .collect(),
    ))
}

fn is_repeatable(arg: &clap::Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append | ArgAction::Count)
}

fn describe_mutex_groups(cmd: &Command, sub_path: &str) -> JsonValue {
    // First, walk clap's native ArgGroups. `ArgGroup::is_multiple()` is
    // declared as `&mut self` in clap 4 (see docs.rs/clap), so we clone the
    // borrowed group to query it. ArgGroup: Clone makes this cheap.
    let mut groups: Vec<JsonValue> = cmd
        .get_groups()
        .filter(|g| {
            // ArgGroup::is_multiple is `&mut self` in clap 4 — clone (cheap;
            // ArgGroup: Clone) and bind the clone to a `mut` local so the
            // call typechecks. mutex == !multiple (i.e. at most one allowed).
            let mut tmp = (*g).clone();
            !tmp.is_multiple()
        })
        .map(|g| {
            let names: Vec<_> = g
                .get_args()
                .map(|id| JsonValue::String(id.as_str().to_string()))
                .collect();
            JsonValue::Array(names)
        })
        .collect();

    // Then, append const-supplemented groups for this sub_path that aren't
    // already represented in clap's native get_groups() output.
    let existing_sets: Vec<std::collections::HashSet<String>> = groups
        .iter()
        .filter_map(|g| g.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .collect();
    for (path, group_lists) in MUTEX_GROUPS {
        if *path == sub_path {
            for group in *group_lists {
                let candidate: std::collections::HashSet<String> =
                    group.iter().map(|n| n.to_string()).collect();
                if existing_sets.contains(&candidate) {
                    continue;
                }
                let names: Vec<_> = group
                    .iter()
                    .map(|n| JsonValue::String(n.to_string()))
                    .collect();
                groups.push(JsonValue::Array(names));
            }
        }
    }

    JsonValue::Array(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::on_cli_stack;

    #[test]
    fn build_global_flags_names_every_output_flag() {
        let flags = on_cli_stack(build_global_flags);
        for name in [
            "--select",
            "--limit",
            "--lines",
            "--get",
            "--template",
            "--quiet",
            "--rows",
            "--header",
            "--max-chars",
            "--omit",
            "--where",
            "--where-not",
            "--where-in",
            "--where-has",
            "--where-missing",
            "--where-gt",
            "--where-gte",
            "--where-lt",
            "--where-lte",
            "--where-contains",
            "--where-prefix",
            "--where-suffix",
            "--where-regex",
        ] {
            assert!(
                flags.get(name).is_some(),
                "global_flags must list `{name}`; got {flags}"
            );
        }
        assert!(
            flags.get("--help").is_none() && flags.get("--version").is_none(),
            "global_flags must read the unbuilt root, without clap's auto flags; got {flags}"
        );
    }

    #[test]
    fn build_agent_context_omits_the_global_output_flags() {
        let ctx = on_cli_stack(build_agent_context);
        let flags = ctx
            .get("items")
            .and_then(|v| v.get("subcommands"))
            .and_then(|v| v.get("list"))
            .and_then(|v| v.get("flags"))
            .expect("items list flags present");
        for name in ["--select", "--limit", "--lines", "--get", "--quiet"] {
            assert!(
                flags.get(name).is_none(),
                "`{name}` is global and belongs only under global_flags; got {flags}"
            );
        }
    }

    #[test]
    fn build_agent_context_includes_items_list_count_flag() {
        let ctx = on_cli_stack(build_agent_context);
        let items = ctx.get("items").expect("items subcommand present");
        let subcommands = items.get("subcommands").expect("items has subcommands");
        let list = subcommands.get("list").expect("items list present");
        let flags = list.get("flags").expect("list flags present");
        let count = flags.get("--count").expect("--count flag present");
        assert_eq!(count.get("type").and_then(|v| v.as_str()), Some("bool"));
    }

    #[test]
    fn build_agent_context_emits_items_list_mutex_group() {
        let ctx = on_cli_stack(build_agent_context);
        let list = ctx
            .get("items")
            .and_then(|v| v.get("subcommands"))
            .and_then(|v| v.get("list"))
            .expect("items list present");
        let groups = list
            .get("mutex_groups")
            .and_then(|v| v.as_array())
            .expect("mutex_groups array");
        // The shape mutex must be present — either via clap's native group
        // accessor (`get_groups()` finds the `#[command(group(...))]` on
        // ItemsOp::List) or via the MUTEX_GROUPS const fallback.
        let shape_group: Vec<&str> =
            vec!["count", "count_by", "group_by", "pluck", "count_distinct"];
        let found = groups.iter().any(|g| {
            let names: Vec<&str> = g
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|n| n.as_str())
                .collect();
            names.iter().all(|n| shape_group.contains(n))
                && shape_group.iter().all(|n| names.contains(n))
        });
        assert!(
            found,
            "items list should expose the shape mutex via clap groups or MUTEX_GROUPS"
        );
    }

    #[test]
    fn build_agent_context_repeatable_where_flag() {
        let ctx = on_cli_stack(build_agent_context);
        let list = ctx
            .get("items")
            .and_then(|v| v.get("subcommands"))
            .and_then(|v| v.get("list"))
            .expect("items list present");
        let flags = list.get("flags").expect("flags");
        // The `--where` predicate is repeatable (ArgAction::Append).
        let where_flag = flags.get("--where").expect("--where present");
        assert_eq!(
            where_flag.get("repeatable").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn infer_type_returns_string_for_repeatable_append() {
        fn walk<'a>(cmd: &'a Command, name: &str) -> Option<&'a Command> {
            if cmd.get_name() == name {
                return Some(cmd);
            }
            for sub in cmd.get_subcommands() {
                if let Some(found) = walk(sub, name) {
                    return Some(found);
                }
            }
            None
        }

        on_cli_stack(|| {
            let cmd = <Cli as CommandFactory>::command();
            // `--where` on `items list` is a Vec<String> Append — its inferred
            // type must be "string".
            let items = walk(&cmd, "items").expect("items subcommand present");
            let list = walk(items, "list").expect("items list present");
            let where_arg = list
                .get_arguments()
                .find(|a| a.get_id().as_str() == "where_eq")
                .expect("--where (id where_eq) flag present");
            assert_eq!(infer_type(where_arg), "string");
        });
    }

    #[test]
    fn enum_values_match_value_enum_variants() {
        use crate::cli::ErrorFormat;
        use crate::convert::ScalarType;
        use crate::dedup::DupTier;
        use clap::ValueEnum;

        fn variants_of<T: ValueEnum>() -> Vec<String> {
            T::value_variants()
                .iter()
                .filter_map(|v| v.to_possible_value())
                .map(|pv| pv.get_name().to_string())
                .collect()
        }

        let scalar = variants_of::<ScalarType>();
        let tier = variants_of::<DupTier>();
        let fmt = variants_of::<ErrorFormat>();

        for (name, vals) in ENUM_VALUES {
            let actual: Vec<String> = match *name {
                "ty" => scalar.clone(),
                "tier" => tier.clone(),
                "error_format" => fmt.clone(),
                other => panic!(
                    "ENUM_VALUES references unknown id `{other}` — add a branch to enum_values_match_value_enum_variants"
                ),
            };
            let expected: Vec<String> = vals.iter().map(|s| s.to_string()).collect();
            assert_eq!(
                actual, expected,
                "ENUM_VALUES for `{name}` drifted from <T as ValueEnum>::value_variants(); update the const to match"
            );
        }
    }

    #[test]
    fn scoped_enum_values_match_their_validators() {
        for (path, id, vals) in SCOPED_ENUM_VALUES {
            let source: &[&str] = match (*path, *id) {
                ("flow envelope build", "command") => crate::flow::VALID_COMMANDS,
                ("flow envelope build", "require_artifact") => crate::flow::VALID_ARTIFACTS,
                ("backlog add" | "backlog check" | "backlog list", "kind") => {
                    crate::backlog::schema::KINDS
                }
                ("backlog list", "status") => crate::backlog::schema::STATUSES,
                ("tasks update", "status") => crate::tasks::Status::VOCABULARY,
                ("tasks add", "effort") => crate::tasks::Effort::VOCABULARY,
                other => panic!(
                    "SCOPED_ENUM_VALUES references `{other:?}` — add a branch naming its validator's vocabulary"
                ),
            };
            assert_eq!(
                *vals, source,
                "SCOPED_ENUM_VALUES for `{path} --{id}` drifted from the vocabulary its validator checks"
            );
        }
    }

    #[test]
    fn scoped_enum_values_name_real_args_clap_cannot_enumerate() {
        on_cli_stack(|| {
            let root = <Cli as CommandFactory>::command();
            for (path, id, _) in SCOPED_ENUM_VALUES {
                let sub = path
                    .split(' ')
                    .try_fold(&root, |cmd, name| cmd.find_subcommand(name))
                    .unwrap_or_else(|| {
                        panic!("SCOPED_ENUM_VALUES path `{path}` is not a subcommand")
                    });
                let arg = sub
                    .get_arguments()
                    .find(|a| a.get_id().as_str() == *id)
                    .unwrap_or_else(|| panic!("`{path}` has no argument with clap id `{id}`"));
                assert!(
                    arg.get_value_parser().possible_values().is_none(),
                    "`{path}` `{id}` is enumerated by clap, so its SCOPED_ENUM_VALUES entry is never read"
                );
            }
        });
    }

    #[test]
    fn build_agent_context_scopes_status_values_per_subcommand() {
        let ctx = on_cli_stack(build_agent_context);
        let values = |group: &str, verb: &str| -> Vec<String> {
            ctx[group]["subcommands"][verb]["flags"]["--status"]["values"]
                .as_array()
                .unwrap_or_else(|| panic!("{group} {verb} --status publishes no values"))
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        };
        assert_eq!(
            values("tasks", "update"),
            ["pending", "in-progress", "done", "failed", "deferred"]
        );
        assert_eq!(
            values("backlog", "list"),
            ["open", "promoted", "dismissed", "resolved"]
        );
        assert!(
            ctx["flow"]["subcommands"]["list"]["flags"]["--status"]
                .get("values")
                .is_none(),
            "flow list --status has no vocabulary constant and must not borrow another subcommand's"
        );
    }

    #[test]
    fn a_published_vocabulary_reports_the_enum_type() {
        let ctx = on_cli_stack(build_agent_context);
        let flag = |group: &str, verb: &str, name: &str| {
            ctx[group]["subcommands"][verb]["flags"][name].clone()
        };
        assert_eq!(flag("tasks", "update", "--status")["type"], "enum");
        assert_eq!(flag("tasks", "add", "--effort")["type"], "enum");
        assert_eq!(flag("tasks", "add", "--effort")["values"][0], "S");
        assert_eq!(flag("flow", "list", "--status")["type"], "string");
    }

    #[test]
    fn mutex_groups_paths_match_real_subcommands() {
        let all_paths = on_cli_stack(|| {
            let mut all_paths: Vec<String> = Vec::new();
            walk(&<Cli as CommandFactory>::command(), "", &mut all_paths);
            all_paths
        });

        fn walk(cmd: &Command, parent: &str, out: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                let name = sub.get_name();
                let full = if parent.is_empty() {
                    name.to_string()
                } else {
                    format!("{parent} {name}")
                };
                out.push(full.clone());
                walk(sub, &full, out);
            }
        }

        for (path, _) in MUTEX_GROUPS {
            assert!(
                all_paths.iter().any(|p| p == *path),
                "MUTEX_GROUPS path `{path}` is not a real subcommand path; available paths: {all_paths:?}"
            );
        }
    }

    #[test]
    fn build_agent_context_emits_enum_typed_flag_with_values() {
        let ctx = on_cli_stack(build_agent_context);
        // `set` carries `--type` (ScalarType: str|int|float|bool|date|datetime).
        let set = ctx.get("set").expect("set subcommand present");
        let flags = set.get("flags").expect("set flags present");
        let ty_flag = flags.get("--type").expect("--type flag present in set");
        assert_eq!(ty_flag["type"], serde_json::json!("enum"));
        let values = ty_flag["values"]
            .as_array()
            .expect("--type values array present");
        let names: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
        for v in &["str", "int", "float", "bool", "date", "datetime"] {
            assert!(names.contains(v), "values missing variant `{v}`: {names:?}");
        }
    }
}
