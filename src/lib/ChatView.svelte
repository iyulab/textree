<script lang="ts">
  import { onDestroy } from 'svelte';
  import { chatStore } from './chatStore.svelte';
  import { getGenerationConsent, setGenerationConsent, setAiConsent } from './aiConsent';
  import { prepareAiModel } from './ipc';
  import type { TreeNode } from './ipc';
  import { formatModelDownload } from './modelDownload.helpers';
  import { friendlyError } from './friendlyError.helpers';
  import Icon from './Icon.svelte';

  let {
    vault,
    tree,
    onOpenNote,
    onNewChat,
    onBack,
    onSaved,
  }: {
    vault: string;
    tree: TreeNode[];
    onOpenNote: (path: string) => void;
    onNewChat: () => void;
    onBack: () => void;
    onSaved: (path: string) => void;
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

  function summarize() {
    clearRetry();
    void (async () => {
      await chatStore.summarize(vault, tree);
      scheduleRetryIfPreparing();
    })();
  }

  let saveError = $state<string | null>(null);

  function saveToNote(text: string) {
    saveError = null;
    void chatStore
      .saveSummaryToNote(vault, text)
      .then((p) => onSaved(p))
      .catch((e) => { saveError = friendlyError(e).summary; });
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
      <button class="chat-back" onclick={onBack} title="Back to note" aria-label="Back to note"><Icon name="file-text" /></button>
      <span class="chat-chip" title={chatStore.scope.path ?? 'Whole vault'}>
        ◈ Scope: {chatStore.scope.label}
      </span>
      <button class="chat-summarize" onclick={summarize} disabled={busy} aria-label="Summarize this scope">Summarize</button>
      <button class="chat-newchat" onclick={onNewChat}>New chat</button>
    </div>

    <div class="chat-turns">
      {#each chatStore.turns as turn, i (i)}
        <div class="chat-turn {turn.role}">
          <span class="chat-speaker">{turn.role === 'user' ? 'You' : 'Assistant'}:</span>
          <!-- Live region scoped to the assistant bubble only: a polite region on the whole
               transcript re-announces every prior turn on each streamed token. Static turns are
               already in the DOM (not re-announced); only the streaming bubble's appended tokens
               speak. User turns carry no live region. -->
          <div class="chat-bubble" aria-live={turn.role === 'assistant' ? 'polite' : undefined}>{turn.text}</div>
          {#if turn.role === 'assistant' && turn.text}
            <button class="chat-copy" type="button" aria-label="Copy message"
              onclick={() => navigator.clipboard.writeText(turn.text).catch(() => {})}>Copy</button>
            {#if chatStore.turns[i - 1]?.kind === 'summary'}
              <button class="chat-save" type="button" aria-label="Save summary to a new note"
                onclick={() => saveToNote(turn.text)}>Save to note</button>
            {/if}
          {/if}
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

    {#if saveError}
      <p class="chat-error" role="alert">Save failed: {saveError}</p>
    {/if}

    {#if chatStore.status === 'preparing'}
      {#if formatModelDownload(chatStore.modelDownload) !== null}
        {@const dl = formatModelDownload(chatStore.modelDownload)!}
        <div class="chat-status chat-dl-progress" role="status">
          <span>{dl.label}</span>
          <div class="chat-dl-bar-track" aria-hidden="true">
            <div class="chat-dl-bar-fill" style="width:{dl.ratio * 100}%"></div>
          </div>
          {#if dl.detail}<span class="chat-dl-detail">{dl.detail}</span>{/if}
        </div>
      {:else}
        <p class="chat-status" role="status">Local AI is preparing… (will retry automatically)</p>
      {/if}
    {:else if chatStore.status === 'searching'}
      <p class="chat-status" role="status">Searching notes…</p>
    {:else if chatStore.status === 'generating'}
      <p class="chat-status" role="status">Generating answer…</p>
    {:else if chatStore.status === 'empty'}
      <p class="chat-status" role="status">No related notes found for this question.</p>
    {:else if chatStore.status === 'error'}
      <p class="chat-error" role="alert">{chatStore.errorMessage}</p>
      <button class="chat-retry" type="button" onclick={retryGeneration}>Retry</button>
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
  .chat-back {
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    background: transparent;
    border: none;
    border-radius: var(--radius-s);
    cursor: pointer;
  }
  .chat-back:hover {
    background: var(--bg-hover);
    color: var(--text-normal);
  }
  .chat-chip {
    flex: 1;
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chat-summarize,
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
  .chat-summarize:hover,
  .chat-newchat:hover { background: var(--bg-secondary-alt); }
  .chat-summarize:disabled { opacity: 0.5; cursor: default; }
  .chat-copy {
    align-self: flex-start;
    margin-top: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    cursor: pointer;
  }
  .chat-copy:hover { background: var(--bg-secondary-alt); }
  .chat-save {
    align-self: flex-start;
    margin-top: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    cursor: pointer;
  }
  .chat-save:hover { background: var(--bg-secondary-alt); }
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
  /* Visually hidden, screen-reader only: announces the speaker (the bubble alignment/colour
     conveys it visually, but not to assistive tech). */
  .chat-speaker {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
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
  /* Model download progress shown during the 'preparing' state. */
  .chat-dl-progress { display: flex; flex-direction: column; gap: var(--sp-1); font-style: italic; }
  .chat-dl-bar-track {
    width: 100%;
    height: var(--sp-1);
    background: var(--bg-secondary-alt);
    border-radius: var(--radius-s);
    overflow: hidden;
  }
  .chat-dl-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: var(--radius-s);
    transition: width 0.4s ease;
  }
  .chat-dl-detail { font-size: var(--font-size-smaller); color: var(--text-muted); }
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
