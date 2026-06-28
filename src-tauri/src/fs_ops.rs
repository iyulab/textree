//! Structural edits (M4): note/folder creation, deletion (trash), leaf↔container promotion.
//!
//! Every operation works only within the vault root (parent validation + name validation) and is
//! reflected to the filesystem immediately. Deletion is not permanent removal but a move to
//! `.textree/trash/` (design §5 M4).

use crate::pathsafe::{is_valid_name, is_within};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Supported image extensions (lowercase). Other formats are rejected.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.into())
}

/// Validates that the parent directory is a real directory inside the vault, and the new name is safe.
fn check_parent_and_name(root: &Path, parent: &Path, name: &str) -> io::Result<()> {
    if !is_valid_name(name) {
        return Err(err("invalid name"));
    }
    if !parent.is_dir() {
        return Err(err("parent is not a directory"));
    }
    if !is_within(root, parent) {
        return Err(err("path is outside the vault"));
    }
    Ok(())
}

/// Creates the `parent/stem.md` leaf note. Errors if it already exists.
pub fn create_note(root: &Path, parent: &Path, stem: &str) -> io::Result<PathBuf> {
    check_parent_and_name(root, parent, stem)?;
    let path = parent.join(format!("{stem}.md"));
    if path.exists() {
        return Err(err("an item with the same name already exists"));
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

/// Creates an empty `Untitled.md` leaf in `parent`, auto-numbering on collision
/// (`Untitled`, `Untitled (1)`, …) via `unique_in`. The name is generated and always
/// valid, so unlike `create_note` there is no explicit name to validate.
pub fn create_untitled_note(root: &Path, parent: &Path) -> io::Result<PathBuf> {
    if !parent.is_dir() {
        return Err(err("parent is not a directory"));
    }
    if !is_within(root, parent) {
        return Err(err("path is outside the vault"));
    }
    let path = unique_in(parent, "Untitled.md", false);
    std::fs::write(&path, "")?;
    Ok(path)
}

/// Creates the `parent/name/` container and its folder note `parent/name/name.md`.
/// Also creates the folder note so it becomes an immediately editable container note (design §3.1).
pub fn create_folder(root: &Path, parent: &Path, name: &str) -> io::Result<PathBuf> {
    check_parent_and_name(root, parent, name)?;
    let dir = parent.join(name);
    if dir.exists() {
        return Err(err("an item with the same name already exists"));
    }
    std::fs::create_dir(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), "")?;
    Ok(dir)
}

/// Promotes the leaf note `dir/foo.md` into the container `dir/foo/foo.md` (design §3.3).
/// An operation that makes room to create children. Returns the new container directory.
pub fn promote_leaf(root: &Path, leaf_md: &Path) -> io::Result<PathBuf> {
    if !is_within(root, leaf_md) {
        return Err(err("path is outside the vault"));
    }
    if leaf_md.extension().and_then(|e| e.to_str()) != Some("md") || !leaf_md.is_file() {
        return Err(err("not a leaf note (.md file)"));
    }
    let parent = leaf_md.parent().ok_or_else(|| err("no parent directory"))?;
    let stem = leaf_md
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("cannot read file name"))?;
    let new_dir = parent.join(stem);
    if new_dir.exists() {
        return Err(err("a folder with the same name already exists"));
    }
    std::fs::create_dir(&new_dir)?;
    let new_body = new_dir.join(format!("{stem}.md"));
    std::fs::rename(leaf_md, &new_body)?;
    Ok(new_dir)
}

/// Finds a non-colliding destination path within the trash (`name`, `name (1)`, …).
/// Directories are not split on extension (so names like `journal.backup` are not altered).
pub(crate) fn unique_in(trash: &Path, file_name: &str, is_dir: bool) -> PathBuf {
    let candidate = trash.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match (is_dir, file_name.rsplit_once('.')) {
        (false, Some((s, e))) => (s.to_string(), format!(".{e}")),
        _ => (file_name.to_string(), String::new()),
    };
    let mut n = 1;
    loop {
        let c = trash.join(format!("{stem} ({n}){ext}"));
        if !c.exists() {
            return c;
        }
        n += 1;
    }
}

