# Task 6 Report: AskPanel — streaming Q&A, citations, scope, generation consent

## Commit
`01f62d4` — feat: AskPanel — streaming Q&A with citations, scope, generation consent

---

## Files changed

| File | Action | Summary |
|---|---|---|
| `src/lib/ipc.ts` | Modified | `hostStatus()` return type changed to `{ status: HostStatus; generatorReady: boolean }`. Added `AskEvent` union type, `ask()` wrapper (Channel-based), `prepareGeneration()`. Added `import { Channel }` and `import type { ChatMessage }` from ask.helpers. |
| `src/lib/aiConsent.ts` | Modified | Added `getGenerationConsent()` / `setGenerationConsent()` using `ai-generation-consent` localStorage key. Private-mode safe (try/catch). |
| `src/lib/askStore.svelte.ts` | Created | Runes orchestration class `AskStore` with three-gate state machine (see below), submit/cancel, streaming event handler. Exported singleton `askStore`. |
| `src/lib/AskPanel.svelte` | Created | Q&A panel — consent gate, scope toggle (node scope / whole vault), input + submit button, status display, streaming answer div, citation links. `prepareAiModel()` call on enable. Retry timer for `preparing` state. Token-only styles (no hardcoded colors/sizes). |
| `src/lib/Palette.svelte` | Modified | All `hostStatus()` call sites updated to read `.status` from the returned object (lines 103–105 and 171–174). `hostState` variable type unchanged (`HostStatus | null`). |
| `src/routes/+page.svelte` | Modified | Imported `AskPanel`; mounted as sibling of `Backlinks`/`RelatedNotes` with `vault={root!}`, `nodeScope={semanticScopePath}`, `onOpenNote={openFileFromPalette}`. |

---

## Host-spawn state machine

The brief's skeleton collapsed two distinct gates. The prompt's prose specifies three ordered gates:

**Gate 1 — host process running:**
```
const st = await hostStatus();
if (st.status !== 'ready') { this.status = 'preparing'; return; }
```
Host is still starting (embedding model loading, first-run download). Panel shows "preparing" and retries via a 2 s setTimeout in submitAndPoll().

**Gate 2 — generation model loaded:**
```
if (!st.generatorReady) {
  this.status = 'preparing';
  await prepareGeneration();  // fast 202/no-op until host is Ready
  return;
}
```
Host process is up but the generation model is not yet loaded. prepareGeneration() is a fast 202/no-op; the panel retries via the same timer.

**Gate 3 — semantic search pipeline:**
Once both gates pass: semanticSearch -> selectContext / hasUsableContext -> buildAskPrompt -> ask() streaming.

### enableGeneration() — host-spawn coupling
enableGeneration() in AskPanel.svelte does all three required steps:
1. setGenerationConsent(true) — persists generation flag
2. setAiConsent(true) — persists base AI flag (ensures auto-spawn on future sessions via +page.svelte:950)
3. void prepareAiModel() — spawns the host immediately if not already running
4. Then calls submitAndPoll() which drives through the state gates.

---

## Palette.svelte consumer fix

Palette.svelte used hostStatus() in two places, both treating the result as a bare HostStatus string. Both were updated to read .status:

Before: void hostStatus().then((s) => { hostState = s; })
After:  void hostStatus().then((s) => { hostState = s.status; })

The hostState variable itself remains typed as HostStatus | null (correct — it stores only the status string extracted from the response object).

---

## SemanticHit single source

Per the advisor's correction: SemanticHit remains defined in ipc.ts:202. ask.helpers.ts imports SemanticHit FROM ipc.ts (line 1). AskEvent in ipc.ts references the local SemanticHit. No circular runtime dependency — the type-only import cycle is erased at compile time. The brief's instruction to move SemanticHit to ask.helpers was incorrect and was NOT followed.

---

## Panel mounting

- AskPanel is imported in +page.svelte and mounted inside the {:else if activePath} note-body block as a sibling of Backlinks and RelatedNotes.
- vault={root!} — root is guaranteed non-null in this branch.
- nodeScope={semanticScopePath} — reuses the existing derived value (+page.svelte:840-846).
- onOpenNote={openFileFromPalette} — accepts vault-relative POSIX paths (the form SemanticHit.path / extractCitations produces).

---

## Check / build output

```
npm run check:
  405 FILES  0 ERRORS  0 WARNINGS  0 FILES_WITH_PROBLEMS

npm run build:
  client: built in 2.52s
  server: built in 5.11s
  adapter-static: done
  (chunk size warning is pre-existing, not introduced by this task)
```

---

---

## Review Fixes (opus reviewer — applied 2026-06-21)

### Bug 1 — `cancel()` not wired on note switch / unmount (zombie timer + stale-stream bleed)

**Problem:** `AskPanel` is not unmounted when the user switches notes — only its `nodeScope` prop changes. The singleton `askStore` and the 2 s retry `setTimeout` (`retryTimer`) kept running against the old note scope, allowing note A's answer/status/timer to bleed into the panel now showing note B. `askSeq` did not help because nothing incremented it on a prop change.

