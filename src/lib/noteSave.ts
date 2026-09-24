// Note Save — the open note's autosave and what stands between the editor and leaving it.
//
// Responsibility: hold the edit not yet on disk, write it one save at a time (each only replacing
// the note while it still holds what the edit started from), and say whether the note can be left.
// It knows nothing of the DOM: the page draws banners and moves focus; here we only decide.
//
// The observable part (`NoteSaveState`) is handed in by the page as reactive state, so the banners
// follow it without a mirror; tests hand in a plain object. Everything else is private bookkeeping.

import { friendlyError, type FriendlyError } from "./friendlyError.helpers";
import type { WriteOutcome } from "./ipc";
import { noteStem } from "./vaultPaths.helpers";

/** An edit not yet on disk. `base` is what the note held when the edit started from it. */
export interface PendingEdit {
  path: string;
  text: string;
  base: string;
}

/** What the page shows about the open note's saving. */
export interface NoteSaveState {
  /** The open note has edits that are not on disk yet. Follows the pending edit — never set apart. */
  dirty: boolean;
  /** The last failure to report — a save, but also anything else the page reports through it. */
  saveError: FriendlyError | null;
  /** The last save of the open note's edits failed and they are still unsaved. */
  saveFailure: FriendlyError | null;
  /** The open note was moved or deleted outside the app. */
  removed: boolean;
  /** The copy on disk, while the open note's edits conflict with a change made elsewhere. */
  conflictDisk: string | null;
}

export function initialNoteSaveState(): NoteSaveState {
  return { dirty: false, saveError: null, saveFailure: null, removed: false, conflictDisk: null };
}

/**
 * What saving before leaving came to. `asking`: a question about the unsaved edits is still open —
 * which copy wins, or whether to put back a note deleted elsewhere. `failed`: the edits could not be
 * saved. Neither may be left: leaving would drop the edits. `busy`: edits are still arriving —
 * opening another note may go ahead (they are written to this one), but moving, renaming or
 * deleting this note must not, since a write landing afterwards would bring the old path back.
 */
export type LeaveOutcome = "saved" | "asking" | "failed" | "busy";

/** Where a structure change put a path; a path it did not move comes back unchanged. */
export type Remap = (path: string) => string;

/** Which banner a refusal to leave points at. */
export type LeaveQuestion = "conflict" | "removed" | "failure";

export interface NoteSaveDeps {
  root: () => string | null;
  /** Body path of the open note (null if none). */
  activePath: () => string | null;
  /** Replace a note's content only while it still holds `expected`. */
  write: (root: string, path: string, text: string, expected: string) => Promise<WriteOutcome>;
  /** A save landed. */
  saved?: (path: string, text: string) => void;
  /** An edit was made (anything said about the note as it was no longer holds). */
  edited?: () => void;
  /** The open note was found gone. */
  removed?: () => void;
  /** A structure change moved paths (see `move`); the open note may be among them. */
  moved?: (remap: Remap) => void;
  /**
   * Keep `text` as a new note beside `path` without overwriting anything, and say where it went.
   * Used for edits to a note already left that can no longer be written to it.
   */
  keepCopy?: (root: string, path: string, text: string) => Promise<string>;
  /** Leaving was refused; the named banner should be brought to the person's attention. */
  ask?: (question: LeaveQuestion) => void;
  debounceMs?: number;
}

const DEFAULT_DEBOUNCE_MS = 500;
/** Extra saves before leaving for edits typed while the last one ran. Bounded, so someone who never
 *  stops typing is not held. */
const MAX_TRAILING_SAVES = 5;

export class NoteSave {
  #synced = "";

  #pending: PendingEdit | null = null;
  // Edits to a note already left whose write failed. Nothing on screen holds them any more, so they
  // are kept here — with the vault they belong to — and tried again before every save until they land.
  #stranded = new Map<string, { root: string; edit: PendingEdit }>();
  #warnedOnClose = false;
  // Saves run one at a time. Two in flight would each be based on the same state, and the second
  // would mistake the first for a change made elsewhere.
  #saving: Promise<void> = Promise.resolve();
  #timer: ReturnType<typeof setTimeout> | null = null;
  readonly #debounceMs: number;

