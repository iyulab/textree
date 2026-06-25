export type DownloadSnapshot = {
  phase: string;
  overallPercent: number;
  bytesDownloaded: number;
  totalBytes: number;
  fileIndex: number;
  fileCount: number;
};

const GB = 1_000_000_000;
const gb = (n: number) => (n / GB).toFixed(1);

/**
 * Pure: snapshot → display strings + 0..1 bar ratio. null passthrough when not downloading.
 *
 * ratio is derived from overallPercent so the progress bar matches the displayed % label.
 * detail shows actual bytes transferred as "X / Y GB".
 */
export function formatModelDownload(
  s: DownloadSnapshot | null,
): { label: string; detail: string; ratio: number } | null {
  if (!s) return null;
  // "loading" = bytes done, model initializing → full bar, generic label, no detail.
  if (s.phase === "loading") {
    return { label: "Preparing AI model…", detail: "", ratio: 1 };
  }
  const hasBytes = s.totalBytes > 0;
  const pct = Math.max(0, Math.min(100, Math.round(s.overallPercent)));
  return {
    label: hasBytes && pct > 0 ? `Preparing AI model… ${pct}%` : "Preparing AI model…",
    detail: hasBytes ? `${gb(s.bytesDownloaded)} / ${gb(s.totalBytes)} GB` : "",
    ratio: Math.max(0, Math.min(1, pct / 100)),
  };
}
