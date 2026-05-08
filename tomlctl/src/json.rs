//! T2 of `docs/plans/flow-tracking-overhaul.md`: JSON-document
//! `get`/`set`/`unset` on a dotted path. Sibling of the TOML side's
//! `get` / `set` / `set-json`, scoped to JSON files (e.g.
//! `.claude/settings.json`).
//!
//! Mirrors the TOML pipeline byte-for-byte where it can:
//!   - Containment guard via `io::guard_write_path` (write paths only).
//!   - Exclusive lock around the read+mutate+write critical section.
//!   - In-lock pre-persist `recheck_claude_containment` for TOCTOU narrowing.
//!   - Atomic-replace tempfile via `io::atomic_write`.
//!   - Sidecar refresh via `integrity::refresh_sidecar`, **except**
//!     for `**/settings.json` where Claude Code is a co-writer (P16) — its
//!     `/config` writes bypass tomlctl, so a sidecar would drift after every
//!     UI flip and downstream `--verify-integrity` reads would fail forever.
//!     The skip is path-name-keyed so a project's own `settings.json`
//!     under `.claude/flows/<slug>/...` is also exempt.
//!
//! P19 (both halves): JSON writers refuse a `.toml` target (`kind=validation`);
//! the symmetric TOML-side refusal lives in
//! `cli::dispatch::refuse_json_extension_for_toml_writers` (wired at
//! dispatch.rs L425, L454, L502).
//!
//! Plan deviation: the plan referenced `io::resolve_target` as the
//! containment helper. The actual containment guard in `io.rs` is
//! `guard_write_path` (with `recheck_claude_containment` as the in-lock
//! TOCTOU narrowing call). The two helpers compose to the same effect:
//! pre-lock guard + canonicalise + in-lock re-check; we use them directly
//! rather than introducing a new shim.

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::cli::{JsonOp, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::{maybe_verify_integrity, refresh_sidecar};
use crate::io::{
    atomic_write, guard_write_path, recheck_claude_containment, with_exclusive_lock,
    with_shared_lock,
};
use crate::output::{print_json, print_json_compact};

pub(crate) fn dispatch(op: JsonOp) -> Result<()> {
    match op {
        JsonOp::Get { file, path, raw, json: json_flag, integrity } => {
            handle_get(&file, &path, raw, json_flag, integrity)
        }
        JsonOp::Set { file, path, json, dry_run, integrity } => {
            handle_set(&file, &path, &json, dry_run, integrity)
        }
        JsonOp::Unset { file, path, dry_run, integrity } => {
            handle_unset(&file, &path, dry_run, integrity)
        }
    }
}

/// P19 (positive half): JSON writers refuse `.toml` targets. The negative
/// half (TOML writers refuse `.json`) requires a touch on `cli/dispatch.rs`
/// and is deferred — see module docstring.
fn refuse_toml_extension(file: &Path) -> Result<()> {
    if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")) {
        return Err(tagged_err(
            ErrorKind::Validation,
            Some(file.to_path_buf()),
            format!(
                "tomlctl json operations refuse .toml targets — use `tomlctl set {} ...` (or `tomlctl set-json` / `tomlctl items ...`) for TOML files",
                file.display()
            ),
        ));
    }
    Ok(())
}

/// P16: writes to any `**/settings.json` skip the sidecar refresh because
/// Claude Code itself is a co-writer (its `/config` UI rewrites the file
/// without going through tomlctl, so any sidecar we emit would be stale on
/// the next read). The check is path-name-keyed rather than directory-keyed
/// so a per-flow `settings.json` is also exempt.
fn should_skip_sidecar(file: &Path) -> bool {
    file.file_name().is_some_and(|n| n == "settings.json")
}

/// Strict-read gate mirroring `cli::dispatch::strict_read_check` — fires
/// BEFORE `--verify-integrity` so a missing file under
/// `--strict-read --verify-integrity` surfaces `kind=not_found` rather than
/// `kind=integrity`. (`integrity::verify_integrity` would only see a missing
/// sidecar in that scenario, leading to the wrong tag.)
fn strict_read_check(file: &Path, strict_read: bool) -> Result<()> {
    if !strict_read || file.exists() {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::NotFound,
        Some(file.to_path_buf()),
        format!("file does not exist: {}", file.display()),
    ))
}

