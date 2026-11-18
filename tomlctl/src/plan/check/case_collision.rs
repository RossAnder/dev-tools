//! `plan/file-case-collision`: a file a pending row creates whose stem, the
//! name without its last extension, equals another file's in the same
//! directory once case is ignored but is spelled differently. The other file is
//! one already on disk or one any row plans. One stem under two extensions, or
//! the same spelling twice, is no collision.
//!
//! Whether a file exists is read from its directory's listing by exact name,
//! never `Path::exists`, which folds case on Windows and macOS and would read a
//! case variant of an existing file as that file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::tasks::finding::{Finding, WARNING};
use crate::tasks::schema::{FileKind, Status, Store, normal_path};

/// One `files` entry of one row, split into its directory and exact name.
struct Planned {
    row: u32,
    dir: String,
    name: String,
    kind: Option<FileKind>,
    pending: bool,
}

impl Planned {
    fn path(&self) -> String {
        join(&self.dir, &self.name)
    }
}

pub(super) fn case_collision_findings(tasks: &Store, root: &Path) -> Vec<Finding> {
    let planned: Vec<Planned> = tasks
        .items
        .iter()
        .flat_map(|row| {
            row.files.iter().filter_map(move |file| {
                let (dir, name) = split(file)?;
                Some(Planned {
                    row: row.id,
                    dir,
                    name,
                    kind: tasks
                        .file_note(&row.r#ref, file)
                        .and_then(FileKind::of_note),
                    pending: row.status == Status::Pending,
                })
            })
        })
        .collect();
    let deleted: BTreeSet<String> = planned
        .iter()
        .filter(|entry| entry.kind == Some(FileKind::Delete))
        .map(Planned::path)
        .collect();

    let mut listings: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut collisions: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    for created in &planned {
        if !created.pending || created.kind == Some(FileKind::Delete) {
            continue;
        }
        let listing = listings
            .entry(created.dir.clone())
            .or_insert_with(|| listing(root, &created.dir));
        if created.kind != Some(FileKind::New) && listing.contains(&created.name) {
            continue;
        }
        let created_path = created.path();
        let mut neighbours: BTreeSet<String> = listing
            .iter()
            .map(|name| join(&created.dir, name))
            .filter(|path| !deleted.contains(path))
            .collect();
        neighbours.extend(
            planned
                .iter()
                .filter(|other| other.dir == created.dir && other.kind != Some(FileKind::Delete))
                .map(Planned::path),
        );
        for neighbour in neighbours {
            if !stems_collide(&created_path, &neighbour) {
                continue;
            }
            let key = if created_path <= neighbour {
                (created_path.clone(), neighbour.clone())
            } else {
                (neighbour.clone(), created_path.clone())
            };
            let ids = collisions.entry(key).or_default();
            ids.insert(created.row);
            ids.extend(
                planned
                    .iter()
                    .filter(|other| {
                        other.kind != Some(FileKind::Delete) && other.path() == neighbour
                    })
                    .map(|other| other.row),
            );
        }
    }

    collisions
        .into_iter()
        .map(|((first, second), ids)| Finding {
            class: "plan/file-case-collision",
            severity: WARNING,
            ids: ids.into_iter().collect(),
            detail: format!(
                "`{first}` and `{second}` share a directory and a stem spelled in different \
                 letter case; a case-insensitive filesystem or import resolver can confuse them"
            ),
        })
        .collect()
}

/// The directory and exact name of a repo-relative file, or `None` for a
/// directory entry, an empty name or a path leaving the root.
fn split(file: &str) -> Option<(String, String)> {
    let file = normal_path(file);
    if file.is_empty() || file.ends_with('/') || file.starts_with('/') {
        return None;
    }
    if file
        .split('/')
        .any(|part| part == ".." || part.contains(':'))
    {
        return None;
    }
    let (dir, name) = match file.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), file),
    };
    (!name.is_empty() && name != ".").then_some((dir, name))
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The exact names of the files in `dir`, empty when it cannot be read.
fn listing(root: &Path, dir: &str) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                return None;
            }
            entry.file_name().into_string().ok()
        })
        .collect()
}

/// The name without its last extension; a leading dot opens no extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

fn stems_collide(a: &str, b: &str) -> bool {
    let (a, b) = (stem(a), stem(b));
    a != b && a.to_lowercase() == b.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{split, stem, stems_collide};

    #[test]
    fn a_stem_drops_only_the_last_extension() {
        assert_eq!(stem("src/colourField.test.ts"), "colourField.test");
        assert_eq!(stem("src/.gitignore"), ".gitignore");
        assert_eq!(stem("Makefile"), "Makefile");
    }

    #[test]
    fn only_a_different_spelling_of_one_stem_collides() {
        assert!(stems_collide("src/ColourField.tsx", "src/colourField.ts"));
        assert!(!stems_collide("src/Button.tsx", "src/Button.css"));
        assert!(!stems_collide(
            "src/ColourField.tsx",
            "src/colourField.test.ts"
        ));
    }

    #[test]
    fn a_directory_entry_or_an_escaping_path_is_skipped() {
        assert_eq!(split("src/"), None);
        assert_eq!(split("../x.rs"), None);
        assert_eq!(
            split("./src\\a.rs"),
            Some(("src".to_string(), "a.rs".to_string()))
        );
    }
}
