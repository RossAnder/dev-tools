//! The v2 lens catalogue: lenses, facets, methods and checks as markdown files
//! with TOML frontmatter between `+++` lines, a signal and concern-tag
//! vocabulary, and an optional project layer (`config.toml`, a `project:`
//! vocabulary, project lenses and facets) over the built-in root.
//!
//! [`Catalogue::load`] reads both layers and applies the project config;
//! [`Catalogue::validate`] reports every rule the catalogue breaks, one
//! [`CatalogueError`] variant per rule; [`Catalogue::bind_cell`] resolves a
//! cell's concerns to the content each one binds and the facets it attaches.
//!
//! A lens's Owns section names each owned tag as the first inline-code span
//! of its own paragraph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use globset::{Glob, GlobMatcher};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::Table;
use toml::Value as TomlValue;

use super::fields::{Depth, Family, Kind, str_enum};
use super::store::{Binding, Run};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::hex_lower;
use crate::ledgers::LedgerKind;
use crate::query::compile_user_bytes_regex;

str_enum!(EntryType {
    Lens => "lens",
    Facet => "facet",
    Method => "method",
    Check => "check",
});

impl EntryType {
    fn dir(self) -> &'static str {
        match self {
            Self::Lens => "lenses",
            Self::Facet => "facets",
            Self::Method => "methods",
            Self::Check => "checks",
        }
    }
}

str_enum!(Layer {
    Builtin => "builtin",
    Project => "project",
});

str_enum!(Status {
    Experimental => "experimental",
    Stable => "stable",
    Deprecated => "deprecated",
});

str_enum!(Activation {
    Floor => "floor",
    Signal => "signal",
    Candidate => "candidate",
    OptIn => "opt-in",
});

str_enum!(Mode {
    Judgement => "judgement",
    Mechanical => "mechanical",
    Executor => "executor",
});

str_enum!(SliceRule {
    WholeScope => "whole-scope",
    PerSubsystem => "per-subsystem",
    PerFile => "per-file",
    PerTask => "per-task",
});

str_enum!(Executor {
    Tomlctl => "tomlctl",
    Surveyor => "surveyor",
    Verification => "verification",
    Cell => "cell",
});

str_enum!(SignalKind {
    Stack => "stack",
    Diff => "diff",
    Health => "health",
    Judged => "judged",
});

/// The reserved prefix of every signal and concern tag a project adds.
pub(crate) const PROJECT_PREFIX: &str = "project:";

const COMMON_KEYS: &[&str] = &["id", "status", "version"];
const LENS_KEYS: &[&str] = &[
    "families",
    "mode",
    "produces",
    "owns",
    "activation",
    "apply_when",
    "after_wave",
    "methods",
    "slice",
    "v1_category",
    "research_domain",
];
const FACET_KEYS: &[&str] = &["families", "apply_when", "attaches_to"];
const CHECK_KEYS: &[&str] = &[
    "families",
    "executor",
    "fence",
    "produces",
    "apply_when",
    "activation",
    "discover",
];
const APPLY_WHEN_KEYS: &[&str] = &["signals", "paths", "min_depth", "always_at"];
const CONFIG_CHECK_KEYS: &[&str] = &["id", "families", "paths", "command", "fence", "activation"];
const DETECT_KEYS: &[&str] = &[
    "files",
    "contains",
    "manifest",
    "count",
    "window",
    "threshold",
];
/// Frontmatter fields a project `[override.<id>]` may replace; any other key
/// names a body section.
const OVERRIDABLE_FIELDS: &[&str] = &[
    "activation",
    "apply_when",
    "after_wave",
    "slice",
    "attaches_to",
];

/// The lens sections a content hash covers; a facet's, method's or check's
/// hash covers every section.
fn is_versioned(ty: EntryType, key: &str) -> bool {
    ty != EntryType::Lens
        || matches!(key, "question" | "owns" | "does_not_flag")
        || key.starts_with("probes_")
}

/// Every rule the merged catalogue can break. Each carries the file to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatalogueError {
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    NoFrontmatter {
        path: PathBuf,
    },
    BadToml {
        path: PathBuf,
        reason: String,
    },
    Schema {
        path: PathBuf,
        field: String,
        problem: String,
    },
    DuplicateId {
        id: String,
        paths: Vec<PathBuf>,
    },
    CollidesWithBuiltin {
        id: String,
        path: PathBuf,
    },
    UnknownSignal {
        path: PathBuf,
        signal: String,
    },
    UnknownConcern {
        path: PathBuf,
        tag: String,
    },
    OwnsOutsideFamilies {
        path: PathBuf,
        tag: String,
    },
    OwnerCount {
        family: String,
        tag: String,
        owners: Vec<String>,
    },
    OwnsSectionMismatch {
        path: PathBuf,
        owns: Vec<String>,
        named: Vec<String>,
    },
    MissingSection {
        path: PathBuf,
        section: String,
    },
    MissingProbes {
        path: PathBuf,
        family: String,
    },
    UnknownMethod {
        path: PathBuf,
        method: String,
    },
    UnknownAttachTarget {
        path: PathBuf,
        target: String,
    },
    UnknownConfigId {
        path: PathBuf,
        key: String,
        id: String,
    },
    BadConfig {
        path: PathBuf,
        key: String,
        problem: String,
    },
    ProjectPrefix {
        path: PathBuf,
        name: String,
    },
    BadPattern {
        path: PathBuf,
        signal: String,
        reason: String,
    },
    BadV1Category {
        path: PathBuf,
        family: String,
        value: String,
    },
}

