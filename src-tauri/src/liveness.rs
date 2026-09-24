//! Watcher liveness watchdog (Windows ReadDirectoryChangesW overflow workaround).
//!
//! On Windows a buffer overflow makes `notify` unwatch and go silent — no callback
//! signal reaches either debounce arm, so the tree/index silently go stale. This
//! module actively probes the watcher with a canary file: a live watcher surfaces the
//! canary write back as an event; a dead one does not. Two consecutive unobserved
//! probes ⇒ the watcher is recreated.
//!
//! The canary has to be written somewhere the watcher sees, and nothing may be written
//! into the notes folder itself, nor into the working tree of a repository someone else
//! works in. The one place inside the watched folder that is the application's to write
//! is the repository storage, when the folder is a repository's root — so the canary
//! lives there, and only there (see [`canary_dir`]). A folder without one (nothing
//! recorded yet, or a folder inside a larger repository whose storage sits outside it)
//! is not probed: going unprobed costs a slower recovery from a silent watcher, while
//! writing into the folder would break the promise the folder is kept by.

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

/// Directory, inside repository storage, that holds the canary.
const CANARY_DIR_NAME: &str = "textree-canary";

/// Where the canary for the folder at `root` goes, if anywhere.
///
/// Only inside repository storage that itself sits inside the watched folder — the folder is a
/// repository's root. Anywhere else the watcher cannot see the write (the storage is outside
/// the folder) or the write would land in the person's folder. Checked on every probe, since a
/// folder becomes a repository when something is first recorded in it.
pub(crate) fn canary_dir(root: &Path) -> Option<PathBuf> {
    let storage = crate::git_engine::git_dir(root)?;
    let storage_c = std::fs::canonicalize(&storage).ok()?;
    let root_c = std::fs::canonicalize(root).ok()?;
    storage_c
        .starts_with(&root_c)
        .then(|| storage.join(CANARY_DIR_NAME))
}

/// Path of the canary for `token`, inside `tmp_dir`.
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

