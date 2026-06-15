//! 경로 안전성 검증. 볼트 루트 밖으로의 탈출과 위험한 노드 이름을 막는다.
//! commands(IPC)와 fs_ops(변형)가 공유한다.

use std::path::Path;

/// 후보 경로가 볼트 루트 안에 있는지 검증(상위 탈출 방지).
/// canonicalize 기반이라 **이미 존재하는** 경로에만 유효하다 — 새로 만들 경로는
/// 부모 디렉터리에 대해 호출하고, 새 이름은 [`is_valid_name`]으로 검증한다.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(r), Ok(c)) => c.starts_with(r),
        _ => false,
    }
}

/// Windows 예약 장치 이름(확장자 무관하게 예약됨).
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 노드(노트/폴더)의 새 이름으로 안전한가. 경로 구분자·상위참조·예약 dot·Windows
/// 예약 장치명을 거부한다.
pub fn is_valid_name(name: &str) -> bool {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('.') // .textree 등 예약/숨김과 충돌 방지
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return false;
    }
    // 확장자 무관하게 예약되므로 첫 '.' 앞 부분을 대문자로 비교(예: CON, CON.md 모두 거부).
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
        assert!(!is_valid_name("con")); // 대소문자 무시
        assert!(!is_valid_name("NUL"));
        assert!(!is_valid_name("COM1"));
        assert!(!is_valid_name("LPT9"));
        assert!(!is_valid_name("CON.md")); // 확장자 무관
        // 예약명을 포함만 한 정상 이름은 허용.
        assert!(is_valid_name("CONTROL"));
        assert!(is_valid_name("회의록-CON"));
    }

    #[test]
    fn within_is_false_for_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        // 아직 없는 경로는 canonicalize 실패 → false(생성은 부모로 검증해야 함).
        assert!(!is_within(tmp.path(), &PathBuf::from(tmp.path().join("new.md"))));
    }
}
