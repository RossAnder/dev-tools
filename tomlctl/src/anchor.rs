//! `file:line` / `file:symbol` anchors shared by `items sweep`, `items clusters` and `items orphans`.

use std::collections::BTreeSet;
use std::fmt;

/// One `instances` entry, split into the file it names and what it points at
/// within that file. `file` is kept verbatim so it round-trips through
/// `Display` unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired when `items sweep`, `items clusters` and `items orphans` land
pub(crate) struct Anchor {
    pub(crate) file: String,
    pub(crate) at: AnchorAt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired when `items sweep`, `items clusters` and `items orphans` land
pub(crate) enum AnchorAt {
    Line(u64),
    Symbol(String),
}

/// Splits at the first `:` after the last path separator, so a Windows drive
/// letter stays with the file and `Foo::bar` survives as one symbol. An
/// all-digit tail is a line; a digit run too long for `u64` is kept as a
/// symbol rather than rejected.
#[allow(dead_code)] // wired when `items sweep`, `items clusters` and `items orphans` land
pub(crate) fn parse(s: &str) -> Option<Anchor> {
    let after_sep = s.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let colon = after_sep + s[after_sep..].find(':')?;
    let file = &s[..colon];
    let tail = &s[colon + 1..];
    if file.is_empty() || tail.is_empty() {
        return None;
    }
    let at = if tail.bytes().all(|b| b.is_ascii_digit()) {
        match tail.parse::<u64>() {
            Ok(line) => AnchorAt::Line(line),
            Err(_) => AnchorAt::Symbol(tail.to_string()),
        }
    } else {
        AnchorAt::Symbol(tail.to_string())
    };
    Some(Anchor {
        file: file.to_string(),
        at,
    })
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.at {
            AnchorAt::Line(line) => write!(f, "{}:{}", self.file, line),
            AnchorAt::Symbol(symbol) => write!(f, "{}:{}", self.file, symbol),
        }
    }
}

#[allow(dead_code)] // wired when `items sweep`, `items clusters` and `items orphans` land
pub(crate) fn files_of<'a>(anchors: impl Iterator<Item = &'a Anchor>) -> BTreeSet<&'a str> {
    anchors.map(|a| a.file.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_tail_is_a_line() {
        assert_eq!(
            parse("src/a.rs:42"),
            Some(Anchor {
                file: "src/a.rs".to_string(),
                at: AnchorAt::Line(42),
            })
        );
    }

    #[test]
    fn symbol_tail_keeps_its_own_colons() {
        assert_eq!(
            parse("src/a.rs:Foo::bar"),
            Some(Anchor {
                file: "src/a.rs".to_string(),
                at: AnchorAt::Symbol("Foo::bar".to_string()),
            })
        );
    }

    #[test]
    fn drive_letter_stays_with_the_file() {
        assert_eq!(
            parse(r"C:\x\y.rs:foo"),
            Some(Anchor {
                file: r"C:\x\y.rs".to_string(),
                at: AnchorAt::Symbol("foo".to_string()),
            })
        );
    }

    #[test]
    fn missing_or_empty_parts_are_none() {
        assert_eq!(parse("a.rs"), None);
        assert_eq!(parse(":foo"), None);
        assert_eq!(parse("a.rs:"), None);
    }

    #[test]
    fn overlong_digit_tail_is_a_symbol() {
        let tail = "99999999999999999999999";
        assert_eq!(
            parse(&format!("a.rs:{tail}")).map(|a| a.at),
            Some(AnchorAt::Symbol(tail.to_string()))
        );
    }

    #[test]
    fn display_round_trips_verbatim() {
        for input in ["src/a.rs:42", "src/a.rs:Foo::bar", r"C:\x\y.rs:foo"] {
            assert_eq!(parse(input).unwrap().to_string(), input);
        }
    }

    #[test]
    fn files_of_dedups_and_sorts() {
        let anchors = [
            parse("src/b.rs:1").unwrap(),
            parse("src/a.rs:Foo").unwrap(),
            parse("src/b.rs:Bar").unwrap(),
        ];
        assert_eq!(
            files_of(anchors.iter()).into_iter().collect::<Vec<_>>(),
            vec!["src/a.rs", "src/b.rs"]
        );
    }
}
