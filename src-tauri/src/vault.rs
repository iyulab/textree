use serde::Serialize;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Leaf,
    Container,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TreeNode {
    /// 표시 이름(확장자 `.md` 제외)
    pub name: String,
    pub kind: NodeKind,
    /// 노드 자체의 파일시스템 경로. 리프는 `.md` 파일, 컨테이너는 디렉터리.
    /// 구조 편집(이름변경/이동/삭제)이 body 없는 컨테이너도 주소화하기 위함.
    pub path: PathBuf,
    /// 이 노드의 본문을 담은 `.md` 파일 경로. 본문 없는 컨테이너면 `None`.
    pub body_path: Option<PathBuf>,
    pub children: Vec<TreeNode>,
}

/// 볼트 루트의 최상위 노드 목록을 만든다.
pub fn build_tree(root: &Path) -> io::Result<Vec<TreeNode>> {
    build_children(root, None) // 루트는 폴더-노트를 소유할 부모 노드가 없음
}

fn build_children(dir: &Path, skip_note: Option<&str>) -> io::Result<Vec<TreeNode>> {
    // 엔트리 단위 에러(예: 특정 파일 권한 거부)는 의도적으로 무시한다 —
    // 한 노트가 접근 불가여도 트리 전체를 중단시키지 않고 부분 트리를 만든다.
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|r| r.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut nodes = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            nodes.push(build_node_for_dir(&path)?);
        } else if is_markdown(&name) {
            // 폴더-노트(이 디렉터리의 본문)는 부모가 지정한 skip_note로 제외
            if Some(name.as_str()) == skip_note {
                continue;
            }
            nodes.push(TreeNode {
                name: strip_md(&name),
                kind: NodeKind::Leaf,
                path: path.clone(),
                body_path: Some(path),
                children: Vec::new(),
            });
        }
    }
    Ok(nodes)
}

fn build_node_for_dir(dir: &Path) -> io::Result<TreeNode> {
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let body = dir.join(format!("{name}.md"));
    // 주: body_path 경로 검증(볼트 밖 심볼릭링크 탈출 방지)은 Tauri command 경계의
    // is_within 검사(commands.rs, 후속 태스크)에서 수행한다.
    let body_path = if body.is_file() { Some(body) } else { None };
    let folder_note = format!("{name}.md");
    let children = build_children(dir, Some(&folder_note))?;
    Ok(TreeNode {
        name,
        kind: NodeKind::Container,
        path: dir.to_path_buf(),
        body_path,
        children,
    })
}

fn is_markdown(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".md")
}

fn strip_md(name: &str) -> String {
    // 마지막 ".md"만 제거 (대소문자 무시)
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".md") {
        name[..name.len() - 3].to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(dir: &Path, rel: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn empty_vault_yields_empty_tree() {
        let tmp = TempDir::new().unwrap();
        let tree = build_tree(tmp.path()).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn leaf_note_is_mapped() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "프로젝트.md");
        let tree = build_tree(tmp.path()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "프로젝트");
        assert_eq!(tree[0].kind, NodeKind::Leaf);
        assert_eq!(tree[0].path, tmp.path().join("프로젝트.md"));
        assert!(tree[0].body_path.is_some());
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn container_path_is_directory_even_without_body() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "assets/diagram.png"); // body 없는 컨테이너
        let tree = build_tree(tmp.path()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].kind, NodeKind::Container);
        assert_eq!(tree[0].path, tmp.path().join("assets"));
        assert!(tree[0].body_path.is_none());
    }

    #[test]
    fn folder_note_becomes_container_body_not_child() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "일기/일기.md"); // 폴더-노트 (본문)
        touch(tmp.path(), "일기/2026-06-13.md"); // 자식 리프
        let tree = build_tree(tmp.path()).unwrap();
        assert_eq!(tree.len(), 1);
        let diary = &tree[0];
        assert_eq!(diary.name, "일기");
        assert_eq!(diary.kind, NodeKind::Container);
        assert!(diary.body_path.is_some(), "폴더-노트가 body로 잡혀야 함");
        assert_eq!(diary.children.len(), 1);
        assert_eq!(diary.children[0].name, "2026-06-13");
    }

    #[test]
    fn non_markdown_files_are_hidden() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "자료실/자료실.md");
        touch(tmp.path(), "자료실/assets/diagram.png"); // 트리에 안 보임
        let tree = build_tree(tmp.path()).unwrap();
        let archive = &tree[0];
        assert_eq!(archive.children.len(), 1);
        let assets = &archive.children[0];
        assert_eq!(assets.name, "assets");
        assert_eq!(assets.kind, NodeKind::Container);
        assert!(assets.body_path.is_none());
        assert!(assets.children.is_empty(), "png는 트리에서 제외");
    }

    #[test]
    fn dotfolders_are_hidden() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "노트.md");
        touch(tmp.path(), ".textree/order.json");
        touch(tmp.path(), ".git/config");
        let tree = build_tree(tmp.path()).unwrap();
        assert_eq!(tree.len(), 1, ".textree/.git는 숨김");
        assert_eq!(tree[0].name, "노트");
    }

    #[test]
    fn root_level_note_matching_vault_name_is_not_dropped() {
        // TempDir 자체는 dot-prefix(`.tmpXXXX`) 이름을 쓰므로, dot-숨김 규칙과
        // 충돌하지 않도록 볼트 루트로 쓸 비-dot 하위 폴더를 따로 만든다.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        fs::create_dir_all(&root).unwrap();
        let root_name = root.file_name().unwrap().to_string_lossy().to_string();
        touch(&root, &format!("{root_name}.md"));
        let tree = build_tree(&root).unwrap();
        assert_eq!(tree.len(), 1, "루트와 동명인 노트가 누락되면 안 됨");
        assert_eq!(tree[0].name, root_name);
        assert_eq!(tree[0].kind, NodeKind::Leaf);
    }

    #[test]
    fn md_file_and_sibling_folder_with_same_stem_coexist() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "topics/note-a.md");
        touch(tmp.path(), "topics.md"); // topics/topics.md 가 아님 → 루트의 리프
        let tree = build_tree(tmp.path()).unwrap();
        assert_eq!(tree.len(), 2);
        let container = tree.iter().find(|n| n.kind == NodeKind::Container).unwrap();
        assert_eq!(container.name, "topics");
        assert!(container.body_path.is_none());
        let leaf = tree.iter().find(|n| n.kind == NodeKind::Leaf).unwrap();
        assert_eq!(leaf.name, "topics");
    }
}
