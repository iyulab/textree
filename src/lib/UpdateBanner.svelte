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
    <span>New version {info.version} available.</span>
    {#if error}<span class="err">{error}</span>{/if}
    <button onclick={install} disabled={busy}>
      {busy ? "Installing…" : "Update & restart"}
    </button>
    <button onclick={() => (dismissed = true)} disabled={busy}>Later</button>
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
  .err { color: var(--text-error); }
</style>
