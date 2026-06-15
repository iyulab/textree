# Textree E2E (Playwright + WebView2 CDP)

Connects via CDP to the **real WebView2** of the running Tauri app. This is not mock IPC —
it goes through the actual filesystem backend, so it verifies the core value that
"the filesystem is the source of truth" exactly as it is. (CDP is **Windows/WebView2 only**.)

## Running

Two terminals are required.

**Terminal 1 — launch the app with a remote debugging port:**

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
npm run tauri dev
```

**Terminal 2 — run the E2E suite:**

```bash
npm run test:e2e
```

## Layout

- `helpers.ts` — CDP connection / app page lookup, a dev bridge (`window.__textreeTest.loadVault`)
  to bypass the native folder dialog, temporary vault fixtures, and native DnD dispatch.
- `smoke.spec.ts` — open vault, select note (depends on `sample-vault`).
- `editing.spec.ts` — M2 editing, debounced autosave, flush on switch.
- `sync.spec.ts` — M3 external sync (reload/create/delete, conflict banner & resolution).
- `structure.spec.ts` — M4 structural edits (create/rename/delete/DnD/＋child/adopt, vault-switch isolation).
- `attachment.spec.ts` — M5 image paste → save into assets/ + link.

## Notes

- Only `smoke.spec.ts` depends on a local `sample-vault/` (gitignored). The rest create and
  clean up isolated fixture vaults under the OS temp directory, so they are self-contained.
- The dev bridge is guarded by `import.meta.env.DEV`, so it is tree-shaken out of production bundles.
