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
use std::path::{Path, PathBuf};

/// The file holding the format number, beside the settings it describes.
pub const FORMAT_FILE: &str = "format";

/// The format this release keeps settings in.
pub const CURRENT: u32 = 1;

/// Steps from each format to the next, in order: step `i` brings format `i` to `i + 1`. The format
/// is written only after the last one, so a step cut short runs again on the next open — every step
/// must be safe to run twice, and must write what it makes before removing what it replaces.
type Step = fn(&Path) -> io::Result<()>;
const STEPS: [Step; CURRENT as usize] = [
    // 0 → 1: the files do not change. Format 1 is where the folder starts saying what it holds.
    |_| Ok(()),
];

#[derive(Debug, PartialEq, Eq)]
pub enum Format {
    Known(u32),
    /// Newer than this release, or not a number at all. Either way, not ours to rewrite.
    Unknown,
}

/// The format of the settings in `dir`. A folder that never said is in format 0 — the files as they
/// were before the marker existed — and so is one that does not exist yet.
pub fn read(dir: &Path) -> io::Result<Format> {
    match std::fs::read_to_string(dir.join(FORMAT_FILE)) {
        Ok(s) => Ok(match s.trim().parse::<u32>() {
            Ok(n) if n <= CURRENT => Format::Known(n),
            _ => Format::Unknown,
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Format::Known(0)),
        Err(e) => Err(e),
    }
}

/// Whether settings may be written into `dir`.
pub fn writable(dir: &Path) -> io::Result<bool> {
    Ok(read(dir)? != Format::Unknown)
}

/// Brings the settings in `dir` to the current format. `false`: they are in one this release does
/// not know, and were left untouched. `write` writes a file atomically.
pub fn bring_up_to_date(
    dir: &Path,
    write: impl Fn(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<bool> {
    let from = match read(dir)? {
        Format::Unknown => return Ok(false),
        Format::Known(n) if n == CURRENT => return Ok(true),
        Format::Known(n) => n,
    };
    for step in &STEPS[from as usize..] {
        step(dir)?;
    }
    write(&dir.join(FORMAT_FILE), CURRENT.to_string().as_bytes())?;
    Ok(true)
}

/// Moves a settings file that cannot be read out of the way, keeping it: the application goes on
/// with defaults instead of writing them over it, and the file is still there for someone to look
/// at. Returns where it went.
pub fn set_aside(dir: &Path, rel: &str, now_secs: u64) -> io::Result<PathBuf> {
    let from = dir.join(rel);
    let mut to = dir.join(format!("{rel}.unreadable-{now_secs}"));
    let mut n = 1;
    while to.exists() {
        n += 1;
        to = dir.join(format!("{rel}.unreadable-{now_secs}-{n}"));
    }
    std::fs::rename(&from, &to)?;
    Ok(to)
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
        assert_eq!(read(&dir).unwrap(), Format::Known(0));
        assert!(bring_up_to_date(&dir, plain_write).unwrap());
        assert_eq!(std::fs::read_to_string(dir.join(FORMAT_FILE)).unwrap(), CURRENT.to_string());
        assert_eq!(read(&dir).unwrap(), Format::Known(CURRENT));
    }

    #[test]
    fn bringing_up_to_date_again_does_nothing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        bring_up_to_date(dir, plain_write).unwrap();
        let wrote = std::cell::Cell::new(false);
        assert!(bring_up_to_date(dir, |_, _| {
            wrote.set(true);
            Ok(())
        })
        .unwrap());
        assert!(!wrote.get());
    }

    #[test]
    fn a_newer_format_is_left_exactly_as_it_is_and_not_writable() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(FORMAT_FILE), (CURRENT + 1).to_string()).unwrap();
        std::fs::write(dir.join("order.json"), b"{\"shape\":\"from a newer release\"}").unwrap();

        assert!(!bring_up_to_date(dir, plain_write).unwrap());
        assert_eq!(std::fs::read_to_string(dir.join(FORMAT_FILE)).unwrap(), (CURRENT + 1).to_string());
        assert_eq!(
            std::fs::read(dir.join("order.json")).unwrap(),
            b"{\"shape\":\"from a newer release\"}"
        );
        assert!(!writable(dir).unwrap());
    }

    #[test]
    fn a_marker_that_is_not_a_number_is_treated_as_unknown_not_as_old() {
        // Taking it for format 0 would "upgrade" whatever is there, and rewrite it.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(FORMAT_FILE), "two").unwrap();
        assert_eq!(read(tmp.path()).unwrap(), Format::Unknown);
        assert!(!writable(tmp.path()).unwrap());
    }

    #[test]
    fn an_unreadable_file_is_set_aside_with_its_bytes() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("favorites.json"), b"{half written").unwrap();
        let first = set_aside(dir, "favorites.json", 1_700_000_000).unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), b"{half written");
        assert!(!dir.join("favorites.json").exists());

        std::fs::write(dir.join("favorites.json"), b"again").unwrap();
        let second = set_aside(dir, "favorites.json", 1_700_000_000).unwrap();
        assert_ne!(first, second, "a second one in the same second does not replace the first");
        assert_eq!(std::fs::read(&first).unwrap(), b"{half written");
    }
}
