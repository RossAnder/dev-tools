//! Parity between the `tasks` finding-class tables and the classes the engine
//! emits, plus existence for the classes prose names in passing.
//!
//! Nothing at runtime notices a table that has fallen behind: a class the docs
//! omit is still raised, and a class they invent still reads as real to the
//! carrier that trusts them. Both sides are parsed here rather than restated,
//! so this file never becomes a third copy of the list.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Documents whose table claims to be exhaustive, and so can be compared as a
/// set.
const DOCUMENTED: &[&str] = &[
    "claude/skills/flow-contract-task-store/SKILL.md",
    "claude/skills/tomlctl/references/tasks.md",
];

/// Documents naming a class or two in passing. Only existence is checked:
/// holding them to a set would grow each into another copy of the table.
const PROSE: &[&str] = &[
    "claude/commands/review-plan.md",
    "claude/skills/flow-contract-plan-restructure/SKILL.md",
    "claude/skills/flow-contract-plan-output-format/SKILL.md",
];

type Classes = BTreeMap<String, BTreeSet<String>>;

/// A source file's text with any test module cut off — a literal inside a
/// fixture is not a class the CLI can emit.
type Bodies = Vec<(String, String)>;

fn source_bodies(tasks_dir: &Path) -> Bodies {
    let mut paths: Vec<PathBuf> = fs::read_dir(tasks_dir)
        .unwrap_or_else(|err| panic!("{}: {err}", tasks_dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "rs").then_some(path)
        })
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let text =
                fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let body = text.split("#[cfg(test)]").next().unwrap_or_default();
            (path.display().to_string(), body.to_string())
        })
        .collect()
}

/// Every class the engine can raise, mapped to the severities it is raised at
/// — `plan/orphan-row` carries two.
///
/// A class is what reaches a `Finding`, never what merely looks like one: the
/// `class:` field's own literal, or the first argument of a helper declaring a
/// `class: &'static str` parameter, which is how the `policy/*` classes arrive.
/// `unresolved_sites` is what keeps those two shapes exhaustive.
fn classes_in(bodies: &Bodies) -> Classes {
    let helpers = helpers_in(bodies);
    let mut classes = Classes::new();

    for (label, text) in bodies {
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if let Some(class) = field_class(line) {
                let severity = severity_after(&lines, index).unwrap_or_else(|| {
                    panic!(
                        "{label}: no severity resolves for `{class}` — the parser needs \
                         teaching the shape it is raised in"
                    )
                });
                classes.entry(class).or_default().insert(severity);
            }
            for (helper, severity) in &helpers {
                if let Some(class) = call_class(&lines, index, helper, label) {
                    classes.entry(class).or_default().insert(severity.clone());
                }
            }
        }
    }
    classes
}

/// Every `Finding` literal whose class neither of the two read shapes reaches
/// — a third construction shape, which the scan would otherwise leave
/// uncounted and so ungated.
fn unresolved_sites(bodies: &Bodies) -> Vec<String> {
    let helpers = helpers_in(bodies);
    let mut unresolved = Vec::new();
    for (label, text) in bodies {
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains("Finding {") || declares(line) {
                continue;
            }
            let field = lines[index..]
                .iter()
                .take(8)
                .find(|line| line.trim_start().starts_with("class"));
            let resolved = match field {
                None => false,
                Some(field) if field_class(field).is_some() => true,
                Some(_) => lines[..=index]
                    .iter()
                    .rev()
                    .find_map(|line| fn_name(line))
                    .is_some_and(|enclosing| helpers.contains_key(&enclosing)),
            };
            if !resolved {
                unresolved.push(format!(
                    "{label}:{}: a `Finding` states its class in a shape the parser \
                     does not read",
                    index + 1
                ));
            }
        }
    }
    unresolved
}

/// Name to the severity its body sets, for each helper a class is passed to.
fn helpers_in(bodies: &Bodies) -> BTreeMap<String, String> {
    let mut helpers = BTreeMap::new();
    for (label, text) in bodies {
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let Some(name) = fn_name(line) else { continue };
            let Some((signature, end)) = signature_from(&lines, index) else {
                continue;
            };
            if !signature.contains("class: &'static str") {
                continue;
            }
            let params = signature.split_once('(').map(|(_, rest)| rest);
            assert!(
                params.is_some_and(|rest| rest.trim_start().starts_with("class:")),
                "{label}: `{name}` takes a class other than first — the parser reads \
                 the leading argument of a call as the class"
            );
            let severity = lines[end..]
                .iter()
                .take(40)
                .find_map(|line| severity_of(line))
                .unwrap_or_else(|| {
                    panic!("{label}: `{name}` sets no severity the parser can read")
                });
            helpers.insert(name, severity);
        }
    }
    helpers
}

