// Pure helpers for saving a scope summary to a new note (generative D18 content-write).
// Imported by tests and by chatStore (runes) — no runes here. The scope kind is taken as an
// inline union (not imported from the chatStore runes module) to keep the pure/runes separation
// clean, mirroring summary.helpers.ts which takes primitives rather than importing ChatScope.
import { sanitizeForFilename } from './h1sync.helpers';

export interface SummaryNoteMeta {
  includedCount: number;
  totalCount: number;
  bodyTruncated: boolean;
}

/** The parent folder that receives the summary note (absolute path):
 *  folder scope -> the folder itself; file scope -> the file's containing folder;
 *  vault scope (path null) -> the vault root. */
export function summaryNoteParent(
  scopeKind: 'file' | 'folder' | 'vault',
  scopePath: string | null,
  root: string,
): string {
  if (scopeKind === 'vault' || scopePath === null) return root;
  if (scopeKind === 'folder') return scopePath;
  const i = Math.max(scopePath.lastIndexOf('/'), scopePath.lastIndexOf('\\'));
  return i >= 0 ? scopePath.slice(0, i) : root;
}

/** File-name stem for the summary note, sanitized. Falls back to "Summary" when the label
 *  sanitizes to nothing usable (the Rust boundary is the final authority on validity). */
export function summaryNoteBaseName(scopeLabel: string): string {
  const base = sanitizeForFilename(`Summary of ${scopeLabel}`);
  return base.length > 0 ? base : 'Summary';
}

/** Wikilink for a source note: [[<basename without .md>]] (app wikilink resolution is by name). */
function wikilink(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  const name = (i >= 0 ? path.slice(i + 1) : path).replace(/\.md$/i, '');
  return `[[${name}]]`;
}

/** Assemble the summary note body: H1 + summary text + Sources list + honest truncation footnote. */
export function buildSummaryNote(
  scopeLabel: string,
  summaryText: string,
  sourcePaths: string[],
  meta: SummaryNoteMeta,
): string {
  const parts: string[] = [`# Summary of ${scopeLabel}`, '', summaryText.trim()];
  if (sourcePaths.length) {
    parts.push('', '## Sources', '', ...sourcePaths.map((p) => `- ${wikilink(p)}`));
  }
  const notes: string[] = [];
  if (meta.includedCount < meta.totalCount)
    notes.push(`Summary based on ${meta.includedCount} of ${meta.totalCount} notes.`);
  if (meta.bodyTruncated) notes.push('Some note excerpts are truncated.');
  if (notes.length) parts.push('', `_(${notes.join(' ')})_`);
  return parts.join('\n') + '\n';
}
