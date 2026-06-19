# Textree E2E (Playwright + WebView2 CDP)

Connects via CDP to the **real WebView2** of the running Tauri app. This is not mock IPC —
it goes through the actual filesystem backend, so it verifies the core value that
"the filesystem is the source of truth" exactly as it is. (CDP is **Windows/WebView2 only**.)

## Running

Two terminals are required.

**Terminal 1 — launch the app with a remote debugging port:**

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
$env:TEXTREE_CANOPY_CLI="..\canopy\dist\cli.js"   # only needed for publish.spec (build canopy first)
npm run tauri dev
```

**Terminal 2 — run the E2E suite:**

```bash
npm run test:e2e
```

## Layout

- `helpers.ts` — CDP connection / app page lookup, a dev bridge (`window.__textreeTest`:
  `loadVault` / `publishTo`) to bypass the native folder dialogs, temporary vault fixtures, and
  native DnD dispatch.

**Core (filesystem ↔ tree ↔ editor):**

- `smoke.spec.ts` — open vault, select note (depends on `sample-vault`).
- `editing.spec.ts` — editing, debounced autosave, flush on switch.
- `sync.spec.ts` — external sync (reload/create/delete, conflict banner & resolution).
- `structure.spec.ts` — structural edits (create/rename/delete/DnD/＋child/adopt, vault-switch isolation).
- `attachment.spec.ts` — image paste → save into assets/ + link.
- `tree.spec.ts` — tree expand/collapse, keyboard navigation, breadcrumb.
- `pagedetail.spec.ts` — inline title (file name) editing.
- `search.spec.ts` — full-text content search.

**UI / shell:**

- `palette.spec.ts` — unified palette (file search + command mode, depends on `sample-vault`).
- `layout.spec.ts` — sidebar collapse/expand + resize, persistence.
- `theme.spec.ts` — theme toggle + token-driven color change, persistence.
- `sidecar-ux.spec.ts` — manual ordering + favorites (star affordance and palette command) persist to the sidecar.

**Editor rendering (pretty-by-default):**

- `livepreview.spec.ts` — inline heading/emphasis/code render + marker hide/reveal.
- `frontmatter.spec.ts` — frontmatter page header (title/icon) + editor folding pill.
- `reading.spec.ts` — reading-view toggle (markers hidden, editor read-only).
- `wikilink.spec.ts` — wikilink render/click navigation, backlinks panel, `[[` autocomplete.

**Publishing:**

- `publish.spec.ts` — publish the vault to a static site via canopy (auto-theming tokens, source
  `.md` byte-unchanged, self-host banner). Requires the app to be launched with
  `TEXTREE_CANOPY_CLI` set to canopy's CLI (e.g. `../canopy/dist/cli.js`) so the backend can spawn it.

  > The dev E2E drives publish via `TEXTREE_CANOPY_CLI` (the dev resolution branch). The **production**
  > path — the bundled `node + cli.js` sidecar under the resource dir — is guarded by the Rust
  > integration test `run_publish_via_assembled_sidecar` (run after assembling the payload; CI runs it
  > in the release build). See `scripts/assemble-canopy-sidecar.ps1`.

## Notes

- `smoke.spec.ts` and `palette.spec.ts` depend on a local `sample-vault/` (gitignored). The rest
  create and clean up isolated fixture vaults under the OS temp directory, so they are self-contained.
- Specs share one running app instance, so app-level state (theme, reading mode) can leak between
  files; tests that care normalize it at their start.
- The dev bridge is guarded by `import.meta.env.DEV`, so it is tree-shaken out of production bundles.