/// The class a `Finding` states as its own field.
fn field_class(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("class:")?;
    first_literal(rest)
}

/// The class a call to `helper` passes, whether inline or wrapped onto the
/// following lines.
fn call_class(lines: &[&str], index: usize, helper: &str, label: &str) -> Option<String> {
    let line = lines[index];
    if line.contains(&format!("fn {helper}(")) {
        return None;
    }
    let rest = call_tail(line, helper)?;
    let class = first_literal(rest).or_else(|| {
        lines[index + 1..]
            .iter()
            .take(3)
            .find_map(|line| first_literal(line))
    });
    let class = class.unwrap_or_else(|| {
        panic!(
            "{label}:{}: `{helper}` is called with no class literal",
            index + 1
        )
    });
    assert!(
        class_shaped(&class),
        "{label}:{}: `{helper}` is passed `{class}`, which is not a class — the \
         parser reads the leading argument of a call as the class",
        index + 1
    );
    Some(class)
}

/// The text after `helper(`, for a call where `helper` is a whole identifier
/// rather than the tail of a longer one.
fn call_tail<'a>(line: &'a str, helper: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(at) = line[from..].find(&format!("{helper}(")) {
        let start = from + at;
        let boundary = line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        if boundary {
            return Some(&line[start + helper.len() + 1..]);
        }
        from = start + helper.len();
    }
    None
}

/// Both the `const ERROR`/`WARNING`/`INFO` spelling and the inline string literal.
fn severity_of(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("severity:")?;
    for severity in ["error", "warning", "info"] {
        if rest.contains(&severity.to_ascii_uppercase()) || rest.contains(severity) {
            return Some(severity.to_string());
        }
    }
    None
}

fn severity_after(lines: &[&str], index: usize) -> Option<String> {
    lines[index..]
        .iter()
        .take(4)
        .find_map(|line| severity_of(line))
}

/// A `Finding` named in a signature or a type is no construction site.
fn declares(line: &str) -> bool {
    let rest = undecorated(line);
    fn_name(line).is_some()
        || ["struct ", "impl ", "enum ", "type ", "trait "]
            .iter()
            .any(|keyword| rest.starts_with(keyword))
}

fn undecorated(line: &str) -> &str {
    let mut rest = line.trim_start();
    for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async ", "const "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    rest
}

fn fn_name(line: &str) -> Option<String> {
    let head = undecorated(line).strip_prefix("fn ")?;
    let name: String = head
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The declaration's text down to the line closing its parameter list, and the
/// index of the line after it.
fn signature_from(lines: &[&str], index: usize) -> Option<(String, usize)> {
    let mut signature = String::new();
    for (offset, line) in lines[index..].iter().take(20).enumerate() {
        signature.push_str(line);
        signature.push('\n');
        let closed = if offset == 0 {
            line.contains(')')
        } else {
            line.trim_start().starts_with(')')
        };
        if closed {
            return Some((signature, index + offset + 1));
        }
    }
    None
}

fn first_literal(text: &str) -> Option<String> {
    let (_, rest) = text.split_once('"')?;
    let (literal, _) = rest.split_once('"')?;
    Some(literal.to_string())
}

fn class_shaped(token: &str) -> bool {
    let Some((group, name)) = token.split_once('/') else {
        return false;
    };
    let segment =
        |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    segment(group) && segment(name)
}

/// Table rows only, keyed on a first cell that is one backticked `group/name`
/// token — which is what keeps a flag table's `` `--slug` `` / `` `--file` ``
/// row and a field table's dotted keys out of the set.
fn documented_classes(text: &str) -> Classes {
    let mut classes = Classes::new();
    for line in text.lines() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(first), Some(second)) = (cells.get(1), cells.get(2)) else {
            continue;
        };
        let Some(class) = class_cell(first) else {
            continue;
        };
        let severities = ["error", "warning", "info"]
            .into_iter()
            .filter(|severity| second.contains(severity))
            .map(str::to_string)
            .collect();
        classes.insert(class, severities);
    }
    classes
}

fn class_cell(cell: &str) -> Option<String> {
    let inner = cell.strip_prefix('`')?.strip_suffix('`')?;
    class_shaped(inner).then(|| inner.to_string())
}

/// Backticked spans outside fenced blocks, where a shell command's path would
/// otherwise read as a class.
fn prose_classes(text: &str) -> BTreeSet<String> {
    let mut fenced = false;
    let mut classes = BTreeSet::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for span in line.split('`').skip(1).step_by(2) {
            if class_shaped(span) {
                classes.insert(span.to_string());
            }
        }
    }
    classes
}

