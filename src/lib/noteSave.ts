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
import { noteStem, pathInside, pathKey, samePath } from "./vaultPaths.helpers";

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
  /** Kept edits landed on the note that is open again (see `#stranded`); show `text`, now on disk. */
  landedOnOpen?: (text: string) => void;
  /** Leaving was refused; the named banner should be brought to the person's attention. */
  ask?: (question: LeaveQuestion) => void;
  debounceMs?: number;
  /** How long closing waits for a save before treating the folder as not answering. Default 5 s. */
  closeWaitMs?: number;
}

const DEFAULT_DEBOUNCE_MS = 500;
/** Extra saves before leaving for edits typed while the last one ran. Bounded, so someone who never
 *  stops typing is not held. */
const MAX_TRAILING_SAVES = 5;
/** A save still running after this long when the window closes is taken as a folder that is not
 *  answering (a stalled sync or network drive). The wait is bounded so it cannot hold the app open. */
const DEFAULT_CLOSE_WAIT_MS = 5000;

export class NoteSave {
  #synced = "";

  #pending: PendingEdit | null = null;
  // Edits to a note already left whose write failed. Nothing on screen holds them any more, so they
  // are kept here — with the vault they belong to — and tried after every save until they land.
  // Oldest first, and never merged: two kept edits to one note are two things the person typed.
  #stranded: { root: string; edit: PendingEdit }[] = [];
  #warnedOnClose = false;
  #warnedStalledOnClose = false;
  // Saves to one note run one at a time, in order: two in flight would each be based on the same
  // state, and the second would mistake the first for a change made elsewhere. Saves to different
  // notes do not wait for each other — a folder that stops answering for one note holds up only it.
  #chains = new Map<string, Promise<void>>();
  // Structure changes under way, and the paths each one moves. A save to a path being moved waits
  // for the move and then goes where the note went; saves elsewhere carry on.
  #moves: { from: string[] | null; done: Promise<void> }[] = [];
  // Edits to a note already left, waiting their turn to be written. Moves carry them along.
  #queued = new Set<PendingEdit>();
  // What an edit was typed on top of: the edit to the same note before it, not on disk yet when this
  // one began, so both start from the same base. Once that one lands, this one starts from it —
  // otherwise it would take its own earlier edit for a change made elsewhere.
  #prev = new WeakMap<PendingEdit, PendingEdit>();
  // Edits on disk. An edit whose later edit landed first is done too: the later one holds it.
  #landed = new WeakSet<PendingEdit>();
  #done = new WeakSet<PendingEdit>();
  #running = new Map<PendingEdit, Promise<void>>();
  #timer: ReturnType<typeof setTimeout> | null = null;
  readonly #debounceMs: number;
  readonly #closeWaitMs: number;

  constructor(
    readonly state: NoteSaveState,
    private readonly deps: NoteSaveDeps,
  ) {
    this.#debounceMs = deps.debounceMs ?? DEFAULT_DEBOUNCE_MS;
    this.#closeWaitMs = deps.closeWaitMs ?? DEFAULT_CLOSE_WAIT_MS;
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
    if (this.#pending && !samePath(this.#pending.path, path)) {
      // Edits to the note being left, typed while the next one was loading (the editor stays on the
      // old note until then). They are written, not replaced by the first edit to the new one.
      this.#writeLeft(this.#pending);
      this.#pending = null;
    }
    const prev = this.#pending;
    const edit = { path, text, base: prev ? prev.base : this.#synced };
    if (prev) this.#prev.set(edit, prev);
    this.#pending = edit;
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
    // The open note's own edits, and the kept edits for notes already left, each in its own note's
    // turn — a kept edit that never lands does not hold up the open note.
    const own = this.#pending
      ? this.#onPath(this.#pending.path, async () => {
          // Read when its turn comes: a move meanwhile may have carried the edit elsewhere.
          if (this.#pending) await this.#write(this.#pending);
        })
      : Promise.resolve();
    const kept = [...this.#stranded].map(({ root, edit }) =>
      this.#onPath(edit.path, () => this.#write(edit, root)),
    );
    return Promise.all([own, ...kept]).then(() => {});
  }

  /** Edits to notes already left that are not on disk yet (see `#stranded`). */
  get stranded(): number {
    return this.#stranded.length;
  }

