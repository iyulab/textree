import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { initialNoteSaveState, NoteSave, type LeaveQuestion, type NoteSaveState } from "./noteSave";
import type { WriteOutcome } from "./ipc";

interface Call {
  path: string;
  text: string;
  expected: string;
  settle: (outcome: WriteOutcome | Error) => void;
}

/** A disk whose writes finish only when the test says so, in any order it chooses. */
function harness(opts: { active?: string } = {}) {
  let active: string | null = opts.active ?? "A.md";
  const calls: Call[] = [];
  const asked: LeaveQuestion[] = [];
  let removedCount = 0;
  const state: NoteSaveState = initialNoteSaveState();
  const save = new NoteSave(state, {
    root: () => "/vault",
    activePath: () => active,
    write: (_root, path, text, expected) =>
      new Promise<WriteOutcome>((resolve, reject) => {
        calls.push({
          path,
          text,
          expected,
          settle: (o) => (o instanceof Error ? reject(o) : resolve(o)),
        });
      }),
    ask: (q) => asked.push(q),
    moved: (remap) => {
      if (active) active = remap(active);
    },
    removed: () => removedCount++,
  });
  return {
    save,
    state,
    calls,
    asked,
    removedCount: () => removedCount,
    open(path: string | null, text = "") {
      active = path;
      save.opened(text);
    },
  };
}

const written: WriteOutcome = { kind: "written" };

/** Let queued promise callbacks run (the save chain hops a few microtasks between writes). */
async function settle() {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("NoteSave — autosave", () => {
  it("saves after a pause in typing, from what the note held when the edit began", async () => {
    const h = harness();
    h.save.opened("old");
    h.save.schedule("A.md", "n");
    h.save.schedule("A.md", "ne");
    h.save.schedule("A.md", "new");
    expect(h.calls).toHaveLength(0);
    vi.advanceTimersByTime(500);
    await settle();
    expect(h.calls.map((c) => [c.text, c.expected])).toEqual([["new", "old"]]);
    h.calls[0].settle(written);
    await settle();
    expect(h.state.dirty).toBe(false);
    expect(h.save.pending).toBeNull();
    expect(h.save.synced).toBe("new");
  });

  it("an edit made while a save runs starts from what that save put on disk", async () => {
    const h = harness();
    h.save.opened("0");
    h.save.schedule("A.md", "1");
    void h.save.flush();
    await settle();
    h.save.schedule("A.md", "2");
    h.calls[0].settle(written);
    await settle();
    expect(h.state.dirty).toBe(true);
    void h.save.flush();
    await settle();
    expect(h.calls[1]).toMatchObject({ text: "2", expected: "1" });
  });

  it("does not save on a timer while the person is asked which copy wins", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    h.save.markConflict("theirs");
    vi.advanceTimersByTime(1000);
    await settle();
    expect(h.calls).toHaveLength(0);
    h.save.keepMine();
    await settle();
    expect(h.calls[0]).toMatchObject({ text: "mine", expected: "theirs" });
  });
});