/// JSON-side dotted-path read. Mirrors `convert::navigate` for TOML, but
/// over `serde_json::Value`. Each segment indexes by string key on objects,
/// or by `usize` index on arrays. Returns `None` on any missing segment or
/// type mismatch (e.g. trying to descend into a scalar / null).
fn navigate_json<'a>(root: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    if path.is_empty() {
        return Some(root);
    }
    let mut cur = root;
    for seg in path.split('.') {
        cur = match cur {
            JsonValue::Object(map) => map.get(seg)?,
            JsonValue::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// JSON-side dotted-path write. Mirrors `convert::set_at_path` for TOML.
/// Auto-vivifies missing intermediate objects. The final segment overwrites
/// (or inserts if missing). Array-index segments must point at an existing
/// slot — auto-vivify of array slots is not supported, matching the TOML
/// side.
fn set_at_path_json(root: &mut JsonValue, path: &str, value: JsonValue) -> Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        bail!(
            "empty key path; path must be a non-empty `.`-separated dotted path (e.g. `permissions.allow`)"
        );
    };

    // Walk parents, auto-vivifying missing object segments.
    let mut cur: &mut JsonValue = root;
    for p in parents {
        if cur.is_array() {
            let idx: usize = p.parse().with_context(|| {
                format!(
                    "path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0.id`)",
                    p
                )
            })?;
            cur = cur
                .as_array_mut()
                .and_then(|arr| arr.get_mut(idx))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "array index `{}` out of bounds (use `tomlctl json get <file> <parent>` to inspect array length first)",
                        idx
                    )
                })?;
            continue;
        }
        // Auto-vivify a missing or non-object segment as a fresh object.
        // `as_object_mut().is_none()` covers null / scalar / array (but
        // we've already split on array above), so clobbering here is the
        // documented semantics: the user asked for an object path.
        if !cur.is_object() {
            *cur = JsonValue::Object(serde_json::Map::new());
        }
        let map = cur.as_object_mut().expect("just-set object");
        cur = map
            .entry(p.to_string())
            .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
    }

    // Final segment: insert into the parent object/array.
    if cur.is_array() {
        let idx: usize = last.parse().with_context(|| {
            format!(
                "final path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0`)",
                last
            )
        })?;
        let arr = cur
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("array lost during traversal"))?;
        if idx >= arr.len() {
            bail!("array index `{}` out of bounds (len {})", idx, arr.len());
        }
        arr[idx] = value;
        return Ok(());
    }
    if !cur.is_object() {
        *cur = JsonValue::Object(serde_json::Map::new());
    }
    let map = cur
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("target parent is not an object"))?;
    map.insert((*last).to_string(), value);
    Ok(())
}

/// JSON-side dotted-path delete. Returns `true` if a value was actually
/// removed, `false` if the leaf was already absent (no-op semantics —
/// `unset` of a missing key is a successful no-op, mirroring `rm -f`).
fn unset_at_path_json(root: &mut JsonValue, path: &str) -> Result<bool> {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        bail!(
            "empty key path; path must be a non-empty `.`-separated dotted path (e.g. `permissions.allow`)"
        );
    };

    let mut cur: &mut JsonValue = root;
    for p in parents {
        match cur {
            JsonValue::Object(map) => {
                let Some(next) = map.get_mut(*p) else {
                    return Ok(false);
                };
                cur = next;
            }
            JsonValue::Array(arr) => {
                let idx: usize = match p.parse() {
                    Ok(n) => n,
                    Err(_) => return Ok(false),
                };
                let Some(next) = arr.get_mut(idx) else {
                    return Ok(false);
                };
                cur = next;
            }
            _ => return Ok(false),
        }
    }
    match cur {
        JsonValue::Object(map) => Ok(map.shift_remove(*last).is_some()),
        JsonValue::Array(arr) => {
            let Ok(idx) = last.parse::<usize>() else {
                return Ok(false);
            };
            if idx >= arr.len() {
                return Ok(false);
            }
            arr.remove(idx);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Read a JSON file off disk. Mirrors `io::read_toml`'s NotFound + Parse
/// tagging contract so the JSON envelope shape is consistent across the
/// TOML and JSON CLIs.
fn read_json_file(file: &Path) -> Result<JsonValue> {
    let s = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(file.to_path_buf()),
                format!("reading {}: {}", file.display(), e),
            ));
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", file.display()));
        }
    };
    match serde_json::from_str::<JsonValue>(&s) {
        Ok(v) => Ok(v),
        Err(e) => Err(tagged_err(
            ErrorKind::Parse,
            Some(file.to_path_buf()),
            format!("parsing {}: {}", file.display(), e),
        )),
    }
}

