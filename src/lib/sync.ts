// Sync Coordinator — 백엔드 `fs_changed` 이벤트와 UI 상태를 조정한다(설계 §4).
//
// 책임: 외부 파일 변경을 받아 (1) 트리를 갱신하고, (2) 열린 노트가 영향받으면
// 충돌 정책(설계 §4.2)에 따라 재로드/삭제표시/충돌배너로 분기한다.
// 상태 자체는 페이지가 보유하고, 여기서는 주입된 핸들러로 조정만 한다.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { listTree, readNote, type TreeNode } from "./ipc";

export type FsChangeKind = "created" | "modified" | "removed";

export interface FsChange {
  kind: FsChangeKind;
  path: string;
}

export interface SyncHandlers {
  /** 현재 볼트 루트(없으면 null). */
  root: () => string | null;
  /** 현재 열린 노트의 본문 경로(없으면 null). */
  activePath: () => string | null;
  /** 에디터에 미저장 편집이 있는가. */
  isDirty: () => boolean;
  /** 갱신된 트리로 교체. */
  setTree: (tree: TreeNode[]) => void;
  /** 깨끗한 노트를 디스크 내용으로 재로드(FS가 진실). */
  reloadActive: (diskContent: string) => void;
  /** 열린 노트가 외부에서 삭제/이동됨. */
  activeRemoved: () => void;
  /** dirty 상태에서 외부 변경 — 비파괴적 충돌 해소를 위해 디스크 내용 전달. */
  conflict: (diskContent: string) => void;
}

/** 경로 비교용 정규화. 구분자 차이를 흡수하고, Windows 대소문자 비구분에 맞춰
 *  소문자로 비교한다(Windows-first; +page의 pathInside와 동일 정책). */
function samePath(a: string, b: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, "/").toLowerCase();
  return norm(a) === norm(b);
}

/** 단일 fs 변경을 처리(트리 갱신 + 열린 노트 반영). */
async function handleChange(handlers: SyncHandlers, payload: FsChange): Promise<void> {
  const root = handlers.root();
  if (!root) return;

  // 1) 어떤 변경이든 트리를 갱신(생성/삭제/이름변경 반영).
  try {
    handlers.setTree(await listTree(root));
  } catch {
    // 트리 갱신 실패는 비치명적 — 다음 이벤트에서 복구.
  }

  // 2) 열린 노트가 영향받는 경우만 본문 처리.
  const active = handlers.activePath();
  if (!active || !samePath(payload.path, active)) return;

  if (payload.kind === "removed") {
    handlers.activeRemoved();
    return;
  }

  // created/modified: 디스크 내용을 읽어 충돌 정책 적용.
  let disk: string;
  try {
    disk = await readNote(root, active);
  } catch {
    handlers.activeRemoved(); // 읽기 실패 = 사실상 사라짐
    return;
  }

  if (handlers.isDirty()) {
    handlers.conflict(disk); // 미저장 편집 보호 — 사용자 선택
  } else {
    handlers.reloadActive(disk); // 조용히 재로드
  }
}

/**
 * `fs_changed` 구독을 시작한다. 반환된 unlisten으로 해제한다.
 * 핸들러를 promise 체인으로 직렬화해, 동시 실행 시 오래된 `listTree`/`readNote`
 * 결과가 최신 결과를 덮어쓰는 순서 역전을 막는다.
 */
export async function startSync(handlers: SyncHandlers): Promise<UnlistenFn> {
  let chain: Promise<void> = Promise.resolve();
  return listen<FsChange>("fs_changed", ({ payload }) => {
    chain = chain.then(() => handleChange(handlers, payload)).catch(() => {});
  });
}