/// Membership only, the other way round from `disagreements`: a name the
/// source does not know is a problem, a name the document omits is not.
fn dangling_names(doc: &str, named: &BTreeSet<String>, known: &BTreeSet<String>) -> Vec<String> {
    let mut problems: Vec<String> = named
        .difference(known)
        .map(|class| format!("{doc}: `{class}` names no class src/tasks knows"))
        .collect();
    if named.is_empty() {
        problems.push(format!(
            "{doc}: names no class at all — it has moved, and this gate now covers nothing"
        ));
    }
    problems
}

fn disagreements(doc: &str, documented: &Classes, source: &Classes) -> Vec<String> {
    let mut problems = Vec::new();
    for (class, severities) in source {
        match documented.get(class) {
            None => problems.push(format!("{doc}: `{class}` is emitted but has no table row")),
            Some(found) if found != severities => problems.push(format!(
                "{doc}: `{class}` is documented {} but emitted {}",
                listed(found),
                listed(severities)
            )),
            Some(_) => {}
        }
    }
    for class in documented.keys() {
        if !source.contains_key(class) {
            problems.push(format!("{doc}: `{class}` is documented but never emitted"));
        }
    }
    problems
}

fn listed(severities: &BTreeSet<String>) -> String {
    severities
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .join(" / ")
}

fn tasks_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("tasks")
}

/// `None` without `claude/`, so a packaged checkout carrying only the crate
/// does not fail on documents it was never shipped.
fn repo_root(test: &str) -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    if !root.join("claude").exists() {
        eprintln!("{test}: claude/ not found, skipping");
        return None;
    }
    Some(root)
}

fn synthetic(text: &str) -> Bodies {
    vec![("synthetic.rs".to_string(), text.to_string())]
}

#[test]
fn documented_finding_classes_match_the_source() {
    let Some(repo_root) = repo_root("documented_finding_classes_match_the_source") else {
        return;
    };
    let bodies = source_bodies(&tasks_dir());
    let source = classes_in(&bodies);

    let mut problems: Vec<String> = unresolved_sites(&bodies);
    for doc in DOCUMENTED {
        let Ok(text) = fs::read_to_string(repo_root.join(doc)) else {
            problems.push(format!("{doc}: not readable"));
            continue;
        };
        problems.extend(disagreements(doc, &documented_classes(&text), &source));
    }

    assert!(
        problems.is_empty(),
        "documented_finding_classes_match_the_source: the finding-class tables \
         disagree with src/tasks, or a `Finding` there is built in a shape the \
         parser does not read:\n  {}",
        problems.join("\n  ")
    );
}

