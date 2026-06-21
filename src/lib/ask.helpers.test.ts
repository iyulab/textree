import { describe, it, expect } from 'vitest';
import { hasUsableContext, selectContext, buildAskPrompt, extractCitations } from './ask.helpers';

const hit = (path: string, score = 0.9) => ({ path, snippet: `snippet of ${path}`, score });

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
