# Textree E2E (Playwright + WebView2 CDP)

Connects via CDP to the **real WebView2** of the running Tauri app. This is not mock IPC —
it goes through the actual filesystem backend, so it verifies the core value that
"the filesystem is the source of truth" exactly as it is. (CDP is **Windows/WebView2 only**.)

## Running

Two terminals are required.

**Terminal 1 — launch the app with a remote debugging port:**

```powershell
$env:TEXTREE_CANOPY_CLI=(Resolve-Path "..\canopy\dist\cli.js").Path   # only needed for publish.spec (build canopy first)
npm run dev:e2e
```

`TEXTREE_CANOPY_CLI` must be an **absolute** path (hence `Resolve-Path`): the backend passes it
to `node` as-is, and under `tauri dev` the app process's working directory is `src-tauri/`, so a
path relative to this folder silently fails to spawn and publish reports
"The publishing tool couldn't finish".

`dev:e2e` runs `scripts/dev-e2e.mjs`, which starts `tauri dev` with the overlay
`src-tauri/tauri.e2e.conf.json`: the overlay injects `--remote-debugging-port` via the window's
`additionalBrowserArgs` (the programmatic `CoreWebView2EnvironmentOptions` path).

The launcher owns the port. It uses 9222 when free; when something else already holds 9222 (a
browser or an Electron app with remote debugging on), it picks a free port instead and says so.
Set `TEXTREE_E2E_CDP_PORT` to demand a specific port — the launcher then fails immediately, naming
the occupant, if that port is taken. The chosen endpoint is written to `textree-e2e-cdp.json` in the
temp directory, and `e2e/helpers.ts` reads it, so the specs need no configuration of their own.
`TEXTREE_E2E_CDP` still overrides the endpoint the specs connect to.

The launcher also points `TEXTREE_DEFAULT_VAULT_BASE` at a temp directory it recreates on every
start. Specs that exercise the first-run flow open whatever folder the app resolves as its default
home; without that isolation it is the developer's real Documents folder, and those specs end up
asserting against personal notes. Start the app any other way and they fail by name (see
`expectIsolatedDefaultVault` in `onboarding.spec.ts`) rather than as a puzzling mismatch.

> ⚠️ The old `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` route no longer works — WebView2
> Runtime 150+ ignores the loader environment variable, so the CDP port silently never opens.
> Setting `additionalBrowserArgs` replaces wry's default arguments, which is why the overlay
> restates `--disable-features=...` and `--autoplay-policy=...` alongside the debug port.
> The overlay must also restate the full window object: `--config` merges via JSON Merge
> Patch (RFC 7396), which replaces the `app.windows` array wholesale — keep it in sync with
> `tauri.conf.json` if the main window config changes.

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