describe("NoteSave — switching notes", () => {
  it("edits typed into the note being left are written, not replaced by the first edit to the next", async () => {
    const h = harness();
    h.save.opened("a0");
    h.save.schedule("A.md", "a1");
    const leaving = h.save.beforeLeaving();
    await settle();
    h.calls[0].settle(written);
    expect(await leaving).toBe("saved");

    // The next note is still loading, so the editor on screen is still the old one.
    h.save.schedule("A.md", "a2");
    h.open("B.md", "b0");
    h.save.schedule("B.md", "b1");
    await settle();

    expect(h.calls.map((c) => [c.path, c.text, c.expected])).toEqual([
      ["A.md", "a1", "a0"],
      ["A.md", "a2", "a1"],
    ]);
    h.calls[1].settle(written);
    vi.advanceTimersByTime(500);
    await settle();
    expect(h.calls[2]).toMatchObject({ path: "B.md", text: "b1", expected: "b0" });
  });

  it("a note left behind that changed on disk is named, and does not raise the open note's banners", async () => {
    const h = harness();
    h.save.schedule("A.md", "a1");
    h.open("B.md");
    h.save.schedule("B.md", "b1");
    await settle();
    h.calls[0].settle({ kind: "conflict", disk: "elsewhere" });
    await settle();
    expect(h.state.saveError?.summary).toContain('"A"');
    expect(h.state.conflictDisk).toBeNull();
    expect(h.state.saveFailure).toBeNull();
  });

  it("typing that goes on while leaving is saved rather than read as a failure", async () => {
    const h = harness();
    h.save.schedule("A.md", "1");
    const leaving = h.save.beforeLeaving();
    for (let i = 2; i <= 3; i++) {
      await settle();
      h.save.schedule("A.md", String(i));
      h.calls.at(-1)!.settle(written);
    }
    await settle();
    h.calls.at(-1)!.settle(written);
    expect(await leaving).toBe("saved");
    expect(h.calls.map((c) => c.text)).toEqual(["1", "2", "3"]);
    expect(h.state.saveFailure).toBeNull();
  });

  it("typing that never stops leaves the edits saving to this note: busy, not failed", async () => {
    const h = harness();
    h.save.schedule("A.md", "0");
    const leaving = h.save.beforeLeaving();
    let n = 0;
    let outcome: string | undefined;
    void leaving.then((o) => (outcome = o));
    while (outcome === undefined) {
      await settle();
      if (outcome !== undefined) break;
      h.save.schedule("A.md", String(++n));
      h.calls.at(-1)!.settle(written);
    }
    expect(outcome).toBe("busy");
    expect(h.asked).toEqual([]);
    expect(h.save.pending?.path).toBe("A.md");
  });

  it("a save that fails keeps the note from being left and points at the failure", async () => {
    const h = harness();
    h.save.schedule("A.md", "1");
    const leaving = h.save.beforeLeaving();
    await settle();
    h.calls[0].settle(new Error("disk full"));
    expect(await leaving).toBe("failed");
    expect(h.asked).toEqual(["failure"]);
    expect(h.state.saveFailure).not.toBeNull();
    expect(h.save.pending?.text).toBe("1");
  });

  it("a conflict found while leaving keeps the note open and asks", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    const leaving = h.save.beforeLeaving();
    await settle();
    h.calls[0].settle({ kind: "conflict", disk: "theirs" });
    expect(await leaving).toBe("asking");
    expect(h.asked).toEqual(["conflict"]);
    expect(h.state.conflictDisk).toBe("theirs");
  });

  it("a note found gone while leaving keeps its edits and asks whether to put it back", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    const leaving = h.save.beforeLeaving();
    await settle();
    h.calls[0].settle({ kind: "conflict", disk: null });
    expect(await leaving).toBe("asking");
    expect(h.asked).toEqual(["removed"]);
    expect(h.state.removed).toBe(true);
    expect(h.state.dirty).toBe(true);
    expect(h.removedCount()).toBe(1);
  });

  it("edits to a note left behind whose write fails are kept, retried, and warned about until they land", async () => {
    const h = harness();
    h.save.opened("a0");
    // Typed into the old note while the next one loads, then the first edit to the next note.
    h.save.schedule("A.md", "a2");
    h.open("B.md", "b0");
    h.save.schedule("B.md", "b1");
    await settle();
    h.calls[0].settle(new Error("disk full"));
    await settle();
    expect(h.save.stranded).toBe(1);
    expect(h.state.saveError?.summary).toContain('"A"');
    expect(h.state.saveFailure).toBeNull(); // the open note has nothing wrong with it

    // The next save tries the kept edits first, in the vault they belong to.
    vi.advanceTimersByTime(500);
    await settle();
    expect(h.calls[1]).toMatchObject({ path: "A.md", text: "a2", expected: "a0" });
    h.calls[1].settle(new Error("disk full"));
    await settle();
    expect(h.calls[2]).toMatchObject({ path: "B.md", text: "b1" });
    h.calls[2].settle(written);
    await settle();
    // The open note saving fine does not hide that the other one's edits are still not on disk.
    expect(h.save.stranded).toBe(1);
    expect(h.state.saveError?.summary).toContain('"A"');

    h.save.schedule("B.md", "b2");
    vi.advanceTimersByTime(500);
    await settle();
    expect(h.calls[3]).toMatchObject({ path: "A.md", text: "a2", expected: "a0" });
    h.calls[3].settle(written);
    await settle();
    h.calls[4].settle(written);
    await settle();
    expect(h.save.stranded).toBe(0);
    expect(h.state.saveError).toBeNull();
  });

  it("kept edits are written into the vault they were typed in, even after another vault opens", async () => {
    let root = "/one";
    let active: string | null = "A.md";
    const roots: string[] = [];
    let fail = true;
    const save = new NoteSave(initialNoteSaveState(), {
      root: () => root,
      activePath: () => active,
      write: async (r) => {
        roots.push(r);
        if (fail) throw new Error("disk full");
        return written;
      },
    });
    save.opened("a0");
    save.schedule("A.md", "a1");
    active = "B.md";
    save.opened("b0");
    save.schedule("B.md", "b1");
    await settle();
    expect(save.stranded).toBe(1);

    root = "/two";
    active = null;
    save.closed();
    fail = false;
    await save.flush();
    expect(roots.at(-1)).toBe("/one");
    expect(save.stranded).toBe(0);
  });

});

