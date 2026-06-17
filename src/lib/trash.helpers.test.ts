import { describe, it, expect } from "vitest";
import { sortTrash, formatDeletedAt, originLabel } from "./trash.helpers";
import type { TrashItem } from "./ipc";

const mk = (o: Partial<TrashItem>): TrashItem => ({
  trashName: "x.md", originalRel: "x.md", deletedAt: 0, isDir: false, ...o,
});

describe("sortTrash", () => {
  it("orders by deletedAt desc, unknown (0) last, without mutating input", () => {
    const input = [mk({ trashName: "a", deletedAt: 10 }), mk({ trashName: "b", deletedAt: 0 }), mk({ trashName: "c", deletedAt: 30 })];
    const out = sortTrash(input);
    expect(out.map((i) => i.trashName)).toEqual(["c", "a", "b"]);
    expect(input.map((i) => i.trashName)).toEqual(["a", "b", "c"]); // input untouched
  });
});

describe("formatDeletedAt", () => {
  it("returns 'unknown' for 0", () => {
    expect(formatDeletedAt(0)).toBe("unknown");
  });
  it("formats a real epoch to YYYY-MM-DD HH:mm", () => {
    // 2024-01-02 03:04 UTC — assert the date portion is stable regardless of local TZ shape.
    expect(formatDeletedAt(1704164640)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });
});

describe("originLabel", () => {
  it("labels unknown-origin entries", () => {
    expect(originLabel(mk({ trashName: "o.md", originalRel: "o.md", deletedAt: 0 }))).toBe("unknown origin");
    expect(originLabel(mk({ trashName: "o.md", originalRel: "refs/o.md", deletedAt: 5 }))).toBe("refs/o.md");
  });
});
