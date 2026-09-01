//! Summary normalisation and set-similarity scoring.
//!
//! The pipeline is byte-oriented ASCII. Every `dedup_id` already on disk is
//! frozen on that choice, so swapping in `str::to_lowercase` would re-key the
//! whole store.

use std::collections::BTreeSet;

/// Closed-class tokens dropped after punctuation folding: they appear in
/// nearly every English summary and so carry no discriminating signal.
/// Negations, modals and verbs are kept — "flakes" and "does not flake" must
/// not fold together. Editing this list re-keys every stored `dedup_id`.
pub(crate) const STOPWORDS: [&str; 24] = [
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "has", "have", "in",
    "into", "is", "it", "of", "on", "or", "that", "the", "to", "with",
];

/// Char-trigram Jaccard at or above which two summaries read as
/// `likely-duplicate`. Below roughly this level, independent issues sharing a
/// subsystem's vocabulary start scoring; the verdict only warns, so a false
/// positive costs one extra read and never merges a row.
pub(crate) const SIMILARITY_STRONG: f64 = 0.75;

/// Word Jaccard at or above which two summaries read as `related`: about a
/// third of the surviving content words shared. Lower, and every pair under
/// one subsystem qualifies; the verdict only proposes an edge.
pub(crate) const SIMILARITY_RELATED: f64 = 0.35;

/// Folds a summary to the canonical form the `dedup_id` hash is taken over.
/// Every byte that is not ASCII alphanumeric becomes a space, so a multi-byte
/// character splits its neighbours into separate tokens rather than folding
/// into one, and the result is ASCII by construction. Idempotent.
pub(crate) fn normalise(summary: &str) -> String {
    let mut folded = String::with_capacity(summary.len());
    for &byte in summary.as_bytes() {
        if byte.is_ascii_alphanumeric() {
            folded.push(byte.to_ascii_lowercase() as char);
        } else {
            folded.push(' ');
        }
    }
    folded
        .split_ascii_whitespace()
        .filter(|token| !STOPWORDS.contains(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Distinct tokens of the normalised form; the argument may be raw or already
/// normalised.
pub(crate) fn word_tokens(summary: &str) -> BTreeSet<String> {
    normalise(summary)
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Trigrams over the normalised form, its single spaces included so word
/// boundaries count. A form shorter than three bytes yields itself, keeping
/// short summaries comparable instead of scoring zero against everything.
pub(crate) fn char_trigrams(summary: &str) -> BTreeSet<String> {
    let folded = normalise(summary);
    if folded.is_empty() {
        return BTreeSet::new();
    }
    if folded.len() < 3 {
        return BTreeSet::from([folded]);
    }
    // `folded` is ASCII, so byte windows never split a character.
    folded
        .as_bytes()
        .windows(3)
        .map(|window| window.iter().map(|&b| b as char).collect())
        .collect()
}

/// Intersection over union. Two empty sets score 0.0, not 1.0: an empty
/// normalised summary must not read as a perfect match to another one.
pub(crate) fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let intersection = a.intersection(b).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_is_idempotent() {
        for raw in [
            "PTY readiness probe flakes on slow CI",
            "  Mixed CASE, punctuation!! and   runs of whitespace  ",
            "checkout total overlaps the confirm button below 1400px",
            "the and of",
            "",
        ] {
            let once = normalise(raw);
            assert_eq!(normalise(&once), once, "not idempotent for {raw:?}");
        }
    }

    #[test]
    fn punctuation_case_and_whitespace_variants_collapse() {
        let a = normalise("pty_readiness_probe flakes on slow CI");
        let b = normalise("PTY-READINESS-PROBE   flakes,  on  slow CI!");
        let c = normalise("\tpty readiness probe... Flakes (on) slow ci\n");
        assert_eq!(a, "pty readiness probe flakes slow ci");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn curly_apostrophe_splits_the_word_and_stays_stable() {
        let once = normalise("Don’t reuse the session token");
        assert_eq!(once, "don t reuse session token");
        assert_eq!(normalise(&once), once);
        assert!(once.is_ascii());
    }

    #[test]
    fn non_ascii_letters_are_dropped_not_lowercased() {
        // A char-oriented pipeline would keep the accented letter and yield
        // "café latte"; the byte pipeline is what every stored id assumes.
        assert_eq!(normalise("Café latte"), "caf latte");
    }

    #[test]
    fn trigrams_span_word_boundaries() {
        let grams = char_trigrams("CPU load");
        assert!(grams.contains("u l"), "{grams:?}");
    }

    #[test]
    fn trigrams_of_a_short_summary_are_the_summary() {
        assert_eq!(char_trigrams("Hi!"), BTreeSet::from(["hi".to_owned()]));
    }

    #[test]
    fn jaccard_of_a_set_with_itself_is_one() {
        let words = word_tokens("pty readiness probe flakes on slow ci");
        assert_eq!(jaccard(&words, &words), 1.0);
        let grams = char_trigrams("pty readiness probe");
        assert_eq!(jaccard(&grams, &grams), 1.0);
    }

    #[test]
    fn jaccard_of_disjoint_sets_is_zero() {
        let a = word_tokens("checkout total overlaps confirm button");
        let b = word_tokens("sqlite migration checksum drifts");
        assert!(a.is_disjoint(&b));
        assert_eq!(jaccard(&a, &b), 0.0);
    }

    #[test]
    fn jaccard_of_two_empty_sets_is_zero() {
        let empty = word_tokens("the and of");
        assert!(empty.is_empty());
        assert_eq!(jaccard(&empty, &empty), 0.0);
    }
}
