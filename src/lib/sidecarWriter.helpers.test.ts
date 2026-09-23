import { describe, expect, it } from "vitest";
import { RetryingWriter } from "./sidecarWriter.helpers";

/** A fake disk whose writes fail while `failing` holds the file name. */
function fakeDisk() {
  const files = new Map<string, string>();
  const failing = new Set<string>();
  const write = async (rel: string, body: string) => {
    if (failing.has(rel)) throw new Error(`cannot write ${rel}`);
    files.set(rel, body);
  };
  return { files, failing, write };
}

describe("RetryingWriter", () => {
  it("writes the file it is asked to", async () => {
    const disk = fakeDisk();
    const writer = new RetryingWriter(disk.write);
    await writer.save("order.json", () => "[1]");
    expect(disk.files.get("order.json")).toBe("[1]");
    expect(writer.pending()).toEqual([]);
  });

  it("writes a failed file again on the next save of a different file", async () => {
    const disk = fakeDisk();
    const errors: string[] = [];
    const writer = new RetryingWriter(disk.write, (rel) => errors.push(rel));
    let favorites = ["a"];

    disk.failing.add("favorites.json");
    await writer.save("favorites.json", () => JSON.stringify(favorites));
    expect(disk.files.has("favorites.json")).toBe(false);
    expect(writer.pending()).toEqual(["favorites.json"]);
    expect(errors).toEqual(["favorites.json"]);

    // The disk recovers; the next write is to another file.
    disk.failing.clear();
    favorites = ["a", "b"];
    await writer.save("order.json", () => "{}");
    // The owed file lands with the state as it is now, not as it was when it failed.
    expect(disk.files.get("favorites.json")).toBe('["a","b"]');
    expect(disk.files.get("order.json")).toBe("{}");
    expect(writer.pending()).toEqual([]);
  });

  it("keeps owing a file while it keeps failing", async () => {
    const disk = fakeDisk();
    const writer = new RetryingWriter(disk.write);
    disk.failing.add("favorites.json");
    await writer.save("favorites.json", () => "[]");
    await writer.save("order.json", () => "{}");
    expect(writer.pending()).toEqual(["favorites.json"]);
    expect(disk.files.get("order.json")).toBe("{}");
  });

  it("does not carry owed writes into another folder", async () => {
    const disk = fakeDisk();
    const writer = new RetryingWriter(disk.write);
    disk.failing.add("favorites.json");
    await writer.save("favorites.json", () => '["from the old folder"]');
    writer.reset();
    disk.failing.clear();
    await writer.save("order.json", () => "{}");
    expect(disk.files.has("favorites.json")).toBe(false);
  });

  it("does not drop a file queued again while its earlier write was in flight", async () => {
    const files = new Map<string, string>();
    let release: () => void = () => {};
    let calls = 0;
    const writer = new RetryingWriter(async (rel, body) => {
      calls += 1;
      if (calls === 1) await new Promise<void>((r) => (release = r)); // first write hangs
      else if (calls === 2) throw new Error("second write fails");
      files.set(rel, body);
    });

    const first = writer.save("favorites.json", () => "old");
    await Promise.resolve();
    await writer.save("favorites.json", () => "new"); // queued again; its own write fails
    release();
    await first;
    // The in-flight write finishing must not clear the newer, still-unwritten state.
    expect(writer.pending()).toEqual(["favorites.json"]);
  });
  it("stops a save in progress when the folder changes, so the rest does not land in the new one", async () => {
    const written: string[] = [];
    let release: () => void = () => {};
    let attempt = 0;
    const writer = new RetryingWriter(async (rel) => {
      if (rel === "favorites.json") {
        attempt += 1;
        if (attempt === 1) throw new Error("owed"); // favorites.json is owed from here on
        await new Promise<void>((r) => (release = r)); // its retry is slow
      }
      written.push(rel);
    });
    await writer.save("favorites.json", () => "[]");

    const saving = writer.save("order.json", () => "{}"); // retries favorites first, then order
    await Promise.resolve();
    writer.reset(); // the folder changes while favorites.json is being retried
    release();
    await saving;

    expect(written).toEqual(["favorites.json"]); // order.json was not written into the new folder
  });
});
