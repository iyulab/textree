<script lang="ts">
  import { onMount } from "svelte";
  import { listTrash, restoreNode, purgeTrash, type TrashItem } from "./ipc";
  import { sortTrash, formatDeletedAt, originLabel } from "./trash.helpers";
  import { friendlyError, type FriendlyError } from "./friendlyError.helpers";

  let {
    root,
    onclose,
    onrestored,
  }: {
    /** Absolute vault root path. */
    root: string;
    /** Close the trash panel. */
    onclose: () => void;
    /** Called after a successful restore so the caller can refresh the tree. */
    onrestored: () => void;
  } = $props();

  let items = $state<TrashItem[]>([]);
  // Surface restore/purge failures instead of swallowing them to the console — a silent
  // no-op on a destructive/recovery action reads as "nothing happened" (data-safety visibility).
  let actionError = $state<FriendlyError | null>(null);

  /** Last component of a path (file/folder name), separator-tolerant. */
  function displayName(p: string): string {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  async function refresh() {
    try {
      items = sortTrash(await listTrash(root));
    } catch (e) {
      actionError = friendlyError(e);
    }
  }

  async function handleRestore(trashName: string) {
    actionError = null;
    try {
      await restoreNode(root, trashName);
      onrestored();
      await refresh();
    } catch (e) {
      actionError = friendlyError(e);
    }
  }

  async function handlePurge(trashName: string) {
    actionError = null;
    try {
      await purgeTrash(root, trashName);
      await refresh();
    } catch (e) {
      actionError = friendlyError(e);
    }
  }

  async function handleEmpty() {
    actionError = null;
    try {
      await purgeTrash(root);
      await refresh();
    } catch (e) {
      actionError = friendlyError(e);
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

<section class="trash-panel" aria-label="Trash" data-testid="trash-panel">
  <header class="trash-header">
    <h2 class="trash-title">Trash</h2>
    <button class="close-btn" onclick={onclose} aria-label="Close trash">×</button>
  </header>

  {#if actionError}
    <p class="trash-error" role="status" title={actionError.raw !== actionError.summary ? actionError.raw : undefined} data-testid="trash-error">⚠ {actionError.summary}</p>
  {/if}

  {#if items.length === 0}
    <p class="empty-state">Trash is empty</p>
  {:else}
    <ul class="trash-list">
      {#each items as item (item.trashName)}
        <li class="trash-item" data-testid="trash-item">
          <div class="item-info">
            <span class="item-name">{displayName(item.originalRel)}</span>
            <span class="item-origin">{originLabel(item)}</span>
            <span class="item-date">{formatDeletedAt(item.deletedAt)}</span>
          </div>
          <div class="item-actions">
            <button
              class="action-btn restore-btn"
              onclick={() => handleRestore(item.trashName)}
              data-testid="trash-restore"
            >Restore</button>
            <button
              class="action-btn purge-btn"
              onclick={() => handlePurge(item.trashName)}
              data-testid="trash-purge"
            >Delete permanently</button>
          </div>
        </li>
      {/each}
    </ul>
    <footer class="trash-footer">
      <button
        class="action-btn empty-btn"
        onclick={handleEmpty}
        data-testid="trash-empty"
      >Empty trash</button>
    </footer>
  {/if}
</section>

<style>
  .trash-panel {
    flex-shrink: 0;
    overflow: auto;
    border-top: 1px solid var(--border);
    padding: var(--sp-3) var(--sp-6);
    background: var(--bg-primary);
  }
  .trash-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-2);
  }
  .trash-title {
    flex: 1;
    margin: 0;
    font-size: var(--font-size-smaller);
    font-weight: var(--font-weight-semibold);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: var(--font-size-ui);
    cursor: pointer;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-s);
    line-height: 1;
  }
  .close-btn:hover {
    background: var(--bg-secondary-alt);
    color: var(--text-normal);
  }
  .empty-state {
    color: var(--text-muted);
    font-size: var(--font-size-small);
    margin: 0;
    padding: var(--sp-2) 0;
  }
  .trash-error {
    color: var(--text-error);
    font-size: var(--font-size-smaller);
    margin: 0 0 var(--sp-2);
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-s);
    background: var(--bg-secondary-alt);
  }
  .trash-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .trash-item {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: var(--sp-2);
    border-radius: var(--radius-s);
    background: var(--bg-secondary-alt);
  }
  .item-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .item-name {
    font-size: var(--font-size-small);
    font-weight: var(--font-weight-semibold);
    color: var(--text-normal);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .item-origin {
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .item-date {
    font-size: var(--font-size-smaller);
    color: var(--text-faint);
  }
  .item-actions {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    flex-shrink: 0;
  }
  .action-btn {
    font: inherit;
    font-size: var(--font-size-smaller);
    padding: var(--sp-1) var(--sp-2);
    cursor: pointer;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    background: var(--bg-primary);
    color: var(--text-normal);
    white-space: nowrap;
    transition: background var(--transition-fast);
  }
  .action-btn:hover {
    background: var(--bg-hover);
  }
  .purge-btn {
    color: var(--text-error);
    border-color: var(--text-error);
  }
  .purge-btn:hover {
    background: var(--bg-hover);
  }
  .trash-footer {
    margin-top: var(--sp-3);
    border-top: 1px solid var(--border);
    padding-top: var(--sp-3);
  }
  .empty-btn {
    color: var(--text-error);
    border-color: var(--text-error);
  }
</style>
