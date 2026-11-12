//! The authored-files listing as glimpse holds it.
//!
//! The listing is `tomlctl::authored_list`'s JSON for the viewed flow; this module only
//! deserialises and indexes it and never reads a file.

use serde::Deserialize;

/// What an authored file is, by where it sits under the flow's `docs/flows/<slug>/`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AuthoredKind {
    Brief,
    Design,
    #[default]
    Note,
    Challenge,
    ColdRead,
    Backlog,
    RunNote,
    Settle,
    /// A kind outside the vocabulary this build knows.
    #[serde(other)]
    Unknown,
}

/// One source reference a note carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct AuthoredSource {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) claim: String,
    pub(crate) url: String,
    pub(crate) read_at: String,
    pub(crate) dirty: bool,
    pub(crate) origin: String,
}

/// One file under the flow's authored directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct AuthoredFile {
    pub(crate) path: String,
    pub(crate) kind: AuthoredKind,
    pub(crate) id: String,
    pub(crate) headline: String,
    pub(crate) state: String,
    pub(crate) task: u32,
    pub(crate) sources: Vec<AuthoredSource>,
}

/// The listing's envelope: `{"files": [...]}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct AuthoredList {
    pub(crate) files: Vec<AuthoredFile>,
}

/// The listing the app holds and the revision it was read at.
#[derive(Debug, Clone, Default)]
pub(crate) struct Authored {
    pub(crate) files: Vec<AuthoredFile>,
    pub(crate) revision: Option<u64>,
}

impl Authored {
    /// Swaps in `list` read at `revision`; false, and nothing changed, when that
    /// revision is the one already held.
    pub(crate) fn apply(&mut self, list: AuthoredList, revision: u64) -> bool {
        if self.revision == Some(revision) {
            return false;
        }
        self.files = list.files;
        self.revision = Some(revision);
        true
    }

    pub(crate) fn clear(&mut self) {
        *self = Authored::default();
    }

    pub(crate) fn by_path(&self, path: &str) -> Option<&AuthoredFile> {
        self.files.iter().find(|file| file.path == path)
    }

    /// The run note of task `task`.
    pub(crate) fn run_note(&self, task: u32) -> Option<&AuthoredFile> {
        self.files
            .iter()
            .find(|file| file.kind == AuthoredKind::RunNote && file.task == task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPE: &str = r#"{"files": [{
        "path": "notes/surface-nav.md",
        "kind": "note",
        "id": "surface-nav",
        "headline": "A seventh surface is a new `Surface` variant",
        "state": "",
        "task": 0,
        "sources": [{"path": "glimpse/src/surface.rs", "line": 16, "claim": "", "url": "", "read_at": "", "dirty": false, "origin": "inline"}]
    }, {
        "path": "notes/run/task-7.md",
        "kind": "run-note",
        "id": "task-7",
        "headline": "done",
        "state": "done",
        "task": 7,
        "sources": []
    }]}"#;

    #[test]
    fn the_documented_shape_loads() {
        let list: AuthoredList = serde_json::from_str(SHAPE).unwrap();
        assert_eq!(list.files.len(), 2);
        let note = &list.files[0];
        assert_eq!(note.kind, AuthoredKind::Note);
        assert_eq!(note.sources[0].line, 16);
        assert_eq!(note.sources[0].origin, "inline");
        assert_eq!(list.files[1].kind, AuthoredKind::RunNote);
    }

    #[test]
    fn an_unknown_kind_loads() {
        let list: AuthoredList =
            serde_json::from_str(r#"{"files": [{"path": "x.md", "kind": "novel"}]}"#).unwrap();
        assert_eq!(list.files[0].kind, AuthoredKind::Unknown);
        assert_eq!(list.files[0].task, 0);
    }

    #[test]
    fn an_unchanged_revision_is_not_reapplied() {
        let mut held = Authored::default();
        let list: AuthoredList = serde_json::from_str(SHAPE).unwrap();
        assert!(held.apply(list, 3));
        assert_eq!(held.files.len(), 2);
        assert!(!held.apply(AuthoredList::default(), 3));
        assert_eq!(held.files.len(), 2);
        assert!(held.apply(AuthoredList::default(), 4));
        assert!(held.files.is_empty());
    }

    #[test]
    fn lookups_find_by_path_and_by_task() {
        let mut held = Authored::default();
        held.apply(serde_json::from_str(SHAPE).unwrap(), 1);
        assert_eq!(
            held.by_path("notes/surface-nav.md").unwrap().id,
            "surface-nav"
        );
        assert_eq!(held.run_note(7).unwrap().path, "notes/run/task-7.md");
        assert!(held.run_note(8).is_none());
        held.clear();
        assert!(held.by_path("notes/surface-nav.md").is_none());
    }
}
