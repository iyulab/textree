import { describe, it, expect } from "vitest";
import { formatModelDownload } from "./modelDownload.helpers";

describe("formatModelDownload", () => {
  it("returns null when snapshot is null", () => {
    expect(formatModelDownload(null)).toBeNull();
  });

  it("formats percent (from bytes), GB detail, and 0..1 ratio", () => {
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 99, fileIndex: 2, fileCount: 3,
      bytesDownloaded: 1_200_000_000, totalBytes: 2_900_000_000,
    })!;
    // 1.2 / 2.9 ≈ 41% — derived from bytes, independent of the (here deliberately wrong) overallPercent.
    expect(r.label).toBe("Preparing AI model… 41%");
    expect(r.detail).toBe("1.2 / 2.9 GB");
    expect(r.ratio).toBeCloseTo(0.414, 2);
  });

  it("derives percent and ratio from cumulative bytes, not LMSupply's overallPercent", () => {
    // Real cold-download sample: overallPercent (18) disagreed with the byte counters
    // (4.0 / 4.9 GB ≈ 82%). Bytes are the reliable cumulative measure, and the % must match
    // the "X / Y GB" detail — so the label/ratio derive from bytes, ignoring overallPercent.
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 18, fileIndex: 2, fileCount: 3,
      bytesDownloaded: 4_000_000_000, totalBytes: 4_900_000_000,
    })!;
    expect(r.label).toBe("Preparing AI model… 82%");
    expect(r.detail).toBe("4.0 / 4.9 GB");
    expect(r.ratio).toBeCloseTo(0.816, 2);
  });

  it("shows a generic label without bytes when totalBytes is 0", () => {
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 0, fileIndex: 1, fileCount: 1,
      bytesDownloaded: 0, totalBytes: 0,
    })!;
    expect(r.label).toBe("Preparing AI model…");
    expect(r.detail).toBe("");
    expect(r.ratio).toBe(0);
  });

  it("treats loading phase (download done, initializing) as full bar", () => {
    const r = formatModelDownload({
      phase: "loading", overallPercent: 100, fileIndex: 3, fileCount: 3,
      bytesDownloaded: 2_900_000_000, totalBytes: 2_900_000_000,
    })!;
    expect(r.label).toBe("Preparing AI model…");
    expect(r.ratio).toBe(1);
  });
});
