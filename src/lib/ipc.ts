import { invoke, Channel } from "@tauri-apps/api/core";
import type { ChatMessage } from './ask.helpers';
import type { DownloadSnapshot } from './modelDownload.helpers';
import type { ByoConfig } from './byoConfig';

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

/** Resolved default vault: its absolute path, and whether the preferred (Documents) location was
 * unusable so the vault was created in a fallback location (home/app-local-data) instead. */
export type DefaultVault = { path: string; fellBack: boolean };

/**
 * Ensures the default vault exists (created + welcome.md seeded on first run) and returns its
 * absolute path. The backend owns OS-path resolution, folder creation, and seeding, and falls
 * back through home → app-local-data when Documents is unusable so first run never blank-screens.
 */
export async function ensureDefaultVault(): Promise<DefaultVault> {
  return invoke<DefaultVault>("ensure_default_vault");
}

export async function listTree(root: string): Promise<TreeNode[]> {
  return invoke<TreeNode[]>("list_tree", { root });
}

export async function readNote(root: string, path: string): Promise<string> {
  return invoke<string>("read_note", { root, path });
}

/**
 * What became of a save. `conflict` means nothing was written: the note no longer held `expected`
 * (someone changed it since), and `disk` is what it holds now — or `null` when it is gone.
 */
export type WriteOutcome = { kind: "written" } | { kind: "conflict"; disk: string | null };

/**
 * Replaces a note's content, but only while it still holds `expected` — the content the editor
 * and disk last agreed on. A save never overwrites a change it has not seen.
 */
export async function writeNote(
  root: string,
  path: string,
  content: string,
  expected: string,
): Promise<WriteOutcome> {
  return invoke<WriteOutcome>("write_note", { root, path, content, expected });
}

/**
 * Records the given notes as one revision of the vault. Returns the revision identifier, or
 * `null` when the notes are already in the state the history holds and nothing was written.
 *
 * Rejects rather than partially applying: an ignored path, a path outside the vault, or a
 * repository busy with another operation all leave the history untouched.
 */
export async function commitNotes(
  root: string,
  paths: string[],
  message: string,
): Promise<string | null> {
  return invoke<string | null>("commit_notes", { root, paths, message });
}

/** One recorded state of a note. `id` is opaque — hand it back to `noteVersionText`. */
export type NoteVersion = {
  id: string;
  message: string;
  /** Unix epoch seconds. */
  seconds: number;
  author: string;
};

/** A note the history holds but the folder no longer does. */
export type DeletedNote = {
  /** Vault-root-relative, `/`-separated. */
  rel: string;
  /**
   * Unix epoch seconds of when it left the folder. For a note removed outside the application
   * nothing recorded that moment, and the last time it was written to history stands in.
   */
  seconds: number;
  /** Whether it was ever recorded deliberately, as opposed to only kept when deleted. */
  recorded: boolean;
};

/** Where a restored note's contents came from. */
export type RestoredNote = {
  /** Vault-root-relative, `/`-separated path it actually landed at. */
  rel: string;
  /**
   * True when the contents are the ones it held at the moment it left the folder, false when
   * they come from the last state recorded before that.
   */
  asDeleted: boolean;
};

/** Every recorded state of one note, newest first. */
export async function noteVersions(root: string, path: string): Promise<NoteVersion[]> {
  return invoke<NoteVersion[]>("note_versions", { root, path });
}

/** What a note held at one recorded state. The file on disk is not touched. */
export async function noteVersionText(
  root: string,
  path: string,
  id: string,
): Promise<string> {
  return invoke<string>("note_version_text", { root, path, id });
}

/**
 * Puts a note back to one of its recorded states, overwriting what is on disk.
 *
 * What is being replaced is kept first, so unrecorded work is never the price of going back.
 */
export async function restoreVersion(
  root: string,
  path: string,
  id: string,
): Promise<void> {
  return invoke<void>("restore_version", { root, path, id });
}

/** What a folder gave up when the app stopped keeping things inside it. */
export type MoveOut = {
  /** Whether settings were carried over to where they live now. */
  settings: boolean;
  /** How many notes the set-aside copies held, now reachable as deleted notes. */
  notes: number;
  /** How many other files (attachments kept beside a deleted note) came with them. */
  files: number;
  /** What is still inside the folder's `.textree/` afterwards, relative to the folder. */
  leftBehind: string[];
};

/**
 * Moves everything the app keeps out of the notes folder, keeping every file.
 *
 * Safe to call on every open: a folder with nothing left inside it reports that nothing moved.
 */
export async function moveStateOutOfVault(root: string): Promise<MoveOut> {
  return invoke<MoveOut>("move_state_out_of_vault", { root });
}

/** Everything the history holds that is no longer in the folder, newest first. */
export async function deletedNotes(root: string): Promise<DeletedNote[]> {
  return invoke<DeletedNote[]>("deleted_notes", { root });
}

/**
 * Brings a deleted note back, in whichever of its two possible states is the newer: what it
 * held when it left the folder, or the last state recorded before that.
 *
 * Never overwrites: if the name is in use the copy is numbered alongside it.
 */
export async function restoreDeleted(root: string, rel: string): Promise<RestoredNote> {
  return invoke<RestoredNote>("restore_deleted", { root, rel });
}

// ── Structure edits (M4) ────────────────────────────────────────────────

export async function createNote(
  root: string,
  parent: string,
  name: string,
): Promise<string> {
  return invoke<string>("create_note", { root, parent, name });
}

