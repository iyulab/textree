import { describe, it, expect } from "vitest";
import { formatModelDownload } from "./modelDownload.helpers";

describe("formatModelDownload", () => {
  it("returns null when snapshot is null", () => {
    expect(formatModelDownload(null)).toBeNull();
  });

  it("formats percent from overallPercent, whole-download GB detail, and 0..1 ratio", () => {
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 41.4, fileIndex: 2, fileCount: 3,
      bytesDownloaded: 1_200_000_000, totalBytes: 2_900_000_000,
    })!;
    expect(r.label).toBe("Preparing AI model… 41%");
    expect(r.detail).toBe("1.2 / 2.9 GB");
    expect(r.ratio).toBeCloseTo(0.414, 2);
  });

  it("shows percent without a GB detail when whole-download bytes are unknown (0 / 0)", () => {
    // The host reports 0 / 0 when some file's size is not known up front; overallPercent is
    // then the library's file-count approximation and still drives the bar.
    const r = formatModelDownload({
      phase: "downloading", overallPercent: 50, fileIndex: 2, fileCount: 4,
      bytesDownloaded: 0, totalBytes: 0,
    })!;
    expect(r.label).toBe("Preparing AI model… 50%");
    expect(r.detail).toBe("");
    expect(r.ratio).toBe(0.5);
  });

  it("shows a generic label before any progress", () => {
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