/// Moves a node (a leaf `.md` or a container directory) to `.textree/trash/`.
/// Not permanent removal but a recoverable trash move. Returns the destination within the trash.
pub fn delete_to_trash(root: &Path, target: &Path) -> io::Result<PathBuf> {
    if !is_within(root, target) {
        return Err(err("path is outside the vault"));
    }
    let root_c = root.canonicalize()?;
    let target_c = target.canonicalize()?;
    if target_c == root_c {
        return Err(err("the vault root cannot be deleted"));
    }
    // .textree itself or anything inside it is not a deletion target (reserved sidecar).
    if target_c.starts_with(root_c.join(".textree")) {
        return Err(err(".textree cannot be deleted"));
    }
    let trash = root.join(".textree").join("trash");
    std::fs::create_dir_all(&trash)?;
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("cannot read name"))?;
    let dest = unique_in(&trash, file_name, target.is_dir());
    std::fs::rename(target, &dest)?;
    Ok(dest)
}

/// Renames a node. A leaf `.md` renames the file; a container renames the directory and its
/// folder note together (node identity = file/folder name, design §3.5). Returns the new path.
pub fn rename_node(root: &Path, target: &Path, new_name: &str) -> io::Result<PathBuf> {
    if !is_valid_name(new_name) {
        return Err(err("invalid name"));
    }
    if !is_within(root, target) {
        return Err(err("path is outside the vault"));
    }
    let parent = target.parent().ok_or_else(|| err("no parent directory"))?;

    if target.is_dir() {
        let old_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| err("cannot read name"))?
            .to_string();
        let new_dir = parent.join(new_name);
        if new_dir.exists() {
            return Err(err("an item with the same name already exists"));
        }
        std::fs::rename(target, &new_dir)?;
        // If a folder note exists, rename it along to match the new folder name.
        let old_body = new_dir.join(format!("{old_name}.md"));
        if old_body.is_file() {
            if let Err(e) = std::fs::rename(&old_body, new_dir.join(format!("{new_name}.md"))) {
                // Step 2 failed → roll back step 1 (the directory rename, best-effort) to avoid partial corruption.
                let _ = std::fs::rename(&new_dir, target);
                return Err(e);
            }
        }
        Ok(new_dir)
    } else {
        if target.extension().and_then(|e| e.to_str()) != Some("md") {
            return Err(err("rename target is not a note/folder"));
        }
        let new_path = parent.join(format!("{new_name}.md"));
        if new_path.exists() {
            return Err(err("an item with the same name already exists"));
        }
        std::fs::rename(target, &new_path)?;
        Ok(new_path)
    }
}

/// Moves a node to a different folder (changes its parent). Returns the new path.
pub fn move_node(root: &Path, src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    if !is_within(root, src) {
        return Err(err("source path is outside the vault"));
    }
    if !dest_dir.is_dir() || !is_within(root, dest_dir) {
        return Err(err("invalid destination folder"));
    }
    let name = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("cannot read name"))?;
    // Block moving a folder into itself or its descendants.
    let src_c = src.canonicalize()?;
    let dest_dir_c = dest_dir.canonicalize()?;
    if dest_dir_c == src_c || dest_dir_c.starts_with(&src_c) {
        return Err(err("cannot move into itself or a descendant folder"));
    }
    let dest = dest_dir.join(name);
    if dest.exists() {
        return Err(err("an item with the same name already exists in the destination folder"));
    }
    std::fs::rename(src, &dest)?;
    Ok(dest)
}

/// Promotes a leaf note into a container, then moves the `src` node inside it (design §3.3).
/// "Drop X onto note Y = make X a child of Y". Bundles promote+move into a single operation, and
/// on move failure rolls back the promotion best-effort so no partial state (a lone folder) is left.
/// Returns the new path of the moved `src`.
pub fn adopt_into_leaf(root: &Path, src: &Path, leaf_md: &Path) -> io::Result<PathBuf> {
    // Pre-mutation validation: disallowed if leaf equals src or is its descendant (move into self).
    if !is_within(root, src) || !is_within(root, leaf_md) {
        return Err(err("path is outside the vault"));
    }
    let src_c = src.canonicalize()?;
    let leaf_c = leaf_md.canonicalize()?;
    if leaf_c == src_c || leaf_c.starts_with(&src_c) {
        return Err(err("cannot move into itself or a descendant"));
    }
    // Promote (promote_leaf validates the .md leaf condition).
    let new_dir = promote_leaf(root, leaf_md)?;
    // Move; on failure roll back the promotion (right after promotion new_dir holds only the folder note).
    match move_node(root, src, &new_dir) {
        Ok(p) => Ok(p),
        Err(move_err) => {
            let stem = new_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let body = new_dir.join(format!("{stem}.md"));
            // If the rollback rename fails (e.g. a transient lock from Windows AV/indexer), the
            // original note vanishes trapped inside new_dir. Rather than swallow it silently, report
            // its location so it can be recovered.
            match std::fs::rename(&body, leaf_md) {
                Ok(()) => {
                    let _ = std::fs::remove_dir(&new_dir);
                    Err(move_err)
                }
                Err(rollback_err) => Err(err(format!(
                    "the move failed and the rollback also failed. The original note is at {}. \
                     (move error: {move_err}; rollback error: {rollback_err})",
                    body.display()
                ))),
            }
        }
    }
}

