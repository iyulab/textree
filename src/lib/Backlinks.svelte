<script lang="ts">
  import type { Backlink } from "./wikilink.helpers";

  let {
    links,
    onOpen,
  }: {
    /** Incoming links to the open note. */
    links: Backlink[];
    /** Open the source note that links here. */
    onOpen: (path: string) => void;
  } = $props();

  /** Display name for a note path: the file stem (`notes/idea.md` → `idea`). */
  function displayName(path: string): string {
    const file = path.split("/").pop() ?? path;
    return file.replace(/\.md$/i, "");
  }
</script>

{#if links.length}
  <section class="backlinks" aria-label="Linked mentions">
    <h2 class="backlinks-title">
      Linked mentions <span class="count">{links.length}</span>
    </h2>
    <ul class="backlinks-list">
      {#each links as link (link.from)}
        <li>
          <button class="backlink" onclick={() => onOpen(link.from)} title={link.from}>
            {displayName(link.from)}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .backlinks {
    flex-shrink: 0;
    max-height: 30%;
    overflow: auto;
    border-top: 1px solid var(--border);
    padding: var(--sp-3) var(--sp-6);
    background: var(--bg-primary);
  }
  .backlinks-title {
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
  .backlinks-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .backlink {
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
  .backlink:hover {
    background: var(--bg-secondary-alt);
  }
  .backlink:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