**Fix applied in `src/lib/AskPanel.svelte`:**
1. Added `import { onDestroy } from 'svelte'`.
2. Added a `$effect` that reads `nodeScope` as a dependency so it re-runs on every prop change, calling `clearRetry()` then `askStore.cancel()`.
3. Added `onDestroy(() => { clearRetry(); askStore.cancel(); })` for the true-unmount path.
4. `clearRetry()` already nulled `retryTimer` — no change needed there.

**Reasoning that note-switch bleed is closed:**
- On A→B switch: `$effect` fires because `nodeScope` changed → `clearRetry()` cancels the pending `setTimeout` → `askStore.cancel()` increments `askSeq`.
- Any in-flight `ask` stream for note A has its `onmessage` callback guard `if (seq !== this.askSeq) return;` — after the `askSeq` bump, the guard rejects every remaining token from A's stream.
- The fresh-submit path is unaffected: `submit()` does its own `const seq = ++this.askSeq` at the top, so a new question after the switch gets a new seq and proceeds normally.

### Bug 2 — `cancel()` left display state populated (stale answer visible on note switch)

**Problem:** `cancel()` bumped `askSeq` and set `status='idle'` but left `answer`, `citations`, `errorMessage`, and `question` untouched, so the panel showed the prior note's answer/question after a note switch.

**Fix applied in `src/lib/askStore.svelte.ts`:**
`cancel()` now resets all display fields:
```ts
cancel() {
  this.askSeq++;
  this.status = 'idle';
  this.question = '';
  this.answer = '';
  this.citations = [];
  this.errorMessage = '';
}
```
This gives a clean slate — after a note switch the input field and answer area are both empty, with no stale content from the previous note.

### Check / build output (post-fix)

```
npm run check:
  405 FILES  0 ERRORS  0 WARNINGS  0 FILES_WITH_PROBLEMS

npm run build:
  client: built in 2.11s
  server: built in 4.18s
  adapter-static: done
```

---

---

## Final Fix — I1: cancel_ask IPC frees host CPU on panel close / note switch (2026-06-21)

### Bug (I1 — cross-task integration gap)
`askStore.cancel()` bumped `askSeq` and cleared UI state but made no IPC call. On note-switch / panel-close, the in-flight Rust `ask` streaming loop kept running — the .NET host continued generating, burning CPU until the 120s `ureq` timeout.

### Fix (minimal — 4 touch points)

**`src-tauri/src/host.rs`** — added `cancel_ask` command immediately before `prepare_generation`:
```rust
#[tauri::command]
pub fn cancel_ask(host: State<'_, Arc<HostHandle>>) {
    host.bump_ask_generation();
}
```
Uses the SAME `bump_ask_generation()` method that `ask` calls at its start, and the same `ask_generation` counter that the streaming loop checks (`handle.ask_generation() != my_gen`). One bump causes any active `ask` loop to return early on its next line read, dropping the reader, closing the TCP connection, and triggering `RequestAborted` on the host side.

**`src-tauri/src/lib.rs`** — registered `host::cancel_ask` in `generate_handler!` alongside `host::ask`.

**`src/lib/ipc.ts`** — added:
```ts
export function cancelAsk(): Promise<void> {
  return invoke<void>('cancel_ask');
}
```

**`src/lib/askStore.svelte.ts`** — imported `cancelAsk` and added fire-and-forget call in `cancel()`:
```ts
cancel() {
  this.askSeq++;
  void cancelAsk();    // fire-and-forget: bump Rust ask_generation → in-flight ask loop aborts → host stops generating
  this.status = 'idle';
  // ... rest unchanged
}
```

### Causal chain after fix
note-switch → `AskPanel.$effect` → `askStore.cancel()` → `cancelAsk()` IPC → Rust bumps `ask_generation` → in-flight `ask` loop's `handle.ask_generation() != my_gen` check → returns early → reader dropped → TCP connection closed → .NET host sees `RequestAborted` → stops generating.

### Verification output
```
cargo test:   101 passed; 0 failed; 4 ignored  (finished in 0.23s)
cargo build:  Finished dev profile (1 warning — pre-existing HealthResponse::status dead_code)
npm run check:  405 FILES  0 ERRORS  0 WARNINGS  0 FILES_WITH_PROBLEMS
npm run build:  ✓ built in 5.74s  (adapter-static done)
```

---

## Self-review

- State machine: three distinct checks before the pipeline runs.
- Cancel safety: askSeq increments on cancel; stale streaming events are dropped at every async boundary.
- Preparing auto-retry: AskPanel submitAndPoll() sets a 2 s retry whenever askStore.status === 'preparing'. Cleared on new explicit submit.
- Styles: all CSS uses var(--token) exclusively — zero hardcoded colors or px values.
- Layer discipline: askStore.svelte.ts is a runes file (not test-importable). Pure logic stays in ask.helpers.ts. ipc.ts is the only invoke boundary.
- Language: all code, comments, and UI labels are English (public repo).
- E2E is Task 7 and is not included here.
