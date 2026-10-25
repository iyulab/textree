//! The format of the settings the application keeps for each notes folder, outside it.
//!
//! That settings folder carries one number: the format its files are in. Each release knows the
//! formats up to its own, and brings a folder in an older one up to date when it is opened, one step
//! at a time. A folder in a format it does not know — written by a newer release, or a marker it
//! cannot read — is left exactly as it is: the application shows its defaults there and writes
//! nothing until it is updated. Otherwise going back to an older release would rewrite the settings
//! in the shape it knows, over the shape it does not, and the newer release would find them mixed or
//! gone.

use std::io;
use std::path::Path;
use tauri_kit_state::{Step, Versioned};

pub use tauri_kit_state::set_aside;
#[cfg(test)]
pub use tauri_kit_state::FORMAT_FILE;

/// The format this release keeps settings in.
pub const CURRENT: u32 = 1;

/// Steps from each format to the next, in order: step `i` brings format `i` to `i + 1`. The format
/// is written only after the last one, so a step cut short runs again on the next open — every step
/// must be safe to run twice, and must write what it makes before removing what it replaces.
const STEPS: [Step; CURRENT as usize] = [
    // 0 → 1: the files do not change. Format 1 is where the folder starts saying what it holds.
    |_, _| Ok(()),
];

const FORMATS: Versioned<'static> = Versioned::new(&STEPS);

/// Whether settings may be written into `dir` — not when they are in a format this release does
/// not know.
pub fn writable(dir: &Path) -> io::Result<bool> {
    FORMATS.writable(dir)
}

/// Brings the settings in `dir` to the current format. `false`: they are in one this release does
/// not know, and were left untouched. `write` writes a file atomically.
pub fn bring_up_to_date(
    dir: &Path,
    write: impl Fn(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<bool> {
    FORMATS.bring_up_to_date(dir, &(), write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn plain_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, bytes)
    }

    #[test]
    fn a_folder_that_never_said_is_brought_to_the_current_format() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("settings");
        assert!(bring_up_to_date(&dir, plain_write).unwrap());
        assert_eq!(std::fs::read_to_string(dir.join(FORMAT_FILE)).unwrap(), CURRENT.to_string());
    }

    #[test]
    fn a_newer_format_is_not_writable() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(FORMAT_FILE), (CURRENT + 1).to_string()).unwrap();
        assert!(!bring_up_to_date(tmp.path(), plain_write).unwrap());
        assert!(!writable(tmp.path()).unwrap());
    }
}
