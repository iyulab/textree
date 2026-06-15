//! Self-write suppression registry.
//!
//! When the app writes a file, the `notify` watcher (M3) can mistake that change
//! for an external change, triggering an editor reload → infinite loop (design §4.1).
//! To prevent this, `write_note` records `(path → content hash)` here just before
//! writing to disk, and when the watcher receives an event it compares the current
//! file content hash to filter out self-writes.
//!
//! Being hash-based, it is more precise than a debounce time window, and only the
//! rare case where an external tool saves the exact same content is ignored, which
//! is harmless (the content is identical).
//!
//! Within the M2 scope only `record` is called (the watcher consumer is M3).
//! Expiration (timestamps) is introduced in M3 when the watcher arrives and stale
//! entries actually become a problem (YAGNI).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Per-path "most recent self-write" content hash map. Managed as a Tauri `State`.
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
    /// Called just before writing. Records the expected content hash for the path
    /// (re-writing the same path overwrites it).
    pub fn record(&self, path: &Path, content: &str) {
        let mut map = self.inner.lock().unwrap();
        map.insert(path.to_path_buf(), content_hash(content));
    }

    /// Cancels a registered self-write expectation. Called when a write fails and
    /// the disk did not change, so the registry does not diverge from actual state.
    pub fn forget(&self, path: &Path) {
        let mut map = self.inner.lock().unwrap();
        map.remove(path);
    }

    /// Called when the watcher receives an fs event. If `current` content matches the
    /// recorded self-write, consumes (removes) the entry and returns `true`.
    ///
    /// Since it is consumed once, an external change right after a self-write is not masked.
    // The consumer (watcher) is added in M3. Until then, unused in non-test builds.
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

        // The first event arriving with the same content is a self-write.
        assert!(sw.take_if_matches(&p("/v/a.md"), "hello"));
        // Already consumed, so a second identical event is no longer treated as a self-write.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "hello"));
    }

    #[test]
    fn different_content_is_external_change() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "hello");
        // An external tool overwrites with different content → not a self-write (entry retained).
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
        // The registration was cancelled, so it is not treated as a self-write.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "hello"));
    }

    #[test]
    fn rerecord_overwrites_previous_hash() {
        let sw = SelfWrites::default();
        sw.record(&p("/v/a.md"), "v1");
        sw.record(&p("/v/a.md"), "v2");
        // Only the last record is valid.
        assert!(!sw.take_if_matches(&p("/v/a.md"), "v1"));
        assert!(sw.take_if_matches(&p("/v/a.md"), "v2"));
    }
}
