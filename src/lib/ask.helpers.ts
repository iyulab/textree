import type { SemanticHit } from './ipc';

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

export interface Citation {
  path: string;
  snippet: string;
}

const MAX_HITS = 5;

export function hasUsableContext(hits: SemanticHit[]): boolean {
  return hits.length > 0;
}

export function selectContext(hits: SemanticHit[], maxHits = MAX_HITS): SemanticHit[] {
  return hits.slice(0, maxHits);
}

export function buildAskPrompt(question: string, hits: SemanticHit[]): ChatMessage[] {
  const system =
    'You are a notes assistant. Answer only using the provided note excerpts below. ' +
    'If the excerpts do not contain the answer, say you could not find it in the notes. ' +
    'Do not invent facts that are not in the excerpts.';
  const selected = selectContext(hits);
  const context = selected.map(h => `Source: ${h.path}\n${h.snippet}`).join('\n\n---\n\n');
  return [
    { role: 'system', content: system },
    { role: 'user', content: `Notes:\n\n${context}\n\nQuestion: ${question}` },
  ];
}

export function extractCitations(hits: SemanticHit[]): Citation[] {
  return hits.map(h => ({ path: h.path, snippet: h.snippet }));
}
