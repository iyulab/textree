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
 * Percent and ratio derive from the cumulative byte counters (bytesDownloaded / totalBytes),
 * NOT from the snapshot's overallPercent: LMSupply's OverallPercentComplete can disagree with
 * its own byte totals (e.g. it reported 18% while bytes showed 4.0 / 4.9 GB ≈ 82%, likely a
 * per-file vs cumulative measure). Bytes are the reliable overall measure and keep the % label
 * consistent with the "X / Y GB" detail. overallPercent is only the fallback when no byte
 * totals are reported yet.
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
  const fraction = hasBytes ? s.bytesDownloaded / s.totalBytes : s.overallPercent / 100;
  const clamped = Math.max(0, Math.min(1, fraction));
  const pct = Math.round(clamped * 100);
  return {
    label: hasBytes && pct > 0 ? `Preparing AI model… ${pct}%` : "Preparing AI model…",
    detail: hasBytes ? `${gb(s.bytesDownloaded)} / ${gb(s.totalBytes)} GB` : "",
    ratio: clamped,
  };
}
