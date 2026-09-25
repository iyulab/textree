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
 * overallPercent is the whole-download share and drives the bar and the % label. The byte pair
 * also describes the whole download (the host reports 0 / 0 when it cannot), so it is shown as
 * the "X / Y GB" detail only when known.
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
  const clamped = Math.max(0, Math.min(1, s.overallPercent / 100));
  const pct = Math.round(clamped * 100);
  return {
    label: pct > 0 ? `Preparing AI model… ${pct}%` : "Preparing AI model…",
    detail: hasBytes ? `${gb(s.bytesDownloaded)} / ${gb(s.totalBytes)} GB` : "",
    ratio: clamped,
  };
}
