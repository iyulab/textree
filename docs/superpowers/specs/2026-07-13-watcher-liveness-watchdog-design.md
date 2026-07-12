# Watcher Liveness Watchdog — Design

> Windows `ReadDirectoryChangesW` overflow workaround. Detects a silently-dead
> file watcher and recovers by recreating the debouncer, reusing the existing
> rescan/rebuild recovery path.
>
> Status: design approved 2026-07-13. Next: implementation plan.
> Scope: `textree/` (Rust backend). No frontend changes.

## 1. Problem

The vault watcher (`src-tauri/src/watcher.rs`) recursively watches the vault root
via `notify-debouncer-full`. On Windows the underlying `ReadDirectoryChangesW`
buffer is a fixed 16 KB (`notify`'s `windows.rs` `BUF_SIZE`). When an external
sync client (OneDrive/Dropbox) floods the folder with changes, the buffer
overflows and the completion routine hits `ERROR_NOTIFY_ENUM_DIR` (1022), which
falls into notify's catch-all `_ =>` unknown-error arm: it **logs, calls
`unwatch()`, and returns**. The watcher is now permanently dead and **no signal
reaches either the `Ok` or `Err` debounce callback arm** — the app cannot know.

Consequence: the tree, tantivy index, and open note silently go stale with no
user-visible signal. This directly violates the data-safety / graceful-degradation
value (Constitution D19), and it is exactly the condition reproduced by the S2
dogfooding scenario ① (a OneDrive-hosted vault under active sync).

A prior slice (2026-07-05, watcher fix A+B, textree `8d9bc86`) handled the
**Ok-arm rescan** signal (Linux `Q_OVERFLOW` / macOS `MUST_SCAN_SUBDIRS`) and
tightened the Err arm. That slice left a documented gap: **Windows overflow reaches
no callback arm at all**, so an Ok-arm consumer cannot see it.

### Decision context

The gap was originally slated for an upstream `notify-rs` issue (a submission-ready
draft exists at `claudedocs/upstream-issues/notify-upstream-issue-submit-2026-07-12.md`
in the umbrella). Owner decided 2026-07-12 to **not submit** — `notify-rs` is not an
iyulab-ecosystem upstream — and instead pursue an **app-side workaround**. This is
that workaround.

## 2. Strategy

Since no callback signal arrives on a dead watcher, the app **actively probes**
liveness. It writes a small canary file into `.textree/tmp/` and expects a live
watcher to surface that write back as an event in the next debounce flush. If the
round-trip fails, the watcher is judged dead and is **recreated** (drop the
debouncer, call `start()` again). Recovery then reuses the existing
`rebuild_index` + `fs_rescan` emit path — the frontend `sync.ts` reconcile from the
A+B slice — so **no new recovery code is added; only detection is new**.

`.textree/tmp/` is the right home for the canary: it is already the atomic-write
temp dir, already sync-ignorable, already swept for orphans, and already excluded
from the tree/search/watcher-surfacing. The canary never touches a user content
folder.

## 3. Components (separation of concerns)

New file `src-tauri/src/liveness.rs` owns the watchdog. Three well-bounded units:

### 3.1 `CanaryToken` — pure
- Produces a canary path under `.textree/tmp/` keyed by a monotonic counter
  (e.g. `.textree/tmp/.watcher-canary-<n>`).
- Answers "is this observed path my canary, and which token?" — used by the
  watcher filter to recognize and suppress canary events.
- No clock, no filesystem. Fully unit-testable.

### 3.2 `LivenessState` — pure state machine
- Tracks the liveness verdict from a sequence of `arm(token)` and `observe(token)`
  calls. No clock, no filesystem, no threads.
- Transitions:
  - `arm(N)` — record that canary N was just written and is awaited.
  - `observe(N)` — the watcher callback saw canary N → the armed token is
    confirmed; reset the consecutive-miss counter.
  - `verdict()` at the next `arm` — if the previously-armed token was never
    observed, increment a consecutive-miss counter; when it reaches the threshold
    (**default 2**), return `Dead`. Otherwise `Alive`/`Pending`.
- The consecutive-miss threshold absorbs false positives from debounce latency or
  slow disks, honoring the §2.3 churn-avoidance principle from the original
  proposal (recreate only on genuine death).

### 3.3 `Watchdog` — thin orchestrator (side effects isolated)
- Wires `CanaryToken` + `LivenessState` to the real clock, a background thread,
  canary file writes, and the debouncer-recreate callback.
- Owns the arming triggers (§4) and the recreate action (§5).
- This is the only unit touching I/O; the two above stay pure.

### 3.4 `watcher.rs` — modified
- `changes_from_events` (or an adjacent filter at the same layer as `is_ignored`)
  recognizes canary paths: they must **not** leak to the frontend as `fs_changed`,
  but their observation must be forwarded to the `Watchdog` (`observe(token)`).
- On a Dead verdict, the watchdog drops the current debouncer and calls the
  existing `start()` to recreate it.
- Recovery emit path (`rebuild_index` + `fs_rescan`) is **unchanged** from A+B.

### 3.5 Reused, unchanged
- `rebuild_index(handle, root)` — full delete-all + re-index (already exists).
- `fs_rescan` Tauri event + frontend `sync.ts` reconcile (already exists).

## 4. Arming policy (probe triggers)

Two triggers, per approved decision "post-flush + long backup interval":

1. **Post-flush arm.** After each debounce callback finishes processing a
   `Changes`/`Rescan` flush, the watchdog arms a new canary. Rationale: overflow
   happens *during* a burst, so a flush is the highest-correlation signal that the
   watcher may be at risk — probe right after activity.
2. **Backup timer.** A background thread arms a canary every **5 minutes** of
   idle (no flushes). Catches death on an otherwise-quiet vault. A long interval
   keeps unsolicited traffic near zero on a healthy idle vault (minimizing
   OneDrive churn).

On a healthy vault the canary round-trips and simply re-confirms `Alive`. Under a
burst, the post-flush probe detects death within seconds; on an idle vault, within
one backup interval.

## 5. Recovery flow

```
Dead verdict
  → drop current debouncer (stops the dead watch)
  → start(app, root, self_writes, index)  // existing constructor
  → rebuild_index(index, root)             // per-path state unknowable
  → app.emit("fs_rescan", ())              // frontend reconciles (existing path)
```

Recovery is identical to the existing `FlushOutcome::Rescan` handling — the
watchdog just reaches the same recovery via a different (proactive) detection.

## 6. Error handling & edge cases

- **Canary write fails** (`.textree/tmp/` unwritable): probe is inconclusive, NOT
  a death verdict. `log::warn` and retry next interval. The watchdog must never
  destabilize the app.
- **Recreate fails** (`start()` returns `Err`): `log::warn`, keep the watchdog
  alive, retry next interval with backoff to avoid a recreate storm.
- **Canary residue**: written only to `.textree/tmp/` (already swept /
  sync-ignorable / excluded). Cleaned on recreate and on shutdown.
- **Vault switch**: replacing the watcher also replaces/cleans the watchdog
  (thread joined), mirroring how `WatcherHandle` drops the old debouncer.
- **Non-Windows**: the canary round-trip is cross-platform, but Linux/macOS already
  catch drops via the Ok-arm rescan (A slice). The watchdog is **defense-in-depth**
  there, not the primary line; Windows is the platform that genuinely depends on it.

## 7. Testing

- **Unit (cargo)** — `LivenessState` full transition coverage: `arm→observe→Alive`;
  `arm → 2 consecutive misses → Dead`; token match/mismatch. Clock- and
  FS-independent, so pure tests cover the death-judgment logic 100%.
- **Unit (cargo)** — `CanaryToken` path generation and recognition (only my canary
  is `true`; user files and other-token canaries are `false`).
- **Integration (`#[ignore]`, real OS)** — on the live debouncer, a canary write
  round-trips to `observe` (proves the healthy path). Overflow itself cannot be
  synthesized (needs an OS trigger), so recreate-on-death is exercised via a
  **manual stress** (bulk file-creation script), combined with the existing
  "sync-folder robustness stress" backlog item.
- **Gates**: `cargo test`, `npm run check` 0/0, `build`. Frontend unchanged
  (reuses `fs_rescan`), so vitest is unaffected.

## 8. Non-goals (YAGNI)

- No direct `ReadDirectoryChangesW` re-binding / notify fork (rejected option C:
  platform-specific unsafe code + notify duplication; conflicts with the
  substrate-reuse principle).
- No blind periodic full rescan (rejected option B: churns a large vault even when
  healthy; violates §2.3).
- No user-facing watchdog UI/telemetry in this slice — recovery is silent and
  reuses the existing reconcile. Surfacing "watcher was restarted" is a possible
  later addition, not part of this slice.
- No tuning UI for the interval/threshold — constants with clear rationale;
  revisit only if dogfooding shows churn or slow detection.

## 9. Open items resolved during brainstorming

- Detection mechanism: **A. canary watchdog hybrid** (over blind-periodic and
  notify-rebind).
- Probe trigger: **post-flush + 5-min backup interval**.
- Death judgment: **2 consecutive misses** (not single-miss), to absorb debounce
  latency false positives.
