//! 구조 편집(M4): 노트/폴더 생성·삭제(휴지통)·리프↔컨테이너 승격.
//!
//! 모든 연산은 볼트 루트 안에서만 동작하며(부모 검증 + 이름 검증), 파일시스템에
//! 즉시 반영한다. 삭제는 영구삭제가 아니라 `.textree/trash/`로 이동한다(설계 §5 M4).

use crate::pathsafe::{is_valid_name, is_within};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 지원하는 이미지 확장자(소문자). 그 외 형식은 거부한다.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.into())
}

/// 부모 디렉터리가 볼트 안의 실제 디렉터리인지, 새 이름이 안전한지 검증.
fn check_parent_and_name(root: &Path, parent: &Path, name: &str) -> io::Result<()> {
    if !is_valid_name(name) {
        return Err(err("사용할 수 없는 이름입니다"));
    }
    if !parent.is_dir() {
        return Err(err("부모가 디렉터리가 아닙니다"));
    }
    if !is_within(root, parent) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    Ok(())
}

/// `parent/stem.md` 리프 노트를 만든다. 이미 있으면 오류.
pub fn create_note(root: &Path, parent: &Path, stem: &str) -> io::Result<PathBuf> {
    check_parent_and_name(root, parent, stem)?;
    let path = parent.join(format!("{stem}.md"));
    if path.exists() {
        return Err(err("같은 이름의 항목이 이미 있습니다"));
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

/// `parent/name/` 컨테이너와 그 폴더-노트 `parent/name/name.md`를 만든다.
/// 폴더-노트까지 만들어 바로 편집 가능한 컨테이너 노트가 되게 한다(설계 §3.1).
pub fn create_folder(root: &Path, parent: &Path, name: &str) -> io::Result<PathBuf> {
    check_parent_and_name(root, parent, name)?;
    let dir = parent.join(name);
    if dir.exists() {
        return Err(err("같은 이름의 항목이 이미 있습니다"));
    }
    std::fs::create_dir(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), "")?;
    Ok(dir)
}

/// 리프 노트 `dir/foo.md`를 컨테이너 `dir/foo/foo.md`로 승격한다(설계 §3.3).
/// 자식을 만들 자리를 마련하는 연산. 반환값은 새 컨테이너 디렉터리.
pub fn promote_leaf(root: &Path, leaf_md: &Path) -> io::Result<PathBuf> {
    if !is_within(root, leaf_md) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    if leaf_md.extension().and_then(|e| e.to_str()) != Some("md") || !leaf_md.is_file() {
        return Err(err("리프 노트(.md 파일)가 아닙니다"));
    }
    let parent = leaf_md.parent().ok_or_else(|| err("부모 디렉터리가 없습니다"))?;
    let stem = leaf_md
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("파일 이름을 읽을 수 없습니다"))?;
    let new_dir = parent.join(stem);
    if new_dir.exists() {
        return Err(err("같은 이름의 폴더가 이미 있습니다"));
    }
    std::fs::create_dir(&new_dir)?;
    let new_body = new_dir.join(format!("{stem}.md"));
    std::fs::rename(leaf_md, &new_body)?;
    Ok(new_dir)
}

/// 휴지통 안에서 충돌하지 않는 목적지 경로를 찾는다(`name`, `name (1)`, …).
/// 디렉터리는 확장자 분리를 하지 않는다(`일기.백업` 같은 이름이 변형되지 않도록).
fn unique_in(trash: &Path, file_name: &str, is_dir: bool) -> PathBuf {
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

/// 노드(리프 `.md` 또는 컨테이너 디렉터리)를 `.textree/trash/`로 이동한다.
/// 영구삭제가 아니라 복구 가능한 휴지통 이동. 반환값은 휴지통 내 목적지.
pub fn delete_to_trash(root: &Path, target: &Path) -> io::Result<PathBuf> {
    if !is_within(root, target) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    let root_c = root.canonicalize()?;
    let target_c = target.canonicalize()?;
    if target_c == root_c {
        return Err(err("볼트 루트는 삭제할 수 없습니다"));
    }
    // .textree 자체나 그 안쪽은 삭제 대상이 아니다(예약 사이드카).
    if target_c.starts_with(root_c.join(".textree")) {
        return Err(err(".textree는 삭제할 수 없습니다"));
    }
    let trash = root.join(".textree").join("trash");
    std::fs::create_dir_all(&trash)?;
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("이름을 읽을 수 없습니다"))?;
    let dest = unique_in(&trash, file_name, target.is_dir());
    std::fs::rename(target, &dest)?;
    Ok(dest)
}

