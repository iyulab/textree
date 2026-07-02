<script lang="ts">
  import type { MdBlock, MdInline } from './chatMarkdown.helpers';

  let { blocks }: { blocks: MdBlock[] } = $props();
</script>

{#snippet inline(nodes: MdInline[])}
  {#each nodes as n}
    {#if n.type === 'text'}{n.value}
    {:else if n.type === 'strong'}<strong>{@render inline(n.children)}</strong>
    {:else if n.type === 'em'}<em>{@render inline(n.children)}</em>
    {:else if n.type === 'strike'}<del>{@render inline(n.children)}</del>
    {:else if n.type === 'code'}<code class="md-code">{n.value}</code>
    {:else if n.type === 'link'}<span class="md-link">{@render inline(n.children)}</span>
    {/if}
  {/each}
{/snippet}

{#snippet block(nodes: MdBlock[])}
  {#each nodes as b}
    {#if b.type === 'paragraph'}<p class="md-p">{@render inline(b.children)}</p>
    {:else if b.type === 'heading'}
      {#if b.level === 1}<h1 class="md-h md-h1">{@render inline(b.children)}</h1>
      {:else if b.level === 2}<h2 class="md-h md-h2">{@render inline(b.children)}</h2>
      {:else if b.level === 3}<h3 class="md-h md-h3">{@render inline(b.children)}</h3>
      {:else if b.level === 4}<h4 class="md-h md-h4">{@render inline(b.children)}</h4>
      {:else if b.level === 5}<h5 class="md-h md-h5">{@render inline(b.children)}</h5>
      {:else}<h6 class="md-h md-h6">{@render inline(b.children)}</h6>
      {/if}
    {:else if b.type === 'bulletList'}
      <ul class="md-ul">{#each b.items as item}<li class="md-li">{@render block(item)}</li>{/each}</ul>
    {:else if b.type === 'orderedList'}
      <ol class="md-ol" start={b.start}>{#each b.items as item}<li class="md-li">{@render block(item)}</li>{/each}</ol>
    {:else if b.type === 'codeBlock'}<pre class="md-pre"><code>{b.value}</code></pre>
    {:else if b.type === 'blockquote'}<blockquote class="md-quote">{@render block(b.children)}</blockquote>
    {:else if b.type === 'hr'}<hr class="md-hr" />
    {/if}
  {/each}
{/snippet}

<div class="chat-md">{@render block(blocks)}</div>

<style>
  /* Tokens only (mirror lpTheme in livePreview.ts). Tight vertical rhythm for a chat bubble:
     first/last child margins are collapsed so the bubble padding owns the outer gap. */
  .chat-md > :global(:first-child) { margin-top: 0; }
  .chat-md > :global(:last-child) { margin-bottom: 0; }
  .md-p { margin: 0 0 var(--sp-2); }
  .md-p:last-child { margin-bottom: 0; }
  .md-h { margin: var(--sp-3) 0 var(--sp-1); font-weight: var(--font-weight-semibold); line-height: var(--line-height-tight); }
  .md-h1 { font-size: var(--font-size-h3); }
  .md-h2 { font-size: var(--font-size-h4); }
  .md-h3 { font-size: var(--font-size-h5); }
  .md-h4, .md-h5, .md-h6 { font-size: var(--font-size-h6); color: var(--text-muted); }
  .md-ul, .md-ol { margin: 0 0 var(--sp-2); padding-left: var(--sp-5); }
  .md-li { margin: var(--sp-1) 0; }
  /* Tight lists: a lone paragraph in an item renders inline (no block margin). */
  .md-li > .md-p:only-child { margin: 0; }
  .md-code {
    font-family: var(--font-monospace);
    font-size: 0.9em;
    background: var(--bg-secondary-alt);
    padding: 0.1em 0.35em;
    border-radius: var(--radius-s);
  }
  .md-pre {
    margin: 0 0 var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    background: var(--bg-secondary-alt);
    border-radius: var(--radius-s);
    overflow-x: auto;
  }
  .md-pre code { font-family: var(--font-monospace); font-size: 0.9em; white-space: pre; }
  .md-quote {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-3);
    border-left: 3px solid var(--border-strong);
    color: var(--text-muted);
  }
  .md-hr { margin: var(--sp-3) 0; border: none; border-top: 1px solid var(--border-strong); }
  .md-link { color: var(--accent); text-decoration: underline; }
</style>
