# Watcher Liveness Watchdog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect a silently-dead file watcher (Windows `ReadDirectoryChangesW` overflow → `unwatch` + silence) by actively probing with a canary file, and recover by recreating the debouncer, reusing the existing rescan/rebuild path.

**Architecture:** A new `liveness.rs` owns a pure `LivenessState` machine, pure canary-path helpers, and a `Watchdog` orchestrator that runs a background thread. The thread periodically (and after real activity) writes a canary into `.textree/tmp/`; a live watcher surfaces it back as an event the debounce callback observes. Two consecutive unobserved probes ⇒ the watcher is judged dead ⇒ the debouncer is dropped and rebuilt, then `rebuild_index` + `fs_rescan` fire (the existing recovery path). The debouncer-building logic lives in `watcher.rs` (it needs watcher internals); the callback references the shared state via a `Weak` pointer to avoid a reference cycle.

**Tech Stack:** Rust, Tauri 2, `notify-debouncer-full`, `std::sync::{Condvar, Mutex, atomic}`.

## Global Constraints

- Repo `textree/` is public GPL-3.0-only → **all content English** (code, comments, commit messages).
- Commits = Conventional Commits, English.
- `TreatWarningsAsErrors` mindset: no compiler warnings.
- Layering unchanged: `commands` → `fs_ops` → `pathsafe`; IPC only through registered commands.
- Canary files live **only** under `.textree/tmp/` (already sync-ignorable, swept, and excluded from tree/search/surfacing). Never touch a user content folder.
- Data-safety > simplicity > readability > performance > features.
- Push is manual — **not** part of this plan.
- Gates (run in `textree/`): `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run check` (0 errors/0 warnings), `npm run tauri build`. Frontend is unchanged (reuses `fs_rescan`), so vitest is unaffected.

---

### Task 1: `LivenessState` pure state machine

**Files:**
- Create: `src-tauri/src/liveness.rs`
- Modify: `src-tauri/src/lib.rs` (add module declaration)
- Test: inline `#[cfg(test)]` in `src-tauri/src/liveness.rs`

**Interfaces:**
- Produces:
  - `pub(crate) enum Verdict { Alive, Dead }` (derives `Debug, PartialEq, Eq`)
  - `pub(crate) struct LivenessState`
  - `LivenessState::new(threshold: u32) -> Self`
  - `LivenessState::arm(&mut self, token: u64) -> Verdict` — records a newly-armed probe; first evaluates whether the previously-armed token was observed (reset miss counter) or not (increment). Returns `Dead` when consecutive misses reach `threshold`.
  - `LivenessState::observe(&mut self, token: u64)` — marks the armed token seen if it matches.
  - `LivenessState::reset(&mut self)` — clears pending/observed/misses (used after recovery).

- [ ] **Step 1: Add the module declaration to `lib.rs`**

Add alongside the other `mod` lines near the top of `src-tauri/src/lib.rs` (e.g. after `pub(crate) mod watcher;` at line 14):

```rust
pub(crate) mod liveness;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/liveness.rs` with only the test module (implementation comes next):

```rust
//! Watcher liveness watchdog (Windows ReadDirectoryChangesW overflow workaround).
//!
//! On Windows a buffer overflow makes `notify` unwatch and go silent — no callback
//! signal reaches either debounce arm, so the tree/index silently go stale. This
//! module actively probes the watcher with a canary file in `.textree/tmp/`: a live
//! watcher surfaces the canary write back as an event; a dead one does not. Two
//! consecutive unobserved probes ⇒ the watcher is recreated.

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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: FAIL — `cannot find type LivenessState` / `Verdict`.

- [ ] **Step 4: Write the minimal implementation**

Insert above the `#[cfg(test)]` module in `src-tauri/src/liveness.rs`:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/liveness.rs src-tauri/src/lib.rs
git commit -m "feat(watcher): add LivenessState probe state machine"
```

---

### Task 2: Canary path helpers + event scan

**Files:**
- Modify: `src-tauri/src/liveness.rs`
- Test: inline `#[cfg(test)]` in `src-tauri/src/liveness.rs`

**Interfaces:**
- Consumes: nothing from Task 1 at runtime (independent helpers).
- Produces:
  - `pub(crate) fn canary_path(tmp_dir: &Path, token: u64) -> PathBuf` → `<tmp_dir>/.watcher-canary-<token>`
  - `pub(crate) fn token_of(path: &Path) -> Option<u64>` → parses a canary filename back to its token; `None` for any non-canary path.
  - `pub(crate) fn observed_canary_tokens(events: &[DebouncedEvent]) -> Vec<u64>` → scans raw debounce events for canary paths (runs BEFORE `is_ignored` drops them from surfacing).

- [ ] **Step 1: Add imports**