export async function createUntitledNote(root: string, parent: string): Promise<string> {
  return invoke<string>("create_untitled_note", { root, parent });
}

export async function createNoteWithContent(
  root: string,
  parent: string,
  name: string,
  content: string,
): Promise<string> {
  return invoke<string>("create_note_with_content", { root, parent, name, content });
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

export async function renameNoteUnique(
  root: string,
  path: string,
  name: string,
): Promise<string> {
  return invoke<string>("rename_note_unique", { root, path, name });
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

export interface CloudPublishResult {
  url: string;
  pageCount: number;
}

/** Render the open vault locally and upload it to pub.textree.me via api /publish. The publish
 * token is read from the OS keychain in the backend (never passed from JS). Reuses the same
 * read-only canopy render as `publishSite`. */
export async function publishToCloud(
  vaultPath: string,
  options: PublishOptions = {},
): Promise<CloudPublishResult> {
  return invoke<CloudPublishResult>("publish_to_cloud", { vaultPath, options });
}

/** Run the in-app web-publish sign-in (browser OAuth loopback + PKCE) and store the resulting
 * token in the OS Credential Manager. Resolves once connected; rejects on cancel/timeout/error.
 * The desktop only ever receives the opaque token — never the cloud identity. */
export function connectPublish(): Promise<void> {
  return invoke<void>("connect_publish");
}

export function clearPublishToken(): Promise<void> {
  return invoke<void>("clear_publish_token");
}

export function hasPublishToken(): Promise<boolean> {
  return invoke<boolean>("has_publish_token");
}






// ── Semantic search (AI-sidecar) ──────────────────────────────────────────

export interface SemanticHit {
  path: string;
  snippet: string;
  score: number;
}

export type HostStatus = "starting" | "ready" | "unavailable";

export async function semanticSearch(
  vault: string,
  query: string,
  scopePath: string | null,
  limit = 20,
): Promise<SemanticHit[]> {
  return invoke<SemanticHit[]>("semantic_search", {
    vault,
    query,
    scopePath,
    limit,
  });
}

export async function hostStatus(): Promise<{
  status: HostStatus;
  generatorReady: boolean;
  generatorError: string | null;
  embedderError: string | null;
  embedderDownload: DownloadSnapshot | null;
  generatorDownload: DownloadSnapshot | null;
  activeProvider: string;
}> {
  return invoke("host_status");
}

export type AskEvent =
  | { kind: 'token'; text: string }
  | { kind: 'citations'; hits: SemanticHit[] }
  | { kind: 'done' }
  | { kind: 'error'; message: string };

export function ask(
  vault: string,
  messages: ChatMessage[],
  citationHits: SemanticHit[],
  scopePath: string | null,
  onEvent: (e: AskEvent) => void,
): Promise<void> {
  const channel = new Channel<AskEvent>();
  channel.onmessage = onEvent;
  return invoke<void>('ask', { vault, messages, citationHits, scopePath, onEvent: channel });
}

export function prepareGeneration(): Promise<void> {
  return invoke<void>('prepare_generation');
}

/** Fire-and-forget: bump Rust ask_generation so any in-flight ask stream aborts on its next line read. */
export function cancelAsk(): Promise<void> {
  return invoke<void>('cancel_ask');
}

/** Spawn the local-AI host if not already up. Idempotent no-op if already Starting/Ready. When
 * `byo` is given, spawns pointed at that OpenAI-compatible endpoint instead of the bundled local
 * model; omitted (or undefined) spawns the bundled local model as before. */
export async function prepareAiModel(byo?: ByoConfig): Promise<void> {
  return invoke<void>("prepare_ai_model", {
    preset: byo?.preset ?? null,
    baseUrl: byo?.baseUrl ?? null,
    model: byo?.model ?? null,
  });
}

/** Force-reconfigure the host (Settings ▸Advanced Save / switch back to bundled local model):
 * unlike `prepareAiModel` (no-op if already up), this always stops the current host and spawns a
 * fresh one with the given config (or the bundled local model if `byo` is omitted). */
export async function restartAiHost(byo?: ByoConfig): Promise<void> {
  return invoke<void>("restart_ai_host", {
    preset: byo?.preset ?? null,
    baseUrl: byo?.baseUrl ?? null,
    model: byo?.model ?? null,
  });
}

export type ByoConnectionTestResult = { ok: true } | { ok: false; message: string };

/** Test-connect to a candidate BYO endpoint (Settings ▸Advanced "Test connection") without
 * spawning/reconfiguring the host. */
export async function testByoConnection(
  preset: string,
  baseUrl: string,
  apiKey?: string,
): Promise<ByoConnectionTestResult> {
  try {
    await invoke<void>("test_byo_connection", { preset, baseUrl, apiKey: apiKey ?? null });
    return { ok: true };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Store the BYO API key in the OS Credential Manager. Never persisted in localStorage. */
export function setByoApiKey(key: string): Promise<void> {
  return invoke<void>("set_byo_api_key", { key });
}

export function clearByoApiKey(): Promise<void> {
  return invoke<void>("clear_byo_api_key");
}

export function hasByoApiKey(): Promise<boolean> {
  return invoke<boolean>("has_byo_api_key");
}

/** Stop the local-AI host now (Settings → turn local AI off). No-op-safe if already down. */
export function stopHost(): Promise<void> {
  return invoke<void>("stop_host");
}

/** Open the OS app log directory in the system file explorer. */
export async function openLogDir(): Promise<void> {
  return invoke<void>("open_log_dir");
}
