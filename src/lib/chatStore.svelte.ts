// Runes orchestration store for the multi-turn local AI Chat (workspace shell).
// NOT imported by tests (runes module — test only pure logic in ask.helpers.ts).
import { ask, cancelAsk, hostStatus, prepareGeneration, readNote, semanticSearch } from './ipc';
import type { SemanticHit } from './ipc';
import {
  buildChatMessages,
  extractCitations,
  fileToContext,
  hasUsableContext,
  resolveGenerationGate,
  selectContext,
  type ChatTurn,
} from './ask.helpers';

export type ChatScopeKind = 'file' | 'folder' | 'vault';
export interface ChatScope {
  kind: ChatScopeKind;
  path: string | null; // file/folder absolute path; null for whole-vault
  label: string;
}
export type ChatState =
  | 'idle' | 'searching' | 'generating' | 'done' | 'error' | 'empty' | 'preparing';

const CONTEXT_LIMIT = 5;

class ChatStore {
  scope = $state<ChatScope>({ kind: 'vault', path: null, label: 'Whole vault' });
  turns = $state<ChatTurn[]>([]);
  status = $state<ChatState>('idle');
  errorMessage = $state('');
  draft = $state('');
  started = $state(false);

  // Monotonic guard — each new run increments it; stale stream callbacks check
  // their captured seq against the current and discard if behind (askStore pattern).
  private seq = 0;

  /** Begin a fresh conversation pinned to `scope` (New chat / re-scope / first entry). */
  startSession(scope: ChatScope) {
    this.cancel();
    this.scope = scope;
    this.turns = [];
    this.status = 'idle';
    this.errorMessage = '';
    this.draft = '';
    this.started = true;
  }

  /** Commit the draft as a user turn and run the pipeline. */
  async send(vault: string) {
    const q = this.draft.trim();
    if (!q) return;
    this.turns = [...this.turns, { role: 'user', text: q, citations: [] }];
    this.draft = '';
    await this.run(vault);
  }

  /** Re-attempt the latest user turn (used by the 'preparing' retry timer). */
  async retry(vault: string) {
    await this.run(vault);
  }

  private async run(vault: string) {
    const seq = ++this.seq;
    this.errorMessage = '';
    const lastUser = [...this.turns].reverse().find((t) => t.role === 'user');
    const q = lastUser?.text ?? '';
    if (!q) return;

    // Gate 1 + 2: host ready + generation model loaded (askStore 3-gate pattern).
    let st: { status: string; generatorReady: boolean; generatorError?: string | null };
    try {
      st = await hostStatus();
    } catch {
      if (seq !== this.seq) return;
      this.status = 'error';
      this.errorMessage = 'Could not reach the local AI host.';
      return;
    }
    if (seq !== this.seq) return;
    const gate = resolveGenerationGate(st);
    if (gate === 'error') {
      this.status = 'error';
      this.errorMessage = `Local AI failed to start: ${st.generatorError}`;
      return;
    }
    if (gate === 'preparing') {
      this.status = 'preparing';
      // Only kick a prepare once the host itself is up (matches prior behavior; the host
      // command no-ops when not Ready anyway). The panel's retry timer re-polls.
      if (st.status === 'ready' && !st.generatorReady) {
        try {
          await prepareGeneration();
        } catch {
          // non-fatal: panel retries and re-polls
        }
      }
      return;
    }
    // gate === 'ready' → fall through to retrieval.

    // Retrieval — scope-granularity dispatch.
    this.status = 'searching';
    let hits: SemanticHit[];
    try {
      if (this.scope.kind === 'file' && this.scope.path) {
        const body = await readNote(vault, this.scope.path);
        hits = fileToContext(this.scope.path, body);
      } else {
        hits = await semanticSearch(vault, q, this.scope.path, CONTEXT_LIMIT);
      }
    } catch (e) {
      if (seq !== this.seq) return;
      this.status = 'error';
      this.errorMessage = String(e);
      return;
    }
    if (seq !== this.seq) return;

    const ctx = selectContext(hits);
    if (!hasUsableContext(ctx)) {
      this.status = 'empty';
      return;
    }

    // Generation — append tokens to a freshly-pushed assistant turn.
    this.status = 'generating';
    const idx = this.turns.lastIndexOf(lastUser!);
    const priorTurns = this.turns.slice(0, idx);
    const messages = buildChatMessages(priorTurns, q, ctx);
    const assistantIdx = this.turns.length;
    this.turns = [...this.turns, { role: 'assistant', text: '', citations: [] }];
    try {
      await ask(vault, messages, ctx, this.scope.path, (e) => {
        if (seq !== this.seq) return; // stale stream — discard
        // Mutate THROUGH the reactive proxy (this.turns[idx]); a captured local
        // ref to the pushed object is the un-proxied original and is NOT reactive.
        if (e.kind === 'token') this.turns[assistantIdx].text += e.text;
        else if (e.kind === 'citations') this.turns[assistantIdx].citations = extractCitations(e.hits);
        else if (e.kind === 'done') this.status = 'done';
        else if (e.kind === 'error') {
          this.status = 'error';
          this.errorMessage = e.message;
        }
      });
    } catch (e) {
      if (seq !== this.seq) return;
      this.status = 'error';
      this.errorMessage = String(e);
    }
  }

  /** User-triggered retry after a generation failure (Retry button). Awaiting prepareGeneration
   *  lets the host clear its last error before we re-poll, so the gate does not immediately
   *  re-resolve to 'error' on a stale value. */
  async retryGeneration(vault: string) {
    this.errorMessage = '';
    this.status = 'preparing';
    try {
      await prepareGeneration();
    } catch {
      // non-fatal: re-poll below will reflect the host state.
    }
    await this.run(vault);
  }

  /** Cancel any in-flight stream and stop the host generating (mode switch, app close). */
  cancel() {
    this.seq++;
    void cancelAsk(); // fire-and-forget: bump Rust ask_generation → host stops
    this.status = 'idle';
  }
}

export const chatStore = new ChatStore();
