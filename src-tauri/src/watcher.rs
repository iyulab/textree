//! External file change watching (M3).
//!
//! Recursively watches the vault root with `notify-debouncer-full`, filters out
//! the app's own writes (echoes) via the self-write registry, then emits
//! `fs_changed` events to the frontend. Dropping the debouncer stops watching,
//! so the returned handle is kept alive in state.

use crate::liveness::{self, Shared};
use crate::search::IndexHandle;
use crate::self_write::SelfWrites;
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tauri::Emitter;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
}

/// Change payload sent to the frontend (app IPC wire type — separate from library types).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct FsChange {
    pub kind: ChangeKind,
    pub path: String,
}

pub type VaultWatcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// Handle to the running vault watchdog. Managed as a Tauri `State`.
/// Opening a new vault drops the previous watchdog (stops its thread and the
/// debouncer) and replaces it.
#[derive(Default)]
pub struct WatcherHandle(pub Mutex<Option<crate::liveness::Watchdog>>);

/// Reduces a notify `EventKind` to our change kind. Kinds we don't care about return `None`.
fn map_kind(kind: &EventKind) -> Option<ChangeKind> {
    if kind.is_create() {
        Some(ChangeKind::Created)
    } else if kind.is_remove() {
        Some(ChangeKind::Removed)
    } else if kind.is_modify() {
        Some(ChangeKind::Modified)
    } else {
        None // ignore Access/Any/Other
    }
}

/// Dot segments relative to the vault root (`.textree`, `.git`, etc.) are hidden in the
/// tree, so the watcher ignores them too. Dots in the root path itself are not counted
/// toward the ignore decision.
fn is_ignored(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')))
}

/// Converts a single `(kind, path)` into a frontend change. Returns `None` (echo
/// suppression) when it's a create/modify and the current file content matches a
/// registered self-write.
fn to_fs_change(kind: ChangeKind, path: &Path, self_writes: &SelfWrites) -> Option<FsChange> {
    if matches!(kind, ChangeKind::Created | ChangeKind::Modified) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                if self_writes.take_if_matches(path, &content) {
                    return None; // self-write echo — suppress
                }
            }
            Err(_) => {
                // If the content can't be read, an echo decision is impossible. Clean up
                // any remaining registration so a stale entry doesn't wrongly suppress a
                // later external change with identical content.
                self_writes.forget(path);
            }
        }
    }
    Some(FsChange {
        kind,
        path: path.display().to_string(),
    })
}

/// One debounce flush, reduced to what the app must do about it.
#[derive(Debug, PartialEq, Eq)]
enum FlushOutcome {
    /// Normal flush: per-path changes to surface (may be empty after suppression).
    Changes(Vec<FsChange>),
    /// The OS dropped events (kernel queue overflow — inotify `Q_OVERFLOW`, FSEvents
    /// `MUST_SCAN_SUBDIRS`): per-path state is unknowable, so everything must be refreshed
    /// (tree, index, open note). The debouncer prepends this signal to the flush as an
    /// `EventKind::Other` event flagged `Rescan`.
    Rescan,
}

/// Turns one debounce flush (raw OS events) into the changes to surface: maps/filters kinds and dot
/// paths, then collapses to one change per path via `process_batch`. Shared by the live watcher and
/// the integration test so both exercise the same path.
///
/// A flush carrying a rescan flag short-circuits to `Rescan` — the batch alongside it is
/// incomplete by definition, and a full refresh supersedes anything it says. Self-write
/// registrations are deliberately left alone: suppression only ever fires on byte-identical
/// content, so a stale entry can only suppress a change that would have been a no-op anyway.
fn changes_from_events(
    events: Vec<DebouncedEvent>,
    root: &Path,
    self_writes: &SelfWrites,
) -> FlushOutcome {
    if events.iter().any(|ev| ev.need_rescan()) {
        return FlushOutcome::Rescan;
    }
    let mut raw: Vec<(ChangeKind, PathBuf)> = Vec::new();
    for ev in events {
        let Some(kind) = map_kind(&ev.kind) else {
            continue;
        };
        for path in &ev.paths {
            if is_ignored(root, path) {
                continue;
            }
            raw.push((kind.clone(), path.clone()));
        }
    }
    FlushOutcome::Changes(process_batch(&raw, self_writes))
}

