import { describe, it, expect } from 'vitest';
import {
  collectScopeNotes,
  budgetConcat,
  buildSummaryMessages,
  type ScopeNote,
} from './summary.helpers';
import type { TreeNode } from './ipc';

const leaf = (path: string): TreeNode => ({ name: path, kind: 'leaf', path, body_path: null, children: [] });
const folder = (path: string, body: string | null, children: TreeNode[]): TreeNode =>
  ({ name: path, kind: 'container', path, body_path: body, children });

describe('collectScopeNotes', () => {
  const tree: TreeNode[] = [
    leaf('/v/a.md'),
    folder('/v/proj', '/v/proj/proj.md', [leaf('/v/proj/x.md'), leaf('/v/proj/y.md')]),
  ];

  it('vault scope (null) collects all note bodies in DFS order', () => {
    expect(collectScopeNotes(tree, null)).toEqual([
      '/v/a.md', '/v/proj/proj.md', '/v/proj/x.md', '/v/proj/y.md',
    ]);
  });

  it('folder scope collects the folder note plus descendants', () => {
    expect(collectScopeNotes(tree, '/v/proj')).toEqual([
      '/v/proj/proj.md', '/v/proj/x.md', '/v/proj/y.md',
    ]);
  });

  it('leaf scope returns just that note', () => {
    expect(collectScopeNotes(tree, '/v/a.md')).toEqual(['/v/a.md']);
  });

  it('unknown scope returns empty', () => {
    expect(collectScopeNotes(tree, '/v/nope')).toEqual([]);
  });

  it('container without a body note contributes only its descendants', () => {
    const t: TreeNode[] = [folder('/v/g', null, [leaf('/v/g/m.md')])];
    expect(collectScopeNotes(t, '/v/g')).toEqual(['/v/g/m.md']);
  });
});

describe('budgetConcat', () => {
  const notes: ScopeNote[] = [
    { path: 'a.md', body: 'A'.repeat(100) },
    { path: 'b.md', body: 'B'.repeat(100) },
  ];

  it('splits the budget fairly across non-empty notes', () => {
    const r = budgetConcat(notes, { totalBudget: 60 });
    expect(r.hits).toHaveLength(2);
    expect(r.hits[0]).toEqual({ path: 'a.md', snippet: 'A'.repeat(30), score: 1 });
    expect(r.hits[1].snippet).toBe('B'.repeat(30));
    expect(r.includedCount).toBe(2);
    expect(r.totalCount).toBe(2);
    // 100-char bodies sliced to 30 → body was truncated
    expect(r.bodyTruncated).toBe(true);
  });

  it('excludes blank-body notes but still counts them in totalCount', () => {
    const r = budgetConcat([{ path: 'a.md', body: 'x' }, { path: 'b.md', body: '   ' }], { totalBudget: 100 });
    expect(r.hits.map(h => h.path)).toEqual(['a.md']);
    expect(r.includedCount).toBe(1);
    expect(r.totalCount).toBe(2);
    // 'x' is 1 char, perNote = floor(100/1) = 100, no truncation
    expect(r.bodyTruncated).toBe(false);
  });

  it('caps at maxNotes (later notes dropped, counted in totalCount)', () => {
    const many: ScopeNote[] = Array.from({ length: 5 }, (_, i) => ({ path: `n${i}.md`, body: 'z'.repeat(50) }));
    const r = budgetConcat(many, { maxNotes: 2, totalBudget: 100 });
    expect(r.includedCount).toBe(2);
    expect(r.totalCount).toBe(5);
    expect(r.hits.map(h => h.path)).toEqual(['n0.md', 'n1.md']);
    // 50-char bodies, perNote = floor(100/2) = 50 → no body truncation (50 > 50 is false)
    expect(r.bodyTruncated).toBe(false);
  });

  it('returns empty hits when all bodies are blank', () => {
    const r = budgetConcat([{ path: 'a.md', body: '' }], { totalBudget: 100 });
    expect(r.hits).toEqual([]);
    expect(r.includedCount).toBe(0);
    expect(r.totalCount).toBe(1);
    expect(r.bodyTruncated).toBe(false);
  });

  it('bodyTruncated is false when all bodies fit within perNote budget', () => {
    // 3-char bodies, perNote = floor(90/3) = 30 → no truncation
    const short: ScopeNote[] = [
      { path: 'a.md', body: 'abc' },
      { path: 'b.md', body: 'def' },
      { path: 'c.md', body: 'ghi' },
    ];
    const r = budgetConcat(short, { totalBudget: 90 });
    expect(r.bodyTruncated).toBe(false);
    expect(r.includedCount).toBe(3);
  });

  it('bodyTruncated is true when bodies exceed perNote even with all notes included', () => {
    // 50-char bodies, perNote = floor(30/2) = 15 → bodies truncated
    const long: ScopeNote[] = [
      { path: 'a.md', body: 'A'.repeat(50) },
      { path: 'b.md', body: 'B'.repeat(50) },
    ];
    const r = budgetConcat(long, { totalBudget: 30 });
    expect(r.bodyTruncated).toBe(true);
    expect(r.includedCount).toBe(2);
    expect(r.totalCount).toBe(2);
    // All included (no count-truncation), body still truncated
  });
});