At the top of `src-tauri/src/liveness.rs` (below the module doc comment), add:

```rust
use notify_debouncer_full::DebouncedEvent;
use std::path::{Path, PathBuf};
```

- [ ] **Step 2: Write the failing tests**

Add to the `tests` module in `src-tauri/src/liveness.rs`:

```rust
    use std::path::PathBuf;

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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: FAIL — `cannot find function canary_path` etc.

- [ ] **Step 4: Write the implementation**

Add to `src-tauri/src/liveness.rs` (below the `LivenessState` impl):

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: PASS (8 tests total).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/liveness.rs
git commit -m "feat(watcher): add canary path helpers and event scan"
```

---

### Task 3: `arm_once` — probe write + verdict (AppHandle-free core)

**Files:**
- Modify: `src-tauri/src/liveness.rs`
- Test: inline `#[cfg(test)]` in `src-tauri/src/liveness.rs`

**Interfaces:**
- Consumes: `LivenessState` (Task 1), `canary_path`/`token_of` (Task 2).
- Produces:
  - `pub(crate) fn clear_canaries(tmp_dir: &Path)` — removes any leftover canary files.
  - `pub(crate) fn arm_once(liveness: &Mutex<LivenessState>, counter: &AtomicU64, tmp_dir: &Path) -> Verdict` — advances the counter, arms the state, keeps exactly one fresh canary on disk, and returns the verdict. Deliberately free of `AppHandle`/threads so the probe decision is unit-testable.

- [ ] **Step 1: Add imports**

Add to the import block at the top of `src-tauri/src/liveness.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
```

- [ ] **Step 2: Write the failing tests**

Add to the `tests` module in `src-tauri/src/liveness.rs`:

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: FAIL — `cannot find function arm_once`.

- [ ] **Step 4: Write the implementation**

Add to `src-tauri/src/liveness.rs` (below `observed_canary_tokens`):

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml liveness::tests`
Expected: PASS (10 tests total).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/liveness.rs
git commit -m "feat(watcher): add arm_once probe cycle (canary write + verdict)"
```

---

### Task 4: `Watchdog` orchestrator + `watcher::build_debouncer` refactor

**Files:**
- Modify: `src-tauri/src/liveness.rs` (add `Shared`, `Watchdog`, thread loop, `recreate`)
- Modify: `src-tauri/src/watcher.rs` (extract `build_debouncer`; remove `start`)
- Test: inline `#[cfg(test)]` in `src-tauri/src/liveness.rs` (one `#[ignore]` real-OS test)

**Interfaces:**
- Consumes: `arm_once` (Task 3), `observed_canary_tokens` (Task 2), and from `watcher`: `changes_from_events`, `FlushOutcome`, `apply_changes_to_index`, `rebuild_index`, `VaultWatcher`.
- Produces:
  - `pub(crate) struct Shared` with `pub(crate)` fields `app: AppHandle`, `root: PathBuf`, `tmp_dir: PathBuf`, `self_writes: Arc<SelfWrites>`, `index: Arc<IndexHandle>`, plus private probe/thread state. Methods `observe(&self, token: u64)` and `request_arm(&self)`.
  - `pub(crate) struct Watchdog` with `Watchdog::spawn(app: AppHandle, root: &Path, self_writes: Arc<SelfWrites>, index: Arc<IndexHandle>) -> Result<Watchdog, String>` and a `Drop` impl (stops thread, drops debouncer, clears canaries).
  - `pub(crate) fn build_debouncer(shared: &Arc<Shared>) -> Result<VaultWatcher, String>` in `watcher.rs`.
  - Constants `BACKUP_INTERVAL: Duration` (300s) and `MISS_THRESHOLD: u32` (2).

**Design notes for the implementer (read before writing):**
- The debounce callback closes over a `Weak<Shared>` (NOT `Arc`) — otherwise `Shared → debouncer → callback → Shared` is a reference cycle that leaks. Each invocation does `weak.upgrade()`; `None` means the watchdog was dropped → do nothing.
- The watchdog thread is the ONLY place that arms and may recreate. The callback only observes and (on real activity) requests an arm. This keeps recreate off the debouncer's own callback thread — dropping a debouncer from inside its own callback would deadlock (a thread joining itself).
- Feedback-loop guard: a canary write itself produces an event → a flush. If that flush re-armed, canary-writes would loop forever. So the callback requests an arm ONLY on a non-empty `Changes` batch or a `Rescan` — a canary-only flush yields an empty batch (canaries are dropped by `is_ignored`) and does NOT re-arm.

- [ ] **Step 1: Add imports to `liveness.rs`**

Add to the import block at the top of `src-tauri/src/liveness.rs`:

```rust
use crate::search::IndexHandle;
use crate::self_write::SelfWrites;
use crate::watcher::{self, VaultWatcher};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
```