/// Saves attachment image bytes to `assets/` in the same folder as the current note body
/// (`note_body`, .md) (design §3.2 attachment mapping). Returns the **relative path** to insert into
/// the body (always forward-slash, e.g. `assets/Pasted-1718....png`) — consistent because the .md
/// and `assets/` share a parent.
pub fn save_attachment(
    root: &Path,
    note_body: &Path,
    bytes: &[u8],
    ext: &str,
) -> io::Result<String> {
    if !is_within(root, note_body) {
        return Err(err("path is outside the vault"));
    }
    // Attachments are placed only relative to the note body (.md) — prevents creating assets/ next to an arbitrary path (server-side contract).
    if note_body.extension().and_then(|e| e.to_str()) != Some("md") {
        return Err(err("attachment target is not a note (.md)"));
    }
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err(err("unsupported image format"));
    }
    let parent = note_body
        .parent()
        .ok_or_else(|| err("the note has no parent directory"))?;
    let assets = parent.join("assets");
    std::fs::create_dir_all(&assets)?;
    // Ensure uniqueness without dependencies: an epoch-millis-based name + the unique_in counter on collision.
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = unique_in(&assets, &format!("Pasted-{millis}.{ext}"), false);
    std::fs::write(&dest, bytes)?;
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("cannot read file name"))?;
    Ok(format!("assets/{name}"))
}

