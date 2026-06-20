<script lang="ts">
  import type { RelatedNote } from "./relatedNotes.helpers";

  let {
    related,
    onOpen,
  }: {
    /** Semantically related notes for the open note. */
    related: RelatedNote[];
    /** Open the related note. */
    onOpen: (path: string) => void;
  } = $props();

  /** Display name for a note path: the file stem (`notes/idea.md` → `idea`). */
  function displayName(path: string): string {
    const file = path.split("/").pop() ?? path;
    return file.replace(/\.md$/i, "");
  }
</script>

{#if related.length}
  <section class="related" aria-label="Related notes">
    <h2 class="related-title">
      Related notes <span class="count">{related.length}</span>
    </h2>
    <ul class="related-list">
      {#each related as item (item.path)}
        <li>
          <button class="related-item" onclick={() => onOpen(item.path)} title={item.path}>
            {displayName(item.path)}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .related {
    flex-shrink: 0;
    max-height: 30%;
    overflow: auto;
    border-top: 1px solid var(--border);
    padding: var(--sp-3) var(--sp-6);
    background: var(--bg-primary);
  }
  .related-title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-2);
    font-size: var(--font-size-smaller);
    font-weight: var(--font-weight-semibold);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 1.4em;
    padding: 0 0.4em;
    border-radius: var(--radius-m);
    background: var(--bg-secondary-alt);
    color: var(--text-muted);
    font-size: var(--font-size-smaller);
  }
  .related-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .related-item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-s);
    color: var(--accent);
    font-family: var(--font-ui);
    font-size: var(--font-size-small);
    cursor: pointer;
  }
  .related-item:hover {
    background: var(--bg-secondary-alt);
  }
  .related-item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