describe('buildSummaryMessages', () => {
  it('grounds the model in the excerpts and forbids invention', () => {
    const r = budgetConcat([{ path: 'a.md', body: 'hello world' }], { totalBudget: 100 });
    const msgs = buildSummaryMessages('My Folder', r);
    expect(msgs[0].role).toBe('system');
    expect(msgs[0].content.toLowerCase()).toContain('summarize only');
    expect(msgs[0].content.toLowerCase()).toContain('do not invent');
    const user = msgs.find(m => m.role === 'user')!;
    expect(user.content).toContain('My Folder');
    expect(user.content).toContain('a.md');
    expect(user.content.toLowerCase()).toContain('summarize these notes');
  });

  it('discloses truncation when not all notes are included', () => {
    const r = budgetConcat(
      Array.from({ length: 3 }, (_, i) => ({ path: `n${i}.md`, body: 'z'.repeat(50) })),
      { maxNotes: 2, totalBudget: 100 },
    );
    const msgs = buildSummaryMessages('V', r);
    expect(msgs.find(m => m.role === 'user')!.content).toContain('2 of 3');
  });

  it('omits the truncation note when all are included', () => {
    const r = budgetConcat([{ path: 'a.md', body: 'x' }], { totalBudget: 100 });
    const msgs = buildSummaryMessages('V', r);
    const user = msgs.find(m => m.role === 'user')!.content;
    expect(user).not.toContain(' of ');
    expect(user).not.toContain('excerpts are truncated');
  });

  it('discloses only body truncation (no count-trunc) when bodies exceed budget but all notes included', () => {
    // 2 notes, 50-char bodies, budget 30 → perNote=15, bodyTruncated=true, includedCount===totalCount
    const r = budgetConcat(
      [{ path: 'a.md', body: 'A'.repeat(50) }, { path: 'b.md', body: 'B'.repeat(50) }],
      { totalBudget: 30 },
    );
    expect(r.bodyTruncated).toBe(true);
    expect(r.includedCount).toBe(r.totalCount); // no count truncation
    const user = buildSummaryMessages('V', r).find(m => m.role === 'user')!.content;
    expect(user).toContain('excerpts are truncated');
    expect(user).not.toContain(' of '); // no count-truncation text
  });

  it('combines both disclosures into a single parenthetical when count and body are both truncated', () => {
    // 5 notes capped to 2, bodies long enough to be sliced
    const r = budgetConcat(
      Array.from({ length: 5 }, (_, i) => ({ path: `n${i}.md`, body: 'z'.repeat(50) })),
      { maxNotes: 2, totalBudget: 30 },
    );
    expect(r.includedCount).toBe(2);
    expect(r.totalCount).toBe(5);
    // perNote = floor(30/2) = 15; 50 > 15 → bodyTruncated true
    expect(r.bodyTruncated).toBe(true);
    const user = buildSummaryMessages('V', r).find(m => m.role === 'user')!.content;
    expect(user).toContain('2 of 5');
    expect(user).toContain('excerpts are truncated');
    // Both should appear in a single parenthetical (only one opening paren)
    expect(user.match(/\(/g)?.length ?? 0).toBe(1);
  });
});
