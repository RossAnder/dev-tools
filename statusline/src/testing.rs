//! Test-only helpers.

/// Strip SGR sequences so an assertion reads as the user sees the line.
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_escapes() {
        assert_eq!(plain("\u{1b}[90m5h\u{1b}[0m 26%"), "5h 26%");
        assert_eq!(plain("plain"), "plain");
        assert_eq!(plain(""), "");
    }
}
