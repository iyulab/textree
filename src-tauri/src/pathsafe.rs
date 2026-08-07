//! Path safety validation. Prevents escape outside the vault root and dangerous node names.
//! Shared by commands (IPC) and fs_ops (mutations).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Validates that the candidate path is inside the vault root (prevents parent escape).
/// Based on canonicalize, so it is only valid for paths that **already exist** — for paths
/// to be created, call it against the parent directory and validate the new name with [`is_valid_name`].
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(r), Ok(c)) => c.starts_with(r),
        _ => false,
    }
}

/// The deepest ancestor of `candidate` that exists, and the names below it, outermost first.
///
/// `None` when walking up runs out of path, which includes a candidate ending in a component
/// that is not a plain name.
fn deepest_existing(candidate: &Path) -> Option<(PathBuf, Vec<OsString>)> {
    let mut existing = candidate.to_path_buf();
    let mut below: Vec<OsString> = Vec::new();
    while !existing.exists() {
        below.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?.to_path_buf();
    }
    below.reverse();
    Some((existing, below))
}

/// The vault-relative, `/`-separated form of a path that need not exist.
///
/// [`is_within`] resolves both ends against the filesystem, which is right for a path something
/// is at and useless for one nothing is at any more. Asking what a note used to hold is asking
/// about a name the folder no longer has, so the check cannot require the name to be there:
/// the deepest part that does exist is resolved as usual, and the names below it are admitted
/// only if each is a plain one, which is what stops the missing part from climbing back out.
///
/// `None` when the path leaves the vault, when it names the vault itself, or when a component
/// below the existing part is anything other than a plain name.
pub fn rel_within(root: &Path, candidate: &Path) -> Option<String> {
    let root_c = root.canonicalize().ok()?;
    let (existing, below) = deepest_existing(candidate)?;
    let existing_c = existing.canonicalize().ok()?;
    let mut out = existing_c.strip_prefix(&root_c).ok()?.to_path_buf();
    for name in below {
        let piece = PathBuf::from(&name);
        let mut parts = piece.components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(_)), None) => out.push(&name),
            _ => return None,
        }
    }
    let text = out.to_str()?.replace('\\', "/");
    (!text.is_empty()).then_some(text)
}

/// Windows reserved device names (reserved regardless of extension).
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether the string is safe as a new name for a node (note/folder). Rejects path separators,
/// parent references, reserved dot names, and Windows reserved device names.
pub fn is_valid_name(name: &str) -> bool {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('.') // avoid collision with reserved/hidden names like .textree
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return false;
    }
    // Reserved regardless of extension, so compare the part before the first '.' in uppercase (e.g. both CON and CON.md are rejected).
    let base = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    !WINDOWS_RESERVED.contains(&base.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn within_root_allows_inner_rejects_outer() {
        let tmp = TempDir::new().unwrap();
        let inner = tmp.path().join("a.md");
        std::fs::write(&inner, b"hi").unwrap();
        assert!(is_within(tmp.path(), &inner));

        let outer = TempDir::new().unwrap();
        let outer_file = outer.path().join("b.md");
        std::fs::write(&outer_file, b"no").unwrap();
        assert!(!is_within(tmp.path(), &outer_file));
    }

    #[test]
    fn valid_name_rejects_dangerous_inputs() {
        // Non-ASCII names are ordinary note titles, not dangerous input. Keep a non-ASCII
        // fixture here: flattening it to ASCII would leave that case untested.
        assert!(is_valid_name("café-notes"));
        assert!(is_valid_name("2026-06-13"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("."));
        assert!(!is_valid_name(".."));
        assert!(!is_valid_name(".textree"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name("a\\b"));
    }

    #[test]
    fn valid_name_rejects_windows_reserved_devices() {
        assert!(!is_valid_name("CON"));
        assert!(!is_valid_name("con")); // case-insensitive
        assert!(!is_valid_name("NUL"));
        assert!(!is_valid_name("COM1"));
        assert!(!is_valid_name("LPT9"));
        assert!(!is_valid_name("CON.md")); // regardless of extension
        // Normal names that merely contain a reserved name are allowed.
        assert!(is_valid_name("CONTROL"));
        assert!(is_valid_name("réunion-CON"));
    }

    #[test]
    fn a_path_nothing_is_at_can_still_be_placed_in_the_vault() {
        // What a deleted note used to hold is asked for by a name the folder no longer has.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();

        assert_eq!(rel_within(tmp.path(), &tmp.path().join("gone.md")).as_deref(), Some("gone.md"));
        assert_eq!(
            rel_within(tmp.path(), &tmp.path().join("sub").join("gone.md")).as_deref(),
            Some("sub/gone.md"),
            "the folder still exists, only the note is missing"
        );
        assert_eq!(
            rel_within(tmp.path(), &tmp.path().join("gone").join("deeper.md")).as_deref(),
            Some("gone/deeper.md"),
            "neither the folder nor the note has to exist"
        );
    }

    #[test]
    fn a_path_nothing_is_at_is_still_refused_when_it_leaves_the_vault() {
        // Admitting missing names must not admit ones that climb out with them.
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        assert_eq!(rel_within(tmp.path(), &outside.path().join("gone.md")), None);
        assert_eq!(rel_within(tmp.path(), &tmp.path().join("..").join("gone.md")), None);
        assert_eq!(rel_within(tmp.path(), tmp.path()), None, "the vault is not a note");
    }

    #[test]
    fn within_is_false_for_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        // A path that does not exist yet fails canonicalize -> false (creation must be validated against the parent).
        assert!(!is_within(tmp.path(), &tmp.path().join("new.md")));
    }
}
