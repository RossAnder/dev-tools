//! Prompt / URL / MCP-config / keystroke-DSL helpers carved out of
//! [`super`] (`pty_transport/mod.rs`). These compose the spawn invocation's
//! `--append-system-prompt`, `--mcp-config`, and the Keystroke-kind input
//! bridge translation. The security-critical spawn block itself stays in
//! `mod.rs`; this sibling only holds the pure helper cluster it calls into.

use super::*;

/// Build the system-prompt addendum appended to every lumina-spawned `claude`
/// session.
///
/// lumina is headless: it cannot render or answer claude's interactive
/// `AskUserQuestion` (AUQ) TUI picker, AND claude buffers an open AUQ's
/// `tool_use` out of the session JSONL until the question is answered (verified
/// against 2.1.156), so a JSONL-tailing consumer can never surface an *open*
/// AUQ. Instead of the native tool, we register a lumina MCP tool
/// (`ask_user_question`, see [`crate::pty::ask`]) and steer claude to call it:
/// it presents the choices in lumina's existing structured picker and blocks
/// until the operator answers. The session id is baked into the prompt because
/// the tool correlates the call to this PTY session by that argument.
pub(crate) fn no_auq_system_prompt(session_id: &str) -> String {
    format!(
        "You are running inside lumina, a headless interface that CANNOT display \
claude's built-in AskUserQuestion picker. NEVER call the built-in AskUserQuestion \
tool. Whenever you need the operator to choose between options or decide between \
approaches, call the `mcp__{ASK_MCP_SERVER_NAME}__ask_user_question` tool (provided by \
the `{ASK_MCP_SERVER_NAME}` MCP server). Always set its `session_id` argument to \
exactly \"{session_id}\". Provide one or more `questions`, each with a short \
`header`, the `question` text, an `options` array (each `{{label, description}}`), \
and `multiSelect` true or false — do NOT add an \"Other\" option yourself (lumina's \
UI always offers a free-text row). The tool blocks until the operator answers in \
the lumina UI and returns their selections. Use it instead of asking the operator \
to type a choice in prose."
    )
}

/// Compose the loopback URL the spawned `claude` uses to reach lumina's
/// `/mcp-ask` server. The child always connects over `127.0.0.1` regardless of
/// lumina's bind `HOST` (which defaults to loopback; a `0.0.0.0` bind also
/// accepts loopback). `PORT` mirrors `app::serve`'s env read.
fn lumina_ask_mcp_url() -> String {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_LUMINA_PORT);
    format!("http://127.0.0.1:{port}/mcp-ask")
}

/// The `--mcp-config` JSON registering the `lumina-ask` HTTP MCP server for a
/// spawned session. `--mcp-config` MERGES with the project's configured servers
/// (it does not replace them) and is session-scoped (no `~/.claude.json`
/// mutation). Claude Code 2.1.x accepts only a FILE PATH here (not inline JSON),
/// so the caller writes this to a temp file.
pub(crate) fn ask_mcp_config_json() -> String {
    format!(
        r#"{{"mcpServers":{{"{ASK_MCP_SERVER_NAME}":{{"type":"http","url":"{url}","timeout":{ASK_MCP_TOOL_TIMEOUT_MS}}}}}}}"#,
        url = lumina_ask_mcp_url()
    )
}

