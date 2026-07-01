import { describe, it, expect } from 'vitest';
import {
  summaryNoteParent,
  summaryNoteBaseName,
  buildSummaryNote,
} from './summaryNote.helpers';
import { buildWikiResolver, findWikiLinks } from './wikilink.helpers';

describe('summaryNoteParent', () => {
  it('folder scope -> the folder path', () => {
    expect(summaryNoteParent('folder', 'C:\\v\\notes', 'C:\\v')).toBe('C:\\v\\notes');
  });
  it('file scope -> the containing folder', () => {
    expect(summaryNoteParent('file', 'C:\\v\\notes\\a.md', 'C:\\v')).toBe('C:\\v\\notes');
    expect(summaryNoteParent('file', '/v/notes/a.md', '/v')).toBe('/v/notes');
  });
  it('vault scope (path null) -> the root', () => {
    expect(summaryNoteParent('vault', null, 'C:\\v')).toBe('C:\\v');
  });
});

describe('summaryNoteBaseName', () => {
  it('prefixes "Summary of" and sanitizes', () => {
    expect(summaryNoteBaseName('My Notes')).toBe('Summary of My Notes');
    expect(summaryNoteBaseName('a/b:c')).toBe('Summary of a b c');
  });
  it('keeps the prefix when the label sanitizes to nothing usable', () => {
    // "Summary of" always survives sanitize, so the base is never empty in practice;
    // the "Summary" fallback is defensive only (the Rust is_valid_name boundary is final).
    expect(summaryNoteBaseName('///')).toBe('Summary of');
  });
});

describe('buildSummaryNote', () => {
  const meta = { includedCount: 2, totalCount: 2, bodyTruncated: false };
  it('builds H1 + body + Sources wikilinks', () => {
    const out = buildSummaryNote('Notes', 'A summary.', ['C:\\v\\notes\\a.md', '/v/notes/b.md'], meta);
    expect(out).toContain('# Summary of Notes');
    expect(out).toContain('A summary.');
    expect(out).toContain('## Sources');
    expect(out).toContain('- [[a]]');
    expect(out).toContain('- [[b]]');
    expect(out.endsWith('\n')).toBe(true);
  });
  it('omits Sources when there are no source paths', () => {
    const out = buildSummaryNote('Notes', 'S.', [], meta);
    expect(out).not.toContain('## Sources');
  });
  it('adds an honest footnote when notes were dropped', () => {
    const out = buildSummaryNote('Notes', 'S.', ['a.md'], { includedCount: 3, totalCount: 10, bodyTruncated: false });
    expect(out).toContain('_(Summary based on 3 of 10 notes.)_');
  });
  it('adds an excerpt-truncation footnote when bodies were sliced', () => {
    const out = buildSummaryNote('Notes', 'S.', ['a.md'], { includedCount: 2, totalCount: 2, bodyTruncated: true });
    expect(out).toContain('Some note excerpts are truncated.');
  });
  it('adds no footnote when nothing was dropped or truncated', () => {
    const out = buildSummaryNote('Notes', 'S.', ['a.md'], meta);
    expect(out).not.toContain('_(');
  });
});

// Guards the whole reason Sources exist (D20 "verifiable"): the [[basename]] links we emit must
// actually resolve back to the source notes. buildWikiResolver resolves a bare name by stem, so a
// basename link resolves regardless of the note's folder (ambiguous stems fall back to the app's
// shortest-path rule — the same behavior any user-typed [[stem]] has).
describe('Sources wikilinks resolve back to the source notes', () => {
  it('each emitted [[basename]] resolves to its source path', () => {
    const sources = ['notes/alpha.md', 'notes/sub/beta.md'];
    const out = buildSummaryNote('Notes', 'S.', sources, { includedCount: 2, totalCount: 2, bodyTruncated: false });
    const resolver = buildWikiResolver(sources);
    const links = findWikiLinks(out);
    expect(links.length).toBe(2);
    expect(resolver.resolve(links[0].target)).toBe('notes/alpha.md');
    expect(resolver.resolve(links[1].target)).toBe('notes/sub/beta.md');
  });
});
