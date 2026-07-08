//! Pure presentation helpers for cloud publishing (slice 2b). No IPC, no runes — vitest-testable.

import { friendlyError } from "./friendlyError.helpers";
import type { CloudPublishResult } from "./ipc";

export function cloudPublishNotice(result: CloudPublishResult): { kind: "ok"; text: string } {
  const pages = result.pageCount === 1 ? "1 page" : `${result.pageCount} pages`;
  return { kind: "ok", text: `Published ${pages} to ${result.url}` };
}

export function cloudPublishErrorNotice(e: unknown): { kind: "error"; text: string } {
  const fe = friendlyError(e);
  return { kind: "error", text: `Publish to web failed: ${fe.summary}` };
}