/// Membership, not equality: a document may name one class, or none.
#[test]
fn prose_class_names_exist_in_the_source() {
    let Some(repo_root) = repo_root("prose_class_names_exist_in_the_source") else {
        return;
    };
    // Emitted classes, not every class-shaped token: a name left behind in a
    // comment must not keep a stale prose mention legal.
    let names: BTreeSet<String> = classes_in(&source_bodies(&tasks_dir()))
        .into_keys()
        .collect();

    let mut problems: Vec<String> = Vec::new();
    for doc in PROSE {
        let Ok(text) = fs::read_to_string(repo_root.join(doc)) else {
            problems.push(format!("{doc}: not readable"));
            continue;
        };
        problems.extend(dangling_names(doc, &prose_classes(&text), &names));
    }

    assert!(
        problems.is_empty(),
        "prose_class_names_exist_in_the_source: prose names a class the source \
         does not:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn a_class_shape_is_required_of_both_sides() {
    let source = classes_in(&source_bodies(&tasks_dir()));
    assert_eq!(
        source.get("plan/orphan-row").map(listed).as_deref(),
        Some("error / warning"),
        "the one class raised at two severities"
    );
    assert_eq!(
        source.get("policy/origin-value").map(listed).as_deref(),
        Some("error"),
        "the helper-borne classes are read through the helper's severity"
    );

    let table = "\
| Class | Severity | Meaning |
|---|---|---|
| `dag/cycle` | error | text |
| `plan/orphan-row` | warning / **error** | text |
| `--slug` / `--file` | — | a flag row, not a class |
| `[[checkpoints]].id` | — | a field row, not a class |
";
    let documented = documented_classes(table);
    assert_eq!(
        documented.keys().collect::<Vec<&String>>(),
        vec!["dag/cycle", "plan/orphan-row"]
    );
    assert_eq!(listed(&documented["plan/orphan-row"]), "error / warning");
}

/// Both construction shapes, so a class added in either is still gated.
#[test]
fn a_new_class_at_either_construction_site_is_found() {
    let source = classes_in(&synthetic(
        r#"
fn invented() -> Finding {
    Finding {
        class: "dag/invented",
        severity: ERROR,
        ids: Vec::new(),
        detail: String::new(),
    }
}

fn vocabulary_finding(
    class: &'static str,
    value: &str,
) -> Option<Finding> {
    Some(Finding {
        class,
        severity: WARNING,
        ids: Vec::new(),
        detail: value.to_string(),
    })
}

fn borne() -> Option<Finding> {
    vocabulary_finding(
        "policy/invented",
        "value",
    )
}
"#,
    ));

    assert_eq!(
        source
            .iter()
            .map(|(class, severities)| format!("{class} {}", listed(severities)))
            .collect::<Vec<String>>(),
        vec!["dag/invented error", "policy/invented warning"]
    );

    let table = "\
| Class | Severity | Meaning |
|---|---|---|
| `dag/invented` | error | text |
";
    assert_eq!(
        disagreements("doc.md", &documented_classes(table), &source),
        vec!["doc.md: `policy/invented` is emitted but has no table row"]
    );
}

/// Narrowing the scan to two shapes is only safe while a third is refused, so
/// the refusal is asserted against one.
#[test]
fn a_third_construction_shape_is_refused_rather_than_dropped() {
    let bodies = synthetic(
        r#"
fn smuggled(class: String) -> Finding {
    Finding {
        class: CLASSES[0],
        severity: ERROR,
        ids: Vec::new(),
        detail: class,
    }
}
"#,
    );
    assert!(classes_in(&bodies).is_empty());
    assert_eq!(
        unresolved_sites(&bodies),
        vec!["synthetic.rs:3: a `Finding` states its class in a shape the parser does not read"]
    );
    assert!(unresolved_sites(&source_bodies(&tasks_dir())).is_empty());
}

/// The other direction: shape alone is not a class, so a path, a key, or a
/// vocabulary value raises no drift failure.
#[test]
fn a_same_shaped_literal_that_is_no_class_is_ignored() {
    let source = classes_in(&synthetic(
        r#"
const STORE_DIR: &str = "flows/tasks";

/// A doc comment naming `plan/orphan-row` in passing.
fn paths(store: &Store) -> Vec<String> {
    let key = "policy/checkpoints";
    let route = format!("{}/{}", "review", "apply");
    vec![key.to_string(), route, STORE_DIR.to_string()]
}

fn real(store: &Store) -> Finding {
    Finding {
        class: "dag/real",
        severity: ERROR,
        ids: Vec::new(),
        detail: "checkpoint/near-miss".to_string(),
    }
}
"#,
    ));

    assert_eq!(source.keys().collect::<Vec<&String>>(), vec!["dag/real"]);

    let table = "\
| Class | Severity | Meaning |
|---|---|---|
| `dag/real` | error | text |
";
    assert!(disagreements("doc.md", &documented_classes(table), &source).is_empty());
}

/// The live table passes, so the gate's teeth are only visible against a table
/// that has drifted.
#[test]
fn a_dropped_row_and_a_widened_severity_are_both_named() {
    let severities = |values: &[&str]| -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    };
    let source: Classes = [
        ("dag/cycle".to_string(), severities(&["error"])),
        (
            "plan/orphan-row".to_string(),
            severities(&["error", "warning"]),
        ),
    ]
    .into_iter()
    .collect();

    let table = "\
| Class | Severity | Meaning |
|---|---|---|
| `plan/orphan-row` | warning | text |
| `files/closure` | warning | text |
";
    let problems = disagreements("doc.md", &documented_classes(table), &source);

    assert_eq!(
        problems.iter().map(String::as_str).collect::<Vec<&str>>(),
        vec![
            "doc.md: `dag/cycle` is emitted but has no table row",
            "doc.md: `plan/orphan-row` is documented warning but emitted error / warning",
            "doc.md: `files/closure` is documented but never emitted",
        ]
    );
}

#[test]
fn prose_reads_backticked_names_outside_fences_only() {
    let doc = "\
Text naming `dag/cycle` and `checkpoint/invalid-cut`, plus dag/uncoded bare and
a `claude/skills/thing/SKILL.md` path and a `[[checkpoints]].id` key.

```bash
tomlctl tasks check --slug flows/wired
```
";
    assert_eq!(
        prose_classes(doc).into_iter().collect::<Vec<String>>(),
        vec!["checkpoint/invalid-cut", "dag/cycle"]
    );
}

/// A partial mention passes; a renamed class leaves a name behind that does not.
#[test]
fn a_dangling_prose_name_is_named_and_a_partial_mention_is_not() {
    let known: BTreeSet<String> = ["dag/cycle", "files/closure", "plan/orphan-row"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let named = |values: &[&str]| -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    };

    assert!(dangling_names("doc.md", &named(&["dag/cycle"]), &known).is_empty());
    assert_eq!(
        dangling_names("doc.md", &named(&["dag/cycle", "dag/renamed"]), &known),
        vec!["doc.md: `dag/renamed` names no class src/tasks knows"]
    );
    assert_eq!(
        dangling_names("doc.md", &named(&[]), &known),
        vec!["doc.md: names no class at all — it has moved, and this gate now covers nothing"]
    );
}
