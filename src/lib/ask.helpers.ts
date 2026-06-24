import type { SemanticHit } from './ipc';

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

export interface Citation {
  path: string;
  snippet: string;
}

export interface ChatTurn {
  role: 'user' | 'assistant';
  text: string;
  citations: Citation[];
  /** 'summary' on a synthetic user turn that seeds a scope summary; absent for normal Q&A. */
  kind?: 'summary';
}

const MAX_HITS = 5;
const GROUNDING_SYSTEM =
  'You are a notes assistant. Answer only using the provided note excerpts below. ' +
  'If the excerpts do not contain the answer, say you could not find it in the notes. ' +
  'Do not invent facts that are not in the excerpts.';
const DEFAULT_MAX_HISTORY = 6;

export function hasUsableContext(hits: SemanticHit[]): boolean {
  return hits.length > 0;
}

export function selectContext(hits: SemanticHit[], maxHits = MAX_HITS): SemanticHit[] {
  return hits.slice(0, maxHits);
}

function formatContext(hits: SemanticHit[]): string {
  return selectContext(hits)
    .map((h) => `Source: ${h.path}\n${h.snippet}`)
    .join('\n\n---\n\n');
}

export function buildChatMessages(
  history: ChatTurn[],
  question: string,
  hits: SemanticHit[],
  opts: { maxHistory?: number } = {},
): ChatMessage[] {
  const maxHistory = opts.maxHistory ?? DEFAULT_MAX_HISTORY;
  const recent = history.slice(-maxHistory);
  const context = formatContext(hits);
  return [
    { role: 'system', content: GROUNDING_SYSTEM },
    ...recent.map((t): ChatMessage => ({ role: t.role, content: t.text })),
    { role: 'user', content: `Notes:\n\n${context}\n\nQuestion: ${question}` },
  ];
}

export function buildAskPrompt(question: string, hits: SemanticHit[]): ChatMessage[] {
  return buildChatMessages([], question, hits);
}

export function extractCitations(hits: SemanticHit[]): Citation[] {
  return hits.map(h => ({ path: h.path, snippet: h.snippet }));
}

export const DEFAULT_FILE_MAX_CHARS = 8000;

/**
 * Build a single-context "hit" from a whole note body, for file-scoped chat
 * (the file is the subject — no retrieval). Budget-cut for the model context
 * window. Empty/whitespace bodies yield no context (-> empty state).
 */
export function fileToContext(
  path: string,
  body: string,
  maxChars = DEFAULT_FILE_MAX_CHARS,
): SemanticHit[] {
  if (body.trim().length === 0) return [];
  return [{ path, snippet: body.slice(0, maxChars), score: 1 }];
}

/**
 * Decide what the chat pipeline should do given the host's generation readiness.
 * Pure mirror of the Rust poll_action extraction — keeps the gate logic unit-testable
 * (chatStore is a runes module and is not imported by tests).
 *
 * A generator error only counts while the host is 'ready': a stale error left over from a
 * crashed/restarting host must not mask the real 'preparing'/'unavailable' state. 'preparing'
 * covers both host-starting and generator-not-yet-loaded.
 */
/**
 * Drop a trailing assistant turn that was never finalized. `run()` pushes an empty assistant turn
 * before streaming, so an interrupted run leaves an empty/partial assistant turn at the tail; a
 * healthy run finalizes it ('done') and it must stay in history. Two non-terminal states leave such
 * an orphan: 'error' (generation failed mid-stream) and 'generating' (an in-flight stream frozen by
 * cancel() on mode-switch/app-close). Pruning both keeps malformed (empty/half-finished) assistant
 * content out of the model's multi-turn history and out of the resumed view — symmetric across every
 * recovery path (Retry, typing a new question, and switching back into Chat after a cancel).
 */
const ORPHAN_LEAVING_STATES = new Set(['error', 'generating']);
export function pruneOrphanedAssistantTurn(turns: ChatTurn[], status: string): ChatTurn[] {
  if (ORPHAN_LEAVING_STATES.has(status) && turns.at(-1)?.role === 'assistant') {
    return turns.slice(0, -1);
  }
  return turns;
}

export function resolveGenerationGate(s: {
  status: string;
  generatorReady: boolean;
  generatorError?: string | null;
}): 'error' | 'preparing' | 'ready' {
  if (s.status === 'ready' && s.generatorError) return 'error';
  if (s.status !== 'ready' || !s.generatorReady) return 'preparing';
  return 'ready';
}
