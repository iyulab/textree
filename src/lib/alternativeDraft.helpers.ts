/**
 * Keeps what is typed into an alternative while it is open in the editor.
 *
 * The text goes to the alternative's draft — kept outside the folder, never to the note's file —
 * a moment after typing stops, and at once when the person goes back to the note or leaves. One
 * writer belongs to one alternative of one folder: it carries both, so a write still owed when the
 * person moves on lands where it was typed, not wherever is open by then.
 *
 * A write that fails keeps the text owed and is tried again with the next one (or the next flush).
 *
 * Pure (no runes, no IPC): the write and the clock are injected, so this is tested directly.
 */

export type WriteDraft = (root: string, id: string, text: string) => Promise<void>;

export type Clock = {
  set: (run: () => void, ms: number) => unknown;
  clear: (handle: unknown) => void;
};

const realClock: Clock = {
  set: (run, ms) => setTimeout(run, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export class DraftWriter {
  /** Text typed and not yet written, or null when the draft holds everything typed. */
  private owed: string | null = null;
  private timer: unknown = null;
  /** The write in progress, if any — writes go one at a time so an older text never lands last. */
  private running: Promise<void> = Promise.resolve();

  constructor(
    readonly root: string,
    readonly id: string,
    private readonly write: WriteDraft,
    private readonly onError: (error: unknown) => void = () => {},
    private readonly delayMs = 400,
    private readonly clock: Clock = realClock,
  ) {}

  /** The editor holds `text` now. */
  schedule(text: string): void {
    this.owed = text;
    if (this.timer !== null) this.clock.clear(this.timer);
    this.timer = this.clock.set(() => {
      this.timer = null;
      void this.flush();
    }, this.delayMs);
  }

  /** Whether typed text has not reached the draft yet. */
  get pending(): boolean {
    return this.owed !== null;
  }

  /**
   * Writes what is owed now. Resolves once everything typed so far is in the draft (true), or the
   * write failed and is still owed (false).
   */
  flush(): Promise<boolean> {
    if (this.timer !== null) {
      this.clock.clear(this.timer);
      this.timer = null;
    }
    const done = this.running.then(async () => {
      const text = this.owed;
      if (text === null) return;
      try {
        await this.write(this.root, this.id, text);
        // Typed again while this was written: that newer text is still owed.
        if (this.owed === text) this.owed = null;
      } catch (e) {
        this.onError(e);
      }
    });
    this.running = done;
    return done.then(() => this.owed === null);
  }
}
