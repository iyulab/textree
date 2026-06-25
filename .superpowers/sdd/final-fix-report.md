# Bug B — Cold Model Download Progress: Final Fix Report

**Branch:** `feat/cold-model-download-progress`
**Date:** 2026-06-25

---

## 1. Problem

`Palette.svelte`'s AI badge progress bar was static: the `$effect` that called `hostStatus()` ran once on entry and re-ran only when `palette.mode` or `palette.term` changed. In the cold-start case (new user waiting for the embedder model download, NOT typing), `hostState` and `modelDownload` froze at the first-sampled value — the bar never advanced.

---

## 2. Palette.svelte — Live-Poll Implementation

### Approach

Added a **dedicated `$effect`** for host polling (deps: `palette.mode`, `consent`), separate from the search `$effect`. This avoids restarting the poll on every keystroke (the search effect re-runs on `palette.term` changes).

The old one-shot `void hostStatus().then.catch` that was inside the search `$effect` was removed. The dedicated polling effect now owns all `hostState`/`modelDownload` writes.

### Cancelled-flag Pattern

A bare `return () => clearTimeout(timer)` is insufficient: if cleanup fires while `hostStatus()` is still in-flight, `clearTimeout` clears nothing, and when the promise resolves it arms a new timer that no cleanup owns — orphaned forever-poll. The fix uses a `cancelled` flag:

```ts
let cancelled = false;
let timer: ReturnType<typeof setTimeout> | null = null;

function poll() {
  void hostStatus()
    .then((s) => {
      if (cancelled) return;           // guard in .then
      hostState = s.status;
      modelDownload = s.embedderDownload ?? s.generatorDownload ?? null;
      // Reschedule only while the badge is still in the preparing state.
      if (resolveSemanticAiUi(_consent, s.status) === "preparing") {
        timer = setTimeout(poll, 2000);
      }
    })
    .catch(() => {
      if (cancelled) return;           // guard in .catch
      hostState = "unavailable";
      modelDownload = null;
    });
}

poll();

return () => {
  cancelled = true;
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
};
```

Cleanup sets `cancelled = true` first, then clears any armed timer. Both `.then` and `.catch` check `cancelled` before writing state or arming another timer. **No leaked timers. No infinite loop.**

### Poll Reschedule Gate

The reschedule condition is `resolveSemanticAiUi(_consent, s.status) === "preparing"` — uses the pure helper (same function `aiUi` is derived from) against the **fresh** snapshot, not the `$state`. The loop stops automatically on `status === "ready"` (maps to "ready") or `status === "unavailable"` (maps to "unavailable" or "prompt"), which covers all non-preparing states.

### Stale-Clear

`modelDownload = s.embedderDownload ?? s.generatorDownload ?? null;`

The trailing `?? null` ensures the value is always `null` (not `undefined`) when neither download is active — no stale progress bar lingers after the host reports ready.

### Consent Capture

`const _consent = consent;` at effect entry captures the current consent value into the closure. This means:
- The effect re-runs when `enableAi()` flips `consent` (since `consent` is read in the effect body).
- The poll closure uses a stable snapshot of consent for its lifetime — no effect self-trigger from `modelDownload`/`hostState` writes.

---

## 3. host.rs — Comment

In `poll_health`, after `let any_dl = ...` and before `std::thread::sleep(poll_interval(ready, any_dl))`, added:

```rust
// When a generator download begins while ready==true, any_dl was false on this iteration,
// so the sleep here can last up to one READY_POLL_INTERVAL (≤10s) before the cadence drops
// to 1s; the generic "preparing" UI in the front-end covers that brief window.
```

This documents the one-interval lag that wasn't previously called out (the existing comment covered only the startup/first-iteration case, not the ready+generator-starts case). No logic change.

---

## 4. Gate Outputs

| Gate | Result |
|------|--------|
| `npm run check` | 0 errors, 0 warnings (412 files) |
| `npm run test:unit` | 249 passed (20 test files) — no regression |
| `npm run build` | ✓ built in 3.98s (pre-existing chunk-size warning, not new) |
| `cargo +stable-x86_64-pc-windows-msvc check` | Finished dev — 1 pre-existing warning (`status` field unused in `HealthResponse`), not introduced by this change |

---

## 5. Self-Review: No Leaked Timer / No Infinite Loop

- **No leaked timer:** `cancelled = true` is set in cleanup before `clearTimeout`. Any in-flight promise that resolves after cleanup sees `cancelled` and returns immediately without arming a new timer.
- **No infinite loop:** The reschedule only fires inside `.then`, guarded by `if (resolveSemanticAiUi(_consent, s.status) === "preparing")`. Once the host reaches `ready` or `unavailable`, the condition is false and no further timer is armed.
- **No self-trigger:** The effect reads `palette.mode` and `consent` (reactive reads). It writes `hostState` and `modelDownload` (reactive writes). Svelte 5 `$effect` does not re-run from writes inside itself — only from tracked reads — so writing `hostState` does not restart the effect.
- **No double-poll:** The old one-shot call in the search effect was removed; only the dedicated polling effect calls `hostStatus()`.
- **Cleanup on mode change:** When `palette.mode !== "semantic"`, the effect cleanup runs (cancelled = true, timer cleared), and `hostState`/`modelDownload` are set to `null` — no stale values.

---

## 6. Files Changed

- `src/lib/Palette.svelte` — replaced one-shot hostStatus call with self-rescheduling poll effect (cancelled-flag pattern, 2s interval, stops on non-preparing)
- `src-tauri/src/host.rs` — added one comment line in `poll_health` about the ready+generator-starts lag window