/// Format a JSON document for on-disk persistence. Two-space indent +
/// trailing newline. `serde_json::to_string_pretty` defaults to two-space
/// indent so a direct call suffices, but we add the trailing `\n` ourselves
/// (the helper does not emit one).
fn format_json_for_disk(v: &JsonValue) -> Result<Vec<u8>> {
    let mut s = serde_json::to_string_pretty(v).context("serialising JSON")?;
    s.push('\n');
    Ok(s.into_bytes())
}

fn handle_get(
    file: &Path,
    path: &str,
    raw: bool,
    json_flag: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    refuse_toml_extension(file)?;
    strict_read_check(file, integrity.strict_read)?;

    // Mirror `read_doc`'s shared-lock policy when `--verify-integrity` is
    // set so a concurrent writer can't slip a (NEW sidecar / OLD file) pair
    // into our read window. Plain reads skip the lock for parity with the
    // TOML side.
    let doc = if integrity.verify_integrity {
        with_shared_lock(file, || {
            // `verify_integrity` is `pub(crate)`; the standard read-side
            // helper is `maybe_verify_integrity`, which gates on
            // `IntegrityOpts.verify_on_read`. We synthesise the equivalent
            // by calling the inner verifier directly when the flag is set.
            verify_sidecar_if_requested(file, true)?;
            read_json_file(file)
        })?
    } else {
        read_json_file(file)?
    };

    let leaf = navigate_json(&doc, path).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            Some(file.to_path_buf()),
            format!("path not found: {}", path),
        )
    })?;

    // `--raw` short-circuits scalar leaves to bare values. Strings unquote;
    // numbers and bools render as themselves. Null renders as `null`. For
    // arrays/objects we fall through to pretty JSON regardless of `--raw`
    // — there is no well-defined bare-scalar form for a composite value.
    // `--json` forces pretty JSON even for scalars; `--raw` and `--json`
    // are not mutex (clap accepts both), and `--raw` wins when both are
    // passed (it is the more restrictive shape).
    if raw && !leaf.is_object() && !leaf.is_array() {
        let bare = match leaf {
            JsonValue::String(s) => s.clone(),
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Number(n) => n.to_string(),
            // Composite types handled by the early `is_object`/`is_array`
            // branch above; this is unreachable.
            _ => leaf.to_string(),
        };
        let stdout = std::io::stdout();
        let mut w = stdout.lock();
        w.write_all(bare.as_bytes())?;
        w.write_all(b"\n")?;
        w.flush()?;
        return Ok(());
    }

    // `--json` is the default for non-raw output; it's accepted as a
    // discoverability flag without semantic effect.
    let _ = json_flag;
    print_json(leaf)
}

/// Sidecar-verify gate parallel to `integrity::maybe_verify_integrity` but
/// driven by the local `--verify-integrity` boolean rather than the
/// `IntegrityOpts` bundle. Avoids constructing an `IntegrityOpts` purely
/// for this dispatch site.
fn verify_sidecar_if_requested(file: &Path, verify: bool) -> Result<()> {
    if !verify {
        return Ok(());
    }
    // `integrity::IntegrityOpts` is the canonical shape `maybe_verify_integrity`
    // takes; build it on the fly.
    let opts = crate::integrity::IntegrityOpts {
        write_sidecar: false,
        verify_on_read: true,
        strict: false,
    };
    maybe_verify_integrity(file, opts)
}

fn handle_set(
    file: &Path,
    path: &str,
    value_json: &str,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    refuse_toml_extension(file)?;
    let parsed: JsonValue = serde_json::from_str(value_json)
        .with_context(|| format!("parsing --json value `{}`", value_json))?;

    if dry_run {
        return dry_run_set(file, path, parsed, integrity);
    }

    with_exclusive_lock(file, || {
        // O17 mirror: in-lock containment guard so a swap of the leaf
        // symlink between guard and persist is caught.
        guard_write_path(file, integrity.allow_outside)?;
        // Read existing doc; treat missing file as `{}` so first-write to
        // a brand-new JSON file works without a separate bootstrap step.
        let mut doc = match fs::read_to_string(file) {
            Ok(s) => serde_json::from_str::<JsonValue>(&s)
                .map_err(|e| tagged_err(
                    ErrorKind::Parse,
                    Some(file.to_path_buf()),
                    format!("parsing {}: {}", file.display(), e),
                ))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                JsonValue::Object(serde_json::Map::new())
            }
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("reading {}", file.display()));
            }
        };
        // Optional pre-mutation integrity check (parity with TOML
        // `mutate_doc` callers that pass `--verify-integrity` to a writer).
        if integrity.verify_integrity {
            verify_sidecar_if_requested(file, true)?;
        }
        set_at_path_json(&mut doc, path, parsed.clone())?;
        // R3 TOCTOU narrowing: re-check containment immediately before
        // the atomic persist, mirroring `mutate_doc`.
        if !integrity.allow_outside {
            recheck_claude_containment(file)?;
        }
        let bytes = format_json_for_disk(&doc)?;
        atomic_write(file, &bytes)?;
        // P16: `settings.json` is co-written by Claude Code; suppress the
        // sidecar refresh to prevent permanent verification drift.
        let skip = should_skip_sidecar(file);
        refresh_sidecar_after_write(file, skip, &integrity)?;
        emit_set_unset_envelope(file, path, "set", skip)?;
        Ok(())
    })
}

