//! Path helpers: the engine's paths are relative to the root and `/`-separated.

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
