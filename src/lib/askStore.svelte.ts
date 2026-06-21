// Runes orchestration store for the local AI Q&A feature (P3).
// NOT imported by tests (runes module — test only pure helpers in ask.helpers.ts).
import { semanticSearch, hostStatus, ask, prepareGeneration } from './ipc';
import {
  selectContext,
  buildAskPrompt,
  extractCitations,
  hasUsableContext,
  type Citation,
} from './ask.helpers';

export type AskState = 'idle' | 'searching' | 'generating' | 'done' | 'error' | 'empty' | 'preparing';

class AskStore {
  question = $state('');
  answer = $state('');
  citations = $state<Citation[]>([]);
  status = $state<AskState>('idle');
  errorMessage = $state('');
  // Monotonically increasing sequence number — each new submit increments it.
  // In-flight callbacks check their captured seq against the current to detect stale results.
  private askSeq = 0;

  async submit(vault: string, scopePath: string | null) {
    const seq = ++this.askSeq;
    this.answer = '';
    this.citations = [];
    this.errorMessage = '';

    let st: { status: string; generatorReady: boolean };
    try {
      st = await hostStatus();
    } catch {
      this.status = 'error';
      this.errorMessage = 'Could not reach the local AI host.';
      return;
    }
    if (seq !== this.askSeq) return;

    // State-machine: three ordered gates before we can run the Q&A pipeline.
    // Gate 1: host process must be ready (status === 'ready').
    if (st.status !== 'ready') {
      // Host is still starting up — show 'preparing' and let the panel retry.
      this.status = 'preparing';
      return;
    }

    // Gate 2: generation model must be loaded (generatorReady === true).
    if (!st.generatorReady) {
      // Host is up but the generation model is not yet loaded.
      // prepareGeneration() is a fast 202/no-op until Ready; re-submit will advance past this gate.
      this.status = 'preparing';
      try {
        await prepareGeneration();
      } catch {
        // non-fatal: let the panel retry and re-poll
      }
      return;
    }

    // Gate 3: semantic search (embedding must be running — covered by gate 1).
    this.status = 'searching';
    let hits;
    try {
      hits = await semanticSearch(vault, this.question, scopePath, 5);
    } catch (e) {
      if (seq !== this.askSeq) return;
      this.status = 'error';
      this.errorMessage = String(e);
      return;
    }
    if (seq !== this.askSeq) return;

    const ctx = selectContext(hits);
    if (!hasUsableContext(ctx)) {
      this.status = 'empty';
      return;
    }

    this.status = 'generating';
    const messages = buildAskPrompt(this.question, ctx);
    try {
      await ask(vault, messages, ctx, scopePath, (e) => {
        if (seq !== this.askSeq) return; // stale stream — discard
        if (e.kind === 'token') this.answer += e.text;
        else if (e.kind === 'citations') this.citations = extractCitations(e.hits);
        else if (e.kind === 'done') this.status = 'done';
        else if (e.kind === 'error') {
          this.status = 'error';
          this.errorMessage = e.message;
        }
      });
    } catch (e) {
      if (seq !== this.askSeq) return;
      this.status = 'error';
      this.errorMessage = String(e);
    }
  }

  /** Cancel the active request (panel close, new question, etc.). */
  cancel() {
    this.askSeq++;
    this.status = 'idle';
  }
}

export const askStore = new AskStore();
