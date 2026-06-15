//! Self-write 억제 레지스트리.
//!
//! 앱이 파일을 쓰면 `notify` 워처(M3)가 그 변경을 외부 변경으로 오인해
//! 에디터 재로드 → 무한 루프가 생길 수 있다(설계 §4.1). 이를 막기 위해
//! `write_note`가 디스크에 쓰기 직전 `(경로 → 내용 해시)`를 여기 기록하고,
//! 워처가 이벤트를 받으면 현재 파일 내용 해시를 대조해 자가쓰기를 걸러낸다.
//!
//! 해시 기반이라 디바운스 타임윈도보다 정확하며, 외부 도구가 정확히 같은
//! 내용으로 저장하는 드문 경우만 무시되는데 이는 무해하다(내용이 동일하므로).
//!
//! M2 범위에서는 `record`만 호출된다(워처 소비자는 M3). 만료(타임스탬프)는
//! 워처가 들어오는 M3에서 stale 엔트리가 실제 문제가 될 때 도입한다(YAGNI).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 경로별 "최근 자가쓰기" 내용 해시 맵. Tauri `State`로 관리된다.
#[derive(Default)]
pub struct SelfWrites {
    inner: Mutex<HashMap<PathBuf, u64>>,
}

fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

impl SelfWrites {
    /// 쓰기 직전 호출. 경로의 기대 내용 해시를 기록한다(같은 경로 재쓰기는 덮어씀).
    pub fn record(&self, path: &Path, content: &str) {
        let mut map = self.inner.lock().unwrap();
        map.insert(path.to_path_buf(), content_hash(content));
    }

    /// 등록된 자가쓰기 기대를 취소한다. 쓰기가 실패해 디스크가 안 바뀐 경우,
    /// 레지스트리가 실제 상태와 어긋나지 않도록 호출한다.
    pub fn forget(&self, path: &Path) {
        let mut map = self.inner.lock().unwrap();
        map.remove(path);
    }

    /// 워처가 fs 이벤트를 받았을 때 호출. `current` 내용이 기록된 자가쓰기와
    /// 일치하면 엔트리를 소비(제거)하고 `true`를 돌려준다.
    ///
    /// 1회성으로 소비하므로, 자가쓰기 직후의 외부 변경이 마스킹되지 않는다.
    // 소비자(워처)는 M3에서 추가된다. 그 전까지 비테스트 빌드에서는 미사용.
    #[allow(dead_code)]
    pub fn take_if_matches(&self, path: &Path, current: &str) -> bool {
        let mut map = self.inner.lock().unwrap();
        if map.get(path) == Some(&content_hash(current)) {
            map.remove(path);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn matching_content_is_self_write_and_consumed_once() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "hello");

        // 같은 내용으로 도착한 첫 이벤트는 자가쓰기.
        assert!(sw.take_if_matches(&p("/v/a.md"), "hello"));
        // 소비되었으므로 두 번째 동일 이벤트는 더 이상 자가쓰기로 보지 않음.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "hello"));
    }

    #[test]
    fn different_content_is_external_change() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "hello");
        // 외부 도구가 다른 내용으로 덮어씀 → 자가쓰기 아님(엔트리 유지).
        assert!(!sw.take_if_matches(&p("/v/a.md"), "edited externally"));
    }

    #[test]
    fn unrecorded_path_is_not_self_write() {
        let sw = SelfWrites::default();
        assert!(!sw.take_if_matches(&p("/v/never-written.md"), "x"));
    }

    #[test]
    fn forget_cancels_a_recorded_expectation() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "hello");
        sw.forget(&p("/v/a.md"));
        // 등록이 취소됐으므로 자가쓰기로 보지 않음.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "hello"));
    }

    #[test]
    fn rerecord_overwrites_previous_hash() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "v1");
        sw.record(&p("/v/a.md"), "v2");
        // 마지막 기록만 유효.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "v1"));
        assert!(sw.take_if_matches(&p("/v/a.md"), "v2"));
    }
}