fn handle_unset(
    file: &Path,
    path: &str,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    refuse_toml_extension(file)?;

    if dry_run {
        return dry_run_unset(file, path, integrity);
    }

    with_exclusive_lock(file, || {
        guard_write_path(file, integrity.allow_outside)?;
        let mut doc = match fs::read_to_string(file) {
            Ok(s) => serde_json::from_str::<JsonValue>(&s).map_err(|e| {
                tagged_err(
                    ErrorKind::Parse,
                    Some(file.to_path_buf()),
                    format!("parsing {}: {}", file.display(), e),
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Missing file → unset is a successful no-op (nothing to
                // remove). Don't materialise an empty file.
                emit_set_unset_envelope(file, path, "unset", should_skip_sidecar(file))?;
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("reading {}", file.display()));
            }
        };
        if integrity.verify_integrity {
            verify_sidecar_if_requested(file, true)?;
        }
        let removed = unset_at_path_json(&mut doc, path)?;
        // No-op semantics: if the leaf was already absent, return success
        // without rewriting the file. Mirrors `mutate_doc_conditional`'s
        // skip-write branch — leaving the on-disk bytes (and sidecar)
        // untouched is the whole point.
        let skip = should_skip_sidecar(file);
        if !removed {
            emit_set_unset_envelope(file, path, "unset", skip)?;
            return Ok(());
        }
        if !integrity.allow_outside {
            recheck_claude_containment(file)?;
        }
        let bytes = format_json_for_disk(&doc)?;
        atomic_write(file, &bytes)?;
        refresh_sidecar_after_write(file, skip, &integrity)?;
        emit_set_unset_envelope(file, path, "unset", skip)?;
        Ok(())
    })
}

/// Refresh the sidecar after a successful JSON write, honouring P16
/// (skip for `settings.json`) and the `--no-write-integrity` /
/// `--strict-integrity` flags. Mirrors `write_toml_with_sidecar`'s
/// failure-handling shape (warn on stderr by default, fail-hard under
/// `--strict-integrity`) so the JSON and TOML write paths surface
/// identical operator-facing behaviour on a stuck disk / bad sidecar.
fn refresh_sidecar_after_write(file: &Path, skip: bool, integrity: &WriteIntegrityArgs) -> Result<()> {
    if skip || integrity.no_write_integrity {
        return Ok(());
    }
    let Err(e) = refresh_sidecar(file) else {
        return Ok(());
    };
    if integrity.strict_integrity {
        return Err(e).with_context(|| {
            format!(
                "wrote {} but sidecar refresh failed (--strict-integrity was set, so this is a hard error)",
                file.display()
            )
        });
    }
    eprintln!(
        "tomlctl: warning: wrote {} but sidecar refresh failed: {:#}",
        file.display(),
        e
    );
    Ok(())
}

/// Emit the success envelope for a write operation. Compact single-line
/// JSON, matching `print_json_compact`'s contract elsewhere in tomlctl
/// so downstream agents can parse with `serde_json::from_str` over a
/// single line.
fn emit_set_unset_envelope(
    file: &Path,
    path: &str,
    _action: &str,
    sidecar_skipped: bool,
) -> Result<()> {
    let mut env = serde_json::json!({
        "ok": true,
        "file": file.display().to_string(),
        "path": path,
    });
    if sidecar_skipped
        && let Some(map) = env.as_object_mut()
    {
        map.insert(
            "sidecar_skipped".to_string(),
            JsonValue::String("co-writer-protected".to_string()),
        );
    }
    print_json_compact(&env)
}

fn dry_run_set(
    file: &Path,
    path: &str,
    new_value: JsonValue,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let _ = integrity; // dry-run reads skip containment-guard side effects
    let doc = match fs::read_to_string(file) {
        Ok(s) => serde_json::from_str::<JsonValue>(&s).map_err(|e| {
            tagged_err(
                ErrorKind::Parse,
                Some(file.to_path_buf()),
                format!("parsing {}: {}", file.display(), e),
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            JsonValue::Object(serde_json::Map::new())
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", file.display()));
        }
    };
    let old_value = navigate_json(&doc, path).cloned();
    emit_dry_run_envelope(file, path, "set", old_value, Some(new_value))
}

fn dry_run_unset(
    file: &Path,
    path: &str,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let _ = integrity;
    let doc = match fs::read_to_string(file) {
        Ok(s) => serde_json::from_str::<JsonValue>(&s).map_err(|e| {
            tagged_err(
                ErrorKind::Parse,
                Some(file.to_path_buf()),
                format!("parsing {}: {}", file.display(), e),
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            JsonValue::Object(serde_json::Map::new())
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", file.display()));
        }
    };
    let old_value = navigate_json(&doc, path).cloned();
    emit_dry_run_envelope(file, path, "unset", old_value, None)
}

/// Compact dry-run envelope. Includes `sidecar_skipped:"co-writer-protected"`
/// when P16 applies (so the dry-run output rehearses the live write's
/// envelope shape), `null` otherwise.
fn emit_dry_run_envelope(
    file: &Path,
    path: &str,
    action: &str,
    old_value: Option<JsonValue>,
    new_value: Option<JsonValue>,
) -> Result<()> {
    let sidecar_skipped = if should_skip_sidecar(file) {
        JsonValue::String("co-writer-protected".to_string())
    } else {
        JsonValue::Null
    };
    let envelope = serde_json::json!({
        "would_change": {
            "file": file.display().to_string(),
            "path": path,
            "action": action,
            "new_value": new_value.unwrap_or(JsonValue::Null),
            "old_value": old_value.unwrap_or(JsonValue::Null),
            "sidecar_skipped": sidecar_skipped,
        },
    });
    print_json_compact(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_json_walks_objects_and_arrays() {
        let v: JsonValue = serde_json::from_str(
            r#"{"permissions":{"allow":["a","b","c"]}}"#,
        )
        .unwrap();
        assert_eq!(
            navigate_json(&v, "permissions.allow.1"),
            Some(&JsonValue::String("b".to_string()))
        );
        assert_eq!(navigate_json(&v, "missing"), None);
        assert_eq!(navigate_json(&v, "permissions.deny"), None);
    }

    #[test]
    fn set_at_path_json_autovivifies_missing_parents() {
        let mut v = JsonValue::Object(serde_json::Map::new());
        set_at_path_json(&mut v, "a.b.c", JsonValue::Bool(true)).unwrap();
        assert_eq!(
            navigate_json(&v, "a.b.c"),
            Some(&JsonValue::Bool(true))
        );
    }

    #[test]
    fn unset_at_path_json_returns_false_on_missing_leaf() {
        let mut v: JsonValue = serde_json::from_str(r#"{"a":{"b":1}}"#).unwrap();
        assert!(!unset_at_path_json(&mut v, "a.x").unwrap());
        assert!(unset_at_path_json(&mut v, "a.b").unwrap());
        assert_eq!(navigate_json(&v, "a.b"), None);
    }

    #[test]
    fn should_skip_sidecar_matches_settings_json_basename() {
        assert!(should_skip_sidecar(Path::new(".claude/settings.json")));
        assert!(should_skip_sidecar(Path::new("/abs/path/settings.json")));
        assert!(!should_skip_sidecar(Path::new(".claude/foo.json")));
        assert!(!should_skip_sidecar(Path::new(".claude/settings.toml")));
    }

    #[test]
    fn refuse_toml_extension_rejects_dot_toml() {
        let err = refuse_toml_extension(Path::new("foo.toml")).unwrap_err();
        let s = format!("{:#}", err);
        assert!(s.contains("refuse .toml"), "got: {s}");
    }

    #[test]
    fn format_json_for_disk_emits_two_space_indent_and_trailing_newline() {
        let v: JsonValue =
            serde_json::from_str(r#"{"a":1,"b":[2,3]}"#).unwrap();
        let bytes = format_json_for_disk(&v).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.ends_with('\n'), "must end in newline: {s:?}");
        // serde_json::to_string_pretty default is two-space indent.
        assert!(s.contains("  \"a\""), "expected two-space indent, got: {s}");
    }
}
