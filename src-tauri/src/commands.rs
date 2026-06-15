use crate::pathsafe::is_within;
use crate::self_write::SelfWrites;
use crate::vault::{self, TreeNode};
use crate::watcher::{self, WatcherHandle};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tempfile::NamedTempFile;

/// 원자적 파일 쓰기: 같은 디렉터리의 임시 파일에 쓴 뒤 대상 경로로 rename.
/// 쓰기 도중 크래시/전원차단이 나도 대상 파일이 truncate되지 않는다("FS가 진실").
fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "대상 경로에 부모 디렉터리가 없습니다")
    })?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    // OS 버퍼가 아닌 물리 저장소까지 내려쓴다(fsync). 그래야 rename 후
    // 전원차단이 나도 내용이 보장된다 — persist만으로는 durability가 없다.
    tmp.as_file().sync_all()?;
    // persist는 동일 볼륨 내 rename이라 원자적이며 기존 파일을 대체한다.
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[tauri::command]
pub fn list_tree(root: String) -> Result<Vec<TreeNode>, String> {
    vault::build_tree(Path::new(&root)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_note(root: String, path: String) -> Result<String, String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    if !is_within(&root, &path) {
        return Err("경로가 볼트 밖입니다".into());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_note(
    root: String,
    path: String,
    content: String,
    self_writes: State<'_, Arc<SelfWrites>>,
) -> Result<(), String> {
    let root = PathBuf::from(root);
    let path = PathBuf::from(path);
    if !is_within(&root, &path) {
        return Err("경로가 볼트 밖입니다".into());
    }
    // 쓰기 "직전" 등록해야 한다: write가 디스크에 닿은 직후 워처가 record보다
    // 먼저 이벤트를 받으면 에코 루프가 생기기 때문(설계 §4.1).
    self_writes.record(&path, &content);
    match atomic_write(&path, &content) {
        Ok(()) => Ok(()),
        Err(e) => {
            // 쓰기 실패 시 디스크는 안 바뀌었으므로 stale 등록을 제거해
            // 레지스트리가 실제 디스크 상태와 어긋나지 않게 한다.
            self_writes.forget(&path);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub fn open_vault(
    root: String,
    app: AppHandle,
    self_writes: State<'_, Arc<SelfWrites>>,
    watcher_handle: State<'_, WatcherHandle>,
) -> Result<Vec<TreeNode>, String> {
    let root_path = PathBuf::from(&root);
    let tree = vault::build_tree(&root_path).map_err(|e| e.to_string())?;

    // 새 워처 시작 전에 이전 워처를 명시적으로 드롭(감시 중지)해, 볼트 전환 시
    // 이전 볼트의 잔여 이벤트가 새 볼트 처리에 섞이는 것을 줄인다.
    *watcher_handle.0.lock().unwrap() = None;
    let w = watcher::start(app, &root_path, self_writes.inner().clone())?;
    *watcher_handle.0.lock().unwrap() = Some(w);

    Ok(tree)
}

// ── 구조 편집(M4) — fs_ops 위임 ────────────────────────────────

#[tauri::command]
pub fn create_note(root: String, parent: String, name: String) -> Result<String, String> {
    let path = crate::fs_ops::create_note(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn create_folder(root: String, parent: String, name: String) -> Result<String, String> {
    let dir = crate::fs_ops::create_folder(Path::new(&root), Path::new(&parent), &name)
        .map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn promote_node(root: String, path: String) -> Result<String, String> {
    let dir = crate::fs_ops::promote_leaf(Path::new(&root), Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn delete_node(root: String, path: String) -> Result<(), String> {
    crate::fs_ops::delete_to_trash(Path::new(&root), Path::new(&path))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_node(root: String, path: String, name: String) -> Result<String, String> {
    let p = crate::fs_ops::rename_node(Path::new(&root), Path::new(&path), &name)
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn move_node(root: String, path: String, dest: String) -> Result<String, String> {
    let p = crate::fs_ops::move_node(Path::new(&root), Path::new(&path), Path::new(&dest))
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

#[tauri::command]
pub fn adopt_node(root: String, path: String, leaf: String) -> Result<String, String> {
    let p = crate::fs_ops::adopt_into_leaf(Path::new(&root), Path::new(&path), Path::new(&leaf))
        .map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

/// 첨부 이미지 저장. `data`는 base64 인코딩된 바이트. 본문에 삽입할 상대링크 반환.
#[tauri::command]
pub fn save_attachment(
    root: String,
    note: String,
    data: String,
    ext: String,
) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("base64 디코드 실패: {e}"))?;
    crate::fs_ops::save_attachment(Path::new(&root), Path::new(&note), &bytes, &ext)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn atomic_write_replaces_existing_content() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("note.md");
        std::fs::write(&f, "old").unwrap();
        atomic_write(&f, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_creates_when_absent() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("fresh.md");
        atomic_write(&f, "hi").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "hi");
    }
}
