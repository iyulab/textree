//! Heading anchors: the ids that headings get when a document is rendered.
//!
//! The rule is the one of `github-slugger` 2.x, which is what the publishing renderer uses: the
//! text is lower-cased, everything but letters, marks, numbers, `_`, `-` and spaces is dropped,
//! and each space becomes a `-`. Within one document equal slugs get `-1`, `-2`, … in order.
//! No dependency is needed; letters and numbers come from `char::is_alphanumeric` (minus the
//! numbers the algorithm drops) and the combining marks that it does not cover from a small
//! table of ranges.

use std::collections::HashMap;

/// Combining marks (categories Mn, Mc, Me): the common slug algorithm keeps all of them, and
/// `char::is_alphanumeric` covers only some.
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
        | 0x0711
        | 0x0730..=0x074A
        | 0x07A6..=0x07B0
        | 0x07EB..=0x07F3
        | 0x07FD
        | 0x0816..=0x0819
        | 0x081B..=0x0823
        | 0x0825..=0x0827
        | 0x0829..=0x082D
        | 0x0859..=0x085B
        | 0x08D3..=0x08E1
        | 0x08E3..=0x0903
        | 0x093A..=0x093C
        | 0x093E..=0x094F
        | 0x0951..=0x0957
        | 0x0962..=0x0963
        | 0x0981..=0x0983
        | 0x09BC
        | 0x09BE..=0x09C4
        | 0x09C7..=0x09C8
        | 0x09CB..=0x09CD
        | 0x09D7
        | 0x09E2..=0x09E3
        | 0x09FE
        | 0x0A01..=0x0A03
        | 0x0A3C
        | 0x0A3E..=0x0A42
        | 0x0A47..=0x0A48
        | 0x0A4B..=0x0A4D
        | 0x0A51
        | 0x0A70..=0x0A71
        | 0x0A75
        | 0x0A81..=0x0A83
        | 0x0ABC
        | 0x0ABE..=0x0AC5
        | 0x0AC7..=0x0AC9
        | 0x0ACB..=0x0ACD
        | 0x0AE2..=0x0AE3
        | 0x0AFA..=0x0AFF
        | 0x0B01..=0x0B03
        | 0x0B3C
        | 0x0B3E..=0x0B44
        | 0x0B47..=0x0B48
        | 0x0B4B..=0x0B4D
        | 0x0B55..=0x0B57
        | 0x0B62..=0x0B63
        | 0x0B82
        | 0x0BBE..=0x0BC2
        | 0x0BC6..=0x0BC8
        | 0x0BCA..=0x0BCD
        | 0x0BD7
        | 0x0C00..=0x0C04
        | 0x0C3E..=0x0C44
        | 0x0C46..=0x0C48
        | 0x0C4A..=0x0C4D
        | 0x0C55..=0x0C56
        | 0x0C62..=0x0C63
        | 0x0C81..=0x0C83
        | 0x0CBC
        | 0x0CBE..=0x0CC4
        | 0x0CC6..=0x0CC8
        | 0x0CCA..=0x0CCD
        | 0x0CD5..=0x0CD6
        | 0x0CE2..=0x0CE3
        | 0x0D00..=0x0D03
        | 0x0D3B..=0x0D3C
        | 0x0D3E..=0x0D44
        | 0x0D46..=0x0D48
        | 0x0D4A..=0x0D4D
        | 0x0D57
        | 0x0D62..=0x0D63
        | 0x0D81..=0x0D83
        | 0x0DCA
        | 0x0DCF..=0x0DD4
        | 0x0DD6
        | 0x0DD8..=0x0DDF
        | 0x0DF2..=0x0DF3
        | 0x0E31
        | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E
        | 0x0EB1
        | 0x0EB4..=0x0EBC
        | 0x0EC8..=0x0ECD
        | 0x0F18..=0x0F19
        | 0x0F35
        | 0x0F37
        | 0x0F39
        | 0x0F3E..=0x0F3F
        | 0x0F71..=0x0F84
        | 0x0F86..=0x0F87
        | 0x0F8D..=0x0F97
        | 0x0F99..=0x0FBC
        | 0x0FC6
        | 0x102B..=0x103E
        | 0x1056..=0x1059
        | 0x105E..=0x1060
        | 0x1062..=0x1064
        | 0x1067..=0x106D
        | 0x1071..=0x1074
        | 0x1082..=0x108D
        | 0x108F
        | 0x109A..=0x109D
        | 0x135D..=0x135F
        | 0x1712..=0x1714
        | 0x1732..=0x1734
        | 0x1752..=0x1753
        | 0x1772..=0x1773
        | 0x17B4..=0x17D3
        | 0x17DD
        | 0x180B..=0x180D
        | 0x1885..=0x1886
        | 0x18A9
        | 0x1920..=0x192B
        | 0x1930..=0x193B
        | 0x1A17..=0x1A1B
        | 0x1A55..=0x1A5E
        | 0x1A60..=0x1A7C
        | 0x1A7F
        | 0x1AB0..=0x1AC0
        | 0x1B00..=0x1B04
        | 0x1B34..=0x1B44
        | 0x1B6B..=0x1B73
        | 0x1B80..=0x1B82
        | 0x1BA1..=0x1BAD
        | 0x1BE6..=0x1BF3
        | 0x1C24..=0x1C37
        | 0x1CD0..=0x1CD2
        | 0x1CD4..=0x1CE8
        | 0x1CED
        | 0x1CF4
        | 0x1CF7..=0x1CF9
        | 0x1DC0..=0x1DF9
        | 0x1DFB..=0x1DFF
        | 0x20D0..=0x20F0
        | 0x2CEF..=0x2CF1
        | 0x2D7F
        | 0x2DE0..=0x2DFF
        | 0x302A..=0x302F
        | 0x3099..=0x309A
        | 0xA66F..=0xA672
        | 0xA674..=0xA67D
        | 0xA69E..=0xA69F
        | 0xA6F0..=0xA6F1
        | 0xA802
        | 0xA806
        | 0xA80B
        | 0xA823..=0xA827
        | 0xA82C
        | 0xA880..=0xA881
        | 0xA8B4..=0xA8C5
        | 0xA8E0..=0xA8F1
        | 0xA8FF
        | 0xA926..=0xA92D
        | 0xA947..=0xA953
        | 0xA980..=0xA983
        | 0xA9B3..=0xA9C0
        | 0xA9E5
        | 0xAA29..=0xAA36
        | 0xAA43
        | 0xAA4C..=0xAA4D
        | 0xAA7B..=0xAA7D
        | 0xAAB0
        | 0xAAB2..=0xAAB4
        | 0xAAB7..=0xAAB8
        | 0xAABE..=0xAABF
        | 0xAAC1
        | 0xAAEB..=0xAAEF
        | 0xAAF5..=0xAAF6
        | 0xABE3..=0xABEA
        | 0xABEC..=0xABED
        | 0xFB1E
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0x101FD
        | 0x102E0
        | 0x10376..=0x1037A
        | 0x10A01..=0x10A03
        | 0x10A05..=0x10A06
        | 0x10A0C..=0x10A0F
        | 0x10A38..=0x10A3A
        | 0x10A3F
        | 0x10AE5..=0x10AE6
        | 0x10D24..=0x10D27
        | 0x10EAB..=0x10EAC
        | 0x10F46..=0x10F50
        | 0x11000..=0x11002
        | 0x11038..=0x11046
        | 0x1107F..=0x11082
        | 0x110B0..=0x110BA
        | 0x11100..=0x11102
        | 0x11127..=0x11134
        | 0x11145..=0x11146
        | 0x11173
        | 0x11180..=0x11182
        | 0x111B3..=0x111C0
        | 0x111C9..=0x111CC
        | 0x111CE..=0x111CF
        | 0x1122C..=0x11237
        | 0x1123E
        | 0x112DF..=0x112EA
        | 0x11300..=0x11303
        | 0x1133B..=0x1133C
        | 0x1133E..=0x11344
        | 0x11347..=0x11348
        | 0x1134B..=0x1134D
        | 0x11357
        | 0x11362..=0x11363
        | 0x11366..=0x1136C
        | 0x11370..=0x11374
        | 0x11435..=0x11446
        | 0x1145E
        | 0x114B0..=0x114C3
        | 0x115AF..=0x115B5
        | 0x115B8..=0x115C0
        | 0x115DC..=0x115DD
        | 0x11630..=0x11640
        | 0x116AB..=0x116B7
        | 0x1171D..=0x1172B
        | 0x1182C..=0x1183A
        | 0x11930..=0x11935
        | 0x11937..=0x11938
        | 0x1193B..=0x1193E
        | 0x11940
        | 0x11942..=0x11943
        | 0x119D1..=0x119D7
        | 0x119DA..=0x119E0
        | 0x119E4
        | 0x11A01..=0x11A0A
        | 0x11A33..=0x11A39
        | 0x11A3B..=0x11A3E
        | 0x11A47
        | 0x11A51..=0x11A5B
        | 0x11A8A..=0x11A99
        | 0x11C2F..=0x11C36
        | 0x11C38..=0x11C3F
        | 0x11C92..=0x11CA7
        | 0x11CA9..=0x11CB6
        | 0x11D31..=0x11D36
        | 0x11D3A
        | 0x11D3C..=0x11D3D
        | 0x11D3F..=0x11D45
        | 0x11D47
        | 0x11D8A..=0x11D8E
        | 0x11D90..=0x11D91
        | 0x11D93..=0x11D97
        | 0x11EF3..=0x11EF6
        | 0x16AF0..=0x16AF4
        | 0x16B30..=0x16B36
        | 0x16F4F
        | 0x16F51..=0x16F87
        | 0x16F8F..=0x16F92
        | 0x16FE4
        | 0x16FF0..=0x16FF1
        | 0x1BC9D..=0x1BC9E
        | 0x1D165..=0x1D169
        | 0x1D16D..=0x1D172
        | 0x1D17B..=0x1D182
        | 0x1D185..=0x1D18B
        | 0x1D1AA..=0x1D1AD
        | 0x1D242..=0x1D244
        | 0x1DA00..=0x1DA36
        | 0x1DA3B..=0x1DA6C
        | 0x1DA75
        | 0x1DA84
        | 0x1DA9B..=0x1DA9F
        | 0x1DAA1..=0x1DAAF
        | 0x1E000..=0x1E006
        | 0x1E008..=0x1E018
        | 0x1E01B..=0x1E021
        | 0x1E023..=0x1E024
        | 0x1E026..=0x1E02A
        | 0x1E130..=0x1E136
        | 0x1E2EC..=0x1E2EF
        | 0x1E8D0..=0x1E8D6
        | 0x1E944..=0x1E94A
        | 0xE0100..=0xE01EF
    )
}

