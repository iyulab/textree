//! Watcher liveness watchdog (Windows ReadDirectoryChangesW overflow workaround).
//!
//! On Windows a buffer overflow makes `notify` unwatch and go silent — no callback
//! signal reaches either debounce arm, so the tree/index silently go stale. This
//! module actively probes the watcher with a canary file in `.textree/tmp/`: a live
//! watcher surfaces the canary write back as an event; a dead one does not. Two
//! consecutive unobserved probes ⇒ the watcher is recreated.

use crate::search::IndexHandle;
use crate::self_write::SelfWrites;
use crate::watcher::{self, VaultWatcher};
use notify_debouncer_full::DebouncedEvent;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Verdict of a liveness probe cycle.
#[derive(Debug, PartialEq, Eq)]
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
pub(crate) struct LivenessState {
    pending: Option<u64>,
    observed: bool,
    misses: u32,
    threshold: u32,
}

impl LivenessState {
    pub(crate) fn new(threshold: u32) -> Self {
        Self {
            pending: None,
            observed: false,
            misses: 0,
            threshold,
        }
    }

    /// Arms a new probe token and returns the verdict for the cycle just closed.
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
    pub(crate) fn observe(&mut self, token: u64) {
        if self.pending == Some(token) {
            self.observed = true;
        }
    }

    /// Clears all probe state (called after a successful recreate).
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
pub(crate) fn canary_path(tmp_dir: &Path, token: u64) -> PathBuf {
    tmp_dir.join(format!("{CANARY_PREFIX}{token}"))
}

/// Recovers the token from a canary path, or `None` if the path is not a canary.
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

/// Removes any canary files left in `tmp_dir`. Only the watchdog writes these, so
/// this is safe; it keeps at most one probe on disk at a time.
pub(crate) fn clear_canaries(tmp_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(tmp_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if token_of(&entry.path()).is_some() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Runs one probe cycle: arm the next token, drop the previous canary, write a fresh
/// one, and return the verdict. A write failure is logged (probe inconclusive) but is
/// never itself a death verdict — the watchdog must not destabilize the app.
pub(crate) fn arm_once(
    liveness: &Mutex<LivenessState>,
    counter: &AtomicU64,
    tmp_dir: &Path,
) -> Verdict {
    let token = counter.fetch_add(1, Ordering::SeqCst);
    let verdict = liveness
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .arm(token);

    if let Err(e) = std::fs::create_dir_all(tmp_dir) {
        log::warn!("watchdog: cannot create canary dir: {e}");
        return verdict;
    }
    clear_canaries(tmp_dir);
    let path = canary_path(tmp_dir, token);
    if let Err(e) = std::fs::write(&path, b"canary") {
        log::warn!("watchdog: canary write failed: {e}");
    }
    verdict
}

/// Idle probe cadence: how long the watchdog waits for real activity before
/// arming a canary on its own. Long, to keep unsolicited I/O near zero on a
/// healthy idle vault. Tunable — see the design's §8 (revisit if dogfooding shows
/// churn or slow detection).
const BACKUP_INTERVAL: Duration = Duration::from_secs(300);

/// Consecutive unobserved probes before the watcher is judged dead.
const MISS_THRESHOLD: u32 = 2;

/// Everything the watchdog thread and the debounce callback share. The callback
/// holds a `Weak<Shared>`; the thread and the `Watchdog` handle hold `Arc<Shared>`.
pub(crate) struct Shared {
    pub(crate) app: AppHandle,
    pub(crate) root: PathBuf,
    pub(crate) tmp_dir: PathBuf,
    pub(crate) self_writes: Arc<SelfWrites>,
    pub(crate) index: Arc<IndexHandle>,
    liveness: Mutex<LivenessState>,
    counter: AtomicU64,
    debouncer: Mutex<Option<VaultWatcher>>,
    arm_requested: Mutex<bool>,
    arm_cv: Condvar,
    stop: AtomicBool,
}

impl Shared {
    /// The debounce callback saw canary `token`.
    pub(crate) fn observe(&self, token: u64) {
        self.liveness
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(token);
    }

    /// The debounce callback saw real (non-canary) activity — wake the thread to
    /// probe. Never called for canary-only flushes (see the feedback-loop guard).
    pub(crate) fn request_arm(&self) {
        *self.arm_requested.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.arm_cv.notify_all();
    }
}

/// Background loop: wait for an arm request or the backup timeout, run one probe
/// cycle, and recreate the watcher if it looks dead.
fn run_thread(shared: Arc<Shared>) {
    loop {
        {
            let guard = shared.arm_requested.lock().unwrap_or_else(|e| e.into_inner());
            let (mut guard, _timeout) = shared
                .arm_cv
                .wait_timeout_while(guard, BACKUP_INTERVAL, |req| {
                    !*req && !shared.stop.load(Ordering::SeqCst)
                })
                .unwrap_or_else(|e| e.into_inner());
            *guard = false;
        }
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let verdict = arm_once(&shared.liveness, &shared.counter, &shared.tmp_dir);
        if verdict == Verdict::Dead {
            log::warn!("watchdog: watcher unresponsive to canary probes — recreating");
            recreate(&shared);
        }
    }
}

/// Drops the (presumed dead) debouncer and builds a fresh one, then recovers via the
/// existing rescan path: rebuild the index and tell the frontend to reconcile. On a
/// build failure it logs and leaves the debouncer absent — the next cycle retries.
fn recreate(shared: &Arc<Shared>) {
    // Drop the old debouncer here (off its own callback thread — safe).
    *shared.debouncer.lock().unwrap_or_else(|e| e.into_inner()) = None;
    match watcher::build_debouncer(shared) {
        Ok(deb) => {
            *shared.debouncer.lock().unwrap_or_else(|e| e.into_inner()) = Some(deb);
            watcher::rebuild_index(&shared.index, &shared.root);
            let _ = shared.app.emit("fs_rescan", ());
            shared.liveness.lock().unwrap_or_else(|e| e.into_inner()).reset();
        }
        Err(e) => log::warn!("watchdog: recreate failed (will retry): {e}"),
    }
}

/// Owns the live debouncer and the watchdog thread. Dropping it stops the thread,
/// drops the debouncer (stops watching), and clears leftover canaries — mirroring
/// how the previous `WatcherHandle` dropped its debouncer on a vault switch.
pub(crate) struct Watchdog {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    pub(crate) fn spawn(
        app: AppHandle,
        root: &Path,
        self_writes: Arc<SelfWrites>,
        index: Arc<IndexHandle>,
    ) -> Result<Watchdog, String> {
        let tmp_dir = root.join(".textree").join("tmp");
        let shared = Arc::new(Shared {
            app,
            root: root.to_path_buf(),
            tmp_dir,
            self_writes,
            index,
            liveness: Mutex::new(LivenessState::new(MISS_THRESHOLD)),
            counter: AtomicU64::new(0),
            debouncer: Mutex::new(None),
            arm_requested: Mutex::new(false),
            arm_cv: Condvar::new(),
            stop: AtomicBool::new(false),
        });

        let deb = watcher::build_debouncer(&shared)?;
        *shared.debouncer.lock().unwrap_or_else(|e| e.into_inner()) = Some(deb);

        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("vault-watchdog".into())
            .spawn(move || run_thread(thread_shared))
            .map_err(|e| e.to_string())?;

        Ok(Watchdog {
            shared,
            thread: Some(thread),
        })
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.arm_cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *self.shared.debouncer.lock().unwrap_or_else(|e| e.into_inner()) = None;
        clear_canaries(&self.shared.tmp_dir);
    }
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

    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn canary_count(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .map(|it| it.flatten().filter(|e| token_of(&e.path()).is_some()).count())
            .unwrap_or(0)
    }

    #[test]
    fn arm_once_keeps_single_canary_and_reports_alive() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".textree").join("tmp");
        let liveness = Mutex::new(LivenessState::new(2));
        let counter = AtomicU64::new(0);

        assert_eq!(arm_once(&liveness, &counter, &dir), Verdict::Alive);
        assert_eq!(arm_once(&liveness, &counter, &dir), Verdict::Alive);
        // The prior canary is cleared before the next is written → exactly one remains.
        assert_eq!(canary_count(&dir), 1);
    }

    #[test]
    fn arm_once_reaches_dead_without_observation() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".textree").join("tmp");
        let liveness = Mutex::new(LivenessState::new(2));
        let counter = AtomicU64::new(0);

