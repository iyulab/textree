import type { DownloadSnapshot } from "./modelDownload.helpers";
import type { HostStatus } from "./ipc";

/** The host_status fields the durable download tracker needs (subset of the full IPC result). */
export type DownloadStatus = {
  status: HostStatus;
  embedderDownload: DownloadSnapshot | null;
  generatorDownload: DownloadSnapshot | null;
};

/**
 * The download snapshot to surface: generator first (the larger, later model), else the embedder.
 * Mirrors the chat gate's choice so the global indicator and the in-view rows agree.
 */
export function pickDownloadSnapshot(st: DownloadStatus): DownloadSnapshot | null {
  return st.generatorDownload ?? st.embedderDownload;
}

/**
 * Whether the durable poll should keep running: a model is actively downloading, OR the host is
 * still starting up (a download may be about to begin). Stops once the host is ready/unavailable
 * with no download in flight — so the loop is bounded, not a permanent background poller.
 */
export function keepPollingDownload(st: DownloadStatus): boolean {
  return pickDownloadSnapshot(st) !== null || st.status === "starting";
}
