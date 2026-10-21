//! Clipboard access through the terminal's OSC 52 escape sequence.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The sequence that asks the terminal to place `text` on the system clipboard.
pub(crate) fn osc52(text: &str) -> String {
    format!("\u{1b}]52;c;{}\u{1b}\\", base64(text.as_bytes()))
}

/// RFC 4648 standard-alphabet base64 with `=` padding.
fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want, "input {input:?}");
        }
    }

    #[test]
    fn osc52_wraps_the_payload_in_the_clipboard_sequence() {
        assert_eq!(osc52("foo"), "\u{1b}]52;c;Zm9v\u{1b}\\");
        assert_eq!(osc52(""), "\u{1b}]52;c;\u{1b}\\");
    }
}