- [ ] **Step 2: Add `Shared`, constants, `arm`/`observe` methods, thread loop, and `recreate`**

Add to `src-tauri/src/liveness.rs` (below `arm_once`):

```rust
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
```

- [ ] **Step 3: Add the `Watchdog` handle + `Drop`**

Add to `src-tauri/src/liveness.rs` (below `recreate`):

```rust
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
```

- [ ] **Step 4: Extract `build_debouncer` in `watcher.rs` and remove `start`**

In `src-tauri/src/watcher.rs`, replace the entire `pub fn start(...) -> Result<VaultWatcher, String> { ... }` function (lines ~205-257) with `build_debouncer`. Keep every other function (`changes_from_events`, `process_batch`, `apply_changes_to_index`, `rebuild_index`, `map_kind`, `is_ignored`, `to_fs_change`) unchanged, but mark `changes_from_events` and `FlushOutcome` `pub(crate)` if not already (they are referenced from `build_debouncer` in the same module, so no visibility change is needed there — only `build_debouncer` itself is new/public).

Add these imports to the top of `watcher.rs` (it already imports the rest):

```rust
use crate::liveness::{self, Shared};
use std::sync::Weak;
```

New function (replaces `start`):

```rust
/// Builds a recursive debouncer for the vault root wired to the watchdog's shared
/// state. The callback holds a `Weak<Shared>` to avoid a reference cycle
/// (`Shared` → debouncer → callback → `Shared`); on each flush it observes canary
/// round-trips, surfaces real changes, and — only on real activity — asks the
/// watchdog to arm the next probe (a canary-only flush must not re-arm, or canary
/// writes would loop).
pub(crate) fn build_debouncer(shared: &Arc<Shared>) -> Result<VaultWatcher, String> {
    let weak: Weak<Shared> = Arc::downgrade(shared);
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            let events = match result {
                Ok(events) => events,
                Err(errors) => {
                    // Watch errors are logged, not acted on: dropped-event recovery does NOT
                    // arrive here (Ok-arm Rescan on Linux/macOS, or the watchdog on Windows),
                    // and refreshing on every transient error would churn.
                    for e in &errors {
                        log::warn!("watcher error: {e}");
                    }
                    return;
                }
            };
            // Observe canary round-trips first — they are dropped by is_ignored below,
            // so this pre-pass is the only place the watchdog can see them.
            for token in liveness::observed_canary_tokens(&events) {
                shared.observe(token);
            }
            match changes_from_events(events, &shared.root, &shared.self_writes) {
                FlushOutcome::Changes(batch) => {
                    for change in &batch {
                        let _ = shared.app.emit("fs_changed", change.clone());
                    }
                    apply_changes_to_index(&shared.index, &shared.root, &batch);
                    // Real activity → probe soon. Canary-only flushes yield an empty
                    // batch and deliberately do NOT re-arm (feedback-loop guard).
                    if !batch.is_empty() {
                        shared.request_arm();
                    }
                }
                FlushOutcome::Rescan => {
                    log::warn!("watcher: OS reported dropped events (rescan) — full refresh");
                    rebuild_index(&shared.index, &shared.root);
                    let _ = shared.app.emit("fs_rescan", ());
                    shared.request_arm();
                }
            }
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watch(&shared.root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    Ok(debouncer)
}
```

Note: `watcher.rs` already imports `Arc` (line 16 `use std::sync::{Arc, Mutex};`). Ensure `changes_from_events` / `FlushOutcome` / `apply_changes_to_index` / `rebuild_index` remain reachable — they are in the same module, so no change. The old `start` had `pub`; nothing else calls `start` after Task 5 rewires `open_vault`, so removing it is safe (the `#[ignore]` end-to-end test in `watcher.rs` builds its own debouncer and does not use `start`).

- [ ] **Step 5: Write the failing real-OS integration test**

Add to the `tests` module in `src-tauri/src/liveness.rs`:

```rust
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
```

Need `use std::time::Duration;` in the test module — add it if not already present via `use super::*;` (the `Duration` import added in Step 1 is at module scope, so `super::*` re-exports it; no extra import needed).

- [ ] **Step 6: Verify the build and unit tests pass; run the ignored test manually**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — all `liveness` and `watcher` unit tests compile and pass (the `#[ignore]` test is skipped).

