import { describe, expect, it } from "vitest";
import { DraftWriter, type Clock } from "./alternativeDraft.helpers";

function manualClock() {
  const timers = new Map<number, () => void>();
  let next = 0;
  const clock: Clock = {
    set: (run) => {
      timers.set(++next, run);
      return next;
    },
    clear: (handle) => void timers.delete(handle as number),
  };
  return {
    clock,
    fire: () => {
      const runs = [...timers.values()];
      timers.clear();
      runs.forEach((r) => r());
    },
    waiting: () => timers.size,
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("DraftWriter", () => {
  it("writes the last text once typing stops, to the alternative it was made for", async () => {
    const t = manualClock();
    const writes: string[] = [];
    const w = new DraftWriter("/v", "ab", async (root, id, text) => void writes.push(`${root}|${id}|${text}`), undefined, 400, t.clock);
    w.schedule("a");
    w.schedule("ab");
    expect(t.waiting()).toBe(1);
    expect(w.pending).toBe(true);
    t.fire();
    await settle();
    expect(writes).toEqual(["/v|ab|ab"]);
    expect(w.pending).toBe(false);
  });

  it("flush writes at once and says everything landed", async () => {
    const t = manualClock();
    const writes: string[] = [];
    const w = new DraftWriter("/v", "ab", async (_r, _i, text) => void writes.push(text), undefined, 400, t.clock);
    w.schedule("now");
    expect(await w.flush()).toBe(true);
    expect(writes).toEqual(["now"]);
    expect(t.waiting()).toBe(0);
    expect(await w.flush()).toBe(true);
    expect(writes, "nothing owed, nothing written").toEqual(["now"]);
  });

  it("a failed write stays owed and goes out with the next one", async () => {
    const t = manualClock();
    const writes: string[] = [];
    const errors: unknown[] = [];
    let failing = true;
    const w = new DraftWriter(
      "/v",
      "ab",
      async (_r, _i, text) => {
        if (failing) throw new Error("disk full");
        writes.push(text);
      },
      (e) => errors.push(e),
      400,
      t.clock,
    );
    w.schedule("kept");
    expect(await w.flush()).toBe(false);
    expect(errors).toHaveLength(1);
    expect(w.pending).toBe(true);
    failing = false;
    expect(await w.flush()).toBe(true);
    expect(writes).toEqual(["kept"]);
  });

  it("text typed while a write is in flight is still owed, and writes land in order", async () => {
    const t = manualClock();
    const writes: string[] = [];
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const w = new DraftWriter(
      "/v",
      "ab",
      async (_r, _i, text) => {
        if (text === "first") await gate;
        writes.push(text);
      },
      undefined,
      400,
      t.clock,
    );
    w.schedule("first");
    const one = w.flush();
    await settle(); // the first write has started
    w.schedule("second");
    const two = w.flush();
    release();
    expect(await one).toBe(false); // "second" was typed meanwhile
    expect(await two).toBe(true);
    expect(writes).toEqual(["first", "second"]);
  });
});
