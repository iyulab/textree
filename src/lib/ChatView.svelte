<script lang="ts">
  import { onDestroy } from 'svelte';
  import { chatStore } from './chatStore.svelte';
  import { getGenerationConsent, setGenerationConsent, setAiConsent } from './aiConsent';
  import { prepareAiModel } from './ipc';

  let {
    vault,
    onOpenNote,
    onNewChat,
  }: {
    vault: string;
    onOpenNote: (path: string) => void;
    onNewChat: () => void;
  } = $props();

  let consented = $state(getGenerationConsent());
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function clearRetry() {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }
  onDestroy(clearRetry);

  // Enable local AI Q&A: persist consent, spawn host, then send the first message.
  function enableAndSend() {
    setGenerationConsent(true);
    setAiConsent(true);
    consented = true;
    void prepareAiModel();
    void sendAndPoll();
  }

  async function sendAndPoll() {
    clearRetry();
    if (!chatStore.draft.trim()) return;
    await chatStore.send(vault);
    scheduleRetryIfPreparing();
  }

  function scheduleRetryIfPreparing() {
    if (chatStore.status === 'preparing') {
      retryTimer = setTimeout(async () => {
        await chatStore.retry(vault);
        scheduleRetryIfPreparing();
      }, 2000);
    }
  }

  async function retryGeneration() {
    clearRetry();
    await chatStore.retryGeneration(vault);
    scheduleRetryIfPreparing();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      if (busy) return;
      clearRetry();
      void sendAndPoll();
    }
  }

  const busy = $derived(chatStore.status === 'searching' || chatStore.status === 'generating');
</script>

<section class="chat-view" aria-label="Chat about your notes">
  {#if !consented}
    <div class="chat-consent">
      <p class="chat-consent-text">
        Ask questions answered from your notes — free, local, private.
        Requires a one-time AI model download (~5 GB).
      </p>
      <button class="chat-enable" onclick={enableAndSend}>Enable local AI Q&amp;A</button>
    </div>
  {:else}
    <div class="chat-scopebar">
      <span class="chat-chip" title={chatStore.scope.path ?? 'Whole vault'}>
        ◈ Scope: {chatStore.scope.label}
      </span>
      <button class="chat-newchat" onclick={onNewChat}>New chat</button>
    </div>

    <div class="chat-turns" aria-live="polite">
      {#each chatStore.turns as turn, i (i)}
        <div class="chat-turn {turn.role}">
          <div class="chat-bubble">{turn.text}</div>
          {#if turn.citations.length}
            <ul class="chat-citations">
              {#each turn.citations as c (c.path)}
                <li>
                  <button class="chat-citation" onclick={() => onOpenNote(c.path)} title={c.path}>
                    {c.path}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/each}
    </div>

    {#if chatStore.status === 'preparing'}
      <p class="chat-status" role="status">Local AI is preparing… (will retry automatically)</p>
    {:else if chatStore.status === 'searching'}
      <p class="chat-status" role="status">Searching notes…</p>
    {:else if chatStore.status === 'generating'}
      <p class="chat-status" role="status">Generating answer…</p>
    {:else if chatStore.status === 'empty'}
      <p class="chat-status" role="status">No related notes found for this question.</p>
    {:else if chatStore.status === 'error'}
      <p class="chat-error" role="alert">{chatStore.errorMessage}</p>
      <button class="chat-retry" onclick={retryGeneration}>Retry</button>
    {/if}

    <div class="chat-composer">
      <input
        class="chat-input"
        bind:value={chatStore.draft}
        onkeydown={handleKeydown}
        placeholder="Ask about your notes…"
        aria-label="Question"
      />
      <button
        class="chat-send"
        onclick={() => { clearRetry(); void sendAndPoll(); }}
        disabled={busy}
        aria-label="Send question"
      >Ask</button>
    </div>
  {/if}
</section>

<style>
  .chat-view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: var(--sp-3) var(--sp-6);
    background: var(--bg-primary);
  }
  .chat-consent { display: flex; flex-direction: column; gap: var(--sp-2); }
  .chat-consent-text { margin: 0; font-size: var(--font-size-small); color: var(--text-muted); }
  .chat-enable {
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
  .chat-enable:hover { background: var(--accent-hover); }
  .chat-scopebar {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-2);
  }
  .chat-chip {
    flex: 1;
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chat-newchat {
    flex-shrink: 0;
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    background: var(--bg-secondary);
    color: var(--text-normal);
    font: inherit;
    font-size: var(--font-size-smaller);
    cursor: pointer;
  }
  .chat-newchat:hover { background: var(--bg-secondary-alt); }
  .chat-turns {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    margin-bottom: var(--sp-2);
  }
  .chat-turn { display: flex; flex-direction: column; gap: var(--sp-1); }
  .chat-turn.user { align-items: flex-end; }
  .chat-bubble {
    max-width: 90%;
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-s);
    font-size: var(--font-size-small);
    line-height: 1.6;
    white-space: pre-wrap;
  }
  .chat-turn.user .chat-bubble { background: var(--accent); color: var(--text-on-accent); }
  .chat-turn.assistant .chat-bubble { background: var(--bg-secondary); color: var(--text-normal); }
  .chat-citations { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  .chat-citation {
    display: block;
    max-width: 100%;
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
  .chat-citation:hover { background: var(--bg-secondary-alt); }
  .chat-citation:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .chat-status { margin: 0 0 var(--sp-2); font-size: var(--font-size-small); color: var(--text-muted); font-style: italic; }
  .chat-error { margin: 0 0 var(--sp-2); font-size: var(--font-size-small); color: var(--text-error); }
  .chat-retry {
    align-self: flex-start;
    margin: 0 0 var(--sp-2);
    padding: var(--sp-1) var(--sp-3);
    font: inherit;
    font-size: var(--font-size-small);
    cursor: pointer;
    color: var(--text-normal);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
  }
  .chat-retry:hover { background: var(--bg-secondary-alt); }
  .chat-retry:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .chat-composer { display: flex; gap: var(--sp-1); flex-shrink: 0; }
  .chat-input {
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
  .chat-input:focus { outline: none; border-color: var(--accent); }
  .chat-send {
    flex-shrink: 0;
    padding: var(--sp-1) var(--sp-3);
    font: inherit;
    font-size: var(--font-size-small);
    cursor: pointer;
    color: var(--text-on-accent);
    background: var(--accent);
    border: none;
    border-radius: var(--radius-s);
  }
  .chat-send:hover:not(:disabled) { background: var(--accent-hover); }
  .chat-send:disabled { opacity: 0.5; cursor: default; }
</style>
