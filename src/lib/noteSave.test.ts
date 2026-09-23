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
    removed: () => removedCount++,
  });
  return {
    save,
    state,
    calls,
    asked,
    removedCount: () => removedCount,
    open(path: string | null) {
      active = path;
      save.opened();
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
    h.save.synced = "old";
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
    h.save.synced = "0";
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
    h.save.synced = "a0";
    h.save.schedule("A.md", "a1");
    const leaving = h.save.beforeLeaving();
    await settle();
    h.calls[0].settle(written);
    expect(await leaving).toBe("saved");

    // The next note is still loading, so the editor on screen is still the old one.
    h.save.schedule("A.md", "a2");
    h.open("B.md");
    h.save.synced = "b0";
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
});

describe("NoteSave — answers", () => {
  it("taking the copy on disk drops the edits and cancels the pending save", async () => {
    const h = harness();
    h.save.schedule("A.md", "mine");
    h.save.markConflict("theirs");
    expect(h.save.takeDisk()).toBe("theirs");
    h.save.reloaded();
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