/// Numbers that are not digits or numerals (superscripts, fractions, circled numbers): counted by
/// `char::is_alphanumeric`, but dropped by the common slug algorithm.
fn is_other_number(c: char) -> bool {
    matches!(c as u32,
        0x00B2..=0x00B3
        | 0x00B9
        | 0x00BC..=0x00BE
        | 0x09F4..=0x09F9
        | 0x0B72..=0x0B77
        | 0x0BF0..=0x0BF2
        | 0x0C78..=0x0C7E
        | 0x0D58..=0x0D5E
        | 0x0D70..=0x0D78
        | 0x0F2A..=0x0F33
        | 0x1369..=0x137C
        | 0x17F0..=0x17F9
        | 0x19DA
        | 0x2070
        | 0x2074..=0x2079
        | 0x2080..=0x2089
        | 0x2150..=0x215F
        | 0x2189
        | 0x2460..=0x249B
        | 0x24EA..=0x24FF
        | 0x2776..=0x2793
        | 0x2CFD
        | 0x3192..=0x3195
        | 0x3220..=0x3229
        | 0x3248..=0x324F
        | 0x3251..=0x325F
        | 0x3280..=0x3289
        | 0x32B1..=0x32BF
        | 0xA830..=0xA835
        | 0x10107..=0x10133
        | 0x10175..=0x10178
        | 0x1018A..=0x1018B
        | 0x102E1..=0x102FB
        | 0x10320..=0x10323
        | 0x10858..=0x1085F
        | 0x10879..=0x1087F
        | 0x108A7..=0x108AF
        | 0x108FB..=0x108FF
        | 0x10916..=0x1091B
        | 0x109BC..=0x109BD
        | 0x109C0..=0x109CF
        | 0x109D2..=0x109FF
        | 0x10A40..=0x10A48
        | 0x10A7D..=0x10A7E
        | 0x10A9D..=0x10A9F
        | 0x10AEB..=0x10AEF
        | 0x10B58..=0x10B5F
        | 0x10B78..=0x10B7F
        | 0x10BA9..=0x10BAF
        | 0x10CFA..=0x10CFF
        | 0x10E60..=0x10E7E
        | 0x10F1D..=0x10F26
        | 0x10F51..=0x10F54
        | 0x10FC5..=0x10FCB
        | 0x11052..=0x11065
        | 0x111E1..=0x111F4
        | 0x1173A..=0x1173B
        | 0x118EA..=0x118F2
        | 0x11C5A..=0x11C6C
        | 0x11FC0..=0x11FD4
        | 0x16B5B..=0x16B61
        | 0x16E80..=0x16E96
        | 0x1D2E0..=0x1D2F3
        | 0x1D360..=0x1D378
        | 0x1E8C7..=0x1E8CF
        | 0x1EC71..=0x1ECAB
        | 0x1ECAD..=0x1ECAF
        | 0x1ECB1..=0x1ECB4
        | 0x1ED01..=0x1ED2D
        | 0x1ED2F..=0x1ED3D
        | 0x1F100..=0x1F10C
    )
}

/// Connector punctuation other than `_` that the common slug algorithm keeps.
fn is_connector(c: char) -> bool {
    matches!(
        c as u32,
        0x203F | 0x2040 | 0x2054 | 0xFE33 | 0xFE34 | 0xFE4D | 0xFE4E | 0xFE4F | 0xFF3F
    )
}

/// The slug of one heading text, without the numbering of repeated headings.
pub fn github_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.to_lowercase().chars() {
        if c == ' ' {
            out.push('-');
        } else if c == '-'
            || c == '_'
            || (c.is_alphanumeric() && !is_other_number(c))
            || is_combining_mark(c)
            || is_connector(c)
        {
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
            // Other numbers (circled, superscript, fractions) are dropped; numerals are kept.
            ("② Item", "-item"),
            ("x² and ½", "x-and-"),
            ("Ⅳ ٣", "ⅳ-٣"),
            // Marks that are not letters are kept.
            ("e\u{0301}\u{0e31}", "e\u{0301}\u{0e31}"),
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