/// 노드 이름을 바꾼다. 리프 `.md`는 파일을, 컨테이너는 디렉터리와 폴더-노트를
/// 함께 rename한다(노드 정체성 = 파일/폴더 이름, 설계 §3.5). 반환값은 새 경로.
pub fn rename_node(root: &Path, target: &Path, new_name: &str) -> io::Result<PathBuf> {
    if !is_valid_name(new_name) {
        return Err(err("사용할 수 없는 이름입니다"));
    }
    if !is_within(root, target) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    let parent = target.parent().ok_or_else(|| err("부모 디렉터리가 없습니다"))?;

    if target.is_dir() {
        let old_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| err("이름을 읽을 수 없습니다"))?
            .to_string();
        let new_dir = parent.join(new_name);
        if new_dir.exists() {
            return Err(err("같은 이름의 항목이 이미 있습니다"));
        }
        std::fs::rename(target, &new_dir)?;
        // 폴더-노트가 있으면 새 폴더명에 맞춰 함께 rename.
        let old_body = new_dir.join(format!("{old_name}.md"));
        if old_body.is_file() {
            if let Err(e) = std::fs::rename(&old_body, new_dir.join(format!("{new_name}.md"))) {
                // 2단계 실패 → 1단계(디렉터리 rename) 롤백(best-effort)해 부분손상 방지.
                let _ = std::fs::rename(&new_dir, target);
                return Err(e);
            }
        }
        Ok(new_dir)
    } else {
        if target.extension().and_then(|e| e.to_str()) != Some("md") {
            return Err(err("이름변경 대상이 노트/폴더가 아닙니다"));
        }
        let new_path = parent.join(format!("{new_name}.md"));
        if new_path.exists() {
            return Err(err("같은 이름의 항목이 이미 있습니다"));
        }
        std::fs::rename(target, &new_path)?;
        Ok(new_path)
    }
}

/// 노드를 다른 폴더로 이동한다(부모 변경). 반환값은 새 경로.
pub fn move_node(root: &Path, src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    if !is_within(root, src) {
        return Err(err("원본 경로가 볼트 밖입니다"));
    }
    if !dest_dir.is_dir() || !is_within(root, dest_dir) {
        return Err(err("대상 폴더가 올바르지 않습니다"));
    }
    let name = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("이름을 읽을 수 없습니다"))?;
    // 폴더를 자기 자신이나 그 하위로 이동하는 것을 차단.
    let src_c = src.canonicalize()?;
    let dest_dir_c = dest_dir.canonicalize()?;
    if dest_dir_c == src_c || dest_dir_c.starts_with(&src_c) {
        return Err(err("자기 자신이나 하위 폴더로는 이동할 수 없습니다"));
    }
    let dest = dest_dir.join(name);
    if dest.exists() {
        return Err(err("대상 폴더에 같은 이름의 항목이 이미 있습니다"));
    }
    std::fs::rename(src, &dest)?;
    Ok(dest)
}

/// 리프 노트를 컨테이너로 승격한 뒤 `src` 노드를 그 안으로 이동한다(설계 §3.3).
/// "X를 노트 Y 위에 드롭 = X를 Y의 자식으로". promote+move를 한 연산으로 묶어,
/// 이동 실패 시 승격을 best-effort 롤백해 부분상태(폴더만 남는 것)를 남기지 않는다.
/// 반환값은 이동된 `src`의 새 경로.
pub fn adopt_into_leaf(root: &Path, src: &Path, leaf_md: &Path) -> io::Result<PathBuf> {
    // 변형 전 검증: leaf가 src와 같거나 그 하위면 불가(자기 안으로 이동).
    if !is_within(root, src) || !is_within(root, leaf_md) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    let src_c = src.canonicalize()?;
    let leaf_c = leaf_md.canonicalize()?;
    if leaf_c == src_c || leaf_c.starts_with(&src_c) {
        return Err(err("자기 자신이나 하위로는 이동할 수 없습니다"));
    }
    // 승격(promote_leaf가 .md 리프 여부 검증).
    let new_dir = promote_leaf(root, leaf_md)?;
    // 이동; 실패 시 승격 롤백(승격 직후라 new_dir엔 폴더노트만 있음).
    match move_node(root, src, &new_dir) {
        Ok(p) => Ok(p),
        Err(move_err) => {
            let stem = new_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let body = new_dir.join(format!("{stem}.md"));
            // 롤백 rename이 실패하면(예: Windows AV/인덱서의 일시적 잠금) 원본 노트가
            // new_dir에 갇힌 채 사라진다. 조용히 삼키지 않고 위치를 알려 복구 가능케 한다.
            match std::fs::rename(&body, leaf_md) {
                Ok(()) => {
                    let _ = std::fs::remove_dir(&new_dir);
                    Err(move_err)
                }
                Err(rollback_err) => Err(err(format!(
                    "이동에 실패했고 원복도 실패했습니다. 원본 노트는 {}에 있습니다. \
                     (이동 오류: {move_err}; 원복 오류: {rollback_err})",
                    body.display()
                ))),
            }
        }
    }
}

