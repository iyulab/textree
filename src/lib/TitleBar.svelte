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
