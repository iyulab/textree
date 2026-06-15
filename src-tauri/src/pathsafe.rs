//! Path safety validation. Prevents escape outside the vault root and dangerous node names.
//! Shared by commands (IPC) and fs_ops (mutations).

use std::path::Path;

/// Validates that the candidate path is inside the vault root (prevents parent escape).
/// Based on canonicalize, so it is only valid for paths that **already exist** — for paths
/// to be created, call it against the parent directory and validate the new name with [`is_valid_name`].
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(r), Ok(c)) => c.starts_with(r),
        _ => false,
    }
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
    use std::path::PathBuf;
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
        assert!(is_valid_name("프로젝트"));
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
        assert!(is_valid_name("회의록-CON"));
    }

    #[test]
    fn within_is_false_for_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        // A path that does not exist yet fails canonicalize -> false (creation must be validated against the parent).
        assert!(!is_within(tmp.path(), &PathBuf::from(tmp.path().join("new.md"))));
    }
}
