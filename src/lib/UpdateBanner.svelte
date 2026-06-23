<script lang="ts">
  import type { UpdateInfo } from "$lib/updater";
  import { applyUpdate } from "$lib/updater";

  let { info }: { info: UpdateInfo } = $props();
  let busy = $state(false);
  let dismissed = $state(false);
  let error = $state<string | null>(null);

  async function install() {
    busy = true;
    error = null;
    try {
      await applyUpdate(info);
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

{#if !dismissed}
  <div class="update-banner" role="status">
    <span class="dot" aria-hidden="true"></span>
    <span class="msg">New version {info.version} available.</span>
    {#if error}<span class="err">{error}</span>{/if}
    <div class="actions">
      <button class="btn-primary" onclick={install} disabled={busy}>
        {busy ? "Installing…" : "Update & restart"}
      </button>
      <button class="btn-quiet" onclick={() => (dismissed = true)} disabled={busy}>
        Later
      </button>
    </div>
  </div>
{/if}

<style>
  .update-banner {
    display: flex;
    gap: var(--sp-2);
    align-items: center;
    padding: var(--sp-2) var(--sp-3);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    font-size: var(--font-size-small);
    color: var(--text-normal);
  }

  /* Small emerald cue that an update is ready — the one accent in an otherwise quiet bar. */
  .dot {
    flex-shrink: 0;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }

  .msg {
    font-weight: var(--font-weight-medium);
  }

  .err {
    color: var(--text-error);
  }

  /* Push the actions to the trailing edge, away from the message. */
  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .btn-primary,
  .btn-quiet {
    font: inherit;
    font-size: var(--font-size-small);
    padding: var(--sp-1) var(--sp-3);
    border-radius: var(--radius-s);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  /* Primary CTA — accent fill, matching .chat-send / .chat-enable. */
  .btn-primary {
    background: var(--accent);
    color: var(--text-on-accent);
    border: 1px solid transparent;
  }
  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  /* Quiet dismiss — ghost until hovered, so it yields to the primary action. */
  .btn-quiet {
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
  }
  .btn-quiet:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-normal);
  }

  .btn-primary:focus-visible,
  .btn-quiet:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .btn-primary:disabled,
  .btn-quiet:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
