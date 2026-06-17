import { invoke } from "@tauri-apps/api/core";

export type NodeKind = "leaf" | "container";

export interface TreeNode {
  name: string;
  kind: NodeKind;
  // The node's own path (leaf=.md, container=directory). The address for structure edits.
  path: string;
  // Rust PathBuf serializes to a string in JSON. null for a container with no body.
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

// ── Structure edits (M4) ────────────────────────────────────────────────

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

/** Promote a leaf note to a container and move the path node into it. Returns the new path. */
export async function adoptNode(
  root: string,
  path: string,
  leaf: string,
): Promise<string> {
  return invoke<string>("adopt_node", { root, path, leaf });
}

/**
 * Save an attached image to assets/ next to the note (note, .md). data is base64 bytes.
 * Returns the relative link to insert into the body (e.g. "assets/Pasted-….png").
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

// ── Sidecar (.textree/) persistence ───────────────────────────────────

/** Read the `.textree/<rel>` sidecar. null if absent. */
export async function readSidecar(
  root: string,
  rel: string,
): Promise<string | null> {
  return invoke<string | null>("read_sidecar", { root, rel });
}

/** Atomic write of the `.textree/<rel>` sidecar (parent auto-created). */
export async function writeSidecar(
  root: string,
  rel: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_sidecar", { root, rel, content });
}

// ── Body full-text search (P1b) ──────────────────────────────────────────

export interface SearchHit {
  /** Vault-relative path (POSIX). The front end combines it with root to open the absolute path. */
  path: string;
  title: string;
  snippet: string;
  /** Highlight [start, end) char indices within the snippet string. */
  ranges: [number, number][];
}

/** Body full-text search. Empty array if the index is missing or the query is empty. */
export async function searchContent(
  query: string,
  limit = 50,
): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search_content", { query, limit });
}

/** Full reindex (>reindex command). */
export async function rebuildIndex(root: string): Promise<void> {
  return invoke<void>("rebuild_index", { root });
}

// ── Publishing (P2 — render the vault to a static site via canopy) ─────────

export interface PublishOptions {
  /** Overrides the site title (defaults to the vault folder name). */
  siteTitle?: string;
  /** Design-token CSS content injected so the published site matches the app. */
  tokensCss?: string;
}

export interface PublishResult {
  pageCount: number;
  outDir: string;
}

/**
 * Publish the vault to a static site at `outDir` (which must be outside the vault). Read-only over
 * the source: the vault `.md` is never mutated. Spawns the canopy renderer in the backend.
 */
export async function publishSite(
  vaultPath: string,
  outDir: string,
  options: PublishOptions = {},
): Promise<PublishResult> {
  return invoke<PublishResult>("publish_site", { vaultPath, outDir, options });
}

// ── Trash (B1) ───────────────────────────────────────────────────────────────

export type TrashItem = {
  trashName: string;
  originalRel: string;
  deletedAt: number;
  isDir: boolean;
};

export async function listTrash(root: string): Promise<TrashItem[]> {
  return invoke<TrashItem[]>("list_trash", { root });
}

export async function restoreNode(root: string, trashName: string): Promise<string> {
  return invoke<string>("restore_node", { root, trashName });
}

export async function purgeTrash(root: string, trashName?: string): Promise<void> {
  return invoke<void>("purge_trash", { root, trashName: trashName ?? null });
}