/// Collapses a debounce batch into at most one change per path.
///
/// A single logical write goes through `atomic_write` (temp in `.textree/tmp/` → rename onto the
/// note), which the OS surfaces as MULTIPLE events on the same final path — on Windows a rename that
/// overwrites an existing file yields Remove + Create + Modify. Processing each event separately
/// defeats the consume-once self-write suppression: the first matching event consumes the
/// registration, so later events for the same write leak through as false external changes →
/// `reloadActive` → editor recreation → lost typing focus (the focus-loss bug).
///
/// Resolving each path by its FINAL filesystem state fixes this at the source: a path that still
/// exists is one (self-write-checked) Modified; a path that is gone is a real Removed. The
/// self-write registry is then consulted exactly once per path, so a self-write never leaks.
fn process_batch(raw: &[(ChangeKind, PathBuf)], self_writes: &SelfWrites) -> Vec<FsChange> {
    // Unique paths in first-seen order (stable, dependency-free dedup).
    let mut paths: Vec<&PathBuf> = Vec::new();
    for (_, p) in raw {
        if !paths.contains(&p) {
            paths.push(p);
        }
    }

    let mut out = Vec::new();
    for path in paths {
        let change = if path.exists() {
            // Still on disk → a single create/modify. to_fs_change reads the content and suppresses
            // it when it matches a registered self-write (consuming the entry once, as intended).
            to_fs_change(ChangeKind::Modified, path, self_writes)
        } else {
            // Gone → a genuine removal. A self-write can't apply to a vanished file; drop any stale
            // registration so it can't wrongly suppress a later change, then emit.
            self_writes.forget(path);
            Some(FsChange {
                kind: ChangeKind::Removed,
                path: path.display().to_string(),
            })
        };
        if let Some(c) = change {
            out.push(c);
        }
    }
    out
}

/// Applies a batch of changes to the index and commits once (.md only; excluding dot
/// paths is already done by the caller's is_ignored). No-op when no index is installed.
pub fn apply_changes_to_index(handle: &IndexHandle, root: &Path, changes: &[FsChange]) {
    let mut guard = handle.0.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.as_mut() else { return };
    let mut touched = false;
    for c in changes {
        let p = Path::new(&c.path);
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            continue;
        }
        state.apply_change(root, p, matches!(c.kind, ChangeKind::Removed));
        touched = true;
    }
    if touched {
        let _ = state.commit();
    }
}

/// Rebuilds the whole index from disk (delete-all + re-index). Used after a rescan, when
/// incremental per-path updates can no longer be trusted. No-op when no index is installed.
pub fn rebuild_index(handle: &IndexHandle, root: &Path) {
    let mut guard = handle.0.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.as_mut() else { return };
    if let Err(e) = state.rebuild(root) {
        log::warn!("watcher: index rebuild after rescan failed: {e}");
    }
}

