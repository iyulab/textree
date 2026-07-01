/*
 * Chat answer markdown → AST. Reuses the editor's markdown parser (@lezer/markdown GFM) so chat
 * answers render with the same markdown understanding as notes, without a new renderer dependency.
 * Pure (no DOM) so it is unit-tested; ChatMarkdown.svelte renders the returned AST read-only.
 *
 * Scope (C6): paragraphs, headings, bold/italic/strikethrough/inline code, fenced code blocks,
 * bullet/ordered (nested) lists, blockquotes, horizontal rules, links. Tables, images, and task
 * checkboxes are out of scope and degrade to literal text (never break).
 */

import { parser as baseParser, GFM } from '@lezer/markdown';
import type { SyntaxNode } from '@lezer/common';

const parser = baseParser.configure(GFM);

export type MdInline =
  | { type: 'text'; value: string }
  | { type: 'strong'; children: MdInline[] }
  | { type: 'em'; children: MdInline[] }
  | { type: 'strike'; children: MdInline[] }
  | { type: 'code'; value: string }
  | { type: 'link'; href: string; children: MdInline[] };

export type MdBlock =
  | { type: 'paragraph'; children: MdInline[] }
  | { type: 'heading'; level: number; children: MdInline[] }
  | { type: 'bulletList'; items: MdBlock[][] }
  | { type: 'orderedList'; start: number; items: MdBlock[][] }
  | { type: 'codeBlock'; lang: string | null; value: string }
  | { type: 'blockquote'; children: MdBlock[] }
  | { type: 'hr' };

/** Marker/structural nodes skipped when reconstructing inline content (their glyphs are not shown). */
const INLINE_MARK = new Set([
  'HeaderMark', 'EmphasisMark', 'CodeMark', 'LinkMark', 'QuoteMark',
  'ListMark', 'StrikethroughMark', 'URL', 'LinkTitle',
]);

/**
 * Rebuild inline content of a node. lezer-markdown emits no explicit text nodes — plain runs are the
 * gaps between element children — so we walk children, emit gap text, and map styled elements.
 */
function reconstructInline(node: SyntaxNode, src: string): MdInline[] {
  const out: MdInline[] = [];
  let pos = node.from;
  const pushText = (s: string) => {
    if (!s) return;
    const last = out[out.length - 1];
    if (last && last.type === 'text') last.value += s;
    else out.push({ type: 'text', value: s });
  };
  for (let c = node.firstChild; c; c = c.nextSibling) {
    if (c.from > pos) pushText(src.slice(pos, c.from));
    switch (c.name) {
      case 'StrongEmphasis': out.push({ type: 'strong', children: reconstructInline(c, src) }); break;
      case 'Emphasis': out.push({ type: 'em', children: reconstructInline(c, src) }); break;
      case 'Strikethrough': out.push({ type: 'strike', children: reconstructInline(c, src) }); break;
      case 'InlineCode': out.push({ type: 'code', value: inlineCodeText(c, src) }); break;
      case 'Link': out.push(mapLink(c, src)); break;
      default:
        if (!INLINE_MARK.has(c.name)) pushText(src.slice(c.from, c.to));
    }
    pos = c.to;
  }
  if (node.to > pos) pushText(src.slice(pos, node.to));
  return out;
}

/** Text inside `InlineCode`, stripping the surrounding backtick CodeMarks. */
function inlineCodeText(node: SyntaxNode, src: string): string {
  const marks: SyntaxNode[] = [];
  for (let c = node.firstChild; c; c = c.nextSibling) if (c.name === 'CodeMark') marks.push(c);
  if (marks.length >= 2) return src.slice(marks[0].to, marks[marks.length - 1].from);
  return src.slice(node.from, node.to);
}

function mapLink(node: SyntaxNode, src: string): MdInline {
  let href = '';
  for (let c = node.firstChild; c; c = c.nextSibling) if (c.name === 'URL') href = src.slice(c.from, c.to);
  const text = src.slice(node.from, node.to);
  const close = text.indexOf(']');
  const label = close > 0 ? text.slice(1, close) : text;
  return { type: 'link', href, children: [{ type: 'text', value: label }] };
}

/** Headings/quotes leave a leading space after the marker; drop it from the first text node. */
function trimLeadingSpace(inline: MdInline[]): MdInline[] {
  if (inline.length && inline[0].type === 'text') {
    inline[0].value = inline[0].value.replace(/^\s+/, '');
    if (!inline[0].value) inline.shift();
  }
  return inline;
}

function mapBlocks(parent: SyntaxNode, src: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  for (let node = parent.firstChild; node; node = node.nextSibling) {
    const n = node.name;
    // Structural marks that appear as children of ListItem/Blockquote but are not blocks.
    if (n === 'ListMark' || n === 'QuoteMark') continue;
    const hm = /^ATXHeading([1-6])$/.exec(n);
    if (hm) {
      blocks.push({ type: 'heading', level: Number(hm[1]), children: trimLeadingSpace(reconstructInline(node, src)) });
      continue;
    }
    if (n === 'Paragraph') { blocks.push({ type: 'paragraph', children: reconstructInline(node, src) }); continue; }
    if (n === 'BulletList') { blocks.push({ type: 'bulletList', items: mapListItems(node, src) }); continue; }
    if (n === 'OrderedList') { blocks.push({ type: 'orderedList', start: orderedStart(node, src), items: mapListItems(node, src) }); continue; }
    if (n === 'FencedCode' || n === 'CodeBlock') { blocks.push(mapCodeBlock(node, src)); continue; }
    if (n === 'Blockquote') { blocks.push({ type: 'blockquote', children: mapBlocks(node, src) }); continue; }
    if (n === 'HorizontalRule') { blocks.push({ type: 'hr' }); continue; }
    // Unsupported/deferred block (e.g. Table): degrade to a literal paragraph of its source text.
    const txt = src.slice(node.from, node.to).trim();
    if (txt) blocks.push({ type: 'paragraph', children: [{ type: 'text', value: txt }] });
  }
  return blocks;
}

function mapListItems(listNode: SyntaxNode, src: string): MdBlock[][] {
  const items: MdBlock[][] = [];
  for (let li = listNode.firstChild; li; li = li.nextSibling) {
    if (li.name === 'ListItem') items.push(mapBlocks(li, src));
  }
  return items;
}

function orderedStart(listNode: SyntaxNode, src: string): number {
  const li = listNode.firstChild;
  if (li) {
    const m = /^(\d+)/.exec(src.slice(li.from, li.to));
    if (m) return Number(m[1]);
  }
  return 1;
}

function mapCodeBlock(node: SyntaxNode, src: string): MdBlock {
  let lang: string | null = null;
  let textNode: SyntaxNode | null = null;
  for (let c = node.firstChild; c; c = c.nextSibling) {
    if (c.name === 'CodeInfo') lang = src.slice(c.from, c.to);
    if (c.name === 'CodeText') textNode = c;
  }
  const value = textNode ? src.slice(textNode.from, textNode.to) : src.slice(node.from, node.to);
  return { type: 'codeBlock', lang, value };
}

/** Parse chat answer text to a small block AST for read-only rendering. */
export function parseChatMarkdown(text: string): MdBlock[] {
  return mapBlocks(parser.parse(text).topNode, text);
}
