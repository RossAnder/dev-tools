//! Promotion targets — what a `promoted_to` value or a `triage --to`
//! argument names.
//!
//! A value resolves, in order, to an `external:` reference, a flow by slug, a
//! flow by the plan its `context.toml` binds, a repo-relative `.md` plan no
//! flow binds, or `Unknown`. Resolution never errors on a value, so a caller
//! walking many rows reports a reason per row instead of aborting on one.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::flow::{FlowProjection, validate_slug};
use crate::io::{
    advise, join_under, read_dir_sorted, read_toml_reason, relativise, relativise_under,
};

const EXTERNAL_PREFIX: &str = "external:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Flow {
        slug: String,
        /// Empty when the flow's `context.toml` carries no `status`.
        status: String,
        plan_path: Option<String>,
    },
    Plan {
        path: String,
    },
    /// The text after the `external:` prefix.
    External(String),
    Unknown(String),
}

impl Target {
    /// A flow parked at `review` or `complete`, which no new work will reach.
    pub(crate) fn is_closed(&self) -> bool {
        matches!(self, Target::Flow { status, .. } if status == "review" || status == "complete")
    }

    pub(crate) fn stored_value(&self) -> String {
        match self {
            Target::Flow { slug, .. } => slug.clone(),
            Target::Plan { path } => path.clone(),
            Target::External(text) => format!("{EXTERNAL_PREFIX}{text}"),
            Target::Unknown(raw) => raw.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct FlowEntry {
    status: String,
    plan_path: Option<String>,
}

/// An index over `.claude/flows/*/context.toml`, built once per command.
#[derive(Debug)]
pub(crate) struct Resolver {
    root: PathBuf,
    flows: BTreeMap<String, FlowEntry>,
    by_plan: HashMap<String, String>,
}

impl Resolver {
    /// A flow whose `context.toml` cannot be read or parsed is left out of
    /// the index rather than failing the build.
    pub(crate) fn new(root: &Path) -> Result<Resolver> {
        let mut resolver = Resolver {
            root: root.to_path_buf(),
            flows: BTreeMap::new(),
            by_plan: HashMap::new(),
        };
        let flows_dir = root.join(".claude").join("flows");
        if !flows_dir.exists() {
            return Ok(resolver);
        }
        for entry in read_dir_sorted(&flows_dir)? {
            let path = entry.path();
            let Some(slug) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if validate_slug(slug).is_err() {
                continue;
            }
            let ctx = path.join("context.toml");
            if !ctx.is_file() {
                continue;
            }
            // `flow list` reports the same file under `skipped` with the same
            // reason; the triage and reconcile envelopes carry no such slot.
            let doc = match read_toml_reason(&ctx) {
                Ok(doc) => doc,
                Err(reason) => {
                    advise!(
                        "tomlctl: warning: skipped {}: {}",
                        relativise(root, &ctx),
                        reason
                    );
                    continue;
                }
            };
            let Some(proj) = FlowProjection::from_toml_value(&doc) else {
                continue;
            };
            let plan_path = proj.plan_path.as_deref().map(normalise);
            if let Some(plan) = &plan_path {
                // Sorted enumeration, so two flows binding one plan resolve
                // to the first slug on every run.
                resolver
                    .by_plan
                    .entry(plan.clone())
                    .or_insert_with(|| slug.to_string());
            }
            resolver.flows.insert(
                slug.to_string(),
                FlowEntry {
                    status: proj.status.unwrap_or_default(),
                    plan_path,
                },
            );
        }
        Ok(resolver)
    }

    pub(crate) fn resolve(&self, raw: &str) -> Target {
        if let Some(text) = raw.strip_prefix(EXTERNAL_PREFIX) {
            return Target::External(text.to_string());
        }
        if validate_slug(raw).is_ok()
            && let Some(flow) = self.flow(raw)
        {
            return flow;
        }
        if let Some(flow) = self.flow_for_plan(&normalise(raw)) {
            return flow;
        }
        let Some(joined) = join_under(&self.root, raw) else {
            return Target::Unknown(raw.to_string());
        };
        let rel = relativise_under(&self.root, &joined).unwrap_or_else(|| normalise(raw));
        // An absolute spelling of a bound plan only matches once re-anchored.
        if let Some(flow) = self.flow_for_plan(&rel) {
            return flow;
        }
        let is_md = joined
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if is_md && joined.is_file() {
            return Target::Plan { path: rel };
        }
        Target::Unknown(raw.to_string())
    }

    fn flow(&self, slug: &str) -> Option<Target> {
        let entry = self.flows.get(slug)?;
        Some(Target::Flow {
            slug: slug.to_string(),
            status: entry.status.clone(),
            plan_path: entry.plan_path.clone(),
        })
    }

    fn flow_for_plan(&self, plan: &str) -> Option<Target> {
        self.flow(self.by_plan.get(plan)?)
    }
}

/// One spelling per plan path: `/` separators and no leading `./`.
fn normalise(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{with_root, write};

    fn seed_flow(root: &Path, slug: &str, status: &str, plan_path: &str) {
        write(
            root,
            &format!(".claude/flows/{slug}/context.toml"),
            format!("status = \"{status}\"\nplan_path = \"{plan_path}\"\n").as_bytes(),
        );
    }

    fn resolve(root: &Path, raw: &str) -> Target {
        Resolver::new(root).unwrap().resolve(raw)
    }

    #[test]
    fn slug_resolves_to_its_flow() {
        with_root(|root| {
            seed_flow(root, "alpha", "in-progress", "docs/plans/alpha.md");
            let target = resolve(root, "alpha");
            assert_eq!(
                target,
                Target::Flow {
                    slug: "alpha".into(),
                    status: "in-progress".into(),
                    plan_path: Some("docs/plans/alpha.md".into()),
                }
            );
            assert_eq!(target.stored_value(), "alpha");
            assert!(!target.is_closed());
        });
    }

    #[test]
    fn bound_plan_path_resolves_to_the_flow_and_stores_the_slug() {
        with_root(|root| {
            seed_flow(root, "alpha", "draft", "./docs/plans/alpha.md");
            write(root, "docs/plans/alpha.md", b"# Plan\n");
            let abs = root.join("docs").join("plans").join("alpha.md");
            for raw in [
                "docs/plans/alpha.md",
                "./docs/plans/alpha.md",
                "docs\\plans\\alpha.md",
                abs.to_str().unwrap(),
            ] {
                let target = resolve(root, raw);
                assert!(
                    matches!(&target, Target::Flow { slug, .. } if slug == "alpha"),
                    "{raw} resolved to {target:?}"
                );
                assert_eq!(target.stored_value(), "alpha");
            }
        });
    }

    #[test]
    fn unbound_plan_resolves_to_plan() {
        with_root(|root| {
            seed_flow(root, "alpha", "draft", "docs/plans/alpha.md");
            write(root, "docs/plans/loose.md", b"# Plan\n");
            let target = resolve(root, "docs\\plans\\loose.md");
            assert_eq!(
                target,
                Target::Plan {
                    path: "docs/plans/loose.md".into()
                }
            );
            assert_eq!(target.stored_value(), "docs/plans/loose.md");
            assert!(!target.is_closed());
        });
    }

    #[test]
    fn external_prefix_resolves_to_external() {
        with_root(|root| {
            seed_flow(root, "alpha", "draft", "docs/plans/alpha.md");
            let target = resolve(root, "external:alpha");
            assert_eq!(target, Target::External("alpha".into()));
            assert_eq!(target.stored_value(), "external:alpha");
        });
    }

    #[test]
    fn unknown_values_resolve_to_unknown() {
        with_root(|root| {
            write(root, "docs/notes.txt", b"not a plan\n");
            for raw in [
                "task-store-polish",
                "docs/plans/missing.md",
                "docs/notes.txt",
            ] {
                let target = resolve(root, raw);
                assert_eq!(target, Target::Unknown(raw.into()));
                assert_eq!(target.stored_value(), raw);
            }
        });
    }

    #[test]
    fn flows_at_review_or_complete_are_closed() {
        with_root(|root| {
            seed_flow(root, "parked", "review", "docs/plans/parked.md");
            seed_flow(root, "done", "complete", "docs/plans/done.md");
            seed_flow(root, "live", "in-progress", "docs/plans/live.md");
            assert!(resolve(root, "parked").is_closed());
            assert!(resolve(root, "done").is_closed());
            assert!(!resolve(root, "live").is_closed());
        });
    }

    #[test]
    fn escaping_values_resolve_to_unknown_even_when_the_file_exists() {
        with_root(|root| {
            write(root, "docs/plans/loose.md", b"# Plan\n");
            let outside = root.parent().unwrap().join("loose.md");
            for raw in [
                "docs/../docs/plans/loose.md",
                "../loose.md",
                outside.to_str().unwrap(),
                "/etc/loose.md",
            ] {
                assert_eq!(resolve(root, raw), Target::Unknown(raw.into()), "{raw}");
            }
        });
    }

    #[test]
    fn malformed_context_is_skipped_without_failing_the_build() {
        with_root(|root| {
            write(
                root,
                ".claude/flows/broken/context.toml",
                b"status = [unclosed\n",
            );
            seed_flow(root, "alpha", "draft", "docs/plans/alpha.md");
            let resolver = Resolver::new(root).expect("a malformed flow must not fail the build");
            assert_eq!(resolver.resolve("broken"), Target::Unknown("broken".into()));
            assert!(matches!(resolver.resolve("alpha"), Target::Flow { .. }));
        });
    }

    #[test]
    fn missing_flows_dir_gives_an_empty_index() {
        with_root(|root| {
            write(root, "docs/plans/loose.md", b"# Plan\n");
            let resolver = Resolver::new(root).unwrap();
            assert_eq!(resolver.resolve("alpha"), Target::Unknown("alpha".into()));
            assert_eq!(
                resolver.resolve("docs/plans/loose.md"),
                Target::Plan {
                    path: "docs/plans/loose.md".into()
                }
            );
        });
    }
}
