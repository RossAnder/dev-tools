//! T8: typed error-kind taxonomy for `--error-format json`.
//!
//! Scope: a tiny, closed taxonomy of error classes that downstream agents can
//! branch on when `tomlctl` exits non-zero. The CLI's default text output is
//! unchanged (the taxonomy lives in an inner `context` layer that the text
//! formatter ignores); JSON output surfaces the deepest tag via
//! `anyhow::Error::downcast_ref::<TaggedError>()`.
//!
//! **Why `TaggedError` carries its own `message` rather than wrapping via
//! `.context(TaggedError { ... })`**: `.context(TaggedError)` makes the tag a
//! separate layer in anyhow's chain, and anyhow's `{:#}` formatter renders
//! each non-empty `Display` as its own `": ..."` segment. Even an empty
//! `Display` leaves a stray `": "` artifact. To keep text-mode output
//! byte-identical to the pre-T8 `bail!("...")` form, the tag has to *be* the
//! inner error — so its `Display` emits the caller's prose verbatim, with no
//! extra prefix, and `anyhow::Error::new(TaggedError { ... })` places it at
//! the chain root. Subsequent `.with_context(...)` wrappers compose normally.
//!
//! Why a hand-rolled `std::error::Error` impl and not `thiserror`: `thiserror`
//! is not currently a dependency, and adding one just for five tag sites is
//! over-kill. The hand-rolled impl costs ~10 lines.
//!
//! **Tag sites are a closed list** — see `docs/plans/tomlctl-capability-gaps.md`
//! Task 8 for the contract. Every other `bail!` / `anyhow!` call falls through
//! to `kind = "other"` in JSON output. Do not extend this list without updating
//! the plan's taxonomy section.

use std::path::PathBuf;

/// Closed taxonomy of error kinds surfaced under `--error-format json`.
#[derive(Debug, Clone)]
pub(crate) enum ErrorKind {
    /// Generic I/O failure (open / read / write not covered by a more specific
    /// kind). Reserved for future use; today no call site tags `Io` directly
    /// — missing-file paths use `NotFound`, hash-mismatches use `Integrity`.
    #[allow(dead_code)]
    Io,
    /// TOML parse failure on the document root (`read_toml` /
    /// `read_doc_borrowed`).
    Parse,
    /// Sidecar hash mismatch or malformed sidecar — the `.sha256` content
    /// disagrees with the file's actual digest.
    Integrity,
    /// A CLI-level validation rule rejected the invocation — flag mutex
    /// violations in `validate_query`, or the prefix-shape check in
    /// `items_next_id`.
    Validation,
    /// The target file does not exist on disk at the path the caller passed.
    NotFound,
    /// Fallback for any untagged error. `--error-format json` emits this when
    /// no `TaggedError` is found in the `anyhow` cause chain. The variant is
    /// never constructed directly — the JSON formatter defaults to the
    /// `"other"` string when the downcast returns `None`.
    #[allow(dead_code)]
    Other,
}

impl ErrorKind {
    /// Stable machine-readable name for JSON output.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Parse => "parse",
            Self::Integrity => "integrity",
            Self::Validation => "validation",
            Self::NotFound => "not_found",
            Self::Other => "other",
        }
    }
}

/// An anyhow-compatible error that carries a kind tag alongside its message.
///
/// Constructed via `tagged_err(kind, file, msg)`; the returned `anyhow::Error`
/// has `TaggedError` as its innermost error. Callers add the usual
/// `.with_context(...)` layers on top for path/operation context. Text-mode
/// `{:#}` rendering is byte-identical to `anyhow!(msg)` wrapped in the same
/// contexts — `TaggedError::Display` emits the message verbatim with no tag
/// prefix. `--error-format json` finds the tag via
/// `err.downcast_ref::<TaggedError>()`, which walks anyhow's context wrappers
/// transparently.
#[derive(Debug, Clone)]
pub(crate) struct TaggedError {
    pub(crate) kind: ErrorKind,
    /// Optional path hint — populated for file-scoped tags
    /// (`NotFound`, `Integrity`, `Parse` where the path is known).
    pub(crate) file: Option<PathBuf>,
    /// The caller's human-readable prose. Placed here (rather than a
    /// separate `.context(msg)` layer) so the tag and the message share a
    /// single chain slot — otherwise anyhow's `{:#}` renders the tag as its
    /// own colon-separated segment and text output drifts from pre-T8 bytes.
    pub(crate) message: String,
}

impl std::fmt::Display for TaggedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // CRITICAL byte-identity invariant: the formatter MUST NOT prefix the
        // message with the kind or any bracketed annotation. Text-mode output
        // passes through anyhow's `{:#}` chain formatter, which calls this
        // `Display` verbatim; any extra text would leak into `tomlctl: ...`
        // stderr and break every agent today parsing that prose via regex.
        f.write_str(&self.message)
    }
}

impl std::error::Error for TaggedError {}

/// Construct an `anyhow::Error` whose innermost error is a `TaggedError`
/// carrying the given kind, optional file path, and message. The returned
/// error renders (`{:#}`, `{}`) identically to `anyhow!(msg)` — the tag is a
/// downcast-only side-channel — so callers can migrate `bail!("...")` sites
/// to tagged form without changing text output.
pub(crate) fn tagged_err(
    kind: ErrorKind,
    file: Option<PathBuf>,
    msg: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(TaggedError {
        kind,
        file,
        message: msg.into(),
    })
}
