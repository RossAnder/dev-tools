//! Shared WebSocket helpers for the pty-sessions and stream ws handlers.
//!
//! Currently carries the Origin allowlist — a browser-CSRF defence only; any
//! local process can forge the header. The trust model is "localhost-only",
//! same as the rest of the /api surface. Both `http/pty_sessions/ws.rs` and
//! the `/api/stream` handler import THIS copy so the two handlers cannot
//! drift on which origins are allowed.

/// Check `Origin` against the localhost allowlist + optional `LUMINA_DEV_ORIGIN`.
/// Empty origin is rejected (browsers always send one; a missing header most
/// likely indicates a forged or non-browser caller — which the localhost
/// trust model already permits via direct mpsc/HTTP, so blocking here only
/// hardens the browser-CSRF path).
pub(crate) fn is_origin_allowed(origin: &str) -> bool {
    if origin.is_empty() {
        return false;
    }
    // Permitted hosts; ports are arbitrary. We do a prefix-match per scheme.
    const ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];
    for scheme in &["http://", "https://"] {
        for host in ALLOWED_HOSTS {
            let prefix = format!("{scheme}{host}");
            // Either bare host or host:port form. `origin` has no path.
            if origin == prefix || origin.starts_with(&format!("{prefix}:")) {
                return true;
            }
        }
    }
    if let Ok(dev) = std::env::var("LUMINA_DEV_ORIGIN")
        && origin == dev
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Origin allowlist: localhost variants pass; an arbitrary remote does not.
    #[test]
    fn origin_allowlist_basic() {
        assert!(is_origin_allowed("http://localhost"));
        assert!(is_origin_allowed("http://localhost:5173"));
        assert!(is_origin_allowed("http://127.0.0.1:24817"));
        assert!(is_origin_allowed("https://[::1]:1234"));
        assert!(!is_origin_allowed("http://evil.example"));
        assert!(!is_origin_allowed(""));
        // Sneaky prefix variant: must not allow `localhost.evil.com`.
        assert!(!is_origin_allowed("http://localhost.evil.com"));
    }
}