/// 첨부 이미지 바이트를 현재 노트 본문(`note_body`, .md)과 같은 폴더의 `assets/`에
/// 저장한다(설계 §3.2 첨부 매핑). 반환값은 본문에 삽입할 **상대경로**(항상 forward-slash,
/// 예: `assets/Pasted-1718....png`) — .md와 `assets/`가 부모를 공유하므로 일관된다.
pub fn save_attachment(
    root: &Path,
    note_body: &Path,
    bytes: &[u8],
    ext: &str,
) -> io::Result<String> {
    if !is_within(root, note_body) {
        return Err(err("경로가 볼트 밖입니다"));
    }
    // 첨부는 노트 본문(.md) 기준으로만 배치한다 — 임의 경로 옆 assets/ 생성 방지(서버측 계약).
    if note_body.extension().and_then(|e| e.to_str()) != Some("md") {
        return Err(err("첨부 대상이 노트(.md)가 아닙니다"));
    }
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err(err("지원하지 않는 이미지 형식입니다"));
    }
    let parent = note_body
        .parent()
        .ok_or_else(|| err("노트의 부모 디렉터리가 없습니다"))?;
    let assets = parent.join("assets");
    std::fs::create_dir_all(&assets)?;
    // 의존성 없이 고유성 확보: epoch millis 기반 이름 + 충돌 시 unique_in 카운터.
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = unique_in(&assets, &format!("Pasted-{millis}.{ext}"), false);
    std::fs::write(&dest, bytes)?;
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("파일 이름을 읽을 수 없습니다"))?;
    Ok(format!("assets/{name}"))
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
        assert!(dir.join("일기.md").is_file(), "폴더-노트가 만들어져야 함");
    }

    #[test]
    fn promote_leaf_moves_md_into_namesake_folder() {
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("foo.md");
        std::fs::write(&leaf, "본문").unwrap();
        let dir = promote_leaf(tmp.path(), &leaf).unwrap();
        assert_eq!(dir, tmp.path().join("foo"));
        assert!(!leaf.exists(), "원래 리프 파일은 이동됨");
        let body = dir.join("foo.md");
        assert!(body.is_file());
        assert_eq!(std::fs::read_to_string(body).unwrap(), "본문", "내용 보존");
    }

    #[test]
    fn delete_moves_to_trash_not_permanent() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("지울노트.md");
        std::fs::write(&note, "x").unwrap();
        let dest = delete_to_trash(tmp.path(), &note).unwrap();
        assert!(!note.exists(), "원본은 사라짐");
        assert!(dest.is_file(), "휴지통에 보존됨");
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
        assert!(trash.join("dup (1).md").is_file(), "충돌은 카운터로 구분");
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
        assert!(new_dir.join("journal.md").is_file(), "폴더-노트도 rename됨");
        assert_eq!(
            std::fs::read_to_string(new_dir.join("journal.md")).unwrap(),
            "다이어리"
        );
        assert!(new_dir.join("2026.md").is_file(), "자식은 그대로");
        assert!(!dir.exists());
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
        // p를 그 하위 c로 이동 불가.
        assert!(move_node(tmp.path(), &parent, &child).is_err());
        // p를 자기 자신으로 이동 불가.
        assert!(move_node(tmp.path(), &parent, &parent).is_err());
    }

    #[test]
    fn move_rejects_duplicate_at_dest() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("dup.md");
        std::fs::write(&note, "").unwrap();
        let folder = tmp.path().join("box");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("dup.md"), "").unwrap(); // 이미 존재
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
        // b가 컨테이너로 승격되고 폴더노트 보존.
        let b_dir = tmp.path().join("b");
        assert!(b_dir.is_dir());
        assert_eq!(std::fs::read_to_string(b_dir.join("b.md")).unwrap(), "비");
        // a가 b/ 안으로 이동, 내용 보존.
        assert_eq!(moved, b_dir.join("a.md"));
        assert!(!a.exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "에이");
    }

    #[test]
    fn adopt_rolls_back_promote_on_move_collision() {
        // src 이름이 리프 stem과 같으면 새 폴더노트(b/b.md)와 충돌 → 이동 실패 → 롤백.
        let tmp = TempDir::new().unwrap();
        let leaf = tmp.path().join("b.md");
        std::fs::write(&leaf, "비").unwrap();
        let box_dir = tmp.path().join("box");
        std::fs::create_dir(&box_dir).unwrap();
        let src = box_dir.join("b.md"); // 충돌 유발(이동 시 b/b.md 이미 존재)
        std::fs::write(&src, "충돌").unwrap();

        assert!(adopt_into_leaf(tmp.path(), &src, &leaf).is_err());
        // 롤백 검증: 리프 복원, 승격 폴더 제거, src 보존.
        assert!(leaf.is_file(), "리프가 복원되어야 함");
        assert!(!tmp.path().join("b").exists(), "승격 폴더가 롤백되어야 함");
        assert_eq!(std::fs::read_to_string(&leaf).unwrap(), "비");
        assert!(src.is_file(), "src는 그대로");
    }

    #[test]
    fn save_attachment_writes_to_assets_and_returns_rel_link() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("note.md");
        std::fs::write(&note, "본문").unwrap();
        let bytes = b"\x89PNG\r\n\x1a\n fake png";

        let rel = save_attachment(tmp.path(), &note, bytes, "png").unwrap();
        assert!(rel.starts_with("assets/"), "상대링크는 assets/ 하위");
        assert!(rel.ends_with(".png"));
        // 디스크에 실제 파일이 같은 폴더의 assets/에 존재하고 내용 일치.
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
        // 대문자/선행 점 정규화 허용.
        assert!(save_attachment(tmp.path(), &note, b"x", ".JPEG").unwrap().ends_with(".jpeg"));
        // 비이미지 거부.
        assert!(save_attachment(tmp.path(), &note, b"x", "exe").is_err());
    }

    #[test]
    fn save_attachment_does_not_overwrite_on_repeat() {
        let tmp = TempDir::new().unwrap();
        let note = tmp.path().join("n.md");
        std::fs::write(&note, "").unwrap();
        let a = save_attachment(tmp.path(), &note, b"a", "png").unwrap();
        let b = save_attachment(tmp.path(), &note, b"b", "png").unwrap();
        assert_ne!(a, b, "두 첨부는 서로 다른 파일이어야 함(덮어쓰기 금지)");
        let count = std::fs::read_dir(tmp.path().join("assets")).unwrap().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn save_attachment_rejects_non_md_target() {
        // .md가 아닌 기존 경로(예: 이미지 파일)를 note로 주면 거부 — 엉뚱한 assets/ 방지.
        let tmp = TempDir::new().unwrap();
        let img = tmp.path().join("pic.png");
        std::fs::write(&img, "x").unwrap();
        assert!(save_attachment(tmp.path(), &img, b"y", "png").is_err());
        assert!(!tmp.path().join("assets").exists(), "거부 시 assets 생성 안 됨");
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
        let body = dir.join("bar.md"); // 폴더-노트
        std::fs::write(&body, "다이어리").unwrap();

        let rel = save_attachment(tmp.path(), &body, b"img", "gif").unwrap();
        assert!(rel.starts_with("assets/"), "폴더노트도 상대링크는 assets/");
        let name = rel.strip_prefix("assets/").unwrap();
        assert!(dir.join("assets").join(name).is_file(), "assets는 폴더노트 옆(bar/assets)");
    }

    #[test]
    fn adopt_rejects_leaf_inside_src() {
        // 리프가 드래그 노드(src) 안에 있으면 변형 전에 거부(승격조차 안 함).
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("p");
        std::fs::create_dir(&src).unwrap();
        let leaf = src.join("child.md");
        std::fs::write(&leaf, "자식").unwrap();

        assert!(adopt_into_leaf(tmp.path(), &src, &leaf).is_err());
        assert!(leaf.is_file(), "거부 시 변형 없음");
        assert!(!src.join("child").exists(), "승격 안 됨");
    }
}
