//! Path helpers: the engine's paths are relative to the root and `/`-separated.

use unicode_normalization::UnicodeNormalization;

/// The text in Unicode normalization form C. Names, headings and tags are compared in this
/// form, so a name written in one form finds a file whose name is stored in the other (as file
/// systems and keyboards often differ).
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// `text` in the normalization form `like` is written in: form C, form D, or (when `like` does
/// not tell — plain ASCII, or mixed) `text` as it is.
pub fn in_form_of(like: &str, text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    let is_c = unicode_normalization::is_nfc(like);
    let is_d = unicode_normalization::is_nfd(like);
    match (is_c, is_d) {
        (true, false) => nfc(text),
        (false, true) => text.nfd().collect(),
        _ => text.to_string(),
    }
}

/// The key two names are compared by: normalization form C, then lower case.
pub fn fold(text: &str) -> String {
    nfc(text).to_lowercase()
}

/// The folder part of a path (`""` for a root-level file).
pub fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(slash) => &path[..slash],
        None => "",
    }
}

/// The last component of a path.
pub fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A file name without its last extension.
pub fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// Decodes `%XX` sequences. A sequence that is not valid is kept as written.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Whether a link destination leaves the hierarchy: a URL scheme (`https:`, `mailto:`, a
/// Windows drive letter), a protocol-relative `//host` or a network path.
pub fn is_external(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
    let mut chars = dest.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (_, c) in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

/// Resolves a relative destination against the folder of the linking document.
/// Returns `None` when the destination climbs out of the root or names the root itself.
pub fn resolve_relative(from_dir: &str, dest: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let rooted = dest.starts_with('/');
    if !rooted && !from_dir.is_empty() {
        parts.extend(from_dir.split('/'));
    }
    for seg in dest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Why a path is not a path inside the root; `Ok` when it is: relative, `/`-separated, with no
/// empty, `.` or `..` piece and none of the characters `\`, `:` or NUL.
pub fn check_inside_root(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("is empty");
    }
    if path.starts_with('/') {
        return Err("is absolute");
    }
    for piece in path.split('/') {
        match piece {
            "" => return Err("has an empty piece"),
            "." | ".." => return Err("has a . or .. piece"),
            _ => {}
        }
        if piece.contains(['\\', ':', '\0']) {
            return Err("has a piece with \\ : or NUL");
        }
    }
    Ok(())
}

/// The path from the folder `from_dir` to `target`, written with `..` pieces where needed.
pub fn relative_between(from_dir: &str, target: &str) -> String {
    let from: Vec<&str> = if from_dir.is_empty() {
        Vec::new()
    } else {
        from_dir.split('/').collect()
    };
    let to: Vec<&str> = target.split('/').collect();
    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

/// Where a relative destination that climbs out of the root points: how many levels above the
/// root, and the pieces below that level. `None` when it stays inside the root.
pub fn outside_location(from_dir: &str, dest: &str) -> Option<(usize, Vec<String>)> {
    let mut stack: Vec<String> = if from_dir.is_empty() {
        Vec::new()
    } else {
        from_dir.split('/').map(String::from).collect()
    };
    let mut ups = 0usize;
    for piece in dest.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    ups += 1;
                }
            }
            other => {
                if ups > 0 {
                    // Below an outside level the pieces are kept as they are.
                }
                stack.push(other.to_string());
            }
        }
    }
    if ups > 0 {
        Some((ups, stack))
    } else {
        None
    }
}

/// The destination that, written in the folder `from_dir`, points at the outside location.
pub fn relative_to_outside(from_dir: &str, ups: usize, pieces: &[String]) -> String {
    let depth = if from_dir.is_empty() {
        0
    } else {
        from_dir.split('/').count()
    };
    let mut out = vec![".."; depth + ups].join("/");
    for p in pieces {
        out.push('/');
        out.push_str(p);
    }
    out
}

/// Writes a path as a link destination. Inside `< >` it is kept as is; otherwise the
/// characters that would end or confuse a destination are written as `%XX`.
pub fn encode_destination(path: &str, angle: bool) -> String {
    // Inside `< >` a path is kept as it is, unless it holds a character that cannot be there.
    if angle && !path.contains(['<', '>', '\n', '\\']) {
        return path.to_string();
    }
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '[' => out.push_str("%5B"),
            ']' => out.push_str("%5D"),
            '|' => out.push_str("%7C"),
            '`' => out.push_str("%60"),
            '\n' => out.push_str("%0A"),
            '\\' => out.push_str("%5C"),
            other => out.push(other),
        }
    }
    out
}
