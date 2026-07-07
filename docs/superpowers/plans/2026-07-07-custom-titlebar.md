# Custom Title Bar + Chrome-on-Demand Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the native Windows title bar with a custom full-width bar (sidebar toggle + brand, center palette trigger, right-side window controls) and move the sidebar's control chrome to hover-reveal.

**Architecture:** A new `TitleBar.svelte` renders above the sidebar/content split and calls the existing `layout`, `theme`, and `palette` stores plus `@tauri-apps/api/window` for window controls. A pure `titlebar.helpers.ts` computes the center label. `+page.svelte` strips brand/vault-name/collapse from the sidebar head and applies hover-reveal CSS to the remaining controls. `tauri.conf.json` disables native decorations; `capabilities/default.json` grants the window-control permissions.

**Tech Stack:** Svelte 5 (runes), TypeScript, Tauri 2 (`@tauri-apps/api/window`), Vitest, Playwright (WebView2 CDP).

## Global Constraints

- Public repo (`iyulab/textree`): all content (code comments, commit messages, UI copy) in **English**.
- Styling: use `var(--token)` from `styles/tokens.css` only — zero hardcoded colors/spacing.
- Layer separation: components (`*.svelte`) → runes stores (`*.svelte.ts`) → pure helpers (`*.helpers.ts`). Tests import only pure helpers, never runes modules.
- All Rust/IPC unaffected here; this is a frontend + config change.
- Gate before merge: `npm run check` (0 errors, 0 warnings), `npm run test:unit`, E2E, build.
- Commits: Conventional Commits. Do NOT push (push is manual).
- Platform: Windows-only (nsis + WebView2).

---

### Task 1: Add window-control and search icons to Icon.svelte

**Files:**
- Modify: `src/lib/Icon.svelte:16-45` (the `PATHS` object)

**Interfaces:**
- Produces: four new icon names usable as `<Icon name="..." />`: `"minus"`, `"square"`, `"restore"`, `"search"`.

- [ ] **Step 1: Add the four icons to the PATHS object**

In `src/lib/Icon.svelte`, add these four entries inside the `PATHS` object (place them after the `"message-square"` entry, before the closing `};`). The geometry follows the existing Lucide 24px/2px-stroke family:

```js
    minus: '<path d="M5 12h14"/>',
    square: '<rect width="18" height="18" x="3" y="3" rx="2"/>',
    restore:
      '<rect width="14" height="14" x="3" y="7" rx="2"/><path d="M7 7V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-2"/>',
    search:
      '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
```

