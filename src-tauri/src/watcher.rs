//! 외부 파일 변경 감시(M3).
//!
//! `notify-debouncer-full`로 볼트 루트를 재귀 감시하고, self-write 레지스트리로
//! 앱 자신의 쓰기(에코)를 걸러낸 뒤 `fs_changed` 이벤트를 프론트로 방출한다.
//! 디바운서가 드롭되면 감시가 멈추므로, 반환된 핸들을 상태로 살려둔다.

use crate::search::IndexHandle;
use crate::self_write::SelfWrites;
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, Debouncer, RecommendedCache,
};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
}

/// 프론트로 보내는 변경 페이로드(앱 IPC wire 타입 — 라이브러리 타입과 분리).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct FsChange {
    pub kind: ChangeKind,
    pub path: String,
}

pub type VaultWatcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// 실행 중인 볼트 워처 핸들. Tauri `State`로 관리한다.
/// 볼트를 새로 열면 이전 디바운서를 드롭(감시 중지)하고 교체한다.
#[derive(Default)]
pub struct WatcherHandle(pub Mutex<Option<VaultWatcher>>);

/// notify `EventKind`를 우리 변경 종류로 축약. 관심 없는 종류는 `None`.
fn map_kind(kind: &EventKind) -> Option<ChangeKind> {
    if kind.is_create() {
        Some(ChangeKind::Created)
    } else if kind.is_remove() {
        Some(ChangeKind::Removed)
    } else if kind.is_modify() {
        Some(ChangeKind::Modified)
    } else {
        None // Access/Any/Other는 무시
    }
}

/// 볼트 루트 기준 dot-세그먼트(`.textree`, `.git` 등)는 트리에서 숨김 대상이므로
/// 워처도 무시한다. 루트 경로 자체에 포함된 dot는 무시 판정에 넣지 않는다.
fn is_ignored(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')))
}

/// 단일 `(kind, path)`를 프론트 변경으로 변환. create/modify이고 현재 파일
/// 내용이 등록된 자가쓰기와 일치하면 `None`(에코 억제).
fn to_fs_change(kind: ChangeKind, path: &Path, self_writes: &SelfWrites) -> Option<FsChange> {
    if matches!(kind, ChangeKind::Created | ChangeKind::Modified) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                if self_writes.take_if_matches(path, &content) {
                    return None; // 자가쓰기 에코 — 억제
                }
            }
            Err(_) => {
                // 내용을 못 읽으면 에코 판정 불가. 등록이 남아 있으면 정리해
                // stale 엔트리가 이후 동일내용 외부변경을 오판 억제하지 않게 한다.
                self_writes.forget(path);
            }
        }
    }
    Some(FsChange {
        kind,
        path: path.display().to_string(),
    })
}

/// 한 배치의 변경을 인덱스에 반영하고 1회 commit한다(.md만, dot 경로 제외는
/// 호출부 is_ignored가 이미 수행). 인덱스 미설치 시 무동작.
pub fn apply_changes_to_index(handle: &IndexHandle, root: &Path, changes: &[FsChange]) {
    let mut guard = handle.0.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.as_mut() else { return };
    let mut touched = false;
    for c in changes {
        let p = Path::new(&c.path);
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            continue;
        }
        state.apply_change(root, p, matches!(c.kind, ChangeKind::Removed));
        touched = true;
    }
    if touched {
        let _ = state.commit();
    }
}

