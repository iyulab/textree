<script lang="ts">
  import { askStore } from './askStore.svelte';
  import { getGenerationConsent, setGenerationConsent, setAiConsent } from './aiConsent';
  import { prepareAiModel } from './ipc';

  let {
    vault,
    nodeScope,
    onOpenNote,
  }: {
    /** Absolute path to the vault root. */
    vault: string;
    /** Scope path for semantic search (current folder). null = whole vault. */
    nodeScope: string | null;
    /** Open a note by vault-relative path. */
    onOpenNote: (path: string) => void;
  } = $props();

  // When the user toggles "Search whole vault", nodeScope is overridden to null.
  let wholeVault = $state(false);
  const scope = () => (wholeVault ? null : nodeScope);

  // Reactively track consent (updated when the user clicks "Enable").
  let consented = $state(getGenerationConsent());

  // Retry timer handle for the 'preparing' state poll.
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function clearRetry() {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }

  /**
   * Enable local AI Q&A:
   *  1. Persist generation consent (new flag).
   *  2. Persist base AI consent too (host auto-spawn on later sessions).
   *  3. Spawn the host if not already running.
   *  4. Immediately start the first submit.
   */
  function enableGeneration() {
    setGenerationConsent(true);
    setAiConsent(true);
    consented = true;
    void prepareAiModel();
    void submitAndPoll();
  }

  async function submitAndPoll() {
    clearRetry();
    if (!askStore.question.trim()) return;
    await askStore.submit(vault, scope());
    // If still preparing, schedule a retry so the panel advances automatically.
    if (askStore.status === 'preparing') {
      retryTimer = setTimeout(() => {
        void submitAndPoll();
      }, 2000);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      clearRetry();
      void submitAndPoll();
    }
  }
</script>

<section class="ask-panel" aria-label="Ask about your notes">
  {#if !consented}
    <div class="ask-consent">
      <p class="ask-consent-text">
        Ask questions answered from your notes — free, local, private.
        Requires a one-time model download (~1 GB).
      </p>
      <button class="ask-enable" onclick={enableGeneration}>
        Enable local AI Q&amp;A
      </button>
    </div>
  {:else}
    <div class="ask-header">
      <h2 class="ask-title">Ask</h2>
      <label class="ask-scope-toggle">
        <input type="checkbox" bind:checked={wholeVault} />
        Whole vault
      </label>
    </div>
    <div class="ask-input-row">
      <input
        class="ask-input"
        bind:value={askStore.question}
        onkeydown={handleKeydown}
        placeholder="Ask about your notes…"
        aria-label="Question"
      />
      <button
        class="ask-submit"
        onclick={() => { clearRetry(); void submitAndPoll(); }}
        disabled={askStore.status === 'searching' || askStore.status === 'generating'}
        aria-label="Submit question"
      >Ask</button>
    </div>

    {#if askStore.status === 'preparing'}
      <p class="ask-status" role="status">Local AI is preparing… (will retry automatically)</p>
    {:else if askStore.status === 'searching'}
      <p class="ask-status" role="status">Searching notes…</p>
    {:else if askStore.status === 'generating'}
      <p class="ask-status" role="status">Generating answer…</p>
    {:else if askStore.status === 'empty'}
      <p class="ask-status" role="status">No related notes found for this question.</p>
    {:else if askStore.status === 'error'}
      <p class="ask-error" role="alert">{askStore.errorMessage}</p>
    {/if}

    {#if askStore.answer}
      <div class="ask-answer" aria-live="polite">{askStore.answer}</div>
    {/if}

    {#if askStore.citations.length}
      <div class="ask-citations-section">
        <h3 class="ask-citations-title">Sources</h3>
        <ul class="ask-citations">
          {#each askStore.citations as c (c.path)}
            <li>
              <button
                class="ask-citation"
                onclick={() => onOpenNote(c.path)}
                title={c.path}
              >{c.path}</button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  {/if}
</section>

<style>
  .ask-panel {
    flex-shrink: 0;
    border-top: 1px solid var(--border);
    padding: var(--sp-3) var(--sp-6);
    background: var(--bg-primary);
  }
  .ask-consent {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .ask-consent-text {
    margin: 0;
    font-size: var(--font-size-small);
    color: var(--text-muted);
  }
  .ask-enable {
    align-self: flex-start;
    padding: var(--sp-1) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    background: var(--accent);
    color: var(--text-on-accent);
    font: inherit;
    font-size: var(--font-size-small);
    cursor: pointer;
  }
  .ask-enable:hover {
    background: var(--accent-hover);
  }
  .ask-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-2);
  }
  .ask-title {
    margin: 0;
    font-size: var(--font-size-smaller);
    font-weight: var(--font-weight-semibold);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    flex: 1;
  }
  .ask-scope-toggle {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    cursor: pointer;
  }
  .ask-input-row {
    display: flex;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .ask-input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--font-size-small);
    padding: var(--sp-1) var(--sp-2);
    color: var(--text-normal);
    background: var(--bg-primary);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-s);
  }
  .ask-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .ask-submit {
    flex-shrink: 0;
    padding: var(--sp-1) var(--sp-2);
    font: inherit;
    font-size: var(--font-size-small);
    cursor: pointer;
    color: var(--text-on-accent);
    background: var(--accent);
    border: none;
    border-radius: var(--radius-s);
  }
  .ask-submit:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .ask-submit:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ask-status {
    margin: 0 0 var(--sp-2);
    font-size: var(--font-size-small);
    color: var(--text-muted);
    font-style: italic;
  }
  .ask-error {
    margin: 0 0 var(--sp-2);
    font-size: var(--font-size-small);
    color: var(--text-error);
  }
  .ask-answer {
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-s);
    background: var(--bg-secondary);
    font-size: var(--font-size-small);
    color: var(--text-normal);
    line-height: 1.6;
    white-space: pre-wrap;
    margin-bottom: var(--sp-2);
  }
  .ask-citations-section {
    margin-top: var(--sp-1);
  }
  .ask-citations-title {
    margin: 0 0 var(--sp-1);
    font-size: var(--font-size-smaller);
    font-weight: var(--font-weight-semibold);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .ask-citations {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .ask-citation {
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ask-citation:hover {
    background: var(--bg-secondary-alt);
  }
  .ask-citation:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
