<script lang="ts">
  import { onMount } from "svelte";
  import { publishPreview, type PublishPreview } from "./ipc";
  import { friendlyError, type FriendlyError } from "./friendlyError.helpers";
  import {
    canPublish,
    hiddenLine,
    publishSummary,
    unrecordedLead,
    unrecordedNames,
    type PublishDestination,
  } from "./publishConfirm.helpers";

  interface Props {
    /** Absolute path of the folder to publish. */
    root: string;
    /** Where the site goes. */
    destination: PublishDestination;
    /** The person chose to publish. */
    onpublish: () => void;
    /** The person chose not to. Nothing is published. */
    oncancel: () => void;
  }
  let { root, destination, onpublish, oncancel }: Props = $props();

  let preview = $state<PublishPreview | null>(null);
  let failure = $state<FriendlyError | null>(null);
  let scrim = $state<HTMLDivElement | null>(null);
  let go = $state<HTMLButtonElement | null>(null);

  const ready = $derived(
    preview !== null && failure === null && canPublish(preview.notes, preview.files),
  );

  onMount(() => {
    scrim?.focus();
    publishPreview(root).then(
      (found) => (preview = found),
      (e) => (failure = friendlyError(e)),
    );
  });

  // Publish is the answer the dialog expects, so it takes focus as soon as it can be pressed.
  $effect(() => {
    if (ready) go?.focus();
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      oncancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={scrim}
  class="scrim"
  role="dialog"
  aria-modal="true"
  aria-label="Publish this folder?"
  tabindex="-1"
  {onkeydown}
  data-testid="publish-confirm"
>
  <div class="dialog">
    <h2 class="title">Publish this folder?</h2>

    {#if failure}
      <p class="failure" data-testid="publish-confirm-error">{failure.summary}</p>
    {:else if preview === null}
      <p class="muted" aria-busy="true" data-testid="publish-confirm-loading">
        Checking what will be published…
      </p>
    {:else}
      <p class="summary" data-testid="publish-confirm-summary">
        {publishSummary(preview.notes, preview.files, destination)}
      </p>
      {#if unrecordedLead(preview.unrecorded)}
        <div class="notice" data-testid="publish-confirm-unrecorded">
          <p>{unrecordedLead(preview.unrecorded)}</p>
          <p class="names">{unrecordedNames(preview.unrecorded)}</p>
        </div>
      {/if}
      {#if hiddenLine(preview.hidden)}
        <p class="muted" data-testid="publish-confirm-hidden">{hiddenLine(preview.hidden)}</p>
      {/if}
    {/if}

    <div class="buttons">
      <button class="btn" onclick={oncancel} data-testid="publish-confirm-cancel">Cancel</button>
      <button
        bind:this={go}
        class="btn primary"
        onclick={onpublish}
        disabled={!ready}
        data-testid="publish-confirm-go"
      >Publish</button>
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
  .scrim:focus {
    outline: none;
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
  .summary {
    margin: 0;
    font-size: var(--font-size-ui);
    color: var(--text-normal);
    overflow-wrap: anywhere;
  }
  .notice {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2);
    border-radius: var(--radius-s);
    background: var(--bg-secondary);
    font-size: var(--font-size-smaller);
    color: var(--text-normal);
  }
  .notice p {
    margin: 0;
  }
  .names {
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .muted {
    margin: 0;
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .failure {
    margin: 0;
    padding: var(--sp-2);
    border-radius: var(--radius-s);
    background: var(--bg-secondary-alt);
    color: var(--text-error);
    font-size: var(--font-size-smaller);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  .btn {
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    background: var(--bg-secondary);
    color: var(--text-normal);
    font-size: var(--font-size-ui);
    font-family: inherit;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-secondary-alt);
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg-primary);
  }
</style>