  constructor(
    readonly state: NoteSaveState,
    private readonly deps: NoteSaveDeps,
  ) {
    this.#debounceMs = deps.debounceMs ?? DEFAULT_DEBOUNCE_MS;
  }

  /**
   * The last text the editor and disk were known to agree on. Set by every load and moved forward by
   * every successful save of the open note; the external-change reconciliation compares disk against
   * it to tell an edit made elsewhere from the person's own unsaved typing.
   */
  get synced(): string {
    return this.#synced;
  }

  /** The edit not yet on disk, if any. */
  get pending(): Readonly<PendingEdit> | null {
    return this.#pending;
  }

  /** The editor changed `path` to `text`. Saved after a pause in typing. */
  schedule(path: string, text: string): void {
    if (this.#pending && this.#pending.path !== path) {
      // Edits to the note being left, typed while the next one was loading (the editor stays on the
      // old note until then). They are written, not replaced by the first edit to the new one.
      const left = this.#pending;
      this.#pending = null;
      this.#enqueue(() => this.#write(left));
    }
    const base = this.#pending?.path === path ? this.#pending.base : this.#synced;
    this.#pending = { path, text, base };
    this.#syncDirty();
    this.deps.edited?.();
    this.#cancelTimer();
    this.#timer = setTimeout(() => {
      this.#timer = null;
      // While the note is in conflict with a change on disk, the person has been asked which copy
      // wins. Saving on a timer would answer for them — and overwrite the copy on disk. The edit
      // stays pending until they choose (keepMine saves it; takeDisk drops it).
      if (this.state.conflictDisk !== null) return;
      void this.flush();
    }, this.#debounceMs);
  }

  /**
   * Write the pending edit now. Never throws: a failure is reported through the state and the edit
   * stays pending, so the next flush retries. While the open note waits on an answer (a conflict, or
   * whether to put it back) it writes nothing — saving would answer for the person.
   */
  flush(): Promise<void> {
    this.#cancelTimer();
    return this.#enqueue(async () => {
      // The open note's own edits first: kept edits for a note already left must not hold them up.
      if (this.#pending) await this.#write(this.#pending);
      for (const { root, edit } of [...this.#stranded.values()]) await this.#write(edit, root);
    });
  }

  /** Edits to notes already left that are not on disk yet (see `#stranded`). */
  get stranded(): number {
    return this.#stranded.size;
  }

  /** Save before leaving the open note — switching, changing vault, restructuring, or closing. */
  async beforeLeaving(): Promise<LeaveOutcome> {
    await this.flush();
    // Edits typed while that save ran are pending again without anything having gone wrong.
    const s = this.state;
    for (let i = 0; i < MAX_TRAILING_SAVES && this.#pending && !s.saveFailure && s.conflictDisk === null && !s.removed; i++) {
      await this.flush();
    }
    if (!this.#pending) return "saved";
    if (s.conflictDisk !== null) {
      this.deps.ask?.("conflict");
      return "asking";
    }
    if (s.removed) {
      this.deps.ask?.("removed");
      return "asking";
    }
    // The save failed: the edits exist only in the editor. The banner offers another try or letting
    // them go.
    if (s.saveFailure) {
      this.deps.ask?.("failure");
      return "failed";
    }
    // Nothing went wrong — the typing simply has not stopped. What is still pending is saved to this
    // note like any other edit, even after another note opens (see schedule).
    return "busy";
  }

  /**
   * The app is about to close. `stay`: something is unsaved and the person has been shown why.
   * Edits kept for a note already left live only in memory, so the first close that finds them
   * still unwritten stays open and warns; the next one closes without them — a disk that never
   * takes them must not keep the app from closing.
   */
  async beforeClosing(): Promise<"close" | "stay"> {
    if (!this.#pending && this.#stranded.size === 0) return "close";
    if ((await this.beforeLeaving()) !== "saved") return "stay";
    if (this.#stranded.size > 0 && !this.#warnedOnClose) {
      this.#warnedOnClose = true;
      this.state.saveError = friendlyError(
        "Some edits to a note you left are still not saved. Close again to quit without them.",
      );
      return "stay";
    }
    return "close";
  }

  /**
   * A note holding `text` on disk was opened (or none, with ""). An edit still pending for the one
   * left keeps its own path.
   */
  opened(text: string): void {
    this.#synced = text;
    this.state.removed = false;
    this.state.conflictDisk = null;
    this.#syncDirty(); // an edit still pending for this very note keeps it unsaved
  }

  /** The editor was closed and its unsaved edits with it. */
  closed(): void {
    this.#cancelTimer();
    this.#pending = null;
    this.#synced = "";
    this.#syncDirty();
    // The edit context is gone, so the errors about it are too — except the warning about edits kept
    // for a note left earlier, which are still waiting to land.
    if (this.#stranded.size === 0) this.state.saveError = null;
    this.state.saveFailure = null;
    this.state.removed = false;
    this.state.conflictDisk = null;
  }

  /** The editor was reloaded with `text` from disk; anything unsaved is gone. */
  reloaded(text: string): void {
    this.#synced = text;
    this.#pending = null;
    this.#syncDirty();
    if (this.#stranded.size === 0) this.state.saveError = null;
    this.state.saveFailure = null;
    this.state.removed = false; // back to normal if it was re-created after being deleted
  }

  /** The open note was moved or deleted outside the app. */
  markRemoved(): void {
    this.state.removed = true;
    this.state.saveFailure = null; // the note being gone is the question now, not the failed save
    this.state.conflictDisk = null; // deletion takes priority over a conflict
    // Unsaved edits stay in the editor. The file is gone, but what they typed since the last save
    // exists nowhere else — they decide whether to put the note back with it or let it go.
    this.#syncDirty();
    this.deps.removed?.();
  }

  /** The open note is on disk again (put back elsewhere, or by a sync tool). */
  markPresent(): boolean {
    if (!this.state.removed) return false;
    this.state.removed = false;
    if (this.#pending) void this.flush(); // saves resume against what the note last saved held
    return true;
  }

  /** The open note's edits conflict with `disk`, a change made elsewhere. */
  markConflict(disk: string): void {
    this.state.removed = false;
    this.state.conflictDisk = disk;
  }

  /** Conflict: keep my edits — they replace the copy the banner showed (and only that copy). */
  keepMine(): void {
    if (this.state.conflictDisk !== null && this.#pending) this.#pending.base = this.state.conflictDisk;
    this.state.conflictDisk = null;
    void this.flush();
  }

  /** Conflict: take the copy on disk. Returns it for the editor to reload with, if there was one. */
  takeDisk(): string | null {
    this.#cancelTimer();
    const disk = this.state.conflictDisk;
    this.state.conflictDisk = null;
    return disk;
  }

  /** Save failure: let the unsaved edits go. Returns what the note last saved, to reload with. */
  discardUnsaved(): string {
    this.#cancelTimer();
    return this.#synced;
  }

  /** Removed note: let the unsaved edits go. */
  discardRemoved(): void {
    this.#cancelTimer();
    this.#pending = null;
    this.#syncDirty();
  }

  /**
   * The removed note was put back holding `text`, at `movedTo` when it could not return to its own
   * path. Edits typed while that ran stay pending, now on top of what was put back.
   */
  putBack(text: string, movedTo: string | null): void {
    this.#synced = text;
    if (this.#pending?.text === text) {
      this.#pending = null;
    } else if (this.#pending) {
      this.#pending.base = text;
    }
    if (movedTo !== null && this.#pending) this.#pending.path = movedTo;
    this.#syncDirty();
    this.state.removed = false;
    this.state.saveError = null;
  }

  /**
   * Move notes on disk — the open one, or a folder holding it. `relocate` does the move and says
   * where paths went. It runs between saves, never alongside one: a save landing on the old path
   * mid-move would bring the note back there, or report it gone. An edit made while it ran is
   * carried to where its note went.
   */
  move(relocate: () => Promise<Remap>): Promise<void> {
    return this.#enqueue(async () => {
      const remap = await relocate();
      if (this.#pending) this.#pending.path = remap(this.#pending.path);
      this.deps.moved?.(remap);
      this.#syncDirty();
    });
  }

  async #keepAsCopy(job: PendingEdit, root: string): Promise<void> {
    const s = this.state;
    const name = noteStem(job.path);
    try {
      if (!this.deps.keepCopy) throw new Error("no place to keep them");
      const copy = await this.deps.keepCopy(root, job.path, job.text);
      // Said even over another warning — the copy is news — and the other warning is carried along.
      const others = this.#stranded.size > 0 ? " Other edits are still waiting to be saved." : "";
      s.saveError = friendlyError(
        `"${name}" changed on disk before your earlier edits to it could be saved, so they were kept as "${noteStem(copy)}".${others}`,
      );
    } catch (e) {
      this.#stranded.set(job.path, { root, edit: job });
      s.saveError = friendlyError(
        `Your last edits to "${name}" are not saved yet — it changed on disk, and keeping them as a copy failed (${friendlyError(e).summary}). They are kept and tried again with every save.`,
      );
    }
  }

  #syncDirty(): void {
    const open = this.deps.activePath();
    this.state.dirty = this.#pending !== null && open !== null && this.#pending.path === open;
  }

  #enqueue(job: () => Promise<void>): Promise<void> {
    const run = this.#saving.then(job);
    this.#saving = run.catch(() => {});
    return run;
  }

  #cancelTimer(): void {
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  async #write(job: PendingEdit, rootOf?: string): Promise<void> {
    const root = rootOf ?? this.deps.root();
    if (!root) return;
    const retry = rootOf !== undefined;
    const s = this.state;
    const open = this.deps.activePath();
    if (job.path === open && (s.conflictDisk !== null || s.removed)) return;
    try {
      const outcome = await this.deps.write(root, job.path, job.text, job.base);
      if (retry) this.#stranded.delete(job.path);
      const active = retry ? null : this.deps.activePath();
      if (outcome.kind === "conflict") {
        if (job.path !== active) {
          // Edits to a note already left that changed (or went) on disk meanwhile: there is no banner
          // to ask on, and writing would overwrite the other change. Keep them as a copy beside it.
          await this.#keepAsCopy(job, root);
          return;
        }
        s.saveError = null;
        s.saveFailure = null;
        // Answered while this save was in flight (e.g. they took the copy on disk): nothing to ask.
        if (!this.#pending) return;
        if (outcome.disk === null) this.markRemoved();
        else s.conflictDisk = outcome.disk;
        return;
      }
      if (retry) {
        // Kept edits landed. Whatever was said about them no longer holds, unless others remain.
        if (this.#stranded.size === 0 && !this.state.saveFailure) this.state.saveError = null;
        if (root === this.deps.root()) this.deps.saved?.(job.path, job.text);
        return;
      }
      this.deps.saved?.(job.path, job.text);
      if (job.path === active) this.#synced = job.text;
      if (this.#pending === job) {
        this.#pending = null;
        this.#syncDirty();
      } else if (this.#pending?.path === job.path) {
        this.#pending.base = job.text; // the newer edit now starts from what this save put on disk
      }
      // Only the open note's own save speaks for it: a note already left saving fine says nothing
      // about whether the open one's edits are saved.
      if (job.path === active) {
        s.saveFailure = null;
        // A note left behind with edits still not on disk keeps its warning up.
        if (this.#stranded.size === 0) s.saveError = null;
      }
    } catch (e) {
      const active = this.deps.activePath();
      if (!retry && this.#pending !== job && job.path !== active) {
        // Edits to a note already left: nothing on screen holds them, so keep them to try again.
        this.#stranded.set(job.path, { root, edit: job });
      }
      if (retry || this.#stranded.has(job.path)) {
        const why = friendlyError(e);
        s.saveError = friendlyError(
          `Your last edits to "${noteStem(job.path)}" are not saved yet (${why.summary}). They are kept and tried again with every save.`,
        );
        return;
      }
      // The edit stays pending, so it can be tried again.
      s.saveError = friendlyError(e);
      if (job.path === active) s.saveFailure = s.saveError;
    }
  }
}