        arm_once(&liveness, &counter, &dir); // miss 0
        arm_once(&liveness, &counter, &dir); // miss 1
        assert_eq!(arm_once(&liveness, &counter, &dir), Verdict::Dead); // miss 2
    }

    // Real-OS proof of the core mechanism WITHOUT an AppHandle: a canary written to
    // .textree/tmp must be surfaced by the live watcher and observed, keeping the
    // liveness Alive across cycles. Mirrors build_debouncer's observe pre-pass with a
    // raw debouncer (AppHandle is unavailable in a cargo test). Timing-dependent → #[ignore].
    // Run: cargo test --manifest-path src-tauri/Cargo.toml -- --ignored canary_round_trips
    #[test]
    #[ignore = "real-OS integration: spins the actual debouncer; timing-dependent"]
    fn canary_round_trips_on_real_watcher() {
        use notify_debouncer_full::notify::RecursiveMode;
        use notify_debouncer_full::{new_debouncer, DebounceEventResult};
        use std::sync::Arc;

        let vault = TempDir::new().unwrap();
        let tmp_dir = vault.path().join(".textree").join("tmp");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let liveness = Arc::new(Mutex::new(LivenessState::new(2)));
        let counter = AtomicU64::new(0);

        let liveness_cb = liveness.clone();
        let mut deb = new_debouncer(
            Duration::from_millis(300),
            None,
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    for t in observed_canary_tokens(&events) {
                        liveness_cb.lock().unwrap().observe(t);
                    }
                }
            },
        )
        .unwrap();
        deb.watch(vault.path(), RecursiveMode::Recursive).unwrap();
        std::thread::sleep(Duration::from_millis(400)); // settle initial watch

        // Arm + write a canary, let it round-trip, then arm again: the prior canary
        // was observed, so misses stay 0 and the verdict is Alive.
        assert_eq!(arm_once(&liveness, &counter, &tmp_dir), Verdict::Alive);
        std::thread::sleep(Duration::from_millis(900)); // debounce + settle
        assert_eq!(
            arm_once(&liveness, &counter, &tmp_dir),
            Verdict::Alive,
            "healthy watcher must observe the canary and keep misses at 0"
        );
    }
}
