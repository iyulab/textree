import { describe, it, expect } from "vitest";
import { pickDownloadSnapshot, keepPollingDownload, type DownloadStatus } from "./aiHost.helpers";
import type { DownloadSnapshot } from "./modelDownload.helpers";

const snap = (bytes: number): DownloadSnapshot => ({
  phase: "downloading", overallPercent: 0, bytesDownloaded: bytes, totalBytes: 4_900_000_000,
  fileIndex: 1, fileCount: 2,
});

const st = (o: Partial<DownloadStatus>): DownloadStatus => ({
  status: "starting", embedderDownload: null, generatorDownload: null, ...o,
});

describe("pickDownloadSnapshot", () => {
  it("prefers the generator download over the embedder", () => {
    const gen = snap(1), emb = snap(2);
    expect(pickDownloadSnapshot(st({ generatorDownload: gen, embedderDownload: emb }))).toBe(gen);
  });
  it("falls back to the embedder when no generator download", () => {
    const emb = snap(2);
    expect(pickDownloadSnapshot(st({ embedderDownload: emb }))).toBe(emb);
  });
  it("is null when neither is downloading", () => {
    expect(pickDownloadSnapshot(st({ status: "ready" }))).toBeNull();
  });
});

describe("keepPollingDownload", () => {
  it("keeps polling while a download is in progress (even if status is ready)", () => {
    expect(keepPollingDownload(st({ status: "ready", embedderDownload: snap(1) }))).toBe(true);
  });
  it("keeps polling while the host is still starting (download may be imminent)", () => {
    expect(keepPollingDownload(st({ status: "starting" }))).toBe(true);
  });
  it("stops once ready with no download in flight", () => {
    expect(keepPollingDownload(st({ status: "ready" }))).toBe(false);
  });
  it("stops when the host is unavailable", () => {
    expect(keepPollingDownload(st({ status: "unavailable" }))).toBe(false);
  });
});
