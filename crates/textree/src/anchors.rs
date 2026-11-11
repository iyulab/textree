//! Heading anchors: the ids that headings get when a document is rendered.
//!
//! The rule is the one of `github-slugger` 2.x, which is what the publishing renderer uses: the
//! text is lower-cased, everything but letters, marks, numbers, `_`, `-` and spaces is dropped,
//! and each space becomes a `-`. Within one document equal slugs get `-1`, `-2`, … in order.
//! No dependency is needed; letters and numbers come from `char::is_alphanumeric` and the
//! combining marks that it does not cover from a small table of ranges.

use std::collections::HashMap;

/// Combining marks that `char::is_alphanumeric` does not already count as letters.
fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x05BF
        | 0x05C1..=0x05C2
        | 0x05C4..=0x05C5
        | 0x05C7
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x06E7..=0x06E8
        | 0x06EA..=0x06ED
        | 0x0900..=0x0903
        | 0x093A..=0x094F
        | 0x0E31
        | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF
        | 0x3099..=0x309A
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F)
}

/// The slug of one heading text, without the numbering of repeated headings.
pub fn github_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.to_lowercase().chars() {
        if c == ' ' {
            out.push('-');
        } else if c == '-' || c == '_' || c.is_alphanumeric() || is_combining_mark(c) {
            out.push(c);
        }
    }
    out
}

/// The anchors of the headings of one document, in order, with repeats numbered.
pub fn anchors_of<S: AsRef<str>>(headings: &[S]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(headings.len());
    for h in headings {
        let base = github_slug(h.as_ref());
        let mut slug = base.clone();
        while seen.contains_key(&slug) {
            let n = seen.get_mut(&base).map(|c| {
                *c += 1;
                *c
            });
            slug = format!("{base}-{}", n.unwrap_or(1));
        }
        seen.insert(slug.clone(), 0);
        out.push(slug);
    }
    out
}

/// A file name stem made from a heading: its slug with runs of `-` made one and the ends
/// trimmed, a name of at most 60 characters that is not a reserved device name on Windows.
/// `None` when nothing is left.
pub fn file_stem_of(heading: &str) -> Option<String> {
    let slug = github_slug(heading);
    let mut stem = String::new();
    for c in slug.chars() {
        if c == '-' && (stem.is_empty() || stem.ends_with('-')) {
            continue;
        }
        stem.push(c);
    }
    let mut stem: String = stem.trim_end_matches('-').chars().take(60).collect();
    stem = stem.trim_end_matches('-').to_string();
    if stem.is_empty() {
        return None;
    }
    const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];
    let device = RESERVED.contains(&stem.as_str())
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if device {
        stem = format!("section-{stem}");
    }
    Some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_follow_github_slugger() {
        for (heading, want) in [
            ("Day 1", "day-1"),
            ("Café & Bar!", "café--bar"),
            ("한글 제목", "한글-제목"),
            ("code and emph link", "code-and-emph-link"),
            ("!!!", ""),
            ("Hello, World!", "hello-world"),
            ("a_b-c", "a_b-c"),
            ("日本語 テスト", "日本語-テスト"),
            ("What’s new?", "whats-new"),
            ("🎉 Party", "-party"),
            ("  lead", "--lead"),
        ] {
            assert_eq!(github_slug(heading), want, "{heading:?}");
        }
    }

    #[test]
    fn repeats_are_numbered() {
        assert_eq!(
            anchors_of(&["Day 1", "Day 1", "Day 1", "Part", "Part"]),
            vec!["day-1", "day-1-1", "day-1-2", "part", "part-1"]
        );
        assert_eq!(anchors_of(&["!!!", "!!!"]), vec!["", "-1"]);
    }

    #[test]
    fn file_stems() {
        assert_eq!(file_stem_of("Café & Bar!").as_deref(), Some("café-bar"));
        assert_eq!(file_stem_of("!!!"), None);
        assert_eq!(file_stem_of("CON").as_deref(), Some("section-con"));
        assert_eq!(file_stem_of("Lpt1").as_deref(), Some("section-lpt1"));
        assert_eq!(file_stem_of(&"x".repeat(70)).map(|s| s.len()), Some(60));
        assert_eq!(file_stem_of("Com0").as_deref(), Some("com0"));
    }
}