/// Restores a trashed node to `original_rel` under root. Recreates missing parent dirs,
/// and on a name collision disambiguates (`name (1)`) — never overwrites (data safety).
/// Returns the restored path.
///
/// SECURITY precondition: callers MUST pre-validate every component of `original_rel`
/// (Component::Normal + is_valid_name) — this function trusts it for the destination
/// and does NOT re-check.
///
/// NOTE: folder-note restore on a name collision degrades the folder-note mapping.
/// Example: restoring `journal/` when `journal/` already exists yields `journal (1)/`
/// containing `journal.md`, which is no longer a valid folder-note (stem mismatch).
/// This is a known content-safe limitation tracked as a follow-up.
pub(crate) fn restore_from_trash(root: &Path, trash_path: &Path, original_rel: &str) -> io::Result<PathBuf> {
    if !is_within(root, trash_path) {
        return Err(err("trash path is outside the vault"));
    }
    let dest = root.join(original_rel);
    let parent = dest.parent().ok_or_else(|| err("no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("cannot read name"))?;
    let final_dest = unique_in(parent, file_name, trash_path.is_dir());
    std::fs::rename(trash_path, &final_dest)?;
    Ok(final_dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_note_makes_md_file() {
        let tmp = TempDir::new().unwrap();
        let p = create_note(tmp.path(), tmp.path(), "새노트").unwrap();
        assert_eq!(p, tmp.path().join("새노트.md"));
        assert!(p.is_file());
    }

    #[test]
    fn create_note_rejects_duplicate_and_bad_name() {
        let tmp = TempDir::new().unwrap();
        create_note(tmp.path(), tmp.path(), "a").unwrap();
        assert!(create_note(tmp.path(), tmp.path(), "a").is_err());
        assert!(create_note(tmp.path(), tmp.path(), "../escape").is_err());
        assert!(create_note(tmp.path(), tmp.path(), ".hidden").is_err());
    }

    #[test]
    fn create_folder_makes_dir_and_folder_note() {
        let tmp = TempDir::new().unwrap();
        let dir = create_folder(tmp.path(), tmp.path(), "일기").unwrap();
        assert!(dir.is_dir());
        assert!(dir.join("일기.md").is_file(), "the folder note should be created");
    }

    #[test]
    fn promote_leaf_moves_md_into_namesake_folder() {
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("foo.md");
        std::fs::write(&leaf, "본문").unwrap();
        let dir = promote_leaf(tmp.path(), &leaf).unwrap();
        assert_eq!(dir, tmp.path().join("foo"));
        assert!(!leaf.exists(), "the original leaf file is moved");
        let body = dir.join("foo.md");
        assert!(body.is_file());
        assert_eq!(std::fs::read_to_string(body).unwrap(), "본문", "content preserved");
    }

    #[test]
    fn delete_moves_to_trash_not_permanent() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("지울노트.md");
        std::fs::write(&note, "x").unwrap();
        let dest = delete_to_trash(tmp.path(), &note).unwrap();
        assert!(!note.exists(), "the original is gone");
        assert!(dest.is_file(), "preserved in the trash");
        assert!(dest.starts_with(tmp.path().join(".textree").join("trash")));
    }

    #[test]
    fn delete_into_trash_disambiguates_collisions() {
        let tmp = TempDir::new().unwrap();
        for _ in 0..2 {
            let note = tmp.path().join("dup.md");
            std::fs::write(&note, "x").unwrap();
            delete_to_trash(tmp.path(), &note).unwrap();
        }
        let trash = tmp.path().join(".textree").join("trash");
        assert!(trash.join("dup.md").is_file());
        assert!(trash.join("dup (1).md").is_file(), "collisions are disambiguated by a counter");
    }

    #[test]
    fn delete_rejects_root_and_textree() {
        let tmp = TempDir::new().unwrap();
        assert!(delete_to_trash(tmp.path(), tmp.path()).is_err());
        let textree = tmp.path().join(".textree");
        std::fs::create_dir_all(&textree).unwrap();
        assert!(delete_to_trash(tmp.path(), &textree).is_err());
    }

    #[test]
    fn rename_leaf_note() {
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("old.md");
        std::fs::write(&leaf, "본문").unwrap();
        let new = rename_node(tmp.path(), &leaf, "new").unwrap();
        assert_eq!(new, tmp.path().join("new.md"));
        assert!(!leaf.exists());
        assert_eq!(std::fs::read_to_string(new).unwrap(), "본문");
    }

    #[test]
    fn rename_container_renames_dir_and_folder_note() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("일기");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("일기.md"), "다이어리").unwrap();
        std::fs::write(dir.join("2026.md"), "자식").unwrap();

        let new_dir = rename_node(tmp.path(), &dir, "journal").unwrap();
        assert_eq!(new_dir, tmp.path().join("journal"));
        assert!(new_dir.join("journal.md").is_file(), "the folder note is renamed too");
        assert_eq!(
            std::fs::read_to_string(new_dir.join("journal.md")).unwrap(),
            "다이어리"
        );
        assert!(new_dir.join("2026.md").is_file(), "children are unchanged");
        assert!(!dir.exists());
    }

    #[test]
    fn rename_container_carries_obsidian_and_canvas_inside() {
        // Renaming a folder must move everything it holds — including an Obsidian config folder and
        // a JSON Canvas file — so a vault shared with Obsidian stays lossless across the rename.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("프로젝트");
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        std::fs::write(dir.join("프로젝트.md"), "본문").unwrap();
        std::fs::write(dir.join(".obsidian").join("app.json"), "{}").unwrap();
        std::fs::write(dir.join("그림.canvas"), "{\"nodes\":[]}").unwrap();

        let new_dir = rename_node(tmp.path(), &dir, "project").unwrap();
        assert!(new_dir.join("project.md").is_file(), "folder note renamed");
        assert_eq!(
            std::fs::read_to_string(new_dir.join(".obsidian").join("app.json")).unwrap(),
            "{}",
            "the Obsidian config moved intact"
        );
        assert_eq!(
            std::fs::read_to_string(new_dir.join("그림.canvas")).unwrap(),
            "{\"nodes\":[]}",
            "the canvas moved intact"
        );
        assert!(!dir.exists());
    }

    #[test]
    fn delete_note_leaves_sibling_obsidian_and_canvas_untouched() {
        // Deleting a note trashes only that file; sibling Obsidian artifacts are never collateral.
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("지울노트.md");
        std::fs::write(&note, "x").unwrap();
        std::fs::create_dir_all(tmp.path().join(".obsidian")).unwrap();
        std::fs::write(tmp.path().join(".obsidian").join("app.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("보드.canvas"), "{}").unwrap();

        delete_to_trash(tmp.path(), &note).unwrap();
        assert!(!note.exists(), "only the note is trashed");
        assert!(tmp.path().join(".obsidian").join("app.json").is_file());
        assert!(tmp.path().join("보드.canvas").is_file());
    }

    #[test]
    fn rename_rejects_duplicate_and_bad_name() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "").unwrap();
        std::fs::write(tmp.path().join("b.md"), "").unwrap();
        assert!(rename_node(tmp.path(), &tmp.path().join("a.md"), "b").is_err());
        assert!(rename_node(tmp.path(), &tmp.path().join("a.md"), "..").is_err());
    }

    #[test]
    fn move_leaf_into_folder() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("noteX.md");
        std::fs::write(&note, "z").unwrap();
        let folder = tmp.path().join("box");
        std::fs::create_dir(&folder).unwrap();

        let dest = move_node(tmp.path(), &note, &folder).unwrap();
        assert_eq!(dest, folder.join("noteX.md"));
        assert!(!note.exists());
        assert!(dest.is_file());
    }

    #[test]
    fn move_rejects_into_self_or_descendant() {
        let tmp = TempDir::new().unwrap();
        let parent = tmp.path().join("p");
        let child = parent.join("c");
        std::fs::create_dir_all(&child).unwrap();
        // Cannot move p into its descendant c.
        assert!(move_node(tmp.path(), &parent, &child).is_err());
        // Cannot move p into itself.
        assert!(move_node(tmp.path(), &parent, &parent).is_err());
    }

    #[test]
    fn move_rejects_duplicate_at_dest() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("dup.md");
        std::fs::write(&note, "").unwrap();
        let folder = tmp.path().join("box");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("dup.md"), "").unwrap(); // already exists
        assert!(move_node(tmp.path(), &note, &folder).is_err());
    }

    #[test]
    fn adopt_promotes_leaf_and_moves_node_inside() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a.md");
        let b = tmp.path().join("b.md");
        std::fs::write(&a, "에이").unwrap();
        std::fs::write(&b, "비").unwrap();

        let moved = adopt_into_leaf(tmp.path(), &a, &b).unwrap();
        // b is promoted to a container and the folder note is preserved.
        let b_dir = tmp.path().join("b");
        assert!(b_dir.is_dir());
        assert_eq!(std::fs::read_to_string(b_dir.join("b.md")).unwrap(), "비");
        // a is moved into b/, content preserved.
        assert_eq!(moved, b_dir.join("a.md"));
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "에이");
    }

    #[test]
    fn adopt_rolls_back_promote_on_move_collision() {
        // If src's name equals the leaf stem, it collides with the new folder note (b/b.md) → move fails → rollback.
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("b.md");
        std::fs::write(&leaf, "비").unwrap();
        let box_dir = tmp.path().join("box");
        std::fs::create_dir(&box_dir).unwrap();
        let src = box_dir.join("b.md"); // triggers the collision (b/b.md already exists on move)
        std::fs::write(&src, "충돌").unwrap();

        assert!(adopt_into_leaf(tmp.path(), &src, &leaf).is_err());
        // Rollback verification: leaf restored, promotion folder removed, src preserved.
        assert!(leaf.is_file(), "the leaf should be restored");
        assert!(!tmp.path().join("b").exists(), "the promotion folder should be rolled back");
        assert_eq!(std::fs::read_to_string(&leaf).unwrap(), "비");
        assert!(src.is_file(), "src is unchanged");
    }

    #[test]
    fn save_attachment_writes_to_assets_and_returns_rel_link() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("note.md");
        std::fs::write(&note, "본문").unwrap();
        let bytes = b"\x89PNG\r\n\x1a\n fake png";

        let rel = save_attachment(tmp.path(), &note, bytes, "png").unwrap();
        assert!(rel.starts_with("assets/"), "the relative link is under assets/");
        assert!(rel.ends_with(".png"));
        // The actual file exists on disk in the same folder's assets/ and its content matches.
        let name = rel.strip_prefix("assets/").unwrap();
        let on_disk = tmp.path().join("assets").join(name);
        assert!(on_disk.is_file());
        assert_eq!(std::fs::read(on_disk).unwrap(), bytes);
    }

    #[test]
    fn save_attachment_normalizes_and_rejects_extension() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("n.md");
        std::fs::write(&note, "").unwrap();
        // Allows normalization of uppercase / leading dot.
        assert!(save_attachment(tmp.path(), &note, b"x", ".JPEG").unwrap().ends_with(".jpeg"));
        // Rejects non-images.
        assert!(save_attachment(tmp.path(), &note, b"x", "exe").is_err());
    }

    #[test]
    fn save_attachment_does_not_overwrite_on_repeat() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("n.md");
        std::fs::write(&note, "").unwrap();
        let a = save_attachment(tmp.path(), &note, b"a", "png").unwrap();
        let b = save_attachment(tmp.path(), &note, b"b", "png").unwrap();
        assert_ne!(a, b, "the two attachments must be different files (no overwriting)");
        let count = std::fs::read_dir(tmp.path().join("assets")).unwrap().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn save_attachment_rejects_non_md_target() {
        // Passing an existing non-.md path (e.g. an image file) as the note is rejected — prevents a misplaced assets/.
        let tmp = TempDir::new().unwrap();
        let img = tmp.path().join("pic.png");
        std::fs::write(&img, "x").unwrap();
        assert!(save_attachment(tmp.path(), &img, b"y", "png").is_err());
        assert!(!tmp.path().join("assets").exists(), "assets is not created on rejection");
    }

    #[test]
    fn save_attachment_rejects_outside_vault() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let note = outside.path().join("ext.md");
        std::fs::write(&note, "").unwrap();
        assert!(save_attachment(tmp.path(), &note, b"x", "png").is_err());
    }

    #[test]
    fn save_attachment_folder_note_uses_sibling_assets() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("bar");
        std::fs::create_dir(&dir).unwrap();
        let body = dir.join("bar.md"); // folder note
        std::fs::write(&body, "다이어리").unwrap();

        let rel = save_attachment(tmp.path(), &body, b"img", "gif").unwrap();
        assert!(rel.starts_with("assets/"), "for a folder note too, the relative link is assets/");
        let name = rel.strip_prefix("assets/").unwrap();
        assert!(dir.join("assets").join(name).is_file(), "assets sits next to the folder note (bar/assets)");
    }

    #[test]
    fn restore_returns_to_original_location() {
        let tmp = TempDir::new().unwrap();
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        let trashed = trash.join("memo.md");
        std::fs::write(&trashed, "x").unwrap();

        let restored = restore_from_trash(tmp.path(), &trashed, "refs/memo.md").unwrap();
        assert_eq!(restored, tmp.path().join("refs").join("memo.md"));
        assert!(restored.is_file(), "parent dir recreated and file restored");
        assert!(!trashed.exists(), "removed from trash");
    }

    #[test]
    fn restore_disambiguates_on_collision_never_overwrites() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("memo.md"), "original").unwrap(); // occupies the spot
        let trash = tmp.path().join(".textree").join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        let trashed = trash.join("memo.md");
        std::fs::write(&trashed, "restored").unwrap();

        let restored = restore_from_trash(tmp.path(), &trashed, "memo.md").unwrap();
        assert_eq!(restored, tmp.path().join("memo (1).md"), "disambiguated, not overwritten");
        assert_eq!(std::fs::read_to_string(tmp.path().join("memo.md")).unwrap(), "original");
    }

    #[test]
    fn adopt_rejects_leaf_inside_src() {
        // If the leaf is inside the dragged node (src), reject before any mutation (don't even promote).
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("p");
        std::fs::create_dir(&src).unwrap();
        let leaf = src.join("child.md");
        std::fs::write(&leaf, "자식").unwrap();

        assert!(adopt_into_leaf(tmp.path(), &src, &leaf).is_err());
        assert!(leaf.is_file(), "no mutation on rejection");
        assert!(!src.join("child").exists(), "not promoted");
    }

    #[test]
    fn create_untitled_note_generates_and_auto_numbers() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let p1 = create_untitled_note(root, root).unwrap();
        assert_eq!(p1.file_name().unwrap().to_str().unwrap(), "Untitled.md");
        assert!(p1.is_file());
        assert_eq!(std::fs::read_to_string(&p1).unwrap(), "");

        let p2 = create_untitled_note(root, root).unwrap();
        assert_eq!(p2.file_name().unwrap().to_str().unwrap(), "Untitled (1).md");

        let p3 = create_untitled_note(root, root).unwrap();
        assert_eq!(p3.file_name().unwrap().to_str().unwrap(), "Untitled (2).md");
    }

    #[test]
    fn create_untitled_note_rejects_parent_outside_vault() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        std::fs::create_dir(&root).unwrap();
        // parent is the vault's parent → outside
        assert!(create_untitled_note(&root, tmp.path()).is_err());
    }
}