(Add a comma after `"message-square"`'s value if it is the last entry — verify the object stays valid.)

- [ ] **Step 2: Verify type-check passes**

Run: `npm run check`
Expected: `0 errors, 0 warnings`. The new names are now part of `keyof typeof PATHS`.

- [ ] **Step 3: Commit**

```bash
git add src/lib/Icon.svelte
git commit -m "feat: add minus, square, restore, search icons for the title bar"
```

---

### Task 2: Pure helper for the center context label

**Files:**
- Create: `src/lib/titlebar.helpers.ts`
- Test: `src/lib/titlebar.helpers.test.ts`

**Interfaces:**
- Produces: `contextLabel(mode: "note" | "chat", noteName: string, vaultName: string): string`
  - Note mode with a non-blank `noteName` → returns `noteName`.
  - Otherwise (Note mode with no open note, or Chat mode) → returns `vaultName`.

- [ ] **Step 1: Write the failing test**

Create `src/lib/titlebar.helpers.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { contextLabel } from "./titlebar.helpers";

describe("contextLabel", () => {
  it("shows the open note name in Note mode", () => {
    expect(contextLabel("note", "My Note", "my-vault")).toBe("My Note");
  });

  it("falls back to the vault name when no note is open in Note mode", () => {
    expect(contextLabel("note", "", "my-vault")).toBe("my-vault");
  });

  it("treats a whitespace-only note name as no note", () => {
    expect(contextLabel("note", "   ", "my-vault")).toBe("my-vault");
  });

  it("shows the vault name in Chat mode even with a note open", () => {
    expect(contextLabel("chat", "My Note", "my-vault")).toBe("my-vault");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/lib/titlebar.helpers.test.ts`
Expected: FAIL — cannot resolve `./titlebar.helpers` (module not found).

- [ ] **Step 3: Write the minimal implementation**

Create `src/lib/titlebar.helpers.ts`:

```ts
// Pure helper for the title bar's center button label. Kept runes-free so vitest can import it
// (pure ↔ runes separation). Note mode shows the open note; everything else shows the vault.
export function contextLabel(
  mode: "note" | "chat",
  noteName: string,
  vaultName: string,
): string {
  if (mode === "note" && noteName.trim().length > 0) return noteName;
  return vaultName;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/titlebar.helpers.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/lib/titlebar.helpers.ts src/lib/titlebar.helpers.test.ts
git commit -m "feat: add contextLabel helper for the title bar center button"
```

---

### Task 3: TitleBar.svelte component

**Files:**
- Create: `src/lib/TitleBar.svelte`

**Interfaces:**
- Consumes: `Icon.svelte` (names `panel-left-close`, `panel-left-open`, `search`, `minus`, `square`, `restore`, `x`); `@tauri-apps/api/window` `getCurrentWindow`.
- Produces: a component with this prop contract:
  ```ts
  {
    label: string;              // center button text; falls back to "Textree" if blank
    collapsed: boolean;         // sidebar collapsed state (drives the toggle icon)
    onToggleSidebar: () => void;
    onOpenPalette: () => void;
  }
  ```

- [ ] **Step 1: Create the component**

Create `src/lib/TitleBar.svelte`:

```svelte
<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";

  let {
    label,
    collapsed,
    onToggleSidebar,
    onOpenPalette,
  }: {
    label: string;
    collapsed: boolean;
    onToggleSidebar: () => void;
    onOpenPalette: () => void;
  } = $props();

  // The maximize/restore glyph tracks the window's maximized state. Refreshed on mount and
  // whenever a control is used; also cheap to re-read on the resize event Tauri surfaces.
  let maximized = $state(false);
  const appWindow = getCurrentWindow();

  async function refreshMaximized() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* non-fatal — glyph just stays as-is */
    }
  }

  $effect(() => {
    void refreshMaximized();
    // Keep the glyph correct when the user maximizes via drag-to-top or OS shortcuts.
    const un = appWindow.onResized(() => void refreshMaximized());
    return () => { void un.then((f) => f()); };
  });

  // Window controls are fire-and-forget: a rejected promise has no meaningful user recovery
  // (a failed minimize is a no-op), so swallow it rather than surfacing an error.
  function minimize() { void appWindow.minimize().catch(() => {}); }
  function toggleMax() { void appWindow.toggleMaximize().catch(() => {}).then(refreshMaximized); }
  function close() { void appWindow.close().catch(() => {}); }
</script>

<!-- data-tauri-drag-region on the bar background makes empty space a window-move handle
     (double-click maximizes). The three clusters below stop that by being real buttons. -->
<div class="titlebar" data-tauri-drag-region>
  <div class="tb-left">
    <button
      class="tb-icon-btn"
      onclick={onToggleSidebar}
      title={collapsed ? "Show sidebar" : "Hide sidebar"}
      aria-label={collapsed ? "Show sidebar" : "Hide sidebar"}
    ><Icon name={collapsed ? "panel-left-open" : "panel-left-close"} /></button>
    <span class="tb-brand">Textree</span>
  </div>

  <button class="tb-center" onclick={onOpenPalette} title="Search or run a command (Ctrl+P)">
    <Icon name="search" />
    <span class="tb-context">{label || "Textree"}</span>
  </button>

  <div class="tb-right">
    <button class="tb-win-btn" onclick={minimize} title="Minimize" aria-label="Minimize"
      ><Icon name="minus" /></button>
    <button class="tb-win-btn" onclick={toggleMax}
      title={maximized ? "Restore" : "Maximize"} aria-label={maximized ? "Restore" : "Maximize"}
      ><Icon name={maximized ? "restore" : "square"} /></button>
    <button class="tb-win-btn tb-close" onclick={close} title="Close" aria-label="Close"
      ><Icon name="x" /></button>
  </div>
</div>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    height: 36px;
    flex-shrink: 0;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    user-select: none;
    -webkit-user-select: none;
  }
  .tb-left,
  .tb-right {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 0 var(--sp-2);
  }
  .tb-left { flex: 1; }
  .tb-right { flex: 1; justify-content: flex-end; }
  .tb-brand {
    font-size: var(--font-size-small);
    font-weight: 600;
    color: var(--text-normal);
  }
  .tb-icon-btn,
  .tb-win-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 28px;
    color: var(--text-muted);
    background: transparent;
    border: none;
    border-radius: var(--radius-s);
    cursor: pointer;
  }
  .tb-icon-btn:hover,
  .tb-win-btn:hover { background: var(--bg-hover); color: var(--text-normal); }
  .tb-close:hover { background: var(--text-error); color: var(--text-on-accent); }
  .tb-center {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    max-width: 40%;
    padding: var(--sp-1) var(--sp-3);
    color: var(--text-muted);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    font: inherit;
    font-size: var(--font-size-smaller);
    cursor: pointer;
  }
  .tb-center:hover { background: var(--bg-hover); }
  .tb-context {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
```

- [ ] **Step 2: Verify type-check passes**

Run: `npm run check`
Expected: `0 errors, 0 warnings`.

> Note: if `--token` names used here (`--bg-hover`, `--text-error`, `--text-on-accent`, `--radius-s`, `--sp-1/2/3`, `--font-size-small/smaller`) are absent from `styles/tokens.css`, substitute the nearest existing token — grep `styles/tokens.css` first. Do not hardcode values.

- [ ] **Step 3: Commit**

```bash
git add src/lib/TitleBar.svelte
git commit -m "feat: add custom TitleBar component with palette trigger and window controls"
```

---

### Task 4: Wire TitleBar into the page and make sidebar chrome hover-revealed

**Files:**
- Modify: `src/routes/+page.svelte` (imports ~line 43-53; app markup ~line 1231-1256; toolbar ~line 1258-1273; styles for `.sidebar-head`/`.toolbar`)

**Interfaces:**
- Consumes: `TitleBar` (Task 3), `contextLabel` (Task 2), existing `layout`, `theme`, `palette`, `root`, `activeName`, `vaultName`.

- [ ] **Step 1: Import TitleBar and contextLabel**

In `src/routes/+page.svelte`, add near the other `$lib` imports (after line 52 `import Palette ...`):

```ts
  import TitleBar from "$lib/TitleBar.svelte";
  import { contextLabel } from "$lib/titlebar.helpers";
```

- [ ] **Step 2: Render TitleBar above the app split**

Find the app root open tag (line 1231):

```svelte
<div class="app" style="--sidebar-width: {layout.width}px">
```

Replace it with a wrapper that stacks the title bar above the existing split. Change it to:

```svelte
<div class="shell">
  <TitleBar
    label={contextLabel(layout.mode, activeName, root ? vaultName(root) : "")}
    collapsed={layout.collapsed}
    onToggleSidebar={() => layout.toggleCollapsed()}
    onOpenPalette={() => palette.show()}
  />
  <div class="app" style="--sidebar-width: {layout.width}px">
```

Then find the matching closing `</div>` of `.app` (it is the `</div>` that currently closes the `class="app"` block — after `<main class="content"> ... </main>` and any collapsed-sidebar expand button). Add one extra `</div>` to close the new `.shell` wrapper. Verify brace/tag balance with `npm run check` in Step 8.

- [ ] **Step 3: Remove brand / vault-name / collapse from the sidebar head**

Find the `.sidebar-head` block (lines 1234-1256). Replace the whole `<div class="sidebar-head"> ... </div>` with a hover-reveal cluster holding only theme + settings:

```svelte
    <div class="sidebar-head">
      <button
        class="icon-btn"
        onclick={() => theme.toggle()}
        title={theme.resolved === "dark" ? "Switch to light theme" : "Switch to dark theme"}
        aria-label="Toggle theme"
      ><Icon name={theme.resolved === "dark" ? "sun" : "moon"} /></button>
      <button
        class="icon-btn"
        onclick={() => (showSettings = true)}
        title="Settings"
        aria-label="Settings"
      ><Icon name="folder" /></button>
    </div>
```

> The settings icon uses `folder` as a placeholder only if no gear icon exists. Grep `Icon.svelte` for a gear/`settings`/`cog` entry; if absent, add a `settings` icon (Lucide gear) to `PATHS` in this step:
> ```js
>     settings:
>       '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
> ```
> and use `<Icon name="settings" />` instead of `folder`.

- [ ] **Step 4: Make the sidebar head + toolbar hover-revealed**

In the `<style>` block, find `.sidebar-head` and `.toolbar` rules. Add opacity-transition rules so they are hidden at rest and revealed when the sidebar is hovered or focused within. Add (or merge into existing rules):

```css
  .sidebar-head,
  .toolbar {
    opacity: 0;
    transition: opacity 0.15s ease;
  }
  .sidebar:hover .sidebar-head,
  .sidebar:hover .toolbar,
  .sidebar:focus-within .sidebar-head,
  .sidebar:focus-within .toolbar {
    opacity: 1;
  }
```

> `:focus-within` keeps the controls reachable by keyboard (Tab into the sidebar reveals them), preserving accessibility while hidden visually.

- [ ] **Step 5: Confirm the collapsed-sidebar expand button still works**

The title bar's sidebar toggle now handles expand-when-collapsed (it is always visible). The old content-side expand button (around line 1557) is now redundant. Remove that button block:

```svelte
    <button
      ...
      onclick={() => layout.toggleCollapsed()}
      title="Expand sidebar"
      aria-label="Expand sidebar"
    >...</button>
```

Grep for `Expand sidebar` to locate the exact block and delete it (and any now-unused CSS/`{#if layout.collapsed}` wrapper that only held it).

- [ ] **Step 6: Add a `.shell` style**

In the `<style>` block add:

```css
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
```

And ensure `.app` no longer sets `height: 100vh` on its own if it did — the shell owns the full height now and `.app` should `flex: 1; min-height: 0;`. Grep `.app {` and adjust: keep its existing `display`/layout but change any `height: 100vh` to `flex: 1; min-height: 0;`.

- [ ] **Step 7: Manual smoke — run the app**

Run (terminal 1): `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev`
Verify visually:
- Custom title bar renders at the top; native bar is still present too (decorations not yet disabled — that is Task 5). This is expected mid-plan.
- Center button shows the vault name (no note) / note name (note open); clicking it opens the palette.
- Sidebar controls (theme, settings, toolbar) are hidden until you hover the sidebar, then fade in.
- Settings button opens the Settings modal.
- Title bar sidebar toggle collapses and re-expands the sidebar.

- [ ] **Step 8: Type-check and commit**

Run: `npm run check`
Expected: `0 errors, 0 warnings`.

```bash
git add src/routes/+page.svelte src/lib/Icon.svelte
git commit -m "feat: render TitleBar and make sidebar chrome hover-revealed"
```

---

### Task 5: Disable native decorations and grant window-control permissions

**Files:**
- Modify: `src-tauri/tauri.conf.json:13-21` (the window definition)
- Modify: `src-tauri/capabilities/default.json:6-13` (permissions array)

**Interfaces:**
- Consumes: the `TitleBar` window controls from Task 3 (they become the only way to min/max/close once native decorations are off).

- [ ] **Step 1: Disable decorations**

In `src-tauri/tauri.conf.json`, add `"decorations": false` to the window object (after `"center": true`):

```json
      {
        "title": "Textree",
        "width": 1100,
        "height": 740,
        "minWidth": 640,
        "minHeight": 480,
        "center": true,
        "decorations": false
      }
```

- [ ] **Step 2: Grant window-control permissions**

In `src-tauri/capabilities/default.json`, add the permissions the title bar uses to the `permissions` array (keep existing entries):

```json
  "permissions": [
    "core:default",
    "opener:default",
    "dialog:allow-open",
    "core:window:allow-destroy",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-is-maximized",
    "core:window:allow-close",
    "core:window:allow-start-dragging",
    "updater:default",
    "process:allow-restart"
  ]
```

> If `npm run tauri dev` logs a "permission ... not allowed" error at runtime for a control, the message names the exact permission string to add here. Adjust to match verbatim.

- [ ] **Step 3: Manual verification — the borderless window**

Run: `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev`
Verify in the running app:
- No native Windows title bar; only the custom one.
- Minimize / maximize (glyph toggles to restore) / close all work from the right cluster.
- Dragging the empty title-bar background moves the window; double-clicking it maximizes.
- Window is still resizable from its edges and supports Aero Snap (drag to screen edge).
  - **If edge resize or Snap is broken** on this WebView2 version: STOP and report. The fallback (adding manual resize handles or `"decorations"` alternatives) is a design amendment, not part of this task.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat: disable native decorations, grant window-control permissions"
```

---

### Task 6: Audit and update E2E selectors + add hover/palette regression

**Files:**
- Modify: `e2e/layout.spec.ts` (and any spec asserting old `.sidebar-head` structure)
- Test: `e2e/layout.spec.ts` (new assertions)

**Interfaces:**
- Consumes: the running app with the new title bar (manual `npm run tauri dev` with CDP, per CLAUDE.md E2E setup).

- [ ] **Step 1: Find specs coupled to the old structure**

Run: `git grep -n "vault-name\|vault-label\|Expand sidebar\|Collapse sidebar\|sidebar-head\|brand" e2e/`
For each hit, determine whether the selector still exists after Task 4. Update or remove assertions that referenced the removed brand span, vault-name button, or the old collapse/expand buttons. Point sidebar-collapse assertions at the new title-bar toggle (`aria-label="Hide sidebar"` / `"Show sidebar"`).

- [ ] **Step 2: Add a title-bar palette-trigger test**

Add to `e2e/layout.spec.ts` (adapt to the file's existing `test`/`connectToApp`/`loadVault` helpers — read the file's top for the exact pattern):

```ts
test("title bar center button opens the command palette", async () => {
  // (load a vault as the file's other tests do)
  await page.getByRole("button", { name: /Search or run a command/i }).click();
  await expect(page.locator(".palette")).toBeVisible({ timeout: 3_000 });
});
```

> Confirm the palette root selector (`.palette` or similar) by reading `src/lib/Palette.svelte`; use whatever class/role that component exposes.

- [ ] **Step 3: Add a hover-reveal test**

```ts
test("sidebar controls are hidden until the sidebar is hovered", async () => {
  const themeBtn = page.getByRole("button", { name: /Toggle theme/i });
  // Hidden at rest (opacity 0 on the containing .sidebar-head).
  await expect(page.locator(".sidebar-head")).toHaveCSS("opacity", "0");
  await page.locator(".sidebar").hover();
  await expect(page.locator(".sidebar-head")).toHaveCSS("opacity", "1");
  await expect(themeBtn).toBeVisible();
});
```

- [ ] **Step 4: Add a sidebar-toggle test via the title bar**

```ts
test("title bar toggle collapses and expands the sidebar", async () => {
  await expect(page.locator(".sidebar")).toBeVisible();
  await page.getByRole("button", { name: /Hide sidebar/i }).click();
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await page.getByRole("button", { name: /Show sidebar/i }).click();
  await expect(page.locator(".sidebar")).toBeVisible();
});
```

- [ ] **Step 5: Run the affected E2E suite**

Run (terminal 1): `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev`
Run (terminal 2): `npx playwright test e2e/layout.spec.ts --workers=1`
Expected: all pass (existing + new).

- [ ] **Step 6: Full gate + commit**

Run: `npm run check` (0/0), `npm run test:unit`.

```bash
git add e2e/
git commit -m "test: update E2E selectors for the custom title bar and hover-reveal chrome"
```

---

## Self-Review Notes

- **Spec coverage:** title bar layout (Tasks 3-5), center palette trigger (Tasks 2-4), window controls + drag + decorations (Tasks 3, 5), sidebar chrome-on-demand incl. new settings button (Task 4), collapse toggle in title bar (Tasks 3-4), removal of vault-name button (Task 4), icons (Task 1), helper unit test (Task 2), E2E + selector audit (Task 6), manual verification of window ops and borderless resize (Tasks 4-5). All spec sections mapped.
- **Token caveat:** Task 3 Step 2 and Task 4 Step 6 instruct grepping `styles/tokens.css` before assuming token names; no hardcoded values.
- **Icon caveat:** Task 4 Step 3 handles the gear-icon-may-not-exist case inline (adds a `settings` icon) rather than leaving it as a placeholder.
- **Ambiguity resolved:** center label falls back to `"Textree"` when the vault name is blank (no vault open) — handled in TitleBar markup (`{label || "Textree"}`).
