import { hostStatus, stopHost } from "./ipc";
import type { DownloadSnapshot } from "./modelDownload.helpers";
import { pickDownloadSnapshot, keepPollingDownload } from "./aiHost.helpers";

/**
 * App-wide, durable tracker for the local-AI model download. The in-view rows (Palette `?`,
 * ChatView) only poll while their view is mounted, so navigating away used to make the progress
 * vanish even though the host kept downloading. This store polls host_status independently of any
 * view and drives a persistent indicator + a cancel control, so a multi-GB cold download is always
 * visible and interruptible wherever the user is.
 */
class AiHost {
  /** Active download snapshot while a model is downloading; null when idle/ready. */
  download = $state<DownloadSnapshot | null>(null);
  /** True between cancel request and host teardown — lets the UI disable the cancel button. */
  cancelling = $state(false);

  // Guards a single poll loop so repeated startPolling() calls (startup, enable, summarize) don't
  // spawn parallel loops. Reset when the loop self-stops.
  #polling = false;

  /** Begin (or no-op if already running) polling for download progress. Idempotent and safe to
   *  call from every trigger that can start a download. Self-stops when nothing is downloading. */
  startPolling(): void {
    if (this.#polling) return;
    this.#polling = true;
    this.cancelling = false;
    void this.#tick();
  }

  async #tick(): Promise<void> {
    if (!this.#polling) return;
    let st: Awaited<ReturnType<typeof hostStatus>>;
    try {
      st = await hostStatus();
    } catch {
      // Host unreachable → nothing to show; stop the loop (a later trigger can restart it).
      this.download = null;
      this.#polling = false;
      return;
    }
    if (!this.#polling) return; // cancelled mid-await
    this.download = pickDownloadSnapshot(st);
    if (keepPollingDownload(st)) {
      setTimeout(() => void this.#tick(), 1000);
    } else {
      this.#polling = false;
    }
  }

  /** Cancel the in-progress download by stopping the host. The user can re-enable AI later. */
  async cancel(): Promise<void> {
    this.cancelling = true;
    this.#polling = false;
    this.download = null;
    try {
      await stopHost();
    } finally {
      this.cancelling = false;
    }
  }
}

export const aiHost = new AiHost();
