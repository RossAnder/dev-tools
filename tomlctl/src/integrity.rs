//! Integrity-sidecar helpers split out from `main.rs` as part of R24.
//!
//! Scope:
//!   - `IntegrityOpts` bundle (the pair of read/write integrity flags)
//!   - `sidecar_path` — canonical `<file>.sha256` path
//!   - `hex_lower` — lowercase hex encoding of a byte slice
//!   - `sha256_hex_of_file` — hash a file's current on-disk bytes
//!   - `verify_integrity` + `maybe_verify_integrity` — the read-side check
//!
//! Deliberately stays free of `Cli`-coupling: `IntegrityOpts` has public
//! fields (no `from_cli` constructor here), so the root module constructs
//! instances wherever the CLI is parsed.

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// Pair of integrity-sidecar-related global flags, bundled to shorten call
/// signatures for read/write paths.
#[derive(Clone, Copy)]
pub(crate) struct IntegrityOpts {
    pub(crate) write_sidecar: bool,
    pub(crate) verify_on_read: bool,
    /// When true, a sidecar-write failure becomes a hard error. When false
    /// (default), the failure is logged to stderr and the outer operation
    /// still succeeds — the primary TOML data has already been atomically
    /// persisted at that point.
    pub(crate) strict: bool,
}

/// Return the canonical sidecar path for `file`: `<file>.sha256`.
pub(crate) fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// Lowercase hex encoding of a byte slice. Replaces the `{:x}` formatter for
/// digest outputs, which stopped implementing `LowerHex` in sha2 0.11.
///
/// Uses a byte-wise lookup table rather than `write!("{:02x}")` in a loop —
/// the formatter path dominated hot paths that hash payloads repeatedly
/// (e.g. `blocks.rs`, `io.rs`). Output is byte-for-byte identical.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

/// Compute the SHA-256 hex digest of a file's current bytes.
///
/// Streams the file through a 64 KiB-buffered reader and feeds chunks into
/// the `Sha256` hasher via incremental `update` — sha2 0.11 dropped the
/// `io::Write` blanket impl on `Sha256`, so we drive the read loop manually
/// rather than via `io::copy`. Peak memory stays bounded by the read
/// buffer rather than the file size.
pub(crate) fn sha256_hex_of_file(file: &Path) -> Result<String> {
    use std::io::Read;
    let f = File::open(file)
        .with_context(|| format!("opening {} for hashing", file.display()))?;
    let mut r = BufReader::with_capacity(64 * 1024, f);
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r
            .read(&mut buf)
            .with_context(|| format!("reading {} for hashing", file.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex_lower(&h.finalize()))
}

/// If `--verify-integrity` was set, verify the target against its sidecar.
/// Errors if the sidecar is missing or the digest disagrees.
pub(crate) fn maybe_verify_integrity(file: &Path, integrity: IntegrityOpts) -> Result<()> {
    if !integrity.verify_on_read {
        return Ok(());
    }
    verify_integrity(file)
}

pub(crate) fn verify_integrity(file: &Path) -> Result<()> {
    let sidecar = sidecar_path(file);
    if !sidecar.exists() {
        bail!(
            "integrity check failed: sidecar {} is missing (expected `<hex>  <basename>` format — rewrite the file without --no-write-integrity to regenerate)",
            sidecar.display()
        );
    }
    let sidecar_text = fs::read_to_string(&sidecar)
        .with_context(|| format!("reading sidecar {}", sidecar.display()))?;
    let expected = sidecar_text.split_whitespace().next().ok_or_else(|| {
        anyhow!(
            "integrity sidecar {} is empty or malformed",
            sidecar.display()
        )
    })?;
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!(
            "integrity sidecar {} does not contain a 64-hex-char digest (got `{}`)",
            sidecar.display(),
            expected
        );
    }
    let actual = sha256_hex_of_file(file)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        bail!(
            "integrity check failed for {}: expected {}, actual {} (sidecar: {})",
            file.display(),
            expected,
            actual,
            sidecar.display()
        );
    }
    Ok(())
}
