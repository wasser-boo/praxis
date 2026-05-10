//! Small utilities shared across the codebase.

/// Return a UTF-8-safe truncation of `s` to at most `max_chars` Unicode scalar
/// values. If the string was longer, append a single ellipsis character so the
/// caller can tell it was clipped. Never panics — unlike byte-indexed slicing
/// (`&s[..n]`), which panics if `n` falls inside a multi-byte UTF-8 codepoint.
///
/// This is intentionally cheap (single linear scan) and produces correct output
/// for any LLM/user content, including emoji and non-ASCII characters.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// Same as [`truncate_chars`] but returns the truncated string with a literal
/// `"..."` suffix instead of the single-character ellipsis. Useful where an
/// ASCII-only marker is preferred for log readability.
pub fn truncate_chars_ascii(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii_short_unchanged() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_ascii_long_clipped() {
        assert_eq!(truncate_chars("hello world", 5), "hello…");
        assert_eq!(truncate_chars_ascii("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_does_not_panic_on_emoji() {
        // Each emoji is multi-byte. A naive `&s[..n]` would panic for many n.
        let emoji_string = "🎉🎊🎈🎁🎂🍰🍪🍩🍿🌟";
        for n in 0..30 {
            let _ = truncate_chars(emoji_string, n);
            let _ = truncate_chars_ascii(emoji_string, n);
        }
    }

    #[test]
    fn truncate_does_not_panic_on_combining_marks() {
        // Decomposed forms ('é' as 'e' + combining acute) — still safe.
        let s = "ne\u{0301}cessite\u{0301}";
        for n in 0..s.chars().count() + 5 {
            let _ = truncate_chars(s, n);
        }
    }

    #[test]
    fn truncate_zero() {
        assert_eq!(truncate_chars("hello", 0), "…");
        assert_eq!(truncate_chars_ascii("hello", 0), "...");
    }
}
