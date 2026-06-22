import { describe, it, expect } from 'vitest';
import {
  hasUsableContext,
  selectContext,
  buildAskPrompt,
  extractCitations,
  buildChatMessages,
  fileToContext,
  resolveGenerationGate,
  type ChatTurn,
} from './ask.helpers';
import type { SemanticHit } from './ipc';

const hit = (path: string, score = 0.9) => ({ path, snippet: `snippet of ${path}`, score });
const hit2 = (path: string, snippet: string): SemanticHit => ({ path, snippet, score: 1 });

describe('ask.helpers', () => {
  it('hasUsableContext is false for empty hits', () => {
    expect(hasUsableContext([])).toBe(false);
    expect(hasUsableContext([hit('a.md')])).toBe(true);
  });

  it('selectContext keeps top-5 by input order (already score-sorted)', () => {
    const hits = Array.from({ length: 8 }, (_, i) => hit(`n${i}.md`));
    expect(selectContext(hits).map(h => h.path)).toEqual(['n0.md','n1.md','n2.md','n3.md','n4.md']);
  });

  it('buildAskPrompt grounds the model in context and forbids hallucination', () => {
    const msgs = buildAskPrompt('What is X?', [hit('a.md')]);
    expect(msgs[0].role).toBe('system');
    expect(msgs[0].content.toLowerCase()).toContain('answer only using');  // grounding phrase
    expect(msgs[0].content.toLowerCase()).toContain("could not find"); // say not found
    expect(msgs.find(m => m.role === 'user')!.content).toContain('What is X?');
    expect(msgs.some(m => m.content.includes('a.md'))).toBe(true);  // source labeled in context
  });

  it('buildAskPrompt caps context to top-5 via selectContext', () => {
    const hits = Array.from({ length: 8 }, (_, i) => hit(`n${i}.md`));
    const msgs = buildAskPrompt('What is X?', hits);
    const userContent = msgs.find(m => m.role === 'user')!.content;
    // Verify only top-5 sources are included
    expect(userContent).toContain('n0.md');
    expect(userContent).toContain('n4.md');
    expect(userContent).not.toContain('n5.md');
    expect(userContent).not.toContain('n6.md');
    expect(userContent).not.toContain('n7.md');
  });

  it('extractCitations maps fed hits to clickable citations', () => {
    expect(extractCitations([hit('a.md'), hit('b.md')]))
      .toEqual([{ path: 'a.md', snippet: 'snippet of a.md' }, { path: 'b.md', snippet: 'snippet of b.md' }]);
  });
});

describe('buildChatMessages', () => {
  it('starts with a single grounding system message', () => {
    const msgs = buildChatMessages([], 'What is X?', [hit2('a.md', 'X is a thing.')]);
    expect(msgs[0].role).toBe('system');
    expect(msgs[0].content.toLowerCase()).toContain('only');
  });

  it('includes prior turns in order between system and the current question', () => {
    const history: ChatTurn[] = [
      { role: 'user', text: 'First question', citations: [] },
      { role: 'assistant', text: 'First answer', citations: [] },
    ];
    const msgs = buildChatMessages(history, 'Follow-up', [hit2('a.md', 'ctx')]);
    expect(msgs.map((m) => m.role)).toEqual(['system', 'user', 'assistant', 'user']);
    expect(msgs[1].content).toBe('First question');
    expect(msgs[2].content).toBe('First answer');
    expect(msgs[3].content).toContain('Follow-up');
  });

  it('embeds the retrieved context into the current (last) user message', () => {
    const msgs = buildChatMessages([], 'Q?', [hit2('notes/a.md', 'the relevant snippet')]);
    const last = msgs[msgs.length - 1];
    expect(last.role).toBe('user');
    expect(last.content).toContain('notes/a.md');
    expect(last.content).toContain('the relevant snippet');
    expect(last.content).toContain('Q?');
  });

  it('trims history to the most recent maxHistory turns (drops oldest)', () => {
    const history: ChatTurn[] = Array.from({ length: 8 }, (_, i) => ({
      role: i % 2 === 0 ? 'user' : 'assistant',
      text: `turn ${i}`,
      citations: [],
    }));
    const msgs = buildChatMessages(history, 'now', [hit2('a.md', 'c')], { maxHistory: 4 });
    const historyMsgs = msgs.slice(1, -1); // drop system + current user
    expect(historyMsgs).toHaveLength(4);
    expect(historyMsgs[0].content).toBe('turn 4'); // oldest 4 dropped
    expect(historyMsgs[3].content).toBe('turn 7');
  });

  it('buildAskPrompt equals buildChatMessages with empty history', () => {
    const hits = [hit2('a.md', 'snippet')];
    expect(buildAskPrompt('Q?', hits)).toEqual(buildChatMessages([], 'Q?', hits));
  });
});

describe('fileToContext', () => {
  it('returns one synthetic hit carrying the whole file body', () => {
    const out = fileToContext('notes/topic.md', 'Line one.\nLine two.');
    expect(out).toHaveLength(1);
    expect(out[0].path).toBe('notes/topic.md');
    expect(out[0].snippet).toBe('Line one.\nLine two.');
  });

  it('truncates a body longer than maxChars', () => {
    const body = 'x'.repeat(50);
    const out = fileToContext('a.md', body, 10);
    expect(out[0].snippet.length).toBe(10);
  });

  it('returns an empty array for an empty/whitespace body (no usable context)', () => {
    expect(fileToContext('a.md', '   \n  ')).toEqual([]);
  });
});

describe('resolveGenerationGate', () => {
  it('ready when host ready, generator ready, no error', () => {
    expect(resolveGenerationGate({ status: 'ready', generatorReady: true })).toBe('ready');
  });
  it('preparing when host ready but generator not yet ready', () => {
    expect(resolveGenerationGate({ status: 'ready', generatorReady: false })).toBe('preparing');
  });
  it('preparing while the host is still starting', () => {
    expect(resolveGenerationGate({ status: 'starting', generatorReady: false })).toBe('preparing');
  });
  it('error when host ready and a generator error is present', () => {
    expect(resolveGenerationGate({ status: 'ready', generatorReady: false, generatorError: 'boom' })).toBe('error');
  });
  it('error wins even if generatorReady is somehow true', () => {
    expect(resolveGenerationGate({ status: 'ready', generatorReady: true, generatorError: 'boom' })).toBe('error');
  });
  it('ignores a stale generator error while the host is not ready', () => {
    expect(resolveGenerationGate({ status: 'unavailable', generatorReady: false, generatorError: 'boom' })).toBe('preparing');
  });
});
