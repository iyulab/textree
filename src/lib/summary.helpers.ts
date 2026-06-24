// Pure helpers for scope summary (generative tier-1). Summary-specific retrieval and prompt.
// Imported by tests; chatStore (runes) consumes these. No runes here.
import type { TreeNode, SemanticHit } from './ipc';
import type { ChatMessage } from './ask.helpers';

export interface ScopeNote {
  path: string;
  body: string;
}

export interface BudgetResult {
  hits: SemanticHit[];
  includedCount: number;
  totalCount: number;
}

const DEFAULT_MAX_NOTES = 30;
const DEFAULT_TOTAL_BUDGET = 12000;

const SUMMARY_SYSTEM =
  'You are a notes summarizer. Summarize ONLY the note excerpts provided below. ' +
  'Group related points together and be concise. ' +
  'Do not invent facts that are not present in the excerpts.';

/** A note's content path: leaf -> its own path; container -> its folder-note body (if any). */
function notePathOf(node: TreeNode): string | null {
  if (node.kind === 'leaf') return node.path;
  return node.body_path; // container: folder note, or null
}

function collectFrom(nodes: TreeNode[], acc: string[]): string[] {
  for (const n of nodes) {
    const p = notePathOf(n);
    if (p) acc.push(p);
    if (n.children?.length) collectFrom(n.children, acc);
  }
  return acc;
}

function findNode(nodes: TreeNode[], path: string): TreeNode | null {
  for (const n of nodes) {
    if (n.path === path) return n;
    const found = n.children?.length ? findNode(n.children, path) : null;
    if (found) return found;
  }
  return null;
}

/**
 * Note content paths under a scope, in DFS (tree) order.
 * scopePath null -> whole vault. A folder scope includes its own folder note plus all descendants.
 * Mirrors +page.svelte collectNotePaths (leaf -> path, container -> body_path).
 */
export function collectScopeNotes(tree: TreeNode[], scopePath: string | null): string[] {
  if (scopePath === null) return collectFrom(tree, []);
  const node = findNode(tree, scopePath);
  if (!node) return [];
  if (node.kind === 'leaf') {
    const p = notePathOf(node);
    return p ? [p] : [];
  }
  return collectFrom([node], []);
}

/**
 * Concatenate note bodies into context hits for a no-query summary: cap to maxNotes (tree order),
 * give each non-blank note an equal slice of the char budget. includedCount/totalCount surface
 * honest truncation. score is meaningless for a summary -> fixed 1.
 */
export function budgetConcat(
  notes: ScopeNote[],
  opts: { maxNotes?: number; totalBudget?: number } = {},
): BudgetResult {
  const maxNotes = opts.maxNotes ?? DEFAULT_MAX_NOTES;
  const totalBudget = opts.totalBudget ?? DEFAULT_TOTAL_BUDGET;
  const totalCount = notes.length;
  const nonEmpty = notes.filter((n) => n.body.trim().length > 0);
  const capped = nonEmpty.slice(0, maxNotes);
  if (capped.length === 0) return { hits: [], includedCount: 0, totalCount };
  const perNote = Math.max(1, Math.floor(totalBudget / capped.length));
  const hits = capped.map((n): SemanticHit => ({ path: n.path, snippet: n.body.slice(0, perNote), score: 1 }));
  return { hits, includedCount: capped.length, totalCount };
}

/** Build the summary chat messages: summary grounding system prompt + excerpt context + instruction. */
export function buildSummaryMessages(scopeLabel: string, result: BudgetResult): ChatMessage[] {
  const context = result.hits.map((h) => `Source: ${h.path}\n${h.snippet}`).join('\n\n---\n\n');
  const truncation =
    result.includedCount < result.totalCount
      ? `\n\n(Summarizing ${result.includedCount} of ${result.totalCount} notes in scope.)`
      : '';
  return [
    { role: 'system', content: SUMMARY_SYSTEM },
    { role: 'user', content: `Notes from "${scopeLabel}":\n\n${context}\n\nSummarize these notes.${truncation}` },
  ];
}