  #strand(root: string, edit: PendingEdit): void {
    this.#stranded.push({ root, edit });
    this.#warnedOnClose = false; // new edits waiting: the next close warns about them again
  }

  #unstrand(edit: PendingEdit): void {
    this.#stranded = this.#stranded.filter((k) => k.edit !== edit);
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
   *
   * The same holds for a save that never comes back at all: a write cannot be taken back once it
   * is handed to the disk, so the close does not cancel it — it stops waiting. The first close that
   * finds a save still running after the wait stays open and says so; the next one closes. Nothing
   * is written after it, so a write that lands late cannot overwrite anything newer.
   */
  async beforeClosing(): Promise<"close" | "stay"> {
    if (!this.#pending && this.#stranded.length === 0) return "close";
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stalled = new Promise<"stalled">((done) => {
      timer = setTimeout(() => done("stalled"), this.#closeWaitMs);
    });
    const left = await Promise.race([this.beforeLeaving(), stalled]);
    clearTimeout(timer);
    if (left === "stalled") {
      if (this.#warnedStalledOnClose) return "close";
      this.#warnedStalledOnClose = true;
      this.state.saveError = friendlyError(
        "Some edits are still being written — the folder isn't answering. Close again to quit without waiting for them.",
      );
      return "stay";
    }
    this.#warnedStalledOnClose = false;
    if (left !== "saved") return "stay";
    if (this.#stranded.length > 0 && !this.#warnedOnClose) {
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
    if (this.#stranded.length === 0) this.state.saveError = null;
    this.state.saveFailure = null;
    this.state.removed = false;
    this.state.conflictDisk = null;
  }

  /** The editor was reloaded with `text` from disk; anything unsaved is gone. */
  reloaded(text: string): void {
    this.#synced = text;
    this.#pending = null;
    this.#syncDirty();
    if (this.#stranded.length === 0) this.state.saveError = null;
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
    if (this.state.conflictDisk !== null && this.#pending) this.#rebase(this.#pending, this.state.conflictDisk);
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
      this.#rebase(this.#pending, text);
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
  move(relocate: () => Promise<Remap>, from?: string | string[]): Promise<void> {
    // `from`: the notes or folders being moved. Saves to anything else neither wait for the move nor
    // hold it up. Without it, the move waits for every save and every save waits for it.
    const moving = from === undefined ? null : [from].flat();
    const covers = (path: string) => moving === null || moving.some((m) => pathInside(path, m));
    const before = [
      ...[...this.#chains].filter(([key]) => covers(key)).map(([, tail]) => tail),
      ...this.#moves.map((m) => m.done),
    ];
    let finished!: () => void;
    const entry = { from: moving, done: new Promise<void>((r) => (finished = r)) };
    this.#moves.push(entry);
    const run = Promise.all(before).then(async () => {
      const remap = await relocate();
      if (this.#pending) this.#pending.path = remap(this.#pending.path);
      for (const edit of this.#queued) edit.path = remap(edit.path);
      for (const { edit } of this.#stranded) edit.path = remap(edit.path);
      this.deps.moved?.(remap);
      this.#syncDirty();
    });
    const end = () => {
      this.#moves = this.#moves.filter((m) => m !== entry);
      finished();
    };
    run.then(end, end);
    return run;
  }

  async #keepAsCopy(job: PendingEdit, root: string): Promise<void> {
    const s = this.state;
    const name = noteStem(job.path);
    try {
      if (!this.deps.keepCopy) throw new Error("no place to keep them");
      const copy = await this.deps.keepCopy(root, job.path, job.text);
      // Said even over another warning — the copy is news — and the other warning is carried along.
      const others = this.#stranded.length > 0 ? " Other edits are still waiting to be saved." : "";
      s.saveError = friendlyError(
        `"${name}" changed on disk before your earlier edits to it could be saved, so they were kept as "${noteStem(copy)}".${others}`,
      );
    } catch (e) {
      this.#strand(root, job);
      s.saveError = friendlyError(
        `Your last edits to "${name}" are not saved yet — it changed on disk, and keeping them as a copy failed (${friendlyError(e).summary}). They are kept and tried again with every save.`,
      );
    }
  }

  #syncDirty(): void {
    const open = this.deps.activePath();
    this.state.dirty = this.#pending !== null && open !== null && this.#pending.path === open;
  }

  /** Runs `job` in `path`'s turn: after the saves to it before this one, and after any move of it. */
  #onPath(path: string, job: () => Promise<void>): Promise<void> {
    const key = pathKey(path);
    const before = [
      this.#chains.get(key) ?? Promise.resolve(),
      ...this.#moves
        .filter((m) => m.from === null || m.from.some((f) => pathInside(path, f)))
        .map((m) => m.done),
    ];
    const run = Promise.all(before).then(job);
    const tail = run.catch(() => {});
    this.#chains.set(key, tail);
    void tail.then(() => {
      if (this.#chains.get(key) === tail) this.#chains.delete(key);
    });
    return run;
  }

  /** Queues the edits to a note being left, in that note's turn. */
  #writeLeft(edit: PendingEdit): void {
    this.#queued.add(edit);
    void this.#onPath(edit.path, () => this.#write(edit))
      .catch(() => {})
      .finally(() => this.#queued.delete(edit));
  }

  /** `edit` now starts from `base`, set on purpose — not from whatever the edit before it lands as. */
  #rebase(edit: PendingEdit, base: string): void {
    edit.base = base;
    this.#prev.delete(edit);
  }

  /** The nearest edit before `job` that is on disk, if any: `job` was typed on top of it. */
  #landedBefore(job: PendingEdit): PendingEdit | null {
    for (let p = this.#prev.get(job); p; p = this.#prev.get(p)) {
      if (this.#landed.has(p)) return p;
    }
    return null;
  }

  /** `job` landed: every edit before it is on disk inside it. */
  #landedNow(job: PendingEdit): void {
    this.#landed.add(job);
    this.#done.add(job);
    for (let p = this.#prev.get(job); p; p = this.#prev.get(p)) {
      this.#done.add(p);
      this.#queued.delete(p);
      this.#unstrand(p);
    }
  }

  #cancelTimer(): void {
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  #write(job: PendingEdit, rootOf?: string): Promise<void> {
    // Already on disk (or held by a later edit that is): nothing to write. Already being written:
    // the same write, not a second one.
    if (this.#done.has(job)) return Promise.resolve();
    const running = this.#running.get(job);
    if (running) return running;
    const run = this.#writeOnce(job, rootOf).finally(() => this.#running.delete(job));
    this.#running.set(job, run);
    return run;
  }

  async #writeOnce(job: PendingEdit, rootOf?: string): Promise<void> {
    const root = rootOf ?? this.deps.root();
    if (!root) return;
    const retry = rootOf !== undefined;
    const s = this.state;
    const open = this.deps.activePath();
    if (job.path === open && (s.conflictDisk !== null || s.removed)) return;
    try {
      const landedBefore = this.#landedBefore(job);
      if (landedBefore) this.#rebase(job, landedBefore.text);
      const outcome = await this.deps.write(root, job.path, job.text, job.base);
      if (retry) this.#unstrand(job);
      if (outcome.kind === "written") this.#landedNow(job);
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
        if (this.#stranded.length === 0 && !this.state.saveFailure) this.state.saveError = null;
        if (root !== this.deps.root()) return;
        this.deps.saved?.(job.path, job.text);
        // The note was opened again meanwhile and shows what disk held before: disk now holds the
        // kept edits, so the screen and the save baseline follow — unless the person is typing in it,
        // whose edits then meet this change as any other change on disk.
        const ownPending = this.#pending !== null && this.#pending.path === job.path;
        if (job.path === this.deps.activePath() && !ownPending) {
          this.#synced = job.text;
          this.deps.landedOnOpen?.(job.text);
        }
        return;
      }
      this.deps.saved?.(job.path, job.text);
      if (job.path === active) this.#synced = job.text;
      if (this.#pending === job) {
        this.#pending = null;
        this.#syncDirty();
      } else if (this.#pending && samePath(this.#pending.path, job.path)) {
        this.#rebase(this.#pending, job.text); // the newer edit now starts from what this save put on disk
      }
      // Only the open note's own save speaks for it: a note already left saving fine says nothing
      // about whether the open one's edits are saved.
      if (job.path === active) {
        s.saveFailure = null;
        // A note left behind with edits still not on disk keeps its warning up.
        if (this.#stranded.length === 0) s.saveError = null;
      }
    } catch (e) {
      const active = this.deps.activePath();
      if (!retry && this.#pending !== job && job.path !== active) {
        // Edits to a note already left: nothing on screen holds them, so keep them to try again.
        this.#strand(root, job);
      }
      if (retry || this.#stranded.some((k) => k.edit === job)) {
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
