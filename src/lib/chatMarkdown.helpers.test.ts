import { describe, it, expect } from 'vitest';
import { parseChatMarkdown } from './chatMarkdown.helpers';

describe('parseChatMarkdown', () => {
  it('renders a plain paragraph', () => {
    expect(parseChatMarkdown('hello world')).toEqual([
      { type: 'paragraph', children: [{ type: 'text', value: 'hello world' }] },
    ]);
  });

  it('renders bold, italic, strikethrough, inline code', () => {
    expect(parseChatMarkdown('a **b** _c_ ~~d~~ `e`')).toEqual([
      { type: 'paragraph', children: [
        { type: 'text', value: 'a ' },
        { type: 'strong', children: [{ type: 'text', value: 'b' }] },
        { type: 'text', value: ' ' },
        { type: 'em', children: [{ type: 'text', value: 'c' }] },
        { type: 'text', value: ' ' },
        { type: 'strike', children: [{ type: 'text', value: 'd' }] },
        { type: 'text', value: ' ' },
        { type: 'code', value: 'e' },
      ] },
    ]);
  });

  it('renders headings and trims the leading space', () => {
    expect(parseChatMarkdown('## Section')).toEqual([
      { type: 'heading', level: 2, children: [{ type: 'text', value: 'Section' }] },
    ]);
  });

  it('renders a fenced code block with language and preserves content', () => {
    expect(parseChatMarkdown('```py\nprint(1)\n```')).toEqual([
      { type: 'codeBlock', lang: 'py', value: 'print(1)' },
    ]);
  });

  it('renders a fenced code block without a language as null lang', () => {
    expect(parseChatMarkdown('```\nx\n```')).toEqual([
      { type: 'codeBlock', lang: null, value: 'x' },
    ]);
  });

  it('renders a bullet list with inline styling inside items', () => {
    expect(parseChatMarkdown('- item **x**\n- item _y_')).toEqual([
      { type: 'bulletList', items: [
        [{ type: 'paragraph', children: [
          { type: 'text', value: 'item ' },
          { type: 'strong', children: [{ type: 'text', value: 'x' }] },
        ] }],
        [{ type: 'paragraph', children: [
          { type: 'text', value: 'item ' },
          { type: 'em', children: [{ type: 'text', value: 'y' }] },
        ] }],
      ] },
    ]);
  });

  it('renders an ordered list preserving its start number', () => {
    expect(parseChatMarkdown('3. three\n4. four')).toEqual([
      { type: 'orderedList', start: 3, items: [
        [{ type: 'paragraph', children: [{ type: 'text', value: 'three' }] }],
        [{ type: 'paragraph', children: [{ type: 'text', value: 'four' }] }],
      ] },
    ]);
  });

  it('renders nested lists', () => {
    expect(parseChatMarkdown('- a\n  - a1\n- b')).toEqual([
      { type: 'bulletList', items: [
        [
          { type: 'paragraph', children: [{ type: 'text', value: 'a' }] },
          { type: 'bulletList', items: [
            [{ type: 'paragraph', children: [{ type: 'text', value: 'a1' }] }],
          ] },
        ],
        [{ type: 'paragraph', children: [{ type: 'text', value: 'b' }] }],
      ] },
    ]);
  });

  it('renders a blockquote without the quote marker', () => {
    expect(parseChatMarkdown('> quoted')).toEqual([
      { type: 'blockquote', children: [
        { type: 'paragraph', children: [{ type: 'text', value: 'quoted' }] },
      ] },
    ]);
  });

  it('renders a horizontal rule', () => {
    expect(parseChatMarkdown('---')).toEqual([{ type: 'hr' }]);
  });

  it('renders a link with its href and label', () => {
    expect(parseChatMarkdown('see [docs](https://x.com) now')).toEqual([
      { type: 'paragraph', children: [
        { type: 'text', value: 'see ' },
        { type: 'link', href: 'https://x.com', children: [{ type: 'text', value: 'docs' }] },
        { type: 'text', value: ' now' },
      ] },
    ]);
  });

  it('falls back to literal text for unterminated emphasis', () => {
    expect(parseChatMarkdown('a **bo')).toEqual([
      { type: 'paragraph', children: [{ type: 'text', value: 'a **bo' }] },
    ]);
  });

  it('returns an empty array for empty input', () => {
    expect(parseChatMarkdown('')).toEqual([]);
  });
});
