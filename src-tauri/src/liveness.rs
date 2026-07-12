//! Watcher liveness watchdog (Windows ReadDirectoryChangesW overflow workaround).
//!
//! On Windows a buffer overflow makes `notify` unwatch and go silent — no callback
//! signal reaches either debounce arm, so the tree/index silently go stale. This
//! module actively probes the watcher with a canary file in `.textree/tmp/`: a live
//! watcher surfaces the canary write back as an event; a dead one does not. Two
//! consecutive unobserved probes ⇒ the watcher is recreated.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