/// 볼트 루트를 재귀 감시하는 디바운서를 시작한다.
pub fn start(
    app: AppHandle,
    root: &Path,
    self_writes: Arc<SelfWrites>,
    index: Arc<IndexHandle>,
) -> Result<VaultWatcher, String> {
    let root_owned = root.to_path_buf();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else {
                return; // 워처 에러는 무시 — 다음 이벤트에서 자연 복구
            };
            let mut batch: Vec<FsChange> = Vec::new();
            for ev in events {
                let Some(kind) = map_kind(&ev.kind) else {
                    continue;
                };
                for path in &ev.paths {
                    if is_ignored(&root_owned, path) {
                        continue;
                    }
                    if let Some(change) = to_fs_change(kind.clone(), path, &self_writes) {
                        let _ = app.emit("fs_changed", change.clone());
                        batch.push(change);
                    }
                }
            }
            apply_changes_to_index(&index, &root_owned, &batch);
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    Ok(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify_debouncer_full::notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn map_kind_classifies_create_modify_remove_and_ignores_access() {
        assert_eq!(
            map_kind(&EventKind::Create(CreateKind::Any)),
            Some(ChangeKind::Created)
        );
        assert_eq!(
            map_kind(&EventKind::Modify(ModifyKind::Any)),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            map_kind(&EventKind::Remove(RemoveKind::Any)),
            Some(ChangeKind::Removed)
        );
        assert_eq!(map_kind(&EventKind::Access(AccessKind::Any)), None);
    }

    #[test]
    fn is_ignored_skips_dot_segments_below_root() {
        let root = PathBuf::from("/vault");
        assert!(is_ignored(&root, &PathBuf::from("/vault/.textree/db.json")));
        assert!(is_ignored(&root, &PathBuf::from("/vault/.git/config")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/노트.md")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/일기/2026.md")));
    }

    #[test]
    fn is_ignored_does_not_count_dot_in_root_path() {
        let root = PathBuf::from("/home/.config/vault");
        // 루트 안의 .config는 무시 판정 대상이 아님.
        assert!(!is_ignored(&root, &PathBuf::from("/home/.config/vault/노트.md")));
        // 루트 아래의 dot 폴더는 여전히 무시.
        assert!(is_ignored(
            &root,
            &PathBuf::from("/home/.config/vault/.textree/x.json")
        ));
    }

    #[test]
    fn self_write_is_suppressed_external_change_is_emitted() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "self-written").unwrap();
        let sw = SelfWrites::default();
        sw.record(&f, "self-written");

        // 등록된 자가쓰기와 내용 일치 → 억제.
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_none());

        // 외부 도구가 다른 내용으로 변경 → 방출.
        std::fs::write(&f, "external edit").unwrap();
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
    }

    #[test]
    fn removed_is_always_emitted() {
        let sw = SelfWrites::default();
        let change = to_fs_change(ChangeKind::Removed, &PathBuf::from("/v/gone.md"), &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Removed));
    }

    use crate::search::{IndexHandle, IndexState};
    use std::sync::Arc;

    #[test]
    fn apply_changes_to_index_upserts_and_deletes() {
        let vault = TempDir::new().unwrap();
        let idx = TempDir::new().unwrap();
        std::fs::write(vault.path().join("a.md"), "처음 본문").unwrap();

        let handle: Arc<IndexHandle> = Arc::new(IndexHandle::default());
        *handle.0.lock().unwrap() = Some(IndexState::open_or_create(idx.path()).unwrap());

        // created/modified → upsert
        let changes = vec![
            FsChange { kind: ChangeKind::Created, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            let st = guard.as_ref().unwrap();
            assert_eq!(st.search("본문", 10).unwrap().len(), 1);
        }

        // removed → delete
        let changes = vec![
            FsChange { kind: ChangeKind::Removed, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            assert_eq!(guard.as_ref().unwrap().search("본문", 10).unwrap().len(), 0);
        }
    }

    #[test]
    fn unreadable_modified_path_emits_and_forgets_stale_entry() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("vanished.md");
        let sw = SelfWrites::default();
        sw.record(&f, "was-self-written");
        // 파일이 없어 읽기 실패 → 이벤트는 통과(Some), 등록은 정리됨.
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
        // forget으로 소비됐으므로, 같은 내용이 다시 와도 자가쓰기로 안 봄.
        std::fs::write(&f, "was-self-written").unwrap();
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_some());
    }
}