/// Builds a recursive debouncer for the vault root wired to the watchdog's shared
/// state. The callback holds a `Weak<Shared>` to avoid a reference cycle
/// (`Shared` → debouncer → callback → `Shared`); on each flush it observes canary
/// round-trips and surfaces real changes. It does NOT arm probes — arming is
/// timer-only (the watchdog thread's backup interval). Flush-driven arming would
/// re-arm on every external burst, clearing the in-flight canary ~75ms after it was
/// written (long before its ~300ms round-trip), so a busy but live watcher would be
/// falsely judged dead and recreated in a storm. A silently-dead watcher emits no
/// flushes anyway, so detection is timer-bound regardless.
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
                }
                FlushOutcome::Rescan => {
                    log::warn!("watcher: OS reported dropped events (rescan) — full refresh");
                    rebuild_index(&shared.index, &shared.root);
                    let _ = shared.app.emit("fs_rescan", ());
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

#[cfg(test)]
mod tests {
    use super::*;
    use notify_debouncer_full::notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
    use notify_debouncer_full::notify::Event;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn rescan_flag_turns_flush_into_rescan_outcome() {
        use notify_debouncer_full::notify::event::Flag;
        use std::time::Instant;
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "content").unwrap();
        let sw = SelfWrites::default();

        // The debouncer prepends the rescan marker to the same flush as surviving events; the
        // whole batch is untrustworthy (events were dropped), so the outcome must be Rescan.
        let rescan = DebouncedEvent::new(
            Event::new(EventKind::Other).set_flag(Flag::Rescan),
            Instant::now(),
        );
        let survivor = DebouncedEvent::new(
            Event::new(EventKind::Create(CreateKind::Any)).add_path(f),
            Instant::now(),
        );
        let out = changes_from_events(vec![rescan, survivor], tmp.path(), &sw);
        assert_eq!(out, FlushOutcome::Rescan);
    }

    #[test]
    fn flush_without_rescan_yields_changes() {
        use std::time::Instant;
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "external content").unwrap();
        let sw = SelfWrites::default();

        let ev = DebouncedEvent::new(
            Event::new(EventKind::Create(CreateKind::Any)).add_path(f),
            Instant::now(),
        );
        let out = changes_from_events(vec![ev], tmp.path(), &sw);
        let FlushOutcome::Changes(batch) = out else {
            panic!("normal flush must yield Changes, got {out:?}");
        };
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn rebuild_index_recovers_changes_the_watcher_missed() {
        let vault = TempDir::new().unwrap();
        let idx = TempDir::new().unwrap();
        std::fs::write(vault.path().join("a.md"), "stale note").unwrap();

        let handle: Arc<IndexHandle> = Arc::new(IndexHandle::default());
        *handle.0.lock().unwrap() = Some(IndexState::open_or_create(idx.path()).unwrap());
        apply_changes_to_index(
            &handle,
            vault.path(),
            &[FsChange {
                kind: ChangeKind::Created,
                path: vault.path().join("a.md").display().to_string(),
            }],
        );

        // Simulate dropped events: a.md vanished and b.md appeared with no per-path event.
        std::fs::remove_file(vault.path().join("a.md")).unwrap();
        std::fs::write(vault.path().join("b.md"), "fresh note").unwrap();

        rebuild_index(&handle, vault.path());

        let guard = handle.0.lock().unwrap();
        let st = guard.as_ref().unwrap();
        assert_eq!(st.search("stale", 10).unwrap().len(), 0, "stale doc must be gone");
        assert_eq!(st.search("fresh", 10).unwrap().len(), 1, "missed file must be indexed");
    }

    #[test]
    fn map_kind_classifies_create_modify_remove_and_ignores_access() {
        assert_eq!(
            map_kind(&EventKind::Create(CreateKind::Any)),
            Some(ChangeKind::Created)
        );
        assert_eq!(
            map_kind(&EventKind::Modify(ModifyKind::Any)),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            map_kind(&EventKind::Remove(RemoveKind::Any)),
            Some(ChangeKind::Removed)
        );
        assert_eq!(map_kind(&EventKind::Access(AccessKind::Any)), None);
    }

    #[test]
    fn is_ignored_skips_dot_segments_below_root() {
        let root = PathBuf::from("/vault");
        assert!(is_ignored(&root, &PathBuf::from("/vault/.textree/db.json")));
        assert!(is_ignored(&root, &PathBuf::from("/vault/.git/config")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/note.md")));
        assert!(!is_ignored(&root, &PathBuf::from("/vault/journal/2026.md")));
    }

    #[test]
    fn is_ignored_does_not_count_dot_in_root_path() {
        let root = PathBuf::from("/home/.config/vault");
        // .config inside the root is not subject to the ignore decision.
        assert!(!is_ignored(&root, &PathBuf::from("/home/.config/vault/note.md")));
        // A dot folder below the root is still ignored.
        assert!(is_ignored(
            &root,
            &PathBuf::from("/home/.config/vault/.textree/x.json")
        ));
    }

    #[test]
    fn self_write_is_suppressed_external_change_is_emitted() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "self-written").unwrap();
        let sw = SelfWrites::default();
        sw.record(&f, "self-written");

        // Content matches the registered self-write → suppress.
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_none());

        // An external tool changes it to different content → emit.
        std::fs::write(&f, "external edit").unwrap();
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
    }

    // The focus-loss regression: one logical atomic write surfaces as Remove+Create+Modify on the
    // same path (observed on Windows). Per-event processing with consume-once suppression leaked the
    // trailing events as false external changes → editor reload → lost typing focus. process_batch
    // must collapse them to a single, fully-suppressed self-write.
    #[test]
    fn atomic_write_multi_event_collapses_to_no_external_change() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "the saved content").unwrap();
        let sw = SelfWrites::default();
        sw.record(&f, "the saved content"); // write_note records before the rename

        let raw = vec![
            (ChangeKind::Removed, f.clone()),
            (ChangeKind::Created, f.clone()),
            (ChangeKind::Modified, f.clone()),
        ];
        let out = process_batch(&raw, &sw);
        assert!(out.is_empty(), "self-write echo leaked through process_batch: {out:?}");
    }

    #[test]
    fn external_edit_collapses_to_one_modified() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "content an external tool wrote").unwrap();
        let sw = SelfWrites::default(); // nothing recorded → genuinely external

        // Even if the OS reports several events for the path, exactly one change is emitted.
        let raw = vec![
            (ChangeKind::Created, f.clone()),
            (ChangeKind::Modified, f.clone()),
        ];
        let out = process_batch(&raw, &sw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn vanished_path_emits_one_removed() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("gone.md"); // never created → does not exist on disk
        let sw = SelfWrites::default();
        let raw = vec![(ChangeKind::Removed, f.clone())];
        let out = process_batch(&raw, &sw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Removed);
    }

    #[test]
    fn distinct_paths_each_emit_once() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a.md");
        let b = tmp.path().join("b.md");
        std::fs::write(&a, "aa").unwrap();
        std::fs::write(&b, "bb").unwrap();
        let sw = SelfWrites::default();
        let raw = vec![
            (ChangeKind::Modified, a.clone()),
            (ChangeKind::Created, b.clone()),
        ];
        let out = process_batch(&raw, &sw);
        assert_eq!(out.len(), 2);
    }

    // End-to-end proof of the focus-loss fix on the real OS watcher: a real atomic_write
    // (record → temp in .textree/tmp → persist/rename onto the note) must surface ZERO external
    // changes. Pre-fix this leaked ≥1 change (the Modify after consume-once + the Remove) →
    // reloadActive → editor recreation → lost typing focus. Timing-dependent, hence #[ignore].
    // Run: cargo test --manifest-path src-tauri/Cargo.toml -- --ignored atomic_write_no_external
    #[test]
    #[ignore = "real-OS integration: spins the actual debouncer; timing-dependent"]
    fn atomic_write_no_external_change_end_to_end() {
        use std::io::Write as _;
        use std::sync::{Arc, Mutex};
        use tempfile::NamedTempFile;

        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let file = root.join("a.md");
        std::fs::write(&file, "v1").unwrap();
        let tmpdir = root.join(".textree").join("tmp");
        std::fs::create_dir_all(&tmpdir).unwrap();

        let sw = Arc::new(SelfWrites::default());
        let emitted: Arc<Mutex<Vec<FsChange>>> = Arc::new(Mutex::new(Vec::new()));

        let root_cb = root.clone();
        let sw_cb = sw.clone();
        let sink = emitted.clone();
        let mut deb = new_debouncer(
            Duration::from_millis(300),
            None,
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    // Exact same path the live watcher takes.
                    if let FlushOutcome::Changes(batch) = changes_from_events(events, &root_cb, &sw_cb) {
                        sink.lock().unwrap().extend(batch);
                    }
                }
            },
        )
        .unwrap();
        deb.watch(&root, RecursiveMode::Recursive).unwrap();

        std::thread::sleep(Duration::from_millis(500)); // drain the initial create
        emitted.lock().unwrap().clear();

        // Simulate write_note: record the expected content, then atomic_write it.
        let new_content = "v2 the content after the user kept typing";
        sw.record(&file, new_content);
        let mut t = NamedTempFile::new_in(&tmpdir).unwrap();
        t.write_all(new_content.as_bytes()).unwrap();
        t.as_file().sync_all().unwrap();
        t.persist(&file).unwrap();

        std::thread::sleep(Duration::from_millis(900)); // debounce + settle

        let got = emitted.lock().unwrap();
        assert!(
            got.is_empty(),
            "atomic write leaked external change(s) — focus-loss bug would recur: {got:?}"
        );
    }

    #[test]
    fn removed_is_always_emitted() {
        let sw = SelfWrites::default();
        let change = to_fs_change(ChangeKind::Removed, &PathBuf::from("/v/gone.md"), &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Removed));
    }

    use crate::search::{IndexHandle, IndexState};
    use std::sync::Arc;

    #[test]
    fn apply_changes_to_index_upserts_and_deletes() {
        let vault = TempDir::new().unwrap();
        let idx = TempDir::new().unwrap();
        std::fs::write(vault.path().join("a.md"), "initial body").unwrap();

        let handle: Arc<IndexHandle> = Arc::new(IndexHandle::default());
        *handle.0.lock().unwrap() = Some(IndexState::open_or_create(idx.path()).unwrap());

        // created/modified → upsert
        let changes = vec![
            FsChange { kind: ChangeKind::Created, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            let st = guard.as_ref().unwrap();
            assert_eq!(st.search("body", 10).unwrap().len(), 1);
        }

        // removed → delete
        let changes = vec![
            FsChange { kind: ChangeKind::Removed, path: vault.path().join("a.md").display().to_string() },
        ];
        apply_changes_to_index(&handle, vault.path(), &changes);
        {
            let guard = handle.0.lock().unwrap();
            assert_eq!(guard.as_ref().unwrap().search("body", 10).unwrap().len(), 0);
        }
    }

    #[test]
    fn unreadable_modified_path_emits_and_forgets_stale_entry() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("vanished.md");
        let sw = SelfWrites::default();
        sw.record(&f, "was-self-written");
        // File doesn't exist so the read fails → the event passes through (Some), the registration is cleaned up.
        let change = to_fs_change(ChangeKind::Modified, &f, &sw);
        assert_eq!(change.map(|c| c.kind), Some(ChangeKind::Modified));
        // Since it was consumed by forget, the same content arriving again is not treated as a self-write.
        std::fs::write(&f, "was-self-written").unwrap();
        assert!(to_fs_change(ChangeKind::Modified, &f, &sw).is_some());
    }
}
