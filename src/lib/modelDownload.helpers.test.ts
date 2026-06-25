import { describe, it, expect } from "vitest";
import { formatModelDownload } from "./modelDownload.helpers";

describe("formatModelDownload", () => {
  it("returns null when snapshot is null", () => {
    expect(formatModelDownload(null)).toBeNull();
  });

  it("formats percent, GB detail, and 0..1 ratio", () => {
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 42, fileIndex: 2, fileCount: 3,
      bytesDownloaded: 1_200_000_000, totalBytes: 2_900_000_000,
    })!;
    expect(r.label).toBe("Preparing AI model… 42%");
    expect(r.detail).toBe("1.2 / 2.9 GB");
    expect(r.ratio).toBeCloseTo(0.42, 2);
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
