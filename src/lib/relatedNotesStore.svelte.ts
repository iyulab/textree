/*
 * Related-notes store — semantic similarity panel for the open note.
 *
 * Calls the AI sidecar's semantic_search with the note's leading text as the query and
 * excludes the current note from results. Degrades silently (empty list) when the sidecar
 * is absent or returns an error. Excluded from vitest (runes module) — the filtering logic
 * is unit-tested via the pure helper (relatedNotes.helpers.ts).
 */

import { semanticSearch } from "./ipc";
import { excludeSelf, type RelatedNote } from "./relatedNotes.helpers";

class RelatedNotesStore {
  items = $state<RelatedNote[]>([]);

  /**
   * Fetch semantically related notes for the open note.
   *
   * @param vaultRoot  Absolute vault path (passed to the IPC).
   * @param notePath   Vault-relative POSIX path of the open note (used to exclude self).
   * @param noteBody   Current body text; the first 800 characters serve as the semantic query.
   * @param scopePath  Absolute folder path to scope the search, or null for the whole vault.
   */
  async load(
    vaultRoot: string,
    notePath: string,
    noteBody: string,
    scopePath: string | null,
  ): Promise<void> {
    try {
      const query = noteBody.slice(0, 800); // lead chunk as the semantic query
      const hits = await semanticSearch(vaultRoot, query, scopePath, 6);
      this.items = excludeSelf(hits, notePath);
    } catch {
      this.items = []; // degrade silently — sidecar absent or unavailable
    }
  }

  /** Drop all results (vault switch / editor close). */
  clear(): void {
    this.items = [];
  }
}

export const relatedNotes = new RelatedNotesStore();