describe("NoteSave — answers", () => {
  it("taking the copy on disk drops the edits and cancels the pending save", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    h.save.markConflict("theirs");
    expect(h.save.takeDisk()).toBe("theirs");
    h.save.reloaded("theirs");
    vi.advanceTimersByTime(1000);
    await settle();
    expect(h.calls).toHaveLength(0);
    expect(h.state.dirty).toBe(false);
  });

  it("putting a removed note back somewhere else carries the edits typed meanwhile to the new path", async () => {
    const h = harness();
    h.save.schedule("A.md", "kept");
    h.save.markRemoved();
    h.save.schedule("A.md", "kept+more");
    h.save.putBack("kept", "A (2).md");
    expect(h.state.removed).toBe(false);
    expect(h.save.pending).toMatchObject({ path: "A (2).md", text: "kept+more", base: "kept" });
  });

  it("the note reappearing resumes saving", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    h.save.markRemoved();
    vi.advanceTimersByTime(1000);
    await settle();
    expect(h.calls).toHaveLength(0);
    expect(h.save.markPresent()).toBe(true);
    await settle();
    expect(h.calls[0]).toMatchObject({ text: "mine" });
  });
});

describe("NoteSave — moving the open note", () => {
  const into = (dir: string) => (p: string) => (p.startsWith("A.md") ? `${dir}/A.md` : p);

  it("a save scheduled during a move waits for it and lands at the new path", async () => {
    const h = harness();
    h.save.opened("a0");
    let finishMove!: () => void;
    const moving = h.save.move(
      () => new Promise((resolve) => (finishMove = () => resolve(into("dest")))),
    );
    h.save.schedule("A.md", "typed while moving");
    vi.advanceTimersByTime(500);
    await settle();
    expect(h.calls).toHaveLength(0); // nothing written to the old path mid-move

    finishMove();
    await moving;
    await settle();
    expect(h.calls.map((c) => [c.path, c.text, c.expected])).toEqual([
      ["dest/A.md", "typed while moving", "a0"],
    ]);
  });

  it("a save already running finishes before the move starts", async () => {
    const h = harness();
    h.save.schedule("A.md", "1");
    void h.save.flush();
    await settle();
    let relocated = false;
    const moving = h.save.move(async () => {
      relocated = true;
      return into("dest");
    });
    await settle();
    expect(relocated).toBe(false);
    h.calls[0].settle(written);
    await moving;
    expect(relocated).toBe(true);
  });

  it("the disk reference point is not moved by what the editor shows, only by loads and saves", async () => {
    const h = harness();
    h.save.opened("on disk");
    h.save.schedule("A.md", "unsaved");
    await h.save.move(async () => into("dest"));
    expect(h.save.synced).toBe("on disk");
    expect(h.save.pending).toMatchObject({ path: "dest/A.md", text: "unsaved", base: "on disk" });
  });

  it("a failed move leaves the edit where it was", async () => {
    const h = harness();
    h.save.schedule("A.md", "kept");
    await expect(h.save.move(() => Promise.reject(new Error("in use")))).rejects.toThrow("in use");
    expect(h.save.pending?.path).toBe("A.md");
  });
});

describe("NoteSave — unsaved marker", () => {
  it("reopening the open note while its edits are still saving keeps it marked unsaved", async () => {
    const h = harness();
    h.save.opened("a0");
    h.save.schedule("A.md", "a1");
    h.open("A.md", "a0"); // opened again before the save ran
    expect(h.state.dirty).toBe(true);
    void h.save.flush();
    await settle();
    h.calls[0].settle(written);
    await settle();
    expect(h.state.dirty).toBe(false);
  });

  it("an edit to the note being left does not mark the next one unsaved", async () => {
    const h = harness();
    h.save.schedule("A.md", "a1");
    h.open("B.md", "b0");
    expect(h.state.dirty).toBe(false);
    expect(h.save.pending?.path).toBe("A.md");
  });

  it("follows the open note through a move", async () => {
    const h = harness();
    h.save.schedule("A.md", "a1");
    await h.save.move(async () => (p) => (p === "A.md" ? "dest/A.md" : p));
    expect(h.state.dirty).toBe(true);
  });
});
