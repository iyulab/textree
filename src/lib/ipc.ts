import { invoke } from "@tauri-apps/api/core";

export type NodeKind = "leaf" | "container";

export interface TreeNode {
  name: string;
  kind: NodeKind;
  // 노드 자체 경로(리프=.md, 컨테이너=디렉터리). 구조 편집의 주소.
  path: string;
  // Rust PathBuf는 JSON에서 문자열로 직렬화됨. body 없는 컨테이너면 null.
  body_path: string | null;
  children: TreeNode[];
}

export async function openVault(root: string): Promise<TreeNode[]> {
  return invoke<TreeNode[]>("open_vault", { root });
}

export async function listTree(root: string): Promise<TreeNode[]> {
  return invoke<TreeNode[]>("list_tree", { root });
}

export async function readNote(root: string, path: string): Promise<string> {
  return invoke<string>("read_note", { root, path });
}

export async function writeNote(
  root: string,
  path: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_note", { root, path, content });
}

// ── 구조 편집(M4) ────────────────────────────────────────────────

export async function createNote(
  root: string,
  parent: string,
  name: string,
): Promise<string> {
  return invoke<string>("create_note", { root, parent, name });
}

export async function createFolder(
  root: string,
  parent: string,
  name: string,
): Promise<string> {
  return invoke<string>("create_folder", { root, parent, name });
}

export async function renameNode(
  root: string,
  path: string,
  name: string,
): Promise<string> {
  return invoke<string>("rename_node", { root, path, name });
}

export async function moveNode(
  root: string,
  path: string,
  dest: string,
): Promise<string> {
  return invoke<string>("move_node", { root, path, dest });
}

/** 리프 노트(leaf)를 컨테이너로 승격하고 path 노드를 그 안으로 이동. 새 경로 반환. */
export async function adoptNode(
  root: string,
  path: string,
  leaf: string,
): Promise<string> {
  return invoke<string>("adopt_node", { root, path, leaf });
}

/**
 * 첨부 이미지를 노트(note, .md) 옆 assets/에 저장. data는 base64 바이트.
 * 본문에 삽입할 상대링크(예: "assets/Pasted-….png")를 반환.
 */
export async function saveAttachment(
  root: string,
  note: string,
  data: string,
  ext: string,
): Promise<string> {
  return invoke<string>("save_attachment", { root, note, data, ext });
}

export async function deleteNode(root: string, path: string): Promise<void> {
  return invoke<void>("delete_node", { root, path });
}

export async function promoteNode(
  root: string,
  path: string,
): Promise<string> {
  return invoke<string>("promote_node", { root, path });
}

// ── 사이드카(.textree/) 영속 ───────────────────────────────────

/** `.textree/<rel>` 사이드카 읽기. 부재 시 null. */
export async function readSidecar(
  root: string,
  rel: string,
): Promise<string | null> {
  return invoke<string | null>("read_sidecar", { root, rel });
}

/** `.textree/<rel>` 사이드카 원자적 쓰기(부모 자동 생성). */
export async function writeSidecar(
  root: string,
  rel: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_sidecar", { root, rel, content });
}

// ── 본문 전문검색(P1b) ──────────────────────────────────────────

export interface SearchHit {
  /** 볼트 상대경로(POSIX). 프론트에서 root와 결합해 절대경로로 오픈. */
  path: string;
  title: string;
  snippet: string;
  /** 스니펫 문자열 내 하이라이트 [start, end) char 인덱스. */
  ranges: [number, number][];
}

/** 본문 전문검색. 인덱스 미설치/빈쿼리 시 빈 배열. */
export async function searchContent(
  query: string,
  limit = 50,
): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search_content", { query, limit });
}

/** 전체 재색인(>재색인 명령). */
export async function rebuildIndex(root: string): Promise<void> {
  return invoke<void>("rebuild_index", { root });
}
