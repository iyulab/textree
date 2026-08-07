<script lang="ts">
  import { lines, reassurance, TITLE } from "./migrationNotice.helpers";
  import type { MoveOut } from "./ipc";

  interface Props {
    /** What the folder gave up. Only shown when something did.  */
    moved: MoveOut;
    /** Dismiss. */
    onclose: () => void;
  }
  let { moved, onclose }: Props = $props();
</script>

<div
  class="scrim"
  role="dialog"
  aria-modal="true"
  aria-label={TITLE}
  tabindex="-1"
  onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); onclose(); } }}
  data-testid="migration-notice"
>
  <div class="dialog">
    <h2 class="title">{TITLE}</h2>
    <ul class="what">
      {#each lines(moved) as line (line)}
        <li>{line}</li>
      {/each}
    </ul>
    <p class="calm">{reassurance()}</p>
    <div class="buttons">
      <button class="btn primary" onclick={onclose} data-testid="migration-ok">Got it</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* --bg-primary: background-based translucent overlay, as the palette overlay does */
    background: color-mix(in srgb, var(--bg-primary) 60%, transparent);
    z-index: 50;
  }
  .dialog {
    width: min(30rem, 92vw);
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: var(--sp-5) var(--sp-6);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg-primary);
    box-shadow: var(--shadow-m);
  }
  .title {
    margin: 0;
    font-size: var(--font-size-ui);
    font-weight: var(--font-weight-semibold);
    color: var(--text-normal);
  }
  .what {
    margin: 0;
    padding-left: var(--sp-4);
    color: var(--text-normal);
    font-size: var(--font-size-small);
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .calm {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--font-size-smaller);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
  }
  .btn {
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    font-size: var(--font-size-ui);
    font-family: inherit;
    cursor: pointer;
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg-primary);
  }
</style>
