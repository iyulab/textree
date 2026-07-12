//! Watcher liveness watchdog (Windows ReadDirectoryChangesW overflow workaround).
//!
//! On Windows a buffer overflow makes `notify` unwatch and go silent — no callback
//! signal reaches either debounce arm, so the tree/index silently go stale. This
//! module actively probes the watcher with a canary file in `.textree/tmp/`: a live
//! watcher surfaces the canary write back as an event; a dead one does not. Two
//! consecutive unobserved probes ⇒ the watcher is recreated.

use notify_debouncer_full::DebouncedEvent;
use std::path::{Path, PathBuf};

/// Verdict of a liveness probe cycle.
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Verdict {
    Alive,
    Dead,
}

/// Tracks whether the watcher is answering canary probes.
///
/// Each probe cycle calls `arm(token)`; a live watcher's callback later calls
/// `observe(token)`. If the previously-armed token was never observed by the time
/// the next `arm` runs, that's a miss. `threshold` consecutive misses ⇒ `Dead`.
/// The threshold absorbs false positives from debounce latency / slow disks so a
/// recreate only fires on a genuinely dead watcher.
#[allow(dead_code)]
pub(crate) struct LivenessState {
    pending: Option<u64>,
    observed: bool,
    misses: u32,
    threshold: u32,
}

impl LivenessState {
    #[allow(dead_code)]
    pub(crate) fn new(threshold: u32) -> Self {
        Self {
            pending: None,
            observed: false,
            misses: 0,
            threshold,
        }
    }

    /// Arms a new probe token and returns the verdict for the cycle just closed.
    #[allow(dead_code)]
    pub(crate) fn arm(&mut self, token: u64) -> Verdict {
        if self.pending.is_some() && !self.observed {
            self.misses += 1;
        } else {
            self.misses = 0;
        }
        self.pending = Some(token);
        self.observed = false;
        if self.misses >= self.threshold {
            Verdict::Dead
        } else {
            Verdict::Alive
        }
    }

    /// Records that the watcher surfaced canary `token`.
    #[allow(dead_code)]
    pub(crate) fn observe(&mut self, token: u64) {
        if self.pending == Some(token) {
            self.observed = true;
        }
    }

    /// Clears all probe state (called after a successful recreate).
    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        self.pending = None;
        self.observed = false;
        self.misses = 0;
    }
}

/// The canary filename prefix. Files under `.textree/tmp/` matching this are the
/// watchdog's own probes — recognized so they can be observed and never surfaced.
const CANARY_PREFIX: &str = ".watcher-canary-";

/// Path of the canary for `token`, inside the vault's `.textree/tmp/` dir.
#[allow(dead_code)]
pub(crate) fn canary_path(tmp_dir: &Path, token: u64) -> PathBuf {
    tmp_dir.join(format!("{CANARY_PREFIX}{token}"))
}

/// Recovers the token from a canary path, or `None` if the path is not a canary.
#[allow(dead_code)]
pub(crate) fn token_of(path: &Path) -> Option<u64> {
    path.file_name()?
        .to_str()?
        .strip_prefix(CANARY_PREFIX)?
        .parse()
        .ok()
}

/// Scans a raw debounce flush for canary tokens the watcher surfaced. Called before
/// `is_ignored` filtering (which would otherwise drop these `.textree` paths), so it
/// is the only place a canary round-trip can be seen.
#[allow(dead_code)]
pub(crate) fn observed_canary_tokens(events: &[DebouncedEvent]) -> Vec<u64> {
    let mut out = Vec::new();
    for ev in events {
        for path in &ev.paths {
            if let Some(token) = token_of(path) {
                out.push(token);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn first_arm_is_alive() {
        let mut s = LivenessState::new(2);
        assert_eq!(s.arm(1), Verdict::Alive);
    }

    #[test]
    fn observed_arm_keeps_misses_zero() {
        let mut s = LivenessState::new(2);
        s.arm(1);
        s.observe(1);
        assert_eq!(s.arm(2), Verdict::Alive); // prior observed → miss stays 0
    }

    #[test]
    fn two_unobserved_arms_reach_dead() {
        let mut s = LivenessState::new(2);
        assert_eq!(s.arm(1), Verdict::Alive); // pending was None → miss 0
        assert_eq!(s.arm(2), Verdict::Alive); // token 1 unobserved → miss 1
        assert_eq!(s.arm(3), Verdict::Dead); //  token 2 unobserved → miss 2 == threshold
    }

    #[test]
    fn observe_wrong_token_is_ignored() {
        let mut s = LivenessState::new(2);
        s.arm(5);
        s.observe(99); // not the pending token
        assert_eq!(s.arm(6), Verdict::Alive); // 5 still unobserved → miss 1
        assert_eq!(s.arm(7), Verdict::Dead);
    }

    #[test]
    fn reset_clears_state() {
        let mut s = LivenessState::new(2);
        s.arm(1);
        s.arm(2); // miss 1
        s.reset();
        assert_eq!(s.arm(3), Verdict::Alive); // fresh: pending None → miss 0
        assert_eq!(s.arm(4), Verdict::Alive); // miss 1 again, not dead
    }

    #[test]
    fn canary_path_encodes_and_parses_token() {
        let p = canary_path(Path::new("/v/.textree/tmp"), 7);
        assert_eq!(p, PathBuf::from("/v/.textree/tmp/.watcher-canary-7"));
        assert_eq!(token_of(&p), Some(7));
    }

    #[test]
    fn token_of_rejects_non_canary_paths() {
        assert_eq!(token_of(Path::new("/v/note.md")), None);
        assert_eq!(token_of(Path::new("/v/.textree/tmp/other.tmp")), None);
        assert_eq!(token_of(Path::new("/v/.watcher-canary-x")), None); // non-numeric suffix
    }

    #[test]
    fn observed_tokens_picks_only_canaries() {
        use notify_debouncer_full::notify::event::CreateKind;
        use notify_debouncer_full::notify::{Event, EventKind};
        use std::time::Instant;

        let canary = canary_path(Path::new("/v/.textree/tmp"), 3);
        let ev_canary = DebouncedEvent::new(
            Event::new(EventKind::Create(CreateKind::Any)).add_path(canary),
            Instant::now(),
        );
        let ev_note = DebouncedEvent::new(
            Event::new(EventKind::Create(CreateKind::Any)).add_path(PathBuf::from("/v/note.md")),
            Instant::now(),
        );
        assert_eq!(observed_canary_tokens(&[ev_canary, ev_note]), vec![3]);
    }
}
