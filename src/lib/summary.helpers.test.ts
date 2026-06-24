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
  });

  it('excludes blank-body notes but still counts them in totalCount', () => {
    const r = budgetConcat([{ path: 'a.md', body: 'x' }, { path: 'b.md', body: '   ' }], { totalBudget: 100 });
    expect(r.hits.map(h => h.path)).toEqual(['a.md']);
    expect(r.includedCount).toBe(1);
    expect(r.totalCount).toBe(2);
  });

  it('caps at maxNotes (later notes dropped, counted in totalCount)', () => {
    const many: ScopeNote[] = Array.from({ length: 5 }, (_, i) => ({ path: `n${i}.md`, body: 'z'.repeat(50) }));
    const r = budgetConcat(many, { maxNotes: 2, totalBudget: 100 });
    expect(r.includedCount).toBe(2);
    expect(r.totalCount).toBe(5);
    expect(r.hits.map(h => h.path)).toEqual(['n0.md', 'n1.md']);
  });

  it('returns empty hits when all bodies are blank', () => {
    const r = budgetConcat([{ path: 'a.md', body: '' }], { totalBudget: 100 });
    expect(r.hits).toEqual([]);
    expect(r.includedCount).toBe(0);
    expect(r.totalCount).toBe(1);
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
    expect(msgs.find(m => m.role === 'user')!.content).not.toContain(' of ');
  });
});
