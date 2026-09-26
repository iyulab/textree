//! Edits that could not be written to a note already left, kept until they are.
//!
//! While the application runs, such an edit waits in memory and is tried again with every save.
//! Kept only there, it would be lost when the application closes before the note's folder takes it —
//! a folder that stopped answering, or a note that changed on disk and could not be copied beside.
//! So each one is also written here, one file per edit, and removed once it lands; the next time the
//! folder is opened, whatever is still here is tried again.
//!
//! These live with the folder's settings, outside the folder itself. They are not drafts — a draft is
//! something being worked on; these are saves that did not happen — so they have a place of their own.
//! Older releases do not look here, so adding it changes nothing they read.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Component, Path, PathBuf};

/// The directory, inside a folder's settings directory.
pub const DIR: &str = "stranded";

/// One kept edit as stored: the note's path inside its folder, what to write, and what the note held
/// when the edit started from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub rel: String,
    pub text: String,
    pub base: String,
    /// When it was kept, in milliseconds since the Unix epoch.
    pub at: u64,
}

/// `path` relative to `root`, `/`-separated — worked out from the text of the two paths alone.
/// Looking at the disk here could hang on the very folder that stopped answering.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(name) => parts.push(name.to_str()?.to_string()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Keeps `kept` in `dir`, under a name that orders by when it was kept. Returns the name.
pub fn keep(
    dir: &Path,
    kept: &Kept,
    write: impl Fn(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<String> {
    let body = serde_json::to_vec(kept).map_err(io::Error::other)?;
    let mut n = 0u32;
    loop {
        let id = format!("{:013}-{n}", kept.at);
        let file = dir.join(format!("{id}.json"));
        if !file.exists() {
            write(&file, &body)?;
            return Ok(id);
        }
        n += 1;
    }
}

/// Every kept edit in `dir`, oldest first. A file that cannot be read is left where it is — it may be
/// the only copy of what someone typed — and skipped.
pub fn list(dir: &Path) -> Vec<(String, Kept)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, Kept)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let id = name.strip_suffix(".json")?.to_string();
            valid_id(&id).then_some(())?;
            let bytes = std::fs::read(entry.path()).ok()?;
            match serde_json::from_slice::<Kept>(&bytes) {
                Ok(kept) => Some((id, kept)),
                Err(e) => {
                    log::warn!("stranded: {name} could not be read, left in place: {e}");
                    None
                }
            }
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Removes a kept edit once it has landed.
pub fn forget(dir: &Path, id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::other("not a kept edit"));
    }
    match std::fs::remove_file(file_of(dir, id)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn file_of(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Ids are digits and one dash — nothing that could name a file anywhere else.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn plain_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, bytes)
    }

    fn kept(rel: &str, text: &str, at: u64) -> Kept {
        Kept { rel: rel.into(), text: text.into(), base: "before".into(), at }
    }

    #[test]
    fn kept_edits_come_back_oldest_first_and_go_once_forgotten() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(DIR);
        let second = keep(&dir, &kept("b.md", "b", 20), plain_write).unwrap();
        let first = keep(&dir, &kept("a.md", "a", 10), plain_write).unwrap();
        let same_moment = keep(&dir, &kept("a.md", "a again", 10), plain_write).unwrap();
        assert_ne!(first, same_moment, "two edits kept in the same moment are both kept");

        let listed: Vec<String> = list(&dir).into_iter().map(|(_, k)| k.text).collect();
        assert_eq!(listed, ["a", "a again", "b"]);

        forget(&dir, &first).unwrap();
        forget(&dir, &first).unwrap(); // already gone is fine
        let listed: Vec<String> = list(&dir).into_iter().map(|(_, k)| k.text).collect();
        assert_eq!(listed, ["a again", "b"]);
        assert!(forget(&dir, &second).is_ok());
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_in_place() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("0000000000010-0.json"), b"{half").unwrap();
        assert!(list(dir).is_empty());
        assert!(dir.join("0000000000010-0.json").exists());
    }

    #[test]
    fn nothing_outside_the_directory_can_be_named() {
        let tmp = TempDir::new().unwrap();
        assert!(forget(tmp.path(), "../settings").is_err());
        assert!(forget(tmp.path(), "").is_err());
    }

    #[test]
    fn a_note_path_is_made_relative_without_looking_at_the_disk() {
        let root = Path::new("/nowhere/notes");
        assert_eq!(relative(root, Path::new("/nowhere/notes/sub/a.md")).as_deref(), Some("sub/a.md"));
        assert_eq!(relative(root, Path::new("/nowhere/other/a.md")), None);
        assert_eq!(relative(root, Path::new("/nowhere/notes/../escape.md")), None);
        assert_eq!(relative(root, root), None);
    }
}