/// Translate one Keystroke-kind DSL token into raw PTY bytes, or `None` if
/// the token is unknown or fails validation (the input bridge logs + skips
/// in that case).
///
/// DSL grammar (one token per `InputFrame`):
///
/// | token             | bytes                                      |
/// |-------------------|--------------------------------------------|
/// | `down`            | `\x1b[B`                                   |
/// | `up`              | `\x1b[A`                                   |
/// | `space`           | `\x20`                                     |
/// | `enter`           | `\r`                                       |
/// | `escape`          | `\x1b`                                     |
/// | `tab`             | `\x09`                                     |
/// | `text:<literal>`  | UTF-8 of `<literal>` with `\n` → `\r`      |
///
/// `text:<literal>` byte-safety rules (rejected → `None`):
/// - any `\x1b` (ESC) in the body
/// - any `\x00`..=`\x1f` EXCLUDING `\t` (`\x09`) and `\n` (`\x0a`)
/// - any `\x7f` (DEL)
/// - body length > `KEYSTROKE_TEXT_MAX` (4 KiB)
///
/// First-colon split: `text:foo:bar` splits into `"text"` + `"foo:bar"` —
/// the literal body may itself contain colons.
pub(crate) fn translate_keystroke_dsl(payload: &str) -> Option<Bytes> {
    match payload {
        "down" => Some(Bytes::from_static(b"\x1b[B")),
        "up" => Some(Bytes::from_static(b"\x1b[A")),
        "space" => Some(Bytes::from_static(b"\x20")),
        "enter" => Some(Bytes::from_static(b"\r")),
        "escape" => Some(Bytes::from_static(b"\x1b")),
        "tab" => Some(Bytes::from_static(b"\x09")),
        other => {
            let mut parts = other.splitn(2, ':');
            let head = parts.next()?;
            if head != "text" {
                return None;
            }
            // `text` with no colon (`other == "text"`) yields no body part.
            // The Keystroke contract is `text:<literal>` — the colon is
            // mandatory. Treat the colon-less form as an unknown token.
            let body = parts.next()?;
            let body_bytes = body.as_bytes();
            if body_bytes.len() > KEYSTROKE_TEXT_MAX {
                return None;
            }
            let mut out = Vec::with_capacity(body_bytes.len());
            for &b in body_bytes {
                match b {
                    0x1b => return None,                // ESC
                    0x7f => return None,                // DEL
                    b'\t' => out.push(b'\t'),           // tab allowed
                    b'\n' => out.push(b'\r'),           // \n → \r translation
                    0x00..=0x1f => return None,         // other C0 controls
                    _ => out.push(b),
                }
            }
            Some(Bytes::from(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn keystroke_dsl_down_arrow() {
        assert_eq!(
            translate_keystroke_dsl("down"),
            Some(Bytes::from_static(b"\x1b[B"))
        );
    }

    #[test]
    fn keystroke_dsl_up_arrow() {
        assert_eq!(
            translate_keystroke_dsl("up"),
            Some(Bytes::from_static(b"\x1b[A"))
        );
    }

    #[test]
    fn keystroke_dsl_space() {
        assert_eq!(
            translate_keystroke_dsl("space"),
            Some(Bytes::from_static(b"\x20"))
        );
    }

    #[test]
    fn keystroke_dsl_enter() {
        assert_eq!(
            translate_keystroke_dsl("enter"),
            Some(Bytes::from_static(b"\r"))
        );
    }

    #[test]
    fn keystroke_dsl_escape() {
        assert_eq!(
            translate_keystroke_dsl("escape"),
            Some(Bytes::from_static(b"\x1b"))
        );
    }

    #[test]
    fn keystroke_dsl_tab() {
        assert_eq!(
            translate_keystroke_dsl("tab"),
            Some(Bytes::from_static(b"\x09"))
        );
    }

    #[test]
    fn keystroke_dsl_text_basic_literal() {
        assert_eq!(
            translate_keystroke_dsl("text:hello"),
            Some(Bytes::from(b"hello".to_vec()))
        );
    }

    #[test]
    fn keystroke_dsl_text_empty_literal_is_zero_bytes() {
        // Empty literal is valid; the bridge emits zero bytes.
        assert_eq!(
            translate_keystroke_dsl("text:"),
            Some(Bytes::from(Vec::<u8>::new()))
        );
    }

    #[test]
    fn keystroke_dsl_text_first_colon_split_preserves_inner_colons() {
        assert_eq!(
            translate_keystroke_dsl("text:foo:bar"),
            Some(Bytes::from(b"foo:bar".to_vec()))
        );
    }

    #[test]
    fn keystroke_dsl_text_newline_translates_to_carriage_return() {
        assert_eq!(
            translate_keystroke_dsl("text:hello\nworld"),
            Some(Bytes::from(b"hello\rworld".to_vec()))
        );
    }

    #[test]
    fn keystroke_dsl_text_rejects_embedded_esc() {
        assert_eq!(translate_keystroke_dsl("text:has\x1bembedded"), None);
    }

    #[test]
    fn keystroke_dsl_text_rejects_del() {
        assert_eq!(translate_keystroke_dsl("text:has\x7fdel"), None);
    }

    #[test]
    fn keystroke_dsl_text_rejects_c0_control_byte() {
        assert_eq!(translate_keystroke_dsl("text:has\x01ctl"), None);
    }

    #[test]
    fn keystroke_dsl_text_allows_tab_and_translates_newline() {
        assert_eq!(
            translate_keystroke_dsl("text:has\thtab\nlf"),
            Some(Bytes::from(b"has\thtab\rlf".to_vec()))
        );
    }

    #[test]
    fn keystroke_dsl_text_rejects_oversize_literal() {
        let payload = format!("text:{}", "x".repeat(KEYSTROKE_TEXT_MAX + 1));
        assert_eq!(translate_keystroke_dsl(&payload), None);
    }

    #[test]
    fn keystroke_dsl_text_accepts_boundary_4k_literal() {
        let payload = format!("text:{}", "x".repeat(KEYSTROKE_TEXT_MAX));
        let out = translate_keystroke_dsl(&payload);
        assert!(out.is_some());
        assert_eq!(out.unwrap().len(), KEYSTROKE_TEXT_MAX);
    }

    #[test]
    fn keystroke_dsl_unknown_token_is_none() {
        assert_eq!(translate_keystroke_dsl("invalid"), None);
    }

    #[test]
    fn keystroke_dsl_empty_string_is_none() {
        assert_eq!(translate_keystroke_dsl(""), None);
    }

    #[test]
    fn keystroke_dsl_text_without_colon_is_none() {
        // The `text` head with no colon body is an unknown token shape.
        assert_eq!(translate_keystroke_dsl("text"), None);
    }
}