impl fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = |path: &PathBuf| path.display().to_string();
        match self {
            Self::Unreadable { path, reason } => write!(f, "{}: unreadable: {reason}", p(path)),
            Self::NoFrontmatter { path } => {
                write!(f, "{}: no TOML frontmatter between `+++` lines", p(path))
            }
            Self::BadToml { path, reason } => write!(f, "{}: invalid TOML: {reason}", p(path)),
            Self::Schema {
                path,
                field,
                problem,
            } => write!(f, "{}: `{field}` {problem}", p(path)),
            Self::DuplicateId { id, paths } => {
                let paths: Vec<String> = paths.iter().map(p).collect();
                write!(
                    f,
                    "id `{id}` is declared more than once: {}",
                    paths.join(", ")
                )
            }
            Self::CollidesWithBuiltin { id, path } => write!(
                f,
                "{}: id `{id}` collides with a built-in entry; use `extend`, `override` or `disable` in config.toml",
                p(path)
            ),
            Self::UnknownSignal { path, signal } => {
                write!(f, "{}: signal `{signal}` is not in the vocabulary", p(path))
            }
            Self::UnknownConcern { path, tag } => {
                write!(
                    f,
                    "{}: concern tag `{tag}` is not in the vocabulary",
                    p(path)
                )
            }
            Self::OwnsOutsideFamilies { path, tag } => write!(
                f,
                "{}: owns `{tag}`, whose families share none with the lens",
                p(path)
            ),
            Self::OwnerCount {
                family,
                tag,
                owners,
            } => write!(
                f,
                "concern `{tag}` in family `{family}` needs exactly one owner, has {}{}",
                owners.len(),
                if owners.is_empty() {
                    String::new()
                } else {
                    format!(": {}", owners.join(", "))
                }
            ),
            Self::OwnsSectionMismatch { path, owns, named } => write!(
                f,
                "{}: the Owns section names [{}] but `owns` is [{}]",
                p(path),
                named.join(", "),
                owns.join(", ")
            ),
            Self::MissingSection { path, section } => {
                write!(f, "{}: missing the `## {section}` section", p(path))
            }
            Self::MissingProbes { path, family } => write!(
                f,
                "{}: declares family `{family}` but has no `## Probes — {family}` section",
                p(path)
            ),
            Self::UnknownMethod { path, method } => {
                write!(f, "{}: method `{method}` does not exist", p(path))
            }
            Self::UnknownAttachTarget { path, target } => {
                write!(
                    f,
                    "{}: attaches to `{target}`, which is not a lens",
                    p(path)
                )
            }
            Self::UnknownConfigId { path, key, id } => {
                write!(
                    f,
                    "{}: `{key}` names `{id}`, which is not an entry",
                    p(path)
                )
            }
            Self::BadConfig { path, key, problem } => write!(f, "{}: `{key}` {problem}", p(path)),
            Self::ProjectPrefix { path, name } => write!(
                f,
                "{}: `{name}` breaks the `{PROJECT_PREFIX}` rule: project signals and tags carry it, built-in ones never do",
                p(path)
            ),
            Self::BadPattern {
                path,
                signal,
                reason,
            } => write!(
                f,
                "{}: signal `{signal}` has a `contains` pattern that does not compile: {reason}",
                p(path)
            ),
            Self::BadV1Category {
                path,
                family,
                value,
            } => write!(
                f,
                "{}: `v1_category` `{value}` is not valid for family `{family}`",
                p(path)
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    pub(crate) heading: String,
    /// The heading lowercased with each run of other characters as `_`:
    /// `Probes — code` is `probes_code`.
    pub(crate) key: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ApplyWhen {
    pub(crate) signals: Vec<String>,
    pub(crate) paths: Vec<String>,
    pub(crate) min_depth: Option<Depth>,
    pub(crate) always_at: Option<Depth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) ty: EntryType,
    pub(crate) id: String,
    pub(crate) layer: Layer,
    pub(crate) path: PathBuf,
    pub(crate) status: Status,
    pub(crate) version: Option<String>,
    pub(crate) families: Vec<Family>,
    pub(crate) mode: Option<Mode>,
    pub(crate) produces: Vec<Kind>,
    pub(crate) owns: Vec<String>,
    pub(crate) activation: Option<Activation>,
    pub(crate) apply_when: ApplyWhen,
    pub(crate) after_wave: Option<u32>,
    pub(crate) methods: Vec<String>,
    pub(crate) slice: Option<SliceRule>,
    /// `(family, category)` pairs as declared; [`Catalogue::validate`] checks them.
    pub(crate) v1_category: Vec<(String, String)>,
    pub(crate) attaches_to: Vec<String>,
    pub(crate) executor: Option<Executor>,
    pub(crate) fence: Option<String>,
    /// Set only on a project `[[checks]]` entry.
    pub(crate) command: Option<String>,
    pub(crate) sections: Vec<Section>,
    pub(crate) hash: String,
    /// Switched off by the project config's `disable`.
    pub(crate) disabled: bool,
    /// Switched on by the project config's `enable`.
    pub(crate) enabled: bool,
}

impl Entry {
    fn new(ty: EntryType, layer: Layer, path: PathBuf, id: String, status: Status) -> Self {
        Self {
            ty,
            id,
            layer,
            path,
            status,
            version: None,
            families: Vec::new(),
            mode: None,
            produces: Vec::new(),
            owns: Vec::new(),
            activation: None,
            apply_when: ApplyWhen::default(),
            after_wave: None,
            methods: Vec::new(),
            slice: None,
            v1_category: Vec::new(),
            attaches_to: Vec::new(),
            executor: None,
            fence: None,
            command: None,
            sections: Vec::new(),
            hash: String::new(),
            disabled: false,
            enabled: false,
        }
    }

    /// The declared activation, except that `experimental` status implies `opt-in`.
    pub(crate) fn effective_activation(&self) -> Option<Activation> {
        if self.status == Status::Experimental {
            Some(Activation::OptIn)
        } else {
            self.activation
        }
    }

    pub(crate) fn section(&self, key: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.key == key)
    }

    pub(crate) fn v1_category_for(&self, family: Family) -> Option<&str> {
        self.v1_category
            .iter()
            .find(|(f, _)| f == family.as_str())
            .map(|(_, c)| c.as_str())
    }

    pub(crate) fn path_text(&self) -> String {
        slash_path(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Signal {
    pub(crate) name: String,
    pub(crate) kind: SignalKind,
    pub(crate) meaning: String,
    /// The declarative detector `review survey` evaluates; absent for `judged`.
    pub(crate) detect: Option<Table>,
    pub(crate) hint: Option<String>,
    pub(crate) layer: Layer,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Concern {
    pub(crate) tag: String,
    pub(crate) meaning: String,
    pub(crate) families: Vec<Family>,
    pub(crate) layer: Layer,
    pub(crate) path: PathBuf,
}

/// The merged `[routing]` globs: the project's are appended to the built-in ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Routing {
    pub(crate) executable: Vec<String>,
    pub(crate) tests: Vec<String>,
}

impl Routing {
    pub(crate) fn compile(&self) -> Result<super::fields::RoutingGlobs> {
        super::fields::RoutingGlobs::new(&self.executable, &self.tests)
    }
}

#[derive(Debug, Clone)]
struct ScopeBlock {
    glob: String,
    matcher: Option<GlobMatcher>,
    disable: Vec<String>,
    facets: Vec<String>,
}

impl ScopeBlock {
    /// A slice entry that is itself a glob matches when the scope's glob matches
    /// its text, so `legacy/**` covers `legacy/src/**`.
    fn covers(&self, slice_entry: &str) -> bool {
        self.matcher
            .as_ref()
            .is_some_and(|m| m.is_match(slice_entry.trim_start_matches("./")))
    }
}

#[derive(Debug, Default)]
struct Config {
    path: PathBuf,
    disable: Vec<String>,
    enable: Vec<String>,
    extend: Vec<(String, Table)>,
    overrides: Vec<(String, Table)>,
    scopes: Vec<ScopeBlock>,
    checks: Vec<Table>,
}

/// A facet attached to a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FacetBinding {
    pub(crate) id: String,
    pub(crate) path: String,
}

/// What [`Catalogue::bind_cell`] resolves a cell to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellBinding {
    /// One per catalogue concern; empty for a probe.
    pub(crate) bound: Vec<Binding>,
    pub(crate) facets: Vec<FacetBinding>,
}

/// [`Catalogue::index`] narrowing: `family` keeps entries declaring it (methods
/// always pass); `signals` fills each row's `matched`.
#[derive(Debug, Clone, Default)]
pub(crate) struct IndexFilter {
    pub(crate) family: Option<Family>,
    pub(crate) signals: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Catalogue {
    entries: Vec<Entry>,
    signals: Vec<Signal>,
    concerns: Vec<Concern>,
    routing: Routing,
    config: Config,
    load_errors: Vec<CatalogueError>,
    tree_sha: String,
    config_sha: Option<String>,
}

impl Catalogue {
    /// Reads the built-in root and, when it exists, the project root. A file
    /// that cannot be read or parsed is reported by [`Catalogue::validate`];
    /// only a missing built-in root is an error here.
    pub(crate) fn load(builtin: &Path, project: Option<&Path>) -> Result<Self> {
        if !builtin.is_dir() {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(builtin.to_path_buf()),
                format!("lens catalogue not found at {}", builtin.display()),
            ));
        }
        let project = project.filter(|p| p.is_dir());
        let mut errors = Vec::new();
        let mut tree: Vec<(String, String)> = Vec::new();
        let mut vocab = Vocabulary::default();

        let builtin_vocab = builtin.join("vocabulary.toml");
        if builtin_vocab.is_file() {
            read_vocabulary(
                &builtin_vocab,
                Layer::Builtin,
                &mut vocab,
                &mut tree,
                &mut errors,
            );
        } else {
            errors.push(CatalogueError::Unreadable {
                path: builtin_vocab,
                reason: "the built-in vocabulary is missing".into(),
            });
        }
        let mut config = Config::default();
        let mut config_sha = None;
        if let Some(root) = project {
            let path = root.join("vocabulary.toml");
            if path.is_file() {
                read_vocabulary(&path, Layer::Project, &mut vocab, &mut tree, &mut errors);
            }
            let path = root.join("config.toml");
            if path.is_file()
                && let Some(text) = read_text(&path, &mut errors)
            {
                config_sha = Some(sha256_hex(text.as_bytes()));
                config = parse_config(&path, &text, &mut errors);
            }
        }

        let mut raws = Vec::new();
        for &ty in EntryType::ALL {
            read_entries(
                builtin,
                ty,
                Layer::Builtin,
                &mut raws,
                &mut tree,
                &mut errors,
            );
        }
        if let Some(root) = project {
            for ty in [EntryType::Lens, EntryType::Facet] {
                read_entries(root, ty, Layer::Project, &mut raws, &mut tree, &mut errors);
            }
        }

        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut entries = Vec::new();
        for mut raw in raws {
            let id = raw
                .front
                .get("id")
                .and_then(TomlValue::as_str)
                .map(str::to_string);
            if let Some(id) = &id {
                apply_config(&mut raw, id, &config, &mut used, &mut errors);
            }
            if let Some(entry) = parse_entry(raw, &mut errors) {
                entries.push(entry);
            }
        }
        for (i, table) in config.checks.iter().enumerate() {
            if let Some(entry) = config_check(&config.path, i, table, &mut errors) {
                entries.push(entry);
            }
        }
        for (key, list) in [("extend", &config.extend), ("override", &config.overrides)] {
            for (id, _) in list {
                if !used.contains(&format!("{key}:{id}")) {
                    errors.push(CatalogueError::UnknownConfigId {
                        path: config.path.clone(),
                        key: key.into(),
                        id: id.clone(),
                    });
                }
            }
        }
        for entry in &mut entries {
            entry.disabled = config.disable.contains(&entry.id);
            entry.enabled = config.enable.contains(&entry.id);
        }

        tree.sort();
        let mut hasher = Sha256::new();
        for (key, digest) in &tree {
            hasher.update(key.as_bytes());
            hasher.update(b"\0");
            hasher.update(digest.as_bytes());
            hasher.update(b"\n");
        }

        Ok(Self {
            entries,
            signals: vocab.signals,
            concerns: vocab.concerns,
            routing: vocab.routing,
            config,
            load_errors: errors,
            tree_sha: hex_lower(&hasher.finalize()),
            config_sha,
        })
    }

    /// The errors [`Catalogue::load`] met reading the layers: files it could
    /// not read or parse, and config ids it could not apply. A subset of
    /// [`Catalogue::validate`].
    pub(crate) fn load_errors(&self) -> &[CatalogueError] {
        &self.load_errors
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The first entry with `id`; [`Catalogue::validate`] refuses a second.
    pub(crate) fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub(crate) fn signals(&self) -> &[Signal] {
        &self.signals
    }

    /// Sha256 over every vocabulary and entry file of both layers, with line
    /// endings normalised; the project `config.toml` is [`Catalogue::config_sha`].
    pub(crate) fn tree_sha(&self) -> &str {
        &self.tree_sha
    }

    pub(crate) fn config_sha(&self) -> Option<&str> {
        self.config_sha.as_deref()
    }

    /// The signals `review survey` evaluates: every kind except `judged`.
    pub(crate) fn detectors(&self) -> Vec<&Signal> {
        self.signals
            .iter()
            .filter(|s| s.kind != SignalKind::Judged)
            .collect()
    }

    pub(crate) fn routing(&self) -> &Routing {
        &self.routing
    }

    /// Every (family, tag) the vocabulary declares, with the non-deprecated
    /// lenses owning it. A valid catalogue has exactly one owner per key.
    pub(crate) fn owners(&self) -> BTreeMap<(String, String), Vec<String>> {
        let mut map: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for concern in &self.concerns {
            for family in &concern.families {
                map.entry((family.as_str().to_string(), concern.tag.clone()))
                    .or_default();
            }
        }
        for lens in self.lenses().filter(|e| e.status != Status::Deprecated) {
            for tag in &lens.owns {
                let Some(concern) = self.concern(tag) else {
                    continue;
                };
                for family in lens
                    .families
                    .iter()
                    .filter(|f| concern.families.contains(f))
                {
                    map.entry((family.as_str().to_string(), tag.clone()))
                        .or_default()
                        .push(lens.id.clone());
                }
            }
        }
        map
    }

    /// One row per entry, in catalogue order.
    pub(crate) fn index(&self, filter: &IndexFilter) -> Vec<JsonValue> {
        self.entries
            .iter()
            .filter(|e| match filter.family {
                Some(family) => e.ty == EntryType::Method || e.families.contains(&family),
                None => true,
            })
            .map(|e| {
                let matched: Vec<&String> = e
                    .apply_when
                    .signals
                    .iter()
                    .filter(|s| filter.signals.contains(s))
                    .collect();
                json!({
                    "type": e.ty.as_str(),
                    "id": e.id,
                    "hash": e.hash,
                    "status": e.status.as_str(),
                    "families": e.families.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
                    "mode": e.mode.map(Mode::as_str),
                    "produces": e.produces.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
                    "activation": e.effective_activation().map(Activation::as_str),
                    "apply_when": {
                        "signals": e.apply_when.signals,
                        "paths": e.apply_when.paths,
                        "min_depth": e.apply_when.min_depth.map(Depth::as_str),
                        "always_at": e.apply_when.always_at.map(Depth::as_str),
                    },
                    "matched": matched,
                    "attaches_to": e.attaches_to,
                    "path": e.path_text(),
                    "layer": e.layer.as_str(),
                    "disabled": e.disabled,
                })
            })
            .collect()
    }

    /// Every rule the merged catalogue breaks; empty when it is valid.
    pub(crate) fn validate(&self) -> Vec<CatalogueError> {
        let mut errors = self.load_errors.clone();
        self.validate_ids(&mut errors);
        self.validate_vocabulary(&mut errors);
        for entry in &self.entries {
            self.validate_entry(entry, &mut errors);
        }
        for ((family, tag), owners) in self.owners() {
            if owners.len() != 1 {
                errors.push(CatalogueError::OwnerCount {
                    family,
                    tag,
                    owners,
                });
            }
        }
        self.validate_config(&mut errors);
        errors
    }

    /// Resolves a cell's concerns — 1–3 lenses of one mode (2 at `deep`), one
    /// `probe:<slug>`, or 1–3 `check:<id>` run by a cell (exempt from the depth
    /// maximum, no facets) — to their bindings,
    /// and attaches every facet whose signal roots the slice meets and whose
    /// `attaches_to` names one of the lenses, with project scope blocks
    /// applied per slice entry and then `facet_edits` (`+id` / `-id`).
    pub(crate) fn bind_cell(
        &self,
        concerns: &[String],
        family: Family,
        signals_with_roots: &[(String, Vec<String>)],
        slice: &[String],
        facet_edits: &[String],
        depth: Depth,
    ) -> Result<CellBinding> {
        let Some(first) = concerns.first() else {
            return Err(refuse("a cell needs at least one concern".into()));
        };
        let mut bound = Vec::new();
        let mut lenses: Vec<&str> = Vec::new();
        if let Some(slug) = first.strip_prefix("probe:") {
            if concerns.len() != 1 {
                return Err(refuse(format!(
                    "`{first}` cannot be bundled with other concerns"
                )));
            }
            if !is_id(slug) {
                return Err(refuse(format!(
                    "`{first}`: a probe slug is lowercase letters, digits and `-`"
                )));
            }
        } else if first.starts_with("check:") {
            if concerns.len() > 3 {
                return Err(refuse(format!(
                    "a cell holds at most 3 checks, got {}",
                    concerns.len()
                )));
            }
            let mut seen: Vec<&str> = Vec::new();
            for concern in concerns {
                let Some(check_id) = concern.strip_prefix("check:") else {
                    return Err(refuse(format!("`{concern}` cannot be bundled with checks")));
                };
                if seen.contains(&check_id) {
                    return Err(refuse(format!("`{concern}` is listed twice")));
                }
                seen.push(check_id);
                let check = self
                    .entries
                    .iter()
                    .find(|e| e.ty == EntryType::Check && e.id == check_id)
                    .ok_or_else(|| refuse(format!("`{concern}`: no such check")))?;
                if check.executor != Some(Executor::Cell) {
                    return Err(refuse(format!("`{concern}` is not run by a cell")));
                }
                self.check_usable(check, concern, family)?;
                bound.push(binding(concern, check, family));
            }
        } else {
            let max = if depth == Depth::Deep { 2 } else { 3 };
            if concerns.len() > max {
                return Err(refuse(format!(
                    "a cell at `{}` holds at most {max} concerns, got {}",
                    depth.as_str(),
                    concerns.len()
                )));
            }
            let mut mode = None;
            for concern in concerns {
                if concern.starts_with("probe:") || concern.starts_with("check:") {
                    return Err(refuse(format!("`{concern}` cannot be bundled with lenses")));
                }
                if lenses.contains(&concern.as_str()) {
                    return Err(refuse(format!("`{concern}` is listed twice")));
                }
                let lens = self
                    .lenses()
                    .find(|e| e.id == *concern)
                    .ok_or_else(|| refuse(format!("`{concern}` is not a lens")))?;
                self.check_usable(lens, concern, family)?;
                if lens.apply_when.min_depth.is_some_and(|min| min > depth) {
                    return Err(refuse(format!(
                        "`{concern}` needs at least depth `{}`",
                        lens.apply_when.min_depth.map_or("", Depth::as_str)
                    )));
                }
                if mode.is_some_and(|m| Some(m) != lens.mode) {
                    return Err(refuse(format!(
                        "`{concern}` is a {} lens; a cell bundles one mode",
                        lens.mode.map_or("?", Mode::as_str)
                    )));
                }
                mode = lens.mode;
                for entry in slice {
                    if let Some(scope) = self
                        .config
                        .scopes
                        .iter()
                        .find(|s| s.covers(entry) && s.disable.contains(concern))
                    {
                        return Err(refuse(format!(
                            "`{concern}` is disabled under `{}`, which covers `{entry}`",
                            scope.glob
                        )));
                    }
                }
                bound.push(binding(concern, lens, family));
                lenses.push(concern);
            }
        }
        let facets = self.attach_facets(&lenses, family, signals_with_roots, slice, facet_edits)?;
        Ok(CellBinding { bound, facets })
    }

    fn lenses(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.ty == EntryType::Lens)
    }

    fn concern(&self, tag: &str) -> Option<&Concern> {
        self.concerns.iter().find(|c| c.tag == tag)
    }

    fn check_usable(&self, entry: &Entry, name: &str, family: Family) -> Result<()> {
        if entry.status == Status::Deprecated {
            return Err(refuse(format!(
                "`{name}` is deprecated and refused in new cells"
            )));
        }
        if entry.disabled {
            return Err(refuse(format!(
                "`{name}` is disabled by the project config"
            )));
        }
        if !entry.families.contains(&family) {
            return Err(refuse(format!(
                "`{name}` does not serve family `{}`",
                family.as_str()
            )));
        }
        Ok(())
    }

    fn attach_facets(
        &self,
        lenses: &[&str],
        family: Family,
        signals_with_roots: &[(String, Vec<String>)],
        slice: &[String],
        facet_edits: &[String],
    ) -> Result<Vec<FacetBinding>> {
        let eligible: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| {
                e.ty == EntryType::Facet
                    && !e.disabled
                    && e.status != Status::Deprecated
                    && e.families.contains(&family)
                    && e.attaches_to
                        .iter()
                        .any(|t| t == "*" || lenses.contains(&t.as_str()))
            })
            .collect();
        let roots = |facet: &Entry| -> Vec<&str> {
            signals_with_roots
                .iter()
                .filter(|(name, _)| facet.apply_when.signals.contains(name))
                .flat_map(|(_, roots)| roots.iter().map(String::as_str))
                .collect()
        };
        let mut chosen: BTreeSet<&str> = BTreeSet::new();
        for entry in slice {
            let mut here: BTreeSet<&str> = eligible
                .iter()
                .filter(|f| roots(f).iter().any(|root| meets(entry, root)))
                .map(|f| f.id.as_str())
                .collect();
            for scope in self.config.scopes.iter().filter(|s| s.covers(entry)) {
                for edit in &scope.facets {
                    match edit.split_at_checked(1) {
                        Some(("+", id)) => {
                            if let Some(f) = eligible.iter().find(|f| f.id == id) {
                                here.insert(f.id.as_str());
                            }
                        }
                        Some(("-", id)) => {
                            here.remove(id);
                        }
                        _ => {}
                    }
                }
            }
            chosen.extend(here);
        }
        for edit in facet_edits {
            let (sign, id) = edit
                .split_at_checked(1)
                .filter(|(sign, _)| matches!(*sign, "+" | "-"))
                .ok_or_else(|| refuse(format!("facet edit `{edit}` must be `+id` or `-id`")))?;
            let facet = self
                .entries
                .iter()
                .find(|e| e.ty == EntryType::Facet && e.id == id)
                .ok_or_else(|| refuse(format!("facet edit `{edit}`: no such facet")))?;
            if sign == "+" {
                self.check_usable(facet, id, family)?;
                chosen.insert(facet.id.as_str());
            } else {
                chosen.remove(id);
            }
        }
        Ok(self
            .entries
            .iter()
            .filter(|e| e.ty == EntryType::Facet && chosen.contains(e.id.as_str()))
            .map(|e| FacetBinding {
                id: e.id.clone(),
                path: e.path_text(),
            })
            .collect())
    }

    fn validate_ids(&self, errors: &mut Vec<CatalogueError>) {
        let mut by_id: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
        for entry in &self.entries {
            by_id.entry(entry.id.as_str()).or_default().push(entry);
        }
        for (id, entries) in by_id.into_iter().filter(|(_, e)| e.len() > 1) {
            let builtin = entries.iter().any(|e| e.layer == Layer::Builtin);
            let project: Vec<&&Entry> = entries
                .iter()
                .filter(|e| e.layer == Layer::Project)
                .collect();
            if builtin && !project.is_empty() {
                for entry in project {
                    errors.push(CatalogueError::CollidesWithBuiltin {
                        id: id.to_string(),
                        path: entry.path.clone(),
                    });
                }
            } else {
                errors.push(CatalogueError::DuplicateId {
                    id: id.to_string(),
                    paths: entries.iter().map(|e| e.path.clone()).collect(),
                });
            }
        }
    }

    fn validate_vocabulary(&self, errors: &mut Vec<CatalogueError>) {
        let named = self
            .signals
            .iter()
            .map(|s| (s.name.as_str(), s.layer, &s.path))
            .chain(
                self.concerns
                    .iter()
                    .map(|c| (c.tag.as_str(), c.layer, &c.path)),
            );
        for (name, layer, path) in named {
            if name.starts_with(PROJECT_PREFIX) != (layer == Layer::Project) {
                errors.push(CatalogueError::ProjectPrefix {
                    path: path.clone(),
                    name: name.to_string(),
                });
            }
        }
        for signal in &self.signals {
            let Some(contains) = signal.detect.as_ref().and_then(|d| d.get("contains")) else {
                continue;
            };
            let patterns: Vec<&str> = match contains {
                TomlValue::String(s) => vec![s.as_str()],
                TomlValue::Array(items) => items.iter().filter_map(TomlValue::as_str).collect(),
                _ => Vec::new(),
            };
            for pattern in patterns {
                if let Err(e) = compile_user_bytes_regex(pattern) {
                    errors.push(CatalogueError::BadPattern {
                        path: signal.path.clone(),
                        signal: signal.name.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    fn validate_entry(&self, entry: &Entry, errors: &mut Vec<CatalogueError>) {
        let path = || entry.path.clone();
        for signal in &entry.apply_when.signals {
            if !self.signals.iter().any(|s| s.name == *signal) {
                errors.push(CatalogueError::UnknownSignal {
                    path: path(),
                    signal: signal.clone(),
                });
            }
        }
        match entry.ty {
            EntryType::Lens => {
                for tag in &entry.owns {
                    if entry.layer == Layer::Project && !tag.starts_with(PROJECT_PREFIX) {
                        errors.push(CatalogueError::ProjectPrefix {
                            path: path(),
                            name: tag.clone(),
                        });
                    }
                    match self.concern(tag) {
                        None => errors.push(CatalogueError::UnknownConcern {
                            path: path(),
                            tag: tag.clone(),
                        }),
                        Some(c) if !entry.families.iter().any(|f| c.families.contains(f)) => {
                            errors.push(CatalogueError::OwnsOutsideFamilies {
                                path: path(),
                                tag: tag.clone(),
                            });
                        }
                        Some(_) => {}
                    }
                }
                for (key, heading) in [("question", "Question"), ("does_not_flag", "Does not flag")]
                {
                    if entry.section(key).is_none() {
                        errors.push(CatalogueError::MissingSection {
                            path: path(),
                            section: heading.into(),
                        });
                    }
                }
                match entry.section("owns") {
                    None => errors.push(CatalogueError::MissingSection {
                        path: path(),
                        section: "Owns".into(),
                    }),
                    Some(section) => {
                        let named = owned_tags(&section.body);
                        let owns: BTreeSet<String> = entry.owns.iter().cloned().collect();
                        if named != owns {
                            errors.push(CatalogueError::OwnsSectionMismatch {
                                path: path(),
                                owns: owns.into_iter().collect(),
                                named: named.into_iter().collect(),
                            });
                        }
                    }
                }
                for family in &entry.families {
                    if entry
                        .section(&format!("probes_{}", family.as_str()))
                        .is_none()
                    {
                        errors.push(CatalogueError::MissingProbes {
                            path: path(),
                            family: family.as_str().into(),
                        });
                    }
                }
                for method in &entry.methods {
                    if !self
                        .entries
                        .iter()
                        .any(|e| e.ty == EntryType::Method && e.id == *method)
                    {
                        errors.push(CatalogueError::UnknownMethod {
                            path: path(),
                            method: method.clone(),
                        });
                    }
                }
                for (family, value) in &entry.v1_category {
                    let kind = match Family::parse(family) {
                        Some(f @ Family::Code) if entry.families.contains(&f) => {
                            Some(LedgerKind::Review)
                        }
                        Some(f @ Family::Perf) if entry.families.contains(&f) => {
                            Some(LedgerKind::Optimise)
                        }
                        _ => None,
                    };
                    let allowed = kind.and_then(LedgerKind::categories).unwrap_or_default();
                    if !allowed.contains(&value.as_str()) {
                        errors.push(CatalogueError::BadV1Category {
                            path: path(),
                            family: family.clone(),
                            value: value.clone(),
                        });
                    }
                }
            }
            EntryType::Facet => {
                for target in &entry.attaches_to {
                    if target != "*" && !self.lenses().any(|e| e.id == *target) {
                        errors.push(CatalogueError::UnknownAttachTarget {
                            path: path(),
                            target: target.clone(),
                        });
                    }
                }
            }
            EntryType::Method | EntryType::Check => {}
        }
    }

    fn validate_config(&self, errors: &mut Vec<CatalogueError>) {
        let config = &self.config;
        let unknown = |key: &str, id: &str| CatalogueError::UnknownConfigId {
            path: config.path.clone(),
            key: key.to_string(),
            id: id.to_string(),
        };
        for (key, ids) in [("disable", &config.disable), ("enable", &config.enable)] {
            for id in ids {
                if self.entry(id).is_none() {
                    errors.push(unknown(key, id));
                }
            }
        }
        for id in config
            .enable
            .iter()
            .filter(|id| config.disable.contains(id))
        {
            errors.push(CatalogueError::BadConfig {
                path: config.path.clone(),
                key: "enable".into(),
                problem: format!("names `{id}`, which `disable` also names; keep one"),
            });
        }
        for scope in &config.scopes {
            let key = format!("scope.{:?}", scope.glob);
            for id in &scope.disable {
                if self.entry(id).is_none() {
                    errors.push(unknown(&format!("{key}.disable"), id));
                }
            }
            for edit in &scope.facets {
                let id = edit.strip_prefix(['+', '-']);
                let facet = id.and_then(|id| {
                    self.entries
                        .iter()
                        .find(|e| e.ty == EntryType::Facet && e.id == id)
                });
                if facet.is_none() {
                    errors.push(unknown(&format!("{key}.facets"), edit));
                }
            }
        }
    }
}

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn binding(concern: &str, entry: &Entry, family: Family) -> Binding {
    Binding {
        concern: concern.to_string(),
        hash: entry.hash.clone(),
        path: entry.path_text(),
        methods: entry.methods.clone(),
        v1_category: entry
            .v1_category_for(family)
            .unwrap_or_default()
            .to_string(),
    }
}

fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

/// Whether a slice entry (a path or a glob) and a signal root share a
/// subtree. A root of `` or `.` is the repository root.
fn meets(slice_entry: &str, root: &str) -> bool {
    let root = root.trim_start_matches("./").trim_end_matches('/');
    if root.is_empty() || root == "." {
        return true;
    }
    let entry = slice_entry.trim_start_matches("./");
    let dir = match entry.find(['*', '?', '[', '{']) {
        Some(i) => entry[..i].rfind('/').map_or("", |j| &entry[..j]),
        None => entry.trim_end_matches('/'),
    };
    dir.is_empty() || within(dir, root) || within(root, dir)
}

fn within(path: &str, dir: &str) -> bool {
    path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The first inline-code span of each paragraph.
fn owned_tags(body: &str) -> BTreeSet<String> {
    body.split("\n\n")
        .filter_map(|para| {
            let start = para.find('`')? + 1;
            let len = para[start..].find('`')?;
            Some(para[start..start + len].to_string())
        })
        .collect()
}

fn heading_key(heading: &str) -> String {
    let mut key = String::new();
    for c in heading.chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c.to_ascii_lowercase());
        } else if !key.is_empty() && !key.ends_with('_') {
            key.push('_');
        }
    }
    key.trim_end_matches('_').to_string()
}

/// Trailing whitespace dropped from every line and blank lines from both ends.
fn tidy(body: &str) -> String {
    let lines: Vec<&str> = body.lines().map(str::trim_end).collect();
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

fn normalise(text: &str) -> String {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let mut lines = text.split_inclusive('\n');
    if lines.next()?.trim_end() != "+++" {
        return None;
    }
    let start = text.find('\n')? + 1;
    let mut offset = start;
    for line in lines {
        if line.trim_end() == "+++" {
            return Some((&text[start..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// The body's `##` sections; a `##` line inside a fenced block is body text.
fn parse_sections(body: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        match fence {
            Some(marker) if trimmed.starts_with(marker) => fence = None,
            Some(_) => {}
            None if trimmed.starts_with("```") => fence = Some("```"),
            None if trimmed.starts_with("~~~") => fence = Some("~~~"),
            None => {
                if let Some(heading) = line.strip_prefix("## ") {
                    let heading = heading.trim().to_string();
                    sections.push(Section {
                        key: heading_key(&heading),
                        heading,
                        body: String::new(),
                    });
                    continue;
                }
            }
        }
        if let Some(section) = sections.last_mut() {
            section.body.push_str(line);
            section.body.push('\n');
        }
    }
    for section in &mut sections {
        section.body = tidy(&section.body);
    }
    sections
}

fn content_hash(ty: EntryType, sections: &[Section]) -> String {
    let mut hasher = Sha256::new();
    for section in sections.iter().filter(|s| is_versioned(ty, &s.key)) {
        hasher.update(section.heading.as_bytes());
        hasher.update(b"\n");
        hasher.update(section.body.as_bytes());
        hasher.update(b"\n\0");
    }
    hex_lower(&hasher.finalize())
}

fn read_text(path: &Path, errors: &mut Vec<CatalogueError>) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(normalise(&text)),
        Err(e) => {
            errors.push(CatalogueError::Unreadable {
                path: path.to_path_buf(),
                reason: e.to_string(),
            });
            None
        }
    }
}

fn parse_toml(path: &Path, text: &str, errors: &mut Vec<CatalogueError>) -> Option<Table> {
    match toml::from_str::<Table>(text) {
        Ok(table) => Some(table),
        Err(e) => {
            errors.push(CatalogueError::BadToml {
                path: path.to_path_buf(),
                reason: e.to_string().trim().to_string(),
            });
            None
        }
    }
}

struct RawEntry {
    ty: EntryType,
    layer: Layer,
    path: PathBuf,
    front: Table,
    sections: Vec<Section>,
}

fn read_entries(
    root: &Path,
    ty: EntryType,
    layer: Layer,
    raws: &mut Vec<RawEntry>,
    tree: &mut Vec<(String, String)>,
    errors: &mut Vec<CatalogueError>,
) {
    let dir = root.join(ty.dir());
    let Ok(listing) = fs::read_dir(&dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = listing
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();
    for path in paths {
        let Some(text) = read_text(&path, errors) else {
            continue;
        };
        let rel = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        tree.push((
            format!("{}:{}/{rel}", layer.as_str(), ty.dir()),
            sha256_hex(text.as_bytes()),
        ));
        let Some((front, body)) = split_frontmatter(&text) else {
            errors.push(CatalogueError::NoFrontmatter { path });
            continue;
        };
        let Some(front) = parse_toml(&path, front, errors) else {
            continue;
        };
        let sections = parse_sections(body);
        raws.push(RawEntry {
            ty,
            layer,
            path,
            front,
            sections,
        });
    }
}

/// Applies the config's `override` and `extend` for `id`, recording each
/// applied table in `used` as `<key>:<id>`.
fn apply_config(
    raw: &mut RawEntry,
    id: &str,
    config: &Config,
    used: &mut BTreeSet<String>,
    errors: &mut Vec<CatalogueError>,
) {
    let bad = |key: String, problem: String| CatalogueError::BadConfig {
        path: config.path.clone(),
        key,
        problem,
    };
    for (_, table) in config.overrides.iter().filter(|(i, _)| i == id) {
        used.insert(format!("override:{id}"));
        for (key, value) in table {
            if OVERRIDABLE_FIELDS.contains(&key.as_str()) {
                raw.front.insert(key.clone(), value.clone());
                continue;
            }
            let Some(section) = raw.sections.iter_mut().find(|s| s.key == *key) else {
                errors.push(bad(
                    format!("override.{id}.{key}"),
                    "names neither an overridable field nor a section of the entry".into(),
                ));
                continue;
            };
            match value.as_str() {
                Some(text) => section.body = tidy(&normalise(text)),
                None => errors.push(bad(
                    format!("override.{id}.{key}"),
                    "must be a string".into(),
                )),
            }
        }
    }
    for (_, table) in config.extend.iter().filter(|(i, _)| i == id) {
        used.insert(format!("extend:{id}"));
        for (key, value) in table {
            let at = format!("extend.{id}.{key}");
            let Some(items) = value
                .as_array()
                .and_then(|a| a.iter().map(TomlValue::as_str).collect::<Option<Vec<_>>>())
            else {
                errors.push(bad(at, "must be an array of strings".into()));
                continue;
            };
            let mut targets = raw
                .sections
                .iter_mut()
                .filter(|s| s.key == *key || (key == "probes" && s.key.starts_with("probes_")))
                .peekable();
            if targets.peek().is_none() {
                errors.push(bad(at, "names no section of the entry".into()));
                continue;
            }
            for section in targets {
                for item in &items {
                    if !section.body.is_empty() {
                        section.body.push('\n');
                    }
                    section.body.push_str("- ");
                    section.body.push_str(item.trim());
                }
            }
        }
    }
}

/// Typed reads of one frontmatter or vocabulary table; every problem is
/// recorded against `path` and reading carries on.
struct Front<'a> {
    table: &'a Table,
    path: &'a Path,
    prefix: String,
    errors: &'a mut Vec<CatalogueError>,
}

impl<'a> Front<'a> {
    fn new(
        table: &'a Table,
        path: &'a Path,
        prefix: String,
        errors: &'a mut Vec<CatalogueError>,
    ) -> Self {
        Self {
            table,
            path,
            prefix,
            errors,
        }
    }

    fn problem(&mut self, field: &str, problem: impl Into<String>) {
        self.errors.push(CatalogueError::Schema {
            path: self.path.to_path_buf(),
            field: format!("{}{field}", self.prefix),
            problem: problem.into(),
        });
    }

    fn only(&mut self, allowed: &[&str], what: &str) {
        let table: &'a Table = self.table;
        for key in table.keys() {
            if !allowed.contains(&key.as_str()) {
                self.problem(key, format!("is not a field of a {what}"));
            }
        }
    }

    fn text(&mut self, key: &str, required: bool) -> Option<String> {
        let table: &'a Table = self.table;
        match table.get(key) {
            None => {
                if required {
                    self.problem(key, "is required");
                }
                None
            }
            Some(TomlValue::String(s)) => Some(s.clone()),
            Some(_) => {
                self.problem(key, "must be a string");
                None
            }
        }
    }

    fn texts(&mut self, key: &str, required: bool) -> Vec<String> {
        let table: &'a Table = self.table;
        match table.get(key) {
            None => {
                if required {
                    self.problem(key, "is required");
                }
                Vec::new()
            }
            Some(TomlValue::Array(items)) => {
                let Some(out) = items
                    .iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
                else {
                    self.problem(key, "must be an array of strings");
                    return Vec::new();
                };
                if required && out.is_empty() {
                    self.problem(key, "must not be empty");
                }
                out
            }
            Some(_) => {
                self.problem(key, "must be an array of strings");
                Vec::new()
            }
        }
    }

    fn word<E>(
        &mut self,
        key: &str,
        required: bool,
        parse: fn(&str) -> Option<E>,
        values: &[&str],
    ) -> Option<E> {
        let text = self.text(key, required)?;
        let parsed = parse(&text);
        if parsed.is_none() {
            self.problem(
                key,
                format!("is {text:?}, not one of {}", values.join(", ")),
            );
        }
        parsed
    }

    fn words<E>(
        &mut self,
        key: &str,
        required: bool,
        parse: fn(&str) -> Option<E>,
        values: &[&str],
    ) -> Vec<E> {
        let mut out = Vec::new();
        for text in self.texts(key, required) {
            match parse(&text) {
                Some(value) => out.push(value),
                None => self.problem(
                    key,
                    format!("holds {text:?}, not one of {}", values.join(", ")),
                ),
            }
        }
        out
    }

    fn number(&mut self, key: &str) -> Option<u32> {
        let table: &'a Table = self.table;
        match table.get(key) {
            None => None,
            Some(TomlValue::Integer(n)) if u32::try_from(*n).is_ok() => u32::try_from(*n).ok(),
            Some(_) => {
                self.problem(key, "must be a non-negative integer");
                None
            }
        }
    }

    fn globs(&mut self, key: &str) -> Vec<String> {
        let globs = self.texts(key, false);
        for glob in &globs {
            if let Err(e) = Glob::new(glob) {
                self.problem(key, format!("holds an invalid glob {glob:?}: {e}"));
            }
        }
        globs
    }

    fn apply_when(&mut self, required: bool) -> ApplyWhen {
        let table: &'a Table = self.table;
        let Some(value) = table.get("apply_when") else {
            if required {
                self.problem("apply_when", "is required");
            }
            return ApplyWhen::default();
        };
        let Some(inner) = value.as_table() else {
            self.problem("apply_when", "must be a table");
            return ApplyWhen::default();
        };
        let mut nested = Front::new(
            inner,
            self.path,
            format!("{}apply_when.", self.prefix),
            self.errors,
        );
        nested.only(APPLY_WHEN_KEYS, "apply_when table");
        ApplyWhen {
            signals: nested.texts("signals", required),
            paths: nested.globs("paths"),
            min_depth: nested.word("min_depth", false, Depth::parse, Depth::VALUES),
            always_at: nested.word("always_at", false, Depth::parse, Depth::VALUES),
        }
    }

    /// A string applies to every declared family but `plan`; a table maps
    /// family to category.
    fn v1_category(&mut self, families: &[Family]) -> Vec<(String, String)> {
        let table: &'a Table = self.table;
        match table.get("v1_category") {
            None => Vec::new(),
            Some(TomlValue::String(category)) => {
                let named: Vec<&Family> = families.iter().filter(|f| **f != Family::Plan).collect();
                if named.is_empty() {
                    return vec![(Family::Plan.as_str().to_string(), category.clone())];
                }
                named
                    .into_iter()
                    .map(|f| (f.as_str().to_string(), category.clone()))
                    .collect()
            }
            Some(TomlValue::Table(map)) => {
                let mut out = Vec::new();
                for (family, category) in map {
                    match category.as_str() {
                        Some(c) => out.push((family.clone(), c.to_string())),
                        None => self.problem("v1_category", "values must be strings"),
                    }
                }
                out
            }
            Some(_) => {
                self.problem(
                    "v1_category",
                    "must be a string or a table of family = category",
                );
                Vec::new()
            }
        }
    }
}

fn parse_entry(raw: RawEntry, errors: &mut Vec<CatalogueError>) -> Option<Entry> {
    let RawEntry {
        ty,
        layer,
        path,
        front,
        sections,
    } = raw;
    let mut f = Front::new(&front, &path, String::new(), errors);
    let specific: &[&str] = match ty {
        EntryType::Lens => LENS_KEYS,
        EntryType::Facet => FACET_KEYS,
        EntryType::Method => &[],
        EntryType::Check => CHECK_KEYS,
    };
    let allowed: Vec<&str> = COMMON_KEYS.iter().chain(specific).copied().collect();
    f.only(&allowed, ty.as_str());
    let id = f.text("id", true)?;
    if !is_id(&id) {
        f.problem("id", "must be lowercase letters, digits and `-`");
    }
    let status = f
        .word("status", true, Status::parse, Status::VALUES)
        .unwrap_or(Status::Stable);
    let version = match front.get("version") {
        None => None,
        Some(TomlValue::String(s)) => Some(s.clone()),
        Some(TomlValue::Integer(n)) => Some(n.to_string()),
        Some(TomlValue::Float(n)) => Some(n.to_string()),
        Some(_) => {
            f.problem("version", "must be a string or a number");
            None
        }
    };
    let mut e = Entry::new(ty, layer, path.clone(), id, status);
    e.version = version;
    match ty {
        EntryType::Lens => {
            e.families = f.words("families", true, Family::parse, Family::VALUES);
            e.mode = f.word("mode", true, Mode::parse, Mode::VALUES);
            e.produces = f.words("produces", true, Kind::parse, Kind::VALUES);
            e.owns = f.texts("owns", true);
            e.activation = f.word("activation", true, Activation::parse, Activation::VALUES);
            e.apply_when = f.apply_when(false);
            e.after_wave = f.number("after_wave");
            e.methods = f.texts("methods", false);
            e.slice = f.word("slice", true, SliceRule::parse, SliceRule::VALUES);
            e.v1_category = f.v1_category(&e.families);
            f.text("research_domain", false);
        }
        EntryType::Facet => {
            e.families = f.words("families", true, Family::parse, Family::VALUES);
            e.apply_when = f.apply_when(true);
            e.attaches_to = f.texts("attaches_to", true);
        }
        EntryType::Method => {}
        EntryType::Check => {
            e.families = f.words("families", true, Family::parse, Family::VALUES);
            e.executor = f.word("executor", true, Executor::parse, Executor::VALUES);
            e.fence = f.text("fence", false);
            e.produces = f.words("produces", false, Kind::parse, Kind::VALUES);
            if e.fence.is_none() && !front.contains_key("produces") {
                f.problem("fence", "or `produces` is required on a check");
            }
            e.apply_when = f.apply_when(false);
            e.activation = f.word("activation", false, Activation::parse, Activation::VALUES);
        }
    }
    e.hash = content_hash(ty, &sections);
    e.sections = sections;
    Some(e)
}

/// A project `[[checks]]` row: a check run by the `verification` agent.
fn config_check(
    path: &Path,
    index: usize,
    table: &Table,
    errors: &mut Vec<CatalogueError>,
) -> Option<Entry> {
    let mut f = Front::new(table, path, format!("checks[{index}]."), errors);
    f.only(CONFIG_CHECK_KEYS, "project check");
    let id = f.text("id", true)?;
    if !is_id(&id) {
        f.problem("id", "must be lowercase letters, digits and `-`");
    }
    let mut e = Entry::new(
        EntryType::Check,
        Layer::Project,
        path.to_path_buf(),
        id,
        Status::Stable,
    );
    e.families = f.words("families", true, Family::parse, Family::VALUES);
    e.apply_when.paths = f.globs("paths");
    e.command = f.text("command", true);
    e.fence = f.text("fence", false);
    e.activation = f.word("activation", false, Activation::parse, Activation::VALUES);
    e.executor = Some(Executor::Verification);
    e.hash = sha256_hex(toml::to_string(table).unwrap_or_default().as_bytes());
    Some(e)
}

#[derive(Default)]
struct Vocabulary {
    signals: Vec<Signal>,
    concerns: Vec<Concern>,
    routing: Routing,
}

fn read_vocabulary(
    path: &Path,
    layer: Layer,
    vocab: &mut Vocabulary,
    tree: &mut Vec<(String, String)>,
    errors: &mut Vec<CatalogueError>,
) {
    let Some(text) = read_text(path, errors) else {
        return;
    };
    tree.push((
        format!("{}:vocabulary.toml", layer.as_str()),
        sha256_hex(text.as_bytes()),
    ));
    let Some(doc) = parse_toml(path, &text, errors) else {
        return;
    };
    let mut top = Front::new(&doc, path, String::new(), errors);
    top.only(&["signals", "concerns", "routing"], "vocabulary");
    let section = |key: &str, errors: &mut Vec<CatalogueError>| -> Option<Table> {
        match doc.get(key) {
            None => None,
            Some(TomlValue::Table(t)) => Some(t.clone()),
            Some(_) => {
                errors.push(CatalogueError::Schema {
                    path: path.to_path_buf(),
                    field: key.to_string(),
                    problem: "must be a table".into(),
                });
                None
            }
        }
    };
    let as_table =
        |name: &str, value: &TomlValue, errors: &mut Vec<CatalogueError>| -> Option<Table> {
            let table = value.as_table().cloned();
            if table.is_none() {
                errors.push(CatalogueError::Schema {
                    path: path.to_path_buf(),
                    field: name.to_string(),
                    problem: "must be a table".into(),
                });
            }
            table
        };

    for (name, value) in section("signals", errors).unwrap_or_default() {
        let at = format!("signals.{name}");
        let Some(table) = as_table(&at, &value, errors) else {
            continue;
        };
        let mut f = Front::new(&table, path, format!("{at}."), errors);
        f.only(&["kind", "meaning", "detect", "hint"], "signal");
        let kind = f.word("kind", true, SignalKind::parse, SignalKind::VALUES);
        let meaning = f.text("meaning", true).unwrap_or_default();
        let hint = f.text("hint", kind == Some(SignalKind::Judged));
        let detect = match table.get("detect") {
            None => {
                if kind.is_some_and(|k| k != SignalKind::Judged) {
                    f.problem("detect", "is required on a detected signal");
                }
                None
            }
            Some(TomlValue::Table(d)) if kind != Some(SignalKind::Judged) => {
                let mut inner = Front::new(d, path, format!("{at}.detect."), f.errors);
                inner.only(DETECT_KEYS, "detect table");
                inner.globs("files");
                Some(d.clone())
            }
            Some(TomlValue::Table(_)) => {
                f.problem("detect", "is not allowed on a judged signal");
                None
            }
            Some(_) => {
                f.problem("detect", "must be a table");
                None
            }
        };
        if let Some(kind) = kind {
            vocab.signals.push(Signal {
                name,
                kind,
                meaning,
                detect,
                hint,
                layer,
                path: path.to_path_buf(),
            });
        }
    }
    for (tag, value) in section("concerns", errors).unwrap_or_default() {
        let at = format!("concerns.{tag}");
        let Some(table) = as_table(&at, &value, errors) else {
            continue;
        };
        let mut f = Front::new(&table, path, format!("{at}."), errors);
        f.only(&["meaning", "families"], "concern tag");
        let meaning = f.text("meaning", true).unwrap_or_default();
        let families = f.words("families", true, Family::parse, Family::VALUES);
        vocab.concerns.push(Concern {
            tag,
            meaning,
            families,
            layer,
            path: path.to_path_buf(),
        });
    }
    if let Some(routing) = section("routing", errors) {
        let mut f = Front::new(&routing, path, "routing.".into(), errors);
        f.only(&["executable", "tests"], "routing table");
        vocab.routing.executable.extend(f.globs("executable"));
        vocab.routing.tests.extend(f.globs("tests"));
    }
}

fn parse_config(path: &Path, text: &str, errors: &mut Vec<CatalogueError>) -> Config {
    let mut config = Config {
        path: path.to_path_buf(),
        ..Config::default()
    };
    let Some(doc) = parse_toml(path, text, errors) else {
        return config;
    };
    let bad = |key: &str, problem: &str| CatalogueError::BadConfig {
        path: path.to_path_buf(),
        key: key.to_string(),
        problem: problem.to_string(),
    };
    let tables =
        |key: &str, value: &TomlValue, errors: &mut Vec<CatalogueError>| -> Vec<(String, Table)> {
            let Some(outer) = value.as_table() else {
                errors.push(bad(key, "must be a table of tables"));
                return Vec::new();
            };
            let mut out = Vec::new();
            for (id, inner) in outer {
                match inner.as_table() {
                    Some(t) => out.push((id.clone(), t.clone())),
                    None => errors.push(bad(&format!("{key}.{id}"), "must be a table")),
                }
            }
            out
        };
    let mut f = Front::new(&doc, path, String::new(), errors);
    config.disable = f.texts("disable", false);
    config.enable = f.texts("enable", false);
    for (key, value) in &doc {
        match key.as_str() {
            "disable" | "enable" => {}
            "extend" => config.extend = tables(key, value, errors),
            "override" => config.overrides = tables(key, value, errors),
            "scope" => {
                for (glob, table) in tables(key, value, errors) {
                    let at = format!("scope.{glob:?}");
                    let matcher = match Glob::new(&glob) {
                        Ok(g) => Some(g.compile_matcher()),
                        Err(e) => {
                            errors.push(bad(&at, &format!("is not a valid glob: {e}")));
                            None
                        }
                    };
                    let mut f = Front::new(&table, path, format!("{at}."), errors);
                    f.only(&["disable", "facets"], "scope block");
                    let disable = f.texts("disable", false);
                    let facets = f.texts("facets", false);
                    config.scopes.push(ScopeBlock {
                        glob,
                        matcher,
                        disable,
                        facets,
                    });
                }
            }
            "checks" => match value.as_array() {
                Some(rows) => {
                    for (i, row) in rows.iter().enumerate() {
                        match row.as_table() {
                            Some(t) => config.checks.push(t.clone()),
                            None => errors.push(bad(&format!("checks[{i}]"), "must be a table")),
                        }
                    }
                }
                None => errors.push(bad(key, "must be an array of tables")),
            },
            _ => errors.push(bad(key, "is not a config key")),
        }
    }
    config
}

/// The Claude config directory: `CLAUDE_CONFIG_DIR` when set and non-empty,
/// else `USERPROFILE`, then `HOME`, joined with `.claude`. Read once per process.
pub(crate) fn claude_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let config = std::env::var("CLAUDE_CONFIG_DIR").ok();
        let user_profile = std::env::var("USERPROFILE").ok();
        let home = std::env::var("HOME").ok();
        claude_dir_from(config.as_deref(), user_profile.as_deref(), home.as_deref())
    })
    .clone()
}

/// Pure over its inputs because `std::env::set_var` is `unsafe` in edition
/// 2024. A set-but-empty `USERPROFILE` still wins over `HOME`, as glimpse
/// and the status line resolve it.
fn claude_dir_from(
    config: Option<&str>,
    user_profile: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(dir) = config.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    Some(PathBuf::from(user_profile.or(home)?).join(".claude"))
}

/// `<claude dir>/skills/review-v2-lenses`, where `deploy-claude.ps1` links the
/// built-in catalogue.
pub(crate) fn default_builtin_root() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("skills").join("review-v2-lenses"))
}

/// The project layer a verb loads. An explicit `--project` must be a
/// directory; without one, `<root>/.claude/review-lenses` when it exists.
pub(crate) fn project_layer(root: &Path, explicit: Option<PathBuf>) -> Result<Option<PathBuf>> {
    match explicit {
        Some(dir) if !dir.is_dir() => Err(tagged_err(
            ErrorKind::NotFound,
            Some(dir.clone()),
            format!("project catalogue layer not found at {}", dir.display()),
        )),
        Some(dir) => Ok(Some(dir)),
        None => Ok(Some(root.join(".claude").join("review-lenses")).filter(|d| d.is_dir())),
    }
}

/// The catalogue rebuilt from the roots `review run start` recorded on `run`,
/// refused when its content no longer matches the recorded shas. An absent or
/// vanished built-in root resolves to [`default_builtin_root`], so a store
/// moved between machines still loads; a relative project root resolves
/// against the repo `root`. A record without `tree_sha` predates the check.
pub(crate) fn run_catalogue(root: &Path, run: &Run) -> Result<Catalogue> {
    let record = run
        .rest
        .get("catalogue")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "run {} records no catalogue; start the run with `review run start`",
                    run.id
                ),
            )
        })?;
    let recorded = record
        .get("builtin")
        .and_then(TomlValue::as_str)
        .map(PathBuf::from);
    let builtin = match recorded {
        Some(path) if path.is_dir() => path,
        recorded => default_builtin_root().or(recorded).ok_or_else(|| {
            tagged_err(
                ErrorKind::NotFound,
                None,
                format!(
                    "run {}: no built-in catalogue root; set CLAUDE_CONFIG_DIR",
                    run.id
                ),
            )
        })?,
    };
    let project = record.get("project").and_then(TomlValue::as_str).map(|p| {
        let path = PathBuf::from(p);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    });
    let catalogue = Catalogue::load(&builtin, project.as_deref())?;
    if let Some(tree_sha) = record.get("tree_sha").and_then(TomlValue::as_str) {
        let config_sha = record.get("config_sha").and_then(TomlValue::as_str);
        let drifted: Vec<&str> = [
            ("tree_sha", tree_sha != catalogue.tree_sha()),
            ("config_sha", config_sha != catalogue.config_sha()),
        ]
        .iter()
        .filter(|(_, differs)| *differs)
        .map(|(name, _)| *name)
        .collect();
        if !drifted.is_empty() {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "run {}: the lens catalogue changed since the run started ({} no longer matches); \
                     start a new run with `review run start`",
                    run.id,
                    drifted.join(", ")
                ),
            ));
        }
    }
    Ok(catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::write;

    const VOCABULARY: &str = r#"
[signals."lang:rust"]
kind = "stack"
meaning = "Rust sources"
[signals."lang:rust".detect]
files = ["**/Cargo.toml"]

[signals."framework:react"]
kind = "stack"
meaning = "React"
[signals."framework:react".detect]
files = ["**/package.json"]
contains = '"react"'

[signals.trust-boundary]
kind = "judged"
meaning = "untrusted input crosses here"
hint = "parsers, HTTP handlers"

[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code", "plan"]

[concerns.edge-cases]
meaning = "boundaries and error paths"
families = ["code"]

[concerns.types]
meaning = "illegal states"
families = ["code"]

[concerns.premortem]
meaning = "how the plan fails"
families = ["plan"]

[routing]
executable = ["**/*.rs"]
tests = ["**/tests/**"]
"#;

    const CORRECTNESS: &str = r#"+++
id = "correctness"
status = "stable"
version = 2
families = ["code", "plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["logic", "edge-cases"]
activation = "floor"
slice = "per-subsystem"
methods = ["trace"]
v1_category = "quality"
+++

# Correctness

## Question
Does it compute the right thing?

## Owns
`logic` — wrong output.

`edge-cases` — boundaries.

## Probes — code
- off-by-one in ranges

## Probes — plan
- a step that cannot run

## Does not flag
- confirmations

## Edit forms
- replace the expression
"#;

    const TYPE_DESIGN: &str = r#"+++
id = "type-design"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup"]
owns = ["types"]
activation = "candidate"
slice = "per-subsystem"
v1_category = { code = "architecture" }
+++
## Question
Can the types hold illegal states?

## Owns
`types` — illegal states.

## Probes — code
- stringly-typed sets

## Does not flag
- style
"#;

    const PREMORTEM: &str = r#"+++
id = "plan-premortem"
status = "experimental"
families = ["plan"]
mode = "judgement"
produces = ["advisory"]
owns = ["premortem"]
activation = "opt-in"
slice = "whole-scope"
apply_when = { min_depth = "standard" }
+++
## Question
How does this plan fail?

## Owns
`premortem` — failure stories.

## Probes — plan
- imagine it shipped and broke

## Does not flag
- certainties
"#;

    const TRACE: &str =
        "+++\nid = \"trace\"\nstatus = \"stable\"\n+++\n## Steps\nFollow the value.\n";

    const RUST: &str = r#"+++
id = "rust"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["lang:rust"] }
attaches_to = ["*"]
+++
## General
Ownership pitfalls.

### For correctness
- integer overflow in release
"#;

    const REACT: &str = r#"+++
id = "react"
status = "stable"
families = ["code"]
apply_when = { signals = ["framework:react"] }
attaches_to = ["correctness"]
+++
## General
Hooks pitfalls.
"#;

    const LINT: &str = r#"+++
id = "lint-baseline"
status = "stable"
families = ["code"]
executor = "verification"
fence = "LINT BASELINE"
activation = "floor"
+++
## Runs
The project's lint command.
"#;

    const PROBE_CHECK: &str = r#"+++
id = "two-control-probe"
status = "stable"
families = ["plan"]
executor = "cell"
produces = ["defect"]
+++
## Runs
Each acceptance command against two controls.
"#;

    struct Fixture {
        _tmp: tempfile::TempDir,
        builtin: PathBuf,
        project: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            let builtin = tmp.path().join("builtin");
            let project = tmp.path().join("project");
            let fx = Self {
                _tmp: tmp,
                builtin,
                project,
            };
            fx.builtin_file("vocabulary.toml", VOCABULARY);
            fx.builtin_file("lenses/correctness.md", CORRECTNESS);
            fx.builtin_file("lenses/type-design.md", TYPE_DESIGN);
            fx.builtin_file("lenses/plan-premortem.md", PREMORTEM);
            fx.builtin_file("methods/trace.md", TRACE);
            fx.builtin_file("facets/rust.md", RUST);
            fx.builtin_file("facets/react.md", REACT);
            fx.builtin_file("checks/lint-baseline.md", LINT);
            fx.builtin_file("checks/two-control-probe.md", PROBE_CHECK);
            fx
        }

        fn builtin_file(&self, rel: &str, text: &str) {
            write(&self.builtin, rel, text.as_bytes());
        }

        fn project_file(&self, rel: &str, text: &str) {
            write(&self.project, rel, text.as_bytes());
        }

        fn load(&self) -> Catalogue {
            Catalogue::load(&self.builtin, Some(&self.project)).expect("the catalogue loads")
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn rust_signals(roots: &[&str]) -> Vec<(String, Vec<String>)> {
        vec![("lang:rust".to_string(), strings(roots))]
    }

    #[test]
    fn parses_toml_frontmatter_and_sections() {
        let fx = Fixture::new();
        let cat = fx.load();
        assert_eq!(cat.validate(), Vec::<CatalogueError>::new());
        let lens = cat.entry("correctness").expect("correctness loads");
        assert_eq!(lens.ty, EntryType::Lens);
        assert_eq!(lens.version.as_deref(), Some("2"));
        assert_eq!(lens.families, vec![Family::Code, Family::Plan]);
        assert_eq!(lens.mode, Some(Mode::Judgement));
        assert_eq!(lens.activation, Some(Activation::Floor));
        assert_eq!(lens.slice, Some(SliceRule::PerSubsystem));
        assert_eq!(lens.owns, strings(&["logic", "edge-cases"]));
        assert_eq!(lens.v1_category_for(Family::Code), Some("quality"));
        assert_eq!(lens.v1_category_for(Family::Plan), None);
        let keys: Vec<&str> = lens.sections.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "question",
                "owns",
                "probes_code",
                "probes_plan",
                "does_not_flag",
                "edit_forms"
            ]
        );
        assert_eq!(
            lens.section("probes_code").map(|s| s.body.as_str()),
            Some("- off-by-one in ranges")
        );
        assert_eq!(lens.hash.len(), 64);
        let facet = cat.entry("rust").expect("rust loads");
        assert_eq!(facet.apply_when.signals, strings(&["lang:rust"]));
        assert_eq!(
            facet.sections.len(),
            1,
            "a `###` heading stays inside its `##` section"
        );
        assert_eq!(cat.detectors().len(), 2, "judged signals are not detectors");
        assert_eq!(cat.routing().executable, strings(&["**/*.rs"]));
        assert_eq!(cat.tree_sha().len(), 64);
        assert_eq!(cat.config_sha(), None);

        let rows = cat.index(&IndexFilter {
            family: Some(Family::Plan),
            signals: Vec::new(),
        });
        let ids: Vec<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
        assert_eq!(
            ids,
            [
                "correctness",
                "plan-premortem",
                "trace",
                "two-control-probe"
            ]
        );
        let premortem = rows.iter().find(|r| r["id"] == "plan-premortem").unwrap();
        assert_eq!(premortem["activation"], "opt-in");

        fx.builtin_file("lenses/broken.md", "## Question\nno frontmatter\n");
        let errors = fx.load().validate();
        assert!(
            matches!(errors.as_slice(), [CatalogueError::NoFrontmatter { path }] if path.ends_with("broken.md")),
            "got {errors:?}"
        );
    }

    #[test]
    fn project_id_colliding_with_builtin_is_refused() {
        let fx = Fixture::new();
        fx.project_file("lenses/correctness.md", CORRECTNESS);
        let errors = fx.load().validate();
        assert!(
            errors.iter().any(|e| matches!(
                e,
                CatalogueError::CollidesWithBuiltin { id, path }
                    if id == "correctness" && path.starts_with(&fx.project)
            )),
            "got {errors:?}"
        );
    }

    #[test]
    fn concern_tag_needs_exactly_one_owner_per_family() {
        let fx = Fixture::new();
        fx.builtin_file(
            "lenses/type-design.md",
            &TYPE_DESIGN
                .replace("\"type-design\"", "\"logic-two\"")
                .replace("[\"types\"]", "[\"logic\"]")
                .replace("`types`", "`logic`"),
        );
        let cat = fx.load();
        let errors = cat.validate();
        let owner_errors: Vec<&CatalogueError> = errors
            .iter()
            .filter(|e| matches!(e, CatalogueError::OwnerCount { .. }))
            .collect();
        assert_eq!(
            owner_errors,
            [
                &CatalogueError::OwnerCount {
                    family: "code".into(),
                    tag: "logic".into(),
                    owners: strings(&["correctness", "logic-two"]),
                },
                &CatalogueError::OwnerCount {
                    family: "code".into(),
                    tag: "types".into(),
                    owners: Vec::new(),
                },
            ],
            "two owners of (code, logic) and none of (code, types); (plan, logic) keeps one"
        );
        assert_eq!(
            cat.owners().get(&("plan".into(), "logic".into())),
            Some(&strings(&["correctness"]))
        );
    }

    #[test]
    fn project_tags_need_the_project_prefix() {
        let fx = Fixture::new();
        fx.project_file(
            "vocabulary.toml",
            "[concerns.legacy]\nmeaning = \"legacy code\"\nfamilies = [\"code\"]\n",
        );
        let errors = fx.load().validate();
        assert!(
            errors.iter().any(
                |e| matches!(e, CatalogueError::ProjectPrefix { name, .. } if name == "legacy")
            ),
            "got {errors:?}"
        );

        fx.project_file(
            "vocabulary.toml",
            "[concerns.\"project:legacy\"]\nmeaning = \"legacy code\"\nfamilies = [\"code\"]\n",
        );
        fx.project_file(
            "lenses/legacy.md",
            &TYPE_DESIGN
                .replace("\"type-design\"", "\"legacy\"")
                .replace("[\"types\"]", "[\"project:legacy\"]")
                .replace("`types`", "`project:legacy`"),
        );
        assert_eq!(fx.load().validate(), Vec::<CatalogueError>::new());

        fx.builtin_file(
            "vocabulary.toml",
            &format!(
                "{VOCABULARY}\n[concerns.\"project:own\"]\nmeaning = \"x\"\nfamilies = [\"code\"]\n"
            ),
        );
        let errors = fx.load().validate();
        assert!(
            errors.iter().any(
                |e| matches!(e, CatalogueError::ProjectPrefix { name, .. } if name == "project:own")
            ),
            "a built-in tag must not carry the prefix; got {errors:?}"
        );
    }

    #[test]
    fn missing_family_probes_section_is_refused() {
        let fx = Fixture::new();
        fx.builtin_file(
            "lenses/correctness.md",
            &CORRECTNESS.replace("## Probes — plan\n- a step that cannot run\n", ""),
        );
        let errors = fx.load().validate();
        assert!(
            matches!(
                errors.as_slice(),
                [CatalogueError::MissingProbes { family, .. }] if family == "plan"
            ),
            "got {errors:?}"
        );
    }

    #[test]
    fn config_disable_enable_override_apply() {
        let fx = Fixture::new();
        let before = fx.load().entry("correctness").unwrap().hash.clone();
        fx.project_file(
            "config.toml",
            r#"
disable = ["type-design"]
enable = ["plan-premortem"]
[extend.correctness]
probes = ["unchecked unwrap"]
does_not_flag = ["generated code"]
[override.correctness]
activation = "candidate"
question = "Is the output right for every input?"
"#,
        );
        let cat = fx.load();
        assert_eq!(cat.validate(), Vec::<CatalogueError>::new());
        assert!(cat.config_sha().is_some());
        assert!(cat.entry("type-design").unwrap().disabled);
        assert!(!cat.entry("correctness").unwrap().disabled);
        assert!(cat.entry("plan-premortem").unwrap().enabled);
        let lens = cat.entry("correctness").unwrap();
        assert_eq!(lens.activation, Some(Activation::Candidate));
        assert_eq!(
            lens.section("question").unwrap().body,
            "Is the output right for every input?"
        );
        assert_eq!(
            lens.section("probes_code").unwrap().body,
            "- off-by-one in ranges\n- unchecked unwrap"
        );
        assert_eq!(
            lens.section("probes_plan").unwrap().body,
            "- a step that cannot run\n- unchecked unwrap"
        );
        assert_eq!(
            lens.section("does_not_flag").unwrap().body,
            "- confirmations\n- generated code"
        );
        assert_ne!(
            lens.hash, before,
            "the bound hash follows the applied config"
        );
        let refused = cat
            .bind_cell(
                &strings(&["type-design"]),
                Family::Code,
                &[],
                &strings(&["src/a.rs"]),
                &[],
                Depth::Standard,
            )
            .unwrap_err();
        assert!(refused.to_string().contains("disabled"), "{refused}");

        fx.project_file(
            "config.toml",
            "disable = [\"nope\"]\n[override.ghost]\nslice = \"per-file\"\n",
        );
        let errors = fx.load().validate();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CatalogueError::UnknownConfigId { key, id, .. } if key == "disable" && id == "nope")),
            "got {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CatalogueError::UnknownConfigId { key, id, .. } if key == "override" && id == "ghost")),
            "got {errors:?}"
        );
    }

    #[test]
    fn config_naming_an_id_in_both_enable_and_disable_is_refused() {
        let fx = Fixture::new();
        fx.project_file(
            "config.toml",
            "enable = [\"plan-premortem\"]\ndisable = [\"plan-premortem\"]\n",
        );
        let errors = fx.load().validate();
        assert!(
            matches!(
                errors.as_slice(),
                [CatalogueError::BadConfig { key, problem, .. }]
                    if key == "enable" && problem.contains("plan-premortem")
            ),
            "got {errors:?}"
        );
    }

    #[test]
    fn scope_block_disables_by_path() {
        let fx = Fixture::new();
        fx.project_file(
            "config.toml",
            "[scope.\"legacy/**\"]\ndisable = [\"type-design\"]\nfacets = [\"-rust\"]\n",
        );
        let cat = fx.load();
        assert_eq!(cat.validate(), Vec::<CatalogueError>::new());
        let bind = |slice: &[&str]| {
            cat.bind_cell(
                &strings(&["type-design"]),
                Family::Code,
                &rust_signals(&["."]),
                &strings(slice),
                &[],
                Depth::Standard,
            )
        };
        let refused = bind(&["legacy/old.rs"]).unwrap_err().to_string();
        assert!(refused.contains("legacy/**"), "{refused}");
        assert!(bind(&["src/a.rs", "legacy/sub/**"]).is_err());
        let ok = bind(&["src/a.rs"]).expect("outside the scope binds");
        assert_eq!(ok.bound[0].concern, "type-design");
        assert_eq!(ok.bound[0].v1_category, "architecture");

        let facets = |slice: &[&str]| {
            cat.bind_cell(
                &strings(&["correctness"]),
                Family::Code,
                &rust_signals(&["."]),
                &strings(slice),
                &[],
                Depth::Standard,
            )
            .unwrap()
            .facets
            .len()
        };
        assert_eq!(
            facets(&["legacy/old.rs"]),
            0,
            "the scope removes `rust` there"
        );
        assert_eq!(facets(&["src/a.rs"]), 1);
    }

    #[test]
    fn facet_attaches_only_where_slice_meets_roots() {
        let cat = Fixture::new().load();
        let signals = vec![
            ("lang:rust".to_string(), strings(&["tomlctl"])),
            ("framework:react".to_string(), strings(&["web"])),
        ];
        let ids = |concern: &str, slice: &[&str], edits: &[&str]| -> Vec<String> {
            cat.bind_cell(
                &strings(&[concern]),
                Family::Code,
                &signals,
                &strings(slice),
                &strings(edits),
                Depth::Standard,
            )
            .unwrap()
            .facets
            .into_iter()
            .map(|f| f.id)
            .collect()
        };
        assert_eq!(ids("correctness", &["tomlctl/src/sweep.rs"], &[]), ["rust"]);
        assert_eq!(ids("correctness", &["tomlctl/src/**"], &[]), ["rust"]);
        assert_eq!(ids("correctness", &["web/src/App.tsx"], &[]), ["react"]);
        assert_eq!(ids("correctness", &["**/*.rs"], &[]), ["react", "rust"]);
        assert!(ids("correctness", &["docs/x.md"], &[]).is_empty());
        assert!(
            ids("type-design", &["web/src/App.tsx"], &[]).is_empty(),
            "react attaches only to correctness"
        );
        assert_eq!(
            ids("correctness", &["web/src/App.tsx"], &["-react", "+rust"]),
            ["rust"]
        );
        assert!(
            ids("correctness", &["tomlctl-extra/a.rs"], &[]).is_empty(),
            "a root matches whole path components"
        );
    }

    #[test]
    fn content_hash_changes_with_probes_not_with_whitespace_outside_sections() {
        let fx = Fixture::new();
        let hash = |fx: &Fixture| fx.load().entry("correctness").unwrap().hash.clone();
        let base = hash(&fx);

        let spaced = CORRECTNESS
            .replace("status = \"stable\"\n", "status   =   \"stable\"\n\n")
            .replace("# Correctness\n", "# Correctness\n\n\n")
            .replace("## Question\n", "## Question   \n\n")
            .replace("- confirmations\n", "- confirmations   \n\n\n")
            .replace("- replace the expression", "- rewrite the expression")
            .replace('\n', "\r\n");
        fx.builtin_file("lenses/correctness.md", &spaced);
        assert_eq!(hash(&fx), base);

        fx.builtin_file(
            "lenses/correctness.md",
            &CORRECTNESS.replace("off-by-one in ranges", "off-by-one in slices"),
        );
        assert_ne!(hash(&fx), base);
    }

    #[test]
    fn deprecated_lens_is_refused_for_new_cells() {
        let fx = Fixture::new();
        fx.builtin_file(
            "lenses/type-design.md",
            &TYPE_DESIGN.replace("status = \"stable\"", "status = \"deprecated\""),
        );
        fx.builtin_file(
            "lenses/types-two.md",
            &TYPE_DESIGN.replace("\"type-design\"", "\"types-two\""),
        );
        let cat = fx.load();
        assert_eq!(
            cat.validate(),
            Vec::<CatalogueError>::new(),
            "a deprecated lens no longer counts as an owner"
        );
        let refused = cat
            .bind_cell(
                &strings(&["type-design"]),
                Family::Code,
                &[],
                &strings(&["src/a.rs"]),
                &[],
                Depth::Standard,
            )
            .unwrap_err();
        assert!(refused.to_string().contains("deprecated"), "{refused}");
        assert!(
            cat.bind_cell(
                &strings(&["types-two"]),
                Family::Code,
                &[],
                &strings(&["src/a.rs"]),
                &[],
                Depth::Standard,
            )
            .is_ok()
        );
    }

    #[test]
    fn uncompilable_contains_pattern_is_refused() {
        let fx = Fixture::new();
        fx.builtin_file(
            "vocabulary.toml",
            &VOCABULARY.replace("contains = '\"react\"'", "contains = ['\"react\"', '(']"),
        );
        let errors = fx.load().validate();
        assert!(
            matches!(
                errors.as_slice(),
                [CatalogueError::BadPattern { signal, .. }] if signal == "framework:react"
            ),
            "got {errors:?}"
        );
    }

    #[test]
    fn claude_dir_prefers_claude_config_dir() {
        assert_eq!(
            claude_dir_from(Some("/cfg"), Some("/up"), Some("/h")),
            Some(PathBuf::from("/cfg"))
        );
        assert_eq!(
            claude_dir_from(Some(""), Some("/up"), Some("/h")),
            Some(PathBuf::from("/up").join(".claude"))
        );
        assert_eq!(
            claude_dir_from(None, None, Some("/h")),
            Some(PathBuf::from("/h").join(".claude"))
        );
        assert_eq!(claude_dir_from(None, None, None), None);
    }

    #[test]
    fn bind_cell_binds_checks_probes_and_depth_limits() {
        let cat = Fixture::new().load();
        let bind = |concerns: &[&str], family: Family, depth: Depth| {
            cat.bind_cell(
                &strings(concerns),
                family,
                &[],
                &strings(&["docs/plan.md"]),
                &[],
                depth,
            )
        };
        let check = bind(&["check:two-control-probe"], Family::Plan, Depth::Quick).unwrap();
        assert_eq!(check.bound[0].concern, "check:two-control-probe");
        assert!(bind(&["check:lint-baseline"], Family::Code, Depth::Quick).is_err());
        assert!(
            bind(&["probe:retry-storm"], Family::Code, Depth::Quick)
                .unwrap()
                .bound
                .is_empty()
        );
        assert!(bind(&["probe:Bad Slug"], Family::Code, Depth::Quick).is_err());
        assert!(
            bind(&["plan-premortem"], Family::Plan, Depth::Quick).is_err(),
            "min_depth"
        );
        assert!(bind(&["plan-premortem"], Family::Plan, Depth::Standard).is_ok());
        assert!(bind(&["correctness", "type-design"], Family::Code, Depth::Deep).is_ok());
        assert!(
            bind(&["type-design"], Family::Plan, Depth::Standard).is_err(),
            "family"
        );
    }

    #[test]
    fn bind_cell_bundles_cell_checks() {
        let fx = Fixture::new();
        for id in ["falsifiability", "acceptance-contradiction"] {
            fx.builtin_file(
                &format!("checks/{id}.md"),
                &PROBE_CHECK.replace("two-control-probe", id),
            );
        }
        let cat = fx.load();
        let bind = |concerns: &[&str], family: Family, depth: Depth| {
            cat.bind_cell(
                &strings(concerns),
                family,
                &[],
                &strings(&["docs/plan.md"]),
                &[],
                depth,
            )
        };
        let all = [
            "check:two-control-probe",
            "check:falsifiability",
            "check:acceptance-contradiction",
        ];
        let cell = bind(&all, Family::Plan, Depth::Deep).unwrap();
        assert_eq!(cell.bound.len(), 3);
        assert!(cell.facets.is_empty());
        assert!(
            bind(
                &["check:two-control-probe", "correctness"],
                Family::Plan,
                Depth::Deep
            )
            .is_err()
        );
        assert!(
            bind(
                &["correctness", "check:two-control-probe"],
                Family::Code,
                Depth::Deep
            )
            .is_err()
        );
        assert!(
            bind(
                &["check:lint-baseline", "check:falsifiability"],
                Family::Plan,
                Depth::Deep
            )
            .is_err()
        );
        assert!(
            bind(
                &["check:falsifiability", "check:falsifiability"],
                Family::Plan,
                Depth::Deep
            )
            .is_err()
        );
    }
}
