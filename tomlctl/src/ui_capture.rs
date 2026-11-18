// The project's UI capture config, read by both `plan check` and the review
// survey so the two agree on which paths are UI paths.

use std::fs;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use toml::Table;

pub(crate) const CONFIG: &str = ".claude/ui-capture.toml";

/// The config's `paths` globs under `root`, with a leading `./` trimmed and `*`
/// stopping at `/`. Empty with no config or no `paths`; `Err` names the problem
/// when the config cannot be read or its `paths` is not an array of globs.
pub(crate) fn load_paths(root: &Path) -> Result<GlobSet, String> {
    let text = match fs::read_to_string(root.join(CONFIG)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(GlobSet::empty()),
        Err(e) => return Err(e.to_string()),
    };
    let table: Table = toml::from_str(&text)
        .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_string())?;
    let Some(raw) = table.get("paths") else {
        return Ok(GlobSet::empty());
    };
    let entries = raw
        .as_array()
        .ok_or_else(|| "`paths` must be an array of strings".to_string())?;
    let mut builder = GlobSetBuilder::new();
    for entry in entries {
        let text = entry
            .as_str()
            .ok_or_else(|| "`paths` must be an array of strings".to_string())?;
        let glob = GlobBuilder::new(text.trim_start_matches("./"))
            .literal_separator(true)
            .build()
            .map_err(|e| format!("`paths` glob {text:?}: {e}"))?;
        builder.add(glob);
    }
    builder.build().map_err(|e| format!("`paths`: {e}"))
}
