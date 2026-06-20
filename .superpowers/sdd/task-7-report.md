# Task 7: Semantic palette mode — Implementation Report

## Files Changed

- `src/lib/palette.helpers.ts` — added `"semantic"` to `PaletteMode` type; added `? prefix` branch to `paletteMode()`; extended `paletteListState()` to return `"searching"` for semantic mode in-flight queries.
- `src/lib/palette.helpers.test.ts` — appended `describe("paletteMode semantic", ...)` block (3 new tests, verbatim from brief).
- `src/lib/Palette.svelte` — added `vaultRoot`/`scopePath` props; imported `semanticSearch`, `hostStatus`, `SemanticHit`, `HostStatus` from `./ipc`; added `semanticHits` and `hostState` state; added semantic `$effect` (debounced, race-guarded, host-gated); updated `count` derived; updated `commit()`; updated placeholder; added semantic results section with graceful degradation row; added `.ai-unavailable` CSS.
- `src/routes/+page.svelte` — added `semanticScopePath` derived; expanded `openFileFromPalette` to reconcile vault-relative POSIX paths (from sidecar) to absolute `TreeNode.path`; passed `vaultRoot={root}` and `scopePath={semanticScopePath}` to `<Palette>`.

## RED→GREEN Evidence

**RED** (tests before implementing `paletteMode` semantic branch):
```
2 failed: 16 passed (18)
× paletteMode semantic > treats ? prefix as semantic
  → expected 'file' to be 'semantic'
× paletteMode semantic > strips ? for the term
  → expected '?ideas' to be 'ideas'
```

**GREEN** (after adding `if (query.startsWith("?")) return "semantic";`):
```
18 passed (18)
Duration 376ms
```

## npm run check Output

```
0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS
```
(394 files checked)

## vitest Output (full suite)

```
14 test files, 185 tests passed
```

## scopePath Derivation

In `+page.svelte`, `semanticScopePath` is `$derived`:
- `selectedNode?.kind === "container"` → `selectedNode.path` (the selected folder)
- else `activePath` is non-null → `parentDir(activePath)` (parent folder of the open note)
- else `null` (no restriction, whole vault)

This is passed as `scopePath` to `<Palette>`, which forwards it to `semanticSearch(vault, term, scope, 20)`.

## Path Reconciliation (semantic hit paths)

Inspected `src-tauri/src/host.rs`: `parse_search_response` maps the sidecar JSON directly, and the test shows `hits[0].path == "a.md"` — vault-relative POSIX, same format as `searchContent`. The existing `openFileFromPalette` used `findNode(tree, path)` which only matches absolute paths. Updated it to:
1. Try exact match first (handles file/content modes, absolute paths).
2. On miss, build a `byRel` map of vault-relative POSIX → absolute `TreeNode.path` (same precedent as `searchContentFromPalette`), then look up the hit's path. Falls back to the raw path if it already starts with the vault root (forward-compat).

## Not-Ready State Rendering

The semantic `$effect` polls `hostStatus()` once per query change. `hostState` is `$state<HostStatus | null>`. A companion derived `aiNotReady` is true when `palette.mode === "semantic" && hostState !== null && hostState !== "ready"`.

In the template, the semantic branch shows the muted row when `aiNotReady` is true:
```svelte
{#if hostState !== null && hostState !== "ready"}
  <li class="status-row ai-unavailable" role="status">
    {hostState === "starting" ? "Local AI is indexing…" : "Local AI is unavailable"}
  </li>
```
The generic status-row block (Searching… / No matches found) is gated by `!aiNotReady`:
```svelte
{#if !aiNotReady && listState === "searching"}
  ...
{:else if !aiNotReady && listState === "no-results"}
  ...
```
This prevents the two rows rendering simultaneously when the host is not ready (the generic row was suppressed; only the AI-status row shows). When `hostState` is `null` (initial, not yet fetched) or `"ready"`, results render normally. Styling: `.ai-unavailable` adds `font-style: italic` on top of `.status-row`'s muted `var(--text-muted)` color. No error, no crash.

Note: the Rust `semantic_search` command also returns an empty array (not an error) when host is not ready, so the `catch` path in the effect is a belt-and-suspenders guard for network errors.

## Commits

```
a329a41 feat(palette): semantic search mode (?) scoped by the current tree node
23e09e5 fix(palette): suppress double status rows when AI sidecar is not ready
```

## Concerns

None blocking. One observation: the `paletteListState` test suite does not cover `mode === "semantic"` and `searching: true` returning `"searching"` (the new branch). The existing tests still pass and no new test for `paletteListState` was requested by the brief; Task 8/review can add one if desired.

---

## Post-Review Fix Report (code-review findings, applied 2026-06-20)

### Fix 1 — Important: graceful-degradation gap on IPC rejection (`src/lib/Palette.svelte`)

**Problem:** `hostStatus()` had no `.catch`, so if `invoke` rejected (app shutdown / bridge error), `hostState` stayed `null` and the "Local AI unavailable" row never appeared — a graceful-degradation failure.

**Change (`src/lib/Palette.svelte`, line 105):**
```ts
// Before
void hostStatus().then((s) => { hostState = s; });

// After
void hostStatus()
  .then((s) => { hostState = s; })
  .catch(() => { hostState = "unavailable"; });
```

### Fix 2 — Minor: missing test coverage for semantic in-flight state (`src/lib/palette.helpers.test.ts`)

**Problem:** `paletteListState` had a `mode === "semantic" && searching` branch returning `"searching"` with no test.

**Change (added to `describe("paletteListState")`):**
```ts
it("reports searching for semantic mode in-flight", () => {
  expect(paletteListState({ mode: "semantic", term: "ideas", count: 0, searching: true })).toBe("searching");
});
```

### Verification

**`npx vitest run src/lib/palette.helpers.test.ts`:**
```
19 passed (19)   ← was 18; new test green
Duration 362ms
```

**`npm run check`:**
```
394 FILES   0 ERRORS   0 WARNINGS   0 FILES_WITH_PROBLEMS
```