/// The probe cadence in use: [`BACKUP_INTERVAL`], unless `TEXTREE_WATCHDOG_INTERVAL_MS` says
/// otherwise. The override exists so an end-to-end run can watch whole probe cycles without
/// waiting minutes for each; nothing else sets it.
fn probe_interval() -> Duration {
    std::env::var("TEXTREE_WATCHDOG_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(BACKUP_INTERVAL)
}

/// Consecutive unobserved probes before the watcher is judged dead.
const MISS_THRESHOLD: u32 = 2;

/// Everything the watchdog thread and the debounce callback share. The callback
/// holds a `Weak<Shared>`; the thread and the `Watchdog` handle hold `Arc<Shared>`.
pub(crate) struct Shared {
    pub(crate) app: AppHandle,
    pub(crate) root: PathBuf,
    pub(crate) self_writes: Arc<SelfWrites>,
    pub(crate) index: Arc<IndexHandle>,
    liveness: Mutex<LivenessState>,
    counter: AtomicU64,
    debouncer: Mutex<Option<VaultWatcher>>,
    /// Set to `true` only by `Watchdog::drop` to wake the thread out of its backup-interval
    /// sleep immediately. Mutated under this mutex (paired with `shutdown_cv`) so the thread's
    /// wait predicate — which reads it and `stop` — cannot miss the wakeup. There is no
    /// post-flush arming: arming is timer-only, so nothing else ever sets this.
    shutdown: Mutex<bool>,
    shutdown_cv: Condvar,
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
}

/// Background loop: sleep for the backup interval (or until `Watchdog::drop` wakes it),
/// run one probe cycle, and recreate the watcher if it looks dead. Arming is timer-only:
/// external flushes never wake this loop, which keeps a busy but live watcher from being
/// probed to death (each probe would clear the in-flight canary before its round-trip).
fn run_thread(shared: Arc<Shared>) {
    loop {
        {
            let guard = shared.shutdown.lock().unwrap_or_else(|e| e.into_inner());
            let (mut guard, _timeout) = shared
                .shutdown_cv
                .wait_timeout_while(guard, probe_interval(), |flag| {
                    !*flag && !shared.stop.load(Ordering::SeqCst)
                })
                .unwrap_or_else(|e| e.into_inner());
            *guard = false;
        }
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        // Nowhere the watcher can see that is ours to write: not probed this cycle.
        let Some(canary_dir) = canary_dir(&shared.root) else {
            continue;
        };
        let verdict = arm_once(&shared.liveness, &shared.counter, &canary_dir);
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
        let shared = Arc::new(Shared {
            app,
            root: root.to_path_buf(),
            self_writes,
            index,
            liveness: Mutex::new(LivenessState::new(MISS_THRESHOLD)),
            counter: AtomicU64::new(0),
            debouncer: Mutex::new(None),
            shutdown: Mutex::new(false),
            shutdown_cv: Condvar::new(),
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
        {
            let mut g = self.shared.shutdown.lock().unwrap_or_else(|e| e.into_inner());
            self.shared.stop.store(true, Ordering::SeqCst);
            *g = true;
        }
        self.shared.shutdown_cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *self.shared.debouncer.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(dir) = canary_dir(&self.shared.root) {
            clear_canaries(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_canary_lives_in_repository_storage_inside_the_folder() {
        let tmp = TempDir::new().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let dir = canary_dir(tmp.path()).unwrap();
        assert!(dir.starts_with(repo.path()), "inside repository storage: {}", dir.display());
    }

    #[test]
    fn a_folder_with_no_place_of_ours_inside_it_is_not_probed() {
        // Nothing recorded yet: no repository, and the folder itself is not ours to write in.
        let plain = TempDir::new().unwrap();
        assert_eq!(canary_dir(plain.path()), None);

        // A folder inside someone's larger repository: its storage is outside what is watched,
        // and nothing is ever written into someone else's working tree.
        let outer = TempDir::new().unwrap();
        git2::Repository::init(outer.path()).unwrap();
        let inner = outer.path().join("docs");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(canary_dir(&inner), None);
    }

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
    // repository storage must be surfaced by the live watcher and observed, keeping the
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
        git2::Repository::init(vault.path()).unwrap();
        let tmp_dir = canary_dir(vault.path()).expect("a repository root has a canary place");
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

    // Regression guard for the recreate-storm bug: under SUSTAINED external file activity
    // (the feature's own target scenario — sync/checkout/other device), a live watcher must
    // stay Alive. A background thread hammers the vault with external .md writes while
    // arm_once runs at a moderate cadence (500ms > the ~300ms round-trip, simulating the
    // timer). With timer-only arming the in-flight canary is never cleared early, so it
    // round-trips and the verdict stays Alive every cycle. Timing-dependent → #[ignore].
    // Run: cargo test --manifest-path src-tauri/Cargo.toml -- --ignored alive_under_sustained
    #[test]
    #[ignore = "real-OS integration: spins the actual debouncer under load; timing-dependent"]
    fn alive_under_sustained_external_activity() {
        use notify_debouncer_full::notify::RecursiveMode;
        use notify_debouncer_full::{new_debouncer, DebounceEventResult};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let vault = TempDir::new().unwrap();
        git2::Repository::init(vault.path()).unwrap();
        let tmp_dir = canary_dir(vault.path()).expect("a repository root has a canary place");
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
                        liveness_cb.lock().unwrap_or_else(|e| e.into_inner()).observe(t);
                    }
                }
            },
        )
        .unwrap();
        deb.watch(vault.path(), RecursiveMode::Recursive).unwrap();
        std::thread::sleep(Duration::from_millis(400)); // settle initial watch

        // Background writer: continuous external activity for the whole probe window.
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = stop.clone();
        let vault_w = vault.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            let mut n: u64 = 0;
            while !stop_w.load(Ordering::SeqCst) {
                let f = vault_w.join(format!("ext-{}.md", n % 8));
                let _ = std::fs::write(&f, format!("external burst content {n}"));
                n += 1;
                std::thread::sleep(Duration::from_millis(50)); // ~20 writes/s
            }
        });

        // Probe at a moderate cadence WHILE the writer runs. arm_once fires during the burst,
        // so this genuinely exercises the "does external churn starve the canary?" path. If
        // the canary were cleared early / starved, misses would reach threshold 2 → Dead.
        for cycle in 0..6 {
            std::thread::sleep(Duration::from_millis(500)); // > 300ms round-trip
            assert_eq!(
                arm_once(&liveness, &counter, &tmp_dir),
                Verdict::Alive,
                "cycle {cycle}: live watcher under external load must stay Alive (no false death)"
            );
        }

        stop.store(true, Ordering::SeqCst);
        writer.join().unwrap();
    }
}