Run (manual, real-OS): `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored canary_round_trips`
Expected: PASS on Windows/macOS/Linux (the live watcher observes the canary).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/liveness.rs src-tauri/src/watcher.rs
git commit -m "feat(watcher): add Watchdog orchestrator and build_debouncer"
```

---

### Task 5: Wire the watchdog into `open_vault`

**Files:**
- Modify: `src-tauri/src/watcher.rs` (change `WatcherHandle` to hold a `Watchdog`)
- Modify: `src-tauri/src/commands.rs:294-349` (`open_vault` uses `Watchdog::spawn`)
- Modify: `src-tauri/src/lib.rs` (imports; `.manage(WatcherHandle::default())` is unchanged)

**Interfaces:**
- Consumes: `Watchdog::spawn` (Task 4).
- Produces: `WatcherHandle` now stores `Option<liveness::Watchdog>`.

- [ ] **Step 1: Change `WatcherHandle` to hold a `Watchdog`**

In `src-tauri/src/watcher.rs`, replace the `WatcherHandle` definition (lines ~37-40):

```rust
/// Handle to the running vault watchdog. Managed as a Tauri `State`.
/// Opening a new vault drops the previous watchdog (stops its thread and the
/// debouncer) and replaces it.
#[derive(Default)]
pub struct WatcherHandle(pub Mutex<Option<crate::liveness::Watchdog>>);
```

- [ ] **Step 2: Rewire `open_vault`**

In `src-tauri/src/commands.rs`, replace lines ~342-346 (the block that drops the old watcher and calls `watcher::start`):

```rust
    // Explicitly drop the previous watchdog (stops its thread and the debouncer) before
    // starting the new one, so leftover events from the old vault don't bleed into the new.
    *watcher_handle.0.lock().unwrap() = None;
    let wd = crate::liveness::Watchdog::spawn(
        app,
        &root_path,
        self_writes.inner().clone(),
        index.inner().clone(),
    )?;
    *watcher_handle.0.lock().unwrap() = Some(wd);
```

The `use crate::watcher::{self, WatcherHandle};` import at the top of `commands.rs` stays (it still uses `WatcherHandle`). `watcher::start` is no longer referenced; if the `self` import becomes unused after removal, drop it to `use crate::watcher::WatcherHandle;` — verify with the build.

- [ ] **Step 3: Verify the full backend builds and tests pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — all tests, no warnings.

- [ ] **Step 4: Verify the frontend typecheck (unchanged, must stay green)**

Run: `npm run check`
Expected: 0 errors, 0 warnings (no frontend changes — `fs_rescan` is reused).

- [ ] **Step 5: Verify the release build**

Run: `npm run tauri build`
Expected: build succeeds (EV signing may fail locally — that is CI-only and unrelated).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/watcher.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(watcher): wire liveness watchdog into open_vault"
```

---

## Manual verification (post-merge, real OS)

Automated coverage stops at the `#[ignore]` canary round-trip (the full `Watchdog` needs an `AppHandle` only a running app provides). The recreate-on-death path is exercised manually, combined with the existing "sync-folder robustness stress" backlog item:

1. Open a vault on Windows; confirm normal edits still surface (`fs_changed`) and search updates.
2. Stress the folder (bulk file-creation script / heavy OneDrive sync) to force a `ReadDirectoryChangesW` overflow.
3. Confirm that within the honest timer-bound detection window — worst case ~3 backup intervals (~10–15 min at `BACKUP_INTERVAL = 300s`, since arming is timer-only, not post-flush) — the log shows `watcher: watcher unresponsive to canary probes — recreating`, the tree/index refresh (`fs_rescan`), and subsequent edits surface again. (Shorten `BACKUP_INTERVAL` locally to make this practical to observe.)
4. Confirm no canary files linger in `.textree/tmp/` after closing the vault.

## Self-Review

- **Spec coverage:** §1 problem → detected by Task 4 (Windows overflow, silent death). §2 strategy (canary in `.textree/tmp`) → Tasks 2-3. §3.1 `CanaryToken` → Task 2. §3.2 `LivenessState` → Task 1. §3.3 `Watchdog` → Task 4. §3.4 watcher filter/observe → Task 4 `build_debouncer`. §3.5 reuse `rebuild_index`/`fs_rescan` → Task 4 `recreate`. §4 arming policy → Task 4 (**revised to timer-only**: the post-flush `request_arm` trigger was removed after review found it storm-recreates a live watcher under sustained load — see design §4; only the `BACKUP_INTERVAL` timer arms now). §5 recovery flow → Task 4 `recreate`. §6 error handling (write fail, recreate fail, residue, vault switch, non-Windows) → Tasks 3-5. §7 testing → Tasks 1-4 units + Task 4 `#[ignore]` + manual. §8 non-goals honored (no notify fork, no blind rescan, no UI). §9 constants (5-min, threshold 2) → Task 4.
- **Placeholder scan:** none — every step has concrete code/commands.
- **Type consistency:** `LivenessState::{new,arm,observe,reset}`, `Verdict::{Alive,Dead}`, `canary_path`, `token_of`, `observed_canary_tokens`, `clear_canaries`, `arm_once`, `Shared`, `Watchdog::spawn`, `build_debouncer` — names and signatures are consistent across Tasks 1-5.
