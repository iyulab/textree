/**
 * Writes settings files so that a failed write is not forgotten.
 *
 * A settings write that fails keeps the screen as it is — the person's change is still in memory —
 * and is written again on the next save of any file this writer handles, from whatever the state
 * is by then. Without that, a failed favorites write followed by a successful order write would
 * leave the favorites file behind for good, with nothing on screen to say so.
 *
 * Pure (no runes, no IPC): the actual write is injected, so this is tested directly.
 */

export type WriteFile = (rel: string, body: string) => Promise<void>;

export class RetryingWriter {
  /** File → how to serialize its current state, for every file whose last write did not land. */
  private unsaved = new Map<string, () => string>();

  constructor(
    private readonly write: WriteFile,
    private readonly onError: (rel: string, error: unknown) => void = () => {},
  ) {}

  /** Write `rel`, and retry every file an earlier save left unwritten. */
  async save(rel: string, serialize: () => string): Promise<void> {
    this.unsaved.set(rel, serialize);
    for (const [file, current] of [...this.unsaved]) {
      try {
        await this.write(file, current());
        // A newer save may have queued this file again meanwhile; that one is still owed.
        if (this.unsaved.get(file) === current) this.unsaved.delete(file);
      } catch (e) {
        this.onError(file, e);
      }
    }
  }

  /** Files still owed a write. */
  pending(): string[] {
    return [...this.unsaved.keys()];
  }

  /**
   * Forget owed writes. Called when the folder changes: the state they would write belongs to the
   * folder being left, and must not land in the one being opened.
   */
  reset(): void {
    this.unsaved.clear();
  }
}
