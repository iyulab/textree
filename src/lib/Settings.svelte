<script lang="ts">
  import { theme, type ThemeMode } from "$lib/theme.svelte";
  import {
    getAiConsent, setAiConsent, getGenerationConsent, setGenerationConsent,
  } from "$lib/aiConsent";
  import {
    prepareAiModel, prepareGeneration, stopHost, hostStatus, type HostStatus,
  } from "$lib/ipc";
  import { computeAiSectionState, themeButtons, planEmbeddingToggle } from "$lib/settings.helpers";

  interface Props {
    root: string | null;
    onOpenVault: () => void;
    onclose: () => void;
  }
  let { root, onOpenVault, onclose }: Props = $props();

  let aiConsent = $state(getAiConsent());
  let genConsent = $state(getGenerationConsent());
  let host = $state<HostStatus | null>(null);

  const ai = $derived(computeAiSectionState(aiConsent, genConsent, host));
  const themes = $derived(themeButtons(theme.mode));

  async function refreshHost(): Promise<void> {
    try {
      host = (await hostStatus()).status;
    } catch {
      host = "unavailable";
    }
  }
  $effect(() => {
    void refreshHost();
  });

  async function toggleEmbedding(next: boolean): Promise<void> {
    const plan = planEmbeddingToggle(next, genConsent);
    setAiConsent(plan.nextAiConsent);
    setGenerationConsent(plan.nextGenConsent);
    aiConsent = plan.nextAiConsent;
    genConsent = plan.nextGenConsent;
    try {
      if (plan.host === "spawn") await prepareAiModel();
      else await stopHost();
    } catch {
      /* failures surface via the host-status badge on the next refresh */
    }
    await refreshHost();
  }

  async function toggleGeneration(next: boolean): Promise<void> {
    setGenerationConsent(next);
    genConsent = next;
    if (next) {
      try {
        await prepareGeneration();
      } catch {
        /* badge reflects host state */
      }
    }
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<div class="overlay" role="presentation" onclick={onclose} onkeydown={() => {}}>
  <div
    class="panel"
    role="dialog"
    aria-modal="true"
    aria-label="Settings"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKey}
  >
    <header class="head">
      <h2>Settings</h2>
      <button type="button" class="close" aria-label="Close settings" onclick={onclose}>×</button>
    </header>

    <section aria-label="Appearance">
      <h3>Appearance</h3>
      <div class="seg-group" role="radiogroup" aria-label="Theme">
        {#each themes as t (t.mode)}
          <button
            type="button"
            role="radio"
            aria-checked={t.active}
            class="seg"
            class:active={t.active}
            onclick={() => theme.set(t.mode as ThemeMode)}
          >{t.label}</button>
        {/each}
      </div>
    </section>

    <section aria-label="Vault">
      <h3>Vault</h3>
      <p class="vault-path" title={root ?? ""}>{root ?? "No vault open"}</p>
      <button type="button" class="action" onclick={onOpenVault}>Open / switch vault</button>
    </section>

    <section aria-label="Local AI">
      <h3>Local AI</h3>
      <label class="toggle">
        <input
          type="checkbox"
          checked={ai.embeddingChecked}
          onchange={(e) => toggleEmbedding(e.currentTarget.checked)}
        />
        <span>Embeddings &amp; search</span>
      </label>
      <label class="toggle" class:disabled={ai.generationDisabled}>
        <input
          type="checkbox"
          checked={ai.generationChecked}
          disabled={ai.generationDisabled}
          onchange={(e) => toggleGeneration(e.currentTarget.checked)}
        />
        <span>Q&amp;A &amp; chat</span>
      </label>
      <p class="badge" role="status">{ai.badge}</p>
      <!-- Phase D seam: BYO endpoint / model picker controls go below this line. -->
    </section>
  </div>
</div>

<style>
  /* Overlay + panel mirror Palette.svelte's proven modal pattern. */
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: color-mix(in srgb, var(--bg-primary) 60%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .panel {
    width: min(28rem, 92vw);
    max-height: 86vh;
    overflow-y: auto;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-m);
    padding: var(--sp-4);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-3);
  }
  h2 {
    /* --fs-l not in tokens.css; substituted with --font-size-editor (16px) for heading prominence */
    font-size: var(--font-size-editor);
    margin: 0;
    color: var(--text-normal);
  }
  .close {
    background: none;
    border: none;
    color: var(--text-muted);
    /* --fs-l not in tokens.css; substituted with --font-size-editor (16px) */
    font-size: var(--font-size-editor);
    cursor: pointer;
  }
  section {
    padding: var(--sp-3) 0;
    border-top: 1px solid var(--border);
  }
  h3 {
    /* --fs-s not in tokens.css; substituted with --font-size-small (13px) */
    font-size: var(--font-size-small);
    text-transform: uppercase;
    color: var(--text-muted);
    margin: 0 0 var(--sp-2);
  }
  .seg-group {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    overflow: hidden;
  }
  .seg {
    background: var(--bg-secondary);
    border: none;
    color: var(--text-normal);
    padding: var(--sp-1) var(--sp-3);
    min-height: var(--sp-6);
    cursor: pointer;
  }
  .seg:hover {
    background: var(--bg-hover);
  }
  .seg.active {
    background: var(--accent);
    color: var(--text-on-accent);
  }
  .vault-path {
    color: var(--text-muted);
    /* --fs-s not in tokens.css; substituted with --font-size-small (13px) */
    font-size: var(--font-size-small);
    word-break: break-all;
    margin: 0 0 var(--sp-2);
  }
  .action {
    background: var(--bg-secondary-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    color: var(--text-normal);
    padding: var(--sp-1) var(--sp-3);
    min-height: var(--sp-6);
    cursor: pointer;
  }
  .action:hover {
    background: var(--bg-hover);
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) 0;
    color: var(--text-normal);
    cursor: pointer;
  }
  .toggle.disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }
  .badge {
    margin: var(--sp-2) 0 0;
    /* --fs-s not in tokens.css; substituted with --font-size-small (13px) */
    font-size: var(--font-size-small);
    color: var(--text-muted);
  }
</style>
