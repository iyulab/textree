# Custom Title Bar + Chrome-on-Demand Sidebar — Design

**Date:** 2026-07-07
**Status:** Approved (design), pending implementation plan

## Goal

Replace the native Windows title bar with a fully custom one, and move the
sidebar's control chrome (theme toggle, settings, note toolbar) to a
hover-reveal model. This aligns the shell with the project's "Chrome on demand"
principle: operators are decoration — hidden by default, revealed on hover, with
discoverability backstopped by the command palette and keyboard shortcuts.

## Title Bar Layout

A full-width bar at the very top of the window, above the sidebar/content split.

```
┌──────────────────────────────────────────────────────────┐
│ ☷ ◈ Textree      [ ⌕ current-note or vault name ]   ─ ▢ ✕ │
└──────────────────────────────────────────────────────────┘
  ↑ left cluster       ↑ center (palette trigger)     ↑ window controls
```

- **Left cluster** (always visible):
  - `☷` sidebar collapse toggle → `layout.toggleCollapsed()` (uses existing
    `panel-left-close` / `panel-left-open` icons reflecting collapsed state).
  - `◈` app icon + `Textree` brand text. Static; not a button. Part of the drag
    region.
- **Center** — palette trigger button:
  - Shows `⌕` + the current context label: the open note's name in Note mode,
    the vault name when no note is open, and the vault name in Chat mode.
  - Click → `palette.show()` (the same entry point as `Ctrl+P`).
  - Context-label computation is a pure function (see Components).
- **Right** — window controls `─ ▢ ✕`:
  - `getCurrentWindow().minimize()`, `.toggleMaximize()`, `.close()` from
    `@tauri-apps/api/window`.
  - Maximize icon toggles between "maximize" and "restore" glyphs based on the
    window's maximized state.
- **Drag to move**: the bar background (everything except the three interactive
  clusters) carries `data-tauri-drag-region`, giving click-drag window move and
  double-click maximize for free.

## Sidebar: Chrome on Demand

The sidebar's top area is simplified and its controls become hover-revealed.

- **Removed** from `.sidebar-head`:
  - The `Textree` brand span (now in the title bar left cluster).
  - The vault-name button (vault switching now lives in the palette's
    `openVault` action and Settings — the vault name is also surfaced in the
    title bar center).
  - The collapse button (now in the title bar left cluster).
- **Hover-revealed cluster** (top of sidebar): `theme | settings`, in that
  order. `opacity: 0` at rest, fades in on sidebar hover.
  - `theme` → `theme.toggle()` (existing).
  - `settings` → `showSettings = true` (NEW button; the action already exists as
    the palette `openSettings` command and `Ctrl+,`, this just adds a visible
    affordance).
- **Note toolbar** (`.toolbar`: new note / new folder / add child / rename /
  delete) also becomes hover-revealed — hidden at rest, fades in on sidebar
  hover, per the same principle.

Keyboard/palette paths for every one of these actions remain unchanged, so
hiding them never removes reachability.

## Components

New:
- **`TitleBar.svelte`** — the custom title bar. Owns the three clusters, the
  window-control handlers, and the drag region. Props: current context label (or
  the inputs to compute it), collapse state + toggle, palette-open callback.
  Rationale: `+page.svelte` is already >1600 lines; window-control logic and
  hover/maximized state are cohesive and belong in their own unit (project's
  layer-separation rule).
- **`titlebar.helpers.ts`** — pure `contextLabel(mode, openNoteName, vaultName)`
  returning the center-button text. vitest target (pure ↔ runes separation).

Modified:
- **`+page.svelte`** — render `<TitleBar>` above the app split; strip brand /
  vault-name / collapse from `.sidebar-head`; add the hover-reveal wrapper +
  settings button; apply hover-reveal to `.toolbar`.
- **`Icon.svelte`** — add `minus` (minimize), `square` / `restore` (maximize
  toggle), and `search` (⌕) icons.
- **`tauri.conf.json`** — add `"decorations": false` to the main window.
- **`capabilities/default.json`** — add `core:window:allow-minimize`,
  `core:window:allow-maximize`, `core:window:allow-unmaximize`,
  `core:window:allow-start-dragging` (`allow-destroy` already present for
  `.close()`).

## Data Flow

```
User clicks center button ─▶ palette.show()            (existing palette store)
User clicks ☷            ─▶ layout.toggleCollapsed()   (existing layout store)
User clicks ─ ▢ ✕        ─▶ getCurrentWindow().{minimize,toggleMaximize,close}()
User hovers sidebar      ─▶ CSS :hover reveals theme|settings + toolbar
User clicks settings     ─▶ showSettings = true         (existing Settings modal)
```

No new state stores. The title bar reads existing `layout` (collapse, mode),
`theme`, and the current note/vault selection already held in `+page.svelte`.

## Error Handling

- Window-control API calls (`minimize`/`toggleMaximize`/`close`) are
  fire-and-forget; a rejected promise is caught and ignored (a failed minimize
  is non-fatal and has no meaningful user recovery). The window cannot end up in
  a stuck state from these.
- If `decorations: false` regresses edge resize / Aero Snap on the target
  Windows/WebView2 version, that surfaces during manual verification (see
  Testing); the design does not add manual resize handles unless verification
  shows they are needed.

## Testing

- **Unit (vitest)**: `titlebar.helpers.ts` `contextLabel()` across Note (with
  and without an open note) and Chat modes.
- **E2E (Playwright)**:
  - Center button click opens the palette.
  - Collapse toggle in the title bar hides/shows the sidebar.
  - Sidebar hover reveals the theme/settings cluster and toolbar (assert opacity
    / visibility transition).
  - Settings button opens the Settings modal.
  - Window controls (minimize/maximize/close) and drag are real OS window
    operations and are NOT covered by E2E; verified manually in the running app.
  - Audit existing specs (`layout.spec.ts`, palette/tree specs) for selectors
    that assume the old `.sidebar-head` structure (brand/vault-name/collapse) and
    update them.

## Out of Scope

- macOS-style overlay title bar (project is Windows-only: nsis + WebView2).
- Custom resize handles (only added if verification shows borderless regresses
  native resize).
- Any change to palette contents, Settings contents, or theme behavior beyond
  wiring the new visible affordances.
