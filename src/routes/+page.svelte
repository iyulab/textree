<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import {
    openVault,
    ensureDefaultVault,
    listTree,
    readNote,
    writeNote,
    createUntitledNote,
    createNoteWithContent,
    createFolder,
    renameNode,
    renameNoteUnique,
    deleteNode,
    moveNode,
    promoteNode,
    adoptNode,
    saveAttachment,
    searchContent,
    rebuildIndex,
    publishSite,
    publishToCloud,
    hasPublishToken,
    connectPublish,
    prepareAiModel,
    openLogDir,
    moveStateOutOfVault,
    type TreeNode,
    type SearchHit,
    type MoveOut,
  } from "$lib/ipc";
  import { worthShowing } from "$lib/migrationNotice.helpers";
  import { getAiConsent } from "$lib/aiConsent";
  import { getByoConfig } from "$lib/byoConfig";
  import { decideStartup, LAST_VAULT_KEY } from "$lib/startup.helpers";
  import { toPublishTokens } from "$lib/publish.helpers";
  import { cloudPublishNotice, cloudPublishErrorNotice } from "$lib/cloudPublish.helpers";
  import tokensCssRaw from "$lib/styles/tokens.css?raw";
  import { startSync } from "$lib/sync";
  import { decideReopen } from "$lib/sync.helpers";
  import { buildWikiResolver } from "$lib/wikilink.helpers";
  import { backlinks } from "$lib/backlinkStore.svelte";
  import { relatedNotes } from "$lib/relatedNotesStore.svelte";
  import { theme } from "$lib/theme.svelte";
  import { layout } from "$lib/layout.svelte";
  import TreeView, { DRAG_MIME } from "$lib/TreeView.svelte";
  import Editor from "$lib/Editor.svelte";
  import Backlinks from "$lib/Backlinks.svelte";
  import RelatedNotes from "$lib/RelatedNotes.svelte";
  import ChatView from "$lib/ChatView.svelte";
  import { chatStore, type ChatScope } from "$lib/chatStore.svelte";
  import { aiHost } from "$lib/aiHost.svelte";
  import { formatModelDownload } from "$lib/modelDownload.helpers";
  import AddVersion from "$lib/AddVersion.svelte";
  import VersionHistory from "$lib/VersionHistory.svelte";
  import DeletedNotes from "$lib/DeletedNotes.svelte";
  import MigrationNotice from "$lib/MigrationNotice.svelte";
  import Settings from "$lib/Settings.svelte";
  import PageHeader from "$lib/PageHeader.svelte";
  import { parseFrontmatter, getField } from "$lib/frontmatter.helpers";
  import { palette } from "$lib/paletteStore.svelte";
  import Palette from "$lib/Palette.svelte";
  import AppMenu from "$lib/AppMenu.svelte";
  import TitleBar from "$lib/TitleBar.svelte";
  import { contextLabel } from "$lib/titlebar.helpers";
  import { buildCommands, activeCommands, type PaletteActions } from "$lib/commands";
  import { matchKeybinding, isFormFieldTag } from "$lib/keybinding.helpers";
  import { mergeOrder, nav } from "$lib/nav.svelte";
  import { moveInArray, findFirstOpenableNote } from "$lib/nav.helpers";
  import { checkForUpdate, type UpdateInfo } from "$lib/updater";
  import UpdateBanner from "$lib/UpdateBanner.svelte";
  import { detectSyncConflicts } from "$lib/syncConflict.helpers";
  import { leaveRefusal } from "$lib/leaveRefusal.helpers";
  import { buildFolderTable, type FolderTable } from "$lib/folderTable.helpers";
  import { views } from "$lib/views.svelte";
  import FolderTableView from "$lib/FolderTable.svelte";
  import Icon from "$lib/Icon.svelte";
  import { friendlyError, type FriendlyError } from "$lib/friendlyError.helpers";
  import { extractFirstH1, isUnnamed, sanitizeForFilename } from "$lib/h1sync.helpers";
  import { NO_VAULT_HINT, NO_VAULT_PROMPT, NO_NOTE_PROMPT } from "$lib/emptyState";
  import { backupStatus } from "$lib/backupStatus.helpers";
  import { initialNoteSaveState, NoteSave } from "$lib/noteSave";
  import {
    baseName,
    noteStem,
    parentDir,
    pathInside,
    remapAdopted,
    remapMoved,
    remapPromoted,
    remapRenamed,
    samePath,
  } from "$lib/vaultPaths.helpers";

  let root = $state<string | null>(null);
  let updateInfo = $state<UpdateInfo | null>(null);
  let tree = $state<TreeNode[]>([]);
  let content = $state("");
  // Live document text — mirrors `content` on note load, then tracks edits so the page header
  // reflects frontmatter changes immediately (the editor owns its own doc; `content` is load-only).
  let liveDoc = $state("");
  // What the page shows about saving the open note; `save` decides it (see noteSave.ts).
  const saveState = $state(initialNoteSaveState());
  const save = new NoteSave(saveState, {
    root: () => root,
    activePath: () => activePath,
    write: writeNote,
    // refresh links from the saved note (relative key, no re-read)
    saved: (path, text) => backlinks.updateSource(toRelative(path), text),
    edited: () => {
      publishNotice = null; // an edit supersedes the last publish notice (the site is now stale)
    },
    removed: () => {
      conflictAttention = false;
    },
    // Like putting a note back: never overwrites, and when its folder is gone the copy goes to the
    // top of the vault — the edits matter more than the place.
    keepCopy: (vault, path, text) =>
      createNoteWithContent(vault, parentDir(path), noteStem(path), text).catch(() =>
        createNoteWithContent(vault, vault, noteStem(path), text),
      ),
    landedOnOpen: (text) => applyReload(text),
    moved: (remap) => {
      if (!activePath) return;
      const to = remap(activePath);
      if (to !== activePath) followOpenNote(to);
    },
    ask: (question) => {
      if (question === "conflict") void drawAttentionToConflict();
      else if (question === "removed") void drawAttentionToRemoved();
      else void drawAttentionToFailure();
    },
  });
  $effect(() => {
    liveDoc = content;
  });
  let frontmatter = $derived(parseFrontmatter(liveDoc));
  // Reading view toggle (ephemeral, per-session) — clean read-only render vs. live-preview editing.
  let reading = $state(false);
  let activeName = $state("");
  let activePath = $state<string | null>(null);
  // Heading to scroll to after the next note load (set by a `[[note#heading]]` click; cleared on any
  // other open so a later plain navigation does not re-scroll).
  let pendingHeading = $state<string | null>(null);
  let startupError = $state<string | null>(null);
  // Path of a previously-opened vault that could not be reopened at startup (moved, deleted, or on a
  // disconnected drive). Set instead of silently creating a new default — that could mask a temporary
  // outage and look like the user's notes vanished. Drives a recovery prompt; null when no such failure.
  let staleVaultPath = $state<string | null>(null);
  // Absolute path where the default vault was created when Documents was unusable (fallback). Drives
  // a dismissible notice so the user knows where their notes live; null when no fallback happened.
  let vaultFallbackPath = $state<string | null>(null);
  let publishNotice = $state<{ kind: "ok" | "error"; text: string; detail?: string; onRetry?: () => void } | null>(null);
  let showAddVersion = $state(false);
  let showVersionHistory = $state(false);
  let showDeletedNotes = $state(false);
  /** What the folder gave up on this open, when it gave up anything. */
  let movedOut = $state<MoveOut | null>(null);
  /** What the last attempt to add a version came to. Not an error — it can also say nothing changed. */
  let versionNotice = $state<string | null>(null);
  // Nothing sends versions anywhere yet; only a remote that receives refs/textree/* can make this true.
  const backup = backupStatus(false);
  let showSettings = $state(false);

  // Sync-conflict surfacing — derived from the live tree (no IPC). Non-destructive: we only
  // flag the duplicate copies a sync tool left behind; the user decides what to do; nothing is removed for them.
  let syncConflicts = $derived(detectSyncConflicts(tree));
  let conflictSig = $derived(syncConflicts.map((c) => c.path).join("|"));
  // Dismissal is keyed by the conflict set's signature, so the banner reappears if a NEW
  // conflict shows up after the user dismissed an earlier set.
  let dismissedConflictSig = $state("");
  let showSyncConflicts = $derived(syncConflicts.length > 0 && conflictSig !== dismissedConflictSig);

  // Saved views whose folder key doesn't belong to this vault root (moved vault / other device).
  // Surfaced so the views don't appear to silently vanish; reset per vault load.
  let dismissedForeignViews = $state(false);
  let showForeignViews = $derived(views.foreignFolders.length > 0 && !dismissedForeignViews);

  // External change (M3) state.
  let reloadVersion = $state(0); // bump on external reload → force Editor re-creation

  // Structure editing (M4) state.
  let selectedNode = $state<TreeNode | null>(null);
  let mode = $state<"none" | "new-folder">("none");
  let nameInput = $state("");
  let opError = $state<FriendlyError | null>(null);
  // Explicit create-target override (e.g. new container after leaf promote). When set, used instead of selection-based inference.
  let createParent = $state<string | null>(null);

  let conflictAttention = $state(false);
  let conflictBanner = $state<HTMLElement | null>(null);

  let failureAttention = $state(false);
  let failureBanner = $state<HTMLElement | null>(null);

  async function drawAttentionToFailure() {
    failureAttention = false;
    await tick();
    failureAttention = true;
    failureBanner?.focus();
  }

  let removedAttention = $state(false);
  let removedBanner = $state<HTMLElement | null>(null);

  async function drawAttentionToRemoved() {
    removedAttention = false;
    await tick();
    removedAttention = true;
    removedBanner?.focus(); // as for the conflict banner: a stray keypress must not answer it
  }

  async function drawAttentionToConflict() {
    conflictAttention = false;
    await tick();
    conflictAttention = true;
    // The banner itself, not a button: a keypress meant for wherever they were going must not
    // answer the question — the first button discards their edits.
    conflictBanner?.focus();
  }

  function handleEdit(text: string) {
    liveDoc = text; // keep the page header in sync with in-editor frontmatter edits
    // The note has moved on from the state that was recorded, so what was said about it no
    // longer describes what is on screen.
    versionNotice = null;
    if (activePath) save.schedule(activePath, text);
  }

  /** Open a vault path and load the tree (separate from the dialog — reused by the test bridge). */
  /** Returns false when the switch did not happen: the open note's edits are not saved yet. */
  async function loadVault(path: string): Promise<boolean> {
    // preserve unsaved edits before switching vault
    if ((await save.beforeLeaving()) !== "saved") return false;
    root = path;
    tree = await openVault(path);
    // When the vault changes, the previous vault's selection, open note, and edit-mode context are invalid.
    // If not cleared, a stale selectedNode would wrongly target the previous vault's path as the
    // parent for creation/move, sending operations to the wrong location or failing.
    closeEditor();
    selectedNode = null;
    cancelMode();
    chatStore.cancel();
    chatStore.started = false;
    layout.setMode("note");
    backlinks.clear(); // drop the previous vault's index (the $effect below rebuilds it)
    relatedNotes.clear(); // drop the previous vault's related panel
    // Before anything reads settings: they may still be inside the folder from an earlier
    // version, and this is what carries them out. Keeping every file is its own guarantee.
    try {
      const moved = await moveStateOutOfVault(path);
      movedOut = worthShowing(moved) ? moved : null;
    } catch (e) {
      // A folder that could not be tidied still opens; the notes are what matter.
      opError = friendlyError(e);
    }
    await nav.load(path); // load favorites/order settings
    await views.load(path); // load saved folder views (views.json, kept with the app)
    dismissedForeignViews = false; // re-evaluate the foreign-views notice for the new vault
    return true;
  }

  /** Persist the opened vault as last-vault and auto-select the first note (wedge1 onboarding).
   *  Shared by startup, the default-vault recovery, and the manual folder picker so every successful
   *  open is remembered and lands on content instead of the "Select a note." empty state. */
  async function finalizeOpenedVault() {
    if (!root) return;
    localStorage.setItem(LAST_VAULT_KEY, root);
    if (!activePath) {
      const first = findFirstOpenableNote(tree, root, nav.order);
      if (first) await handleSelect(first);
    }
  }

  /** Create/open the default vault (first run or recovery), surfacing the fallback location if
   *  Documents was unusable. Throws if no candidate base could be created. */
  async function openDefaultVault() {
    const dv = await ensureDefaultVault();
    if (!(await loadVault(dv.path))) return;
    // Documents was unusable → the vault landed elsewhere. Surface where, so the user always
    // knows where their notes live (data sovereignty) instead of a silent relocation.
    if (dv.fellBack && root) vaultFallbackPath = root;
    await finalizeOpenedVault();
  }

  /** Recovery from a missing last vault: start fresh with the default vault instead of dead-ending. */
  async function startWithDefaultVault() {
    staleVaultPath = null;
    startupError = null;
    try {
      await openDefaultVault();
    } catch (e) {
      root = null;
      startupError = String(e);
    }
  }

  async function chooseVault() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      if (!(await loadVault(selected))) return;
      // A successful manual open clears any prior startup failure and is remembered as last-vault.
      staleVaultPath = null;
      startupError = null;
      await finalizeOpenedVault();
    }
  }

  // True while a publish (to a folder or to the web) is in flight — shows the "Publishing…" banner
  // and keeps a second publish from starting alongside it.
  let publishing = $state(false);

  /**
   * Publish the open vault to a static site at `out` (outside the vault). Read-only over the
   * source. The app's tokens are rewritten for prefers-color-scheme so the site auto-themes.
   * Split from the folder picker so the E2E bridge can drive it without the native dialog.
   */
  async function publishToDir(out: string) {
    // Same busy flag as publishing to the web: the render can take a while (the first one after
    // install reads the renderer's files cold), and without it nothing on screen says it started.
    if (!root || publishing) return;
    publishing = true;
    publishNotice = null;
    try {
      const result = await publishSite(root, out, {
        tokensCss: toPublishTokens(tokensCssRaw),
      });
      publishNotice = {
        kind: "ok",
        text: `Published ${result.pageCount} page(s) → ${result.outDir}. Upload this folder to GitHub Pages or Cloudflare Pages to put it online.`,
      };
    } catch (e) {
      const fe = friendlyError(e);
      publishNotice = {
        kind: "error",
        text: `Publish failed: ${fe.summary}`,
        detail: fe.raw !== fe.summary ? fe.raw : undefined,
      };
    } finally {
      publishing = false;
    }
  }

  async function choosePublishTarget() {
    if (!root || publishing) return;
    const out = await open({
      directory: true,
      multiple: false,
      title: "Choose an empty folder to publish the site into",
    });
    if (typeof out === "string") await publishToDir(out);
  }

  // True while the browser OAuth round-trip (connect_publish) is in flight — shows a distinct
  // "Connecting…" banner instead of "Publishing…".
  let cloudConnecting = $state(false);

  /**
   * Publish the open vault to the web (pub.textree.me) in one action: if no token is stored, guide
   * the user to Settings; otherwise render + upload and show the resulting URL. Read-only over the
   * source (same canopy render as the local publish).
   */
  async function publishToWeb() {
    if (!root || publishing) return;
    // Claim the busy flag synchronously, before the first await, so two rapid invocations can't both
    // pass the guard and start concurrent uploads. `finally` resets it on every path (including the
    // no-token early return below).
    publishing = true;
    publishNotice = null;
    try {
      // First-time publish: no token yet → run the in-app sign-in (browser OAuth loopback + PKCE)
      // instead of dead-ending on "paste a token". On success the token is stored; we continue.
      if (!(await hasPublishToken())) {
        cloudConnecting = true;
        try {
          await connectPublish();
        } finally {
          cloudConnecting = false;
        }
      }
      const result = await publishToCloud(root, { tokensCss: toPublishTokens(tokensCssRaw) });
      publishNotice = cloudPublishNotice(result);
    } catch (e) {
      publishNotice = { ...cloudPublishErrorNotice(e), onRetry: () => void publishToWeb() };
    } finally {
      cloudConnecting = false;
      publishing = false;
    }
  }

  // ── Inline title editing ──────────────────────────────────────────
  let titleEditing = $state(false);
  let titleInput = $state("");
  // On Escape cancel, suppress once the commit from the blur that fires as the input disappears.
  let suppressTitleBlur = false;

  function startTitleEdit() {
    if (!activePath) return;
    titleInput = activeName;
    titleEditing = true;
  }

  /** Focus + select-all on mount (inline title input). Deferred to the next frame because
   *  `use:` actions run before `bind:value` populates the input — selecting synchronously
   *  would select the still-empty value, leaving the caret at the end once value lands. */
  function focusSelect(node: HTMLInputElement) {
    requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
  }

  function cancelTitleEdit() {
    suppressTitleBlur = true; // so the following blur does not commit
    titleEditing = false;
  }

  /** Confirm title edit → rename the active note and follow the new path. */
  async function commitTitle() {
    // Ignore the blur right after an Escape cancel.
    if (suppressTitleBlur) {
      suppressTitleBlur = false;
      return;
    }
    // Re-entry guard: if Enter handles it first, titleEditing=false → ignore the following blur.
    if (!titleEditing) return;
    const name = titleInput.trim();
    titleEditing = false;
    if (!root || !activePath || !name || name === activeName) return;
    const node = findByBody(tree, activePath);
    if (!node) return;
    const left = await save.beforeLeaving(); // preserve unsaved edits before rename
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Rename", left)); // like its failure below: a refusal is not a failed save
      return;
    }
    try {
      const vault = root;
      await save.move(async () =>
        remapRenamed(node.path, await renameNode(vault, node.path, name), node.kind !== "leaf"),
      );
      await refreshTree();
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  // ── T4: first H1 → filename, unnamed notes only (C70 safe sync) ─────
  // Fires on editor blur. Renames the open note to match its first H1 *only while it is still
  // named "Untitled"/"Untitled (n)". Once named, H1 edits never rename (protects inbound links —
  // fs_ops rename does not rewrite [[wikilinks]]). Reverse (filename→body), on-open and background
  // sync are never implemented. Best-effort: any failure leaves the filename unchanged.
  // In-flight sync promise (not a bool): a navigation handler can `await` it to guarantee the
  // rename completes before it switches notes, while the editor blur fires it fire-and-forget.
  // Concurrent callers coalesce onto the same run; it is idempotent once the note is named.
  let syncingName: Promise<void> | null = null;
  function maybeSyncH1Filename(): Promise<void> {
    return (syncingName ??= doSyncH1Filename().finally(() => {
      syncingName = null;
    }));
  }
  async function doSyncH1Filename() {
    if (!root || !activePath || !isUnnamed(activeName)) return;
    const h1 = extractFirstH1(liveDoc);
    if (!h1) return;
    const candidate = sanitizeForFilename(h1);
    if (!candidate || candidate === activeName) return;
    // Capture the target before any await: if the user navigates to another note during the
    // flush/rename, activePath changes — we must not rename (or follow) the wrong note.
    const pathToRename = activePath;
    try {
      await save.flush(); // persist the body (including the H1) to the current path before renaming
      if (save.pending) return; // unsaved edits could not be saved (or are in conflict) → skip the rename
      if (activePath !== pathToRename) return; // navigated away during flush → skip
      let newPath = pathToRename;
      const vault = root;
      await save.move(async () => {
        newPath = await renameNoteUnique(vault, pathToRename, candidate);
        return remapRenamed(pathToRename, newPath, false);
      });
      await refreshTree();
      if (activePath === newPath) selectedNode = null; // still on the renamed note
      opError = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  // ── Inline tree rename (T2) ────────────────────────────────────────
  let renamingPath = $state<string | null>(null);

  /** Start an inline rename of the given node (F2 / Rename button / palette). */
  function beginTreeRename(node: TreeNode | null) {
    if (!node) return;
    selectedNode = node;
    renamingPath = node.path;
  }

  function cancelTreeRename() {
    renamingPath = null;
  }

  /** Commit an inline tree rename. Returns null on success, FriendlyError on failure (input stays open). */
  async function commitTreeRename(
    node: TreeNode,
    rawName: string,
  ): Promise<FriendlyError | null> {
    if (!root) return null;
    const name = rawName.trim();
    if (!name || name === node.name) {
      renamingPath = null; // no-op cancel
      return null;
    }
    const left = await save.beforeLeaving(); // preserve unsaved edits before the structure change
    if (left === "asking") {
      return friendlyError("Rename canceled — first answer the question about the open note.");
    }
    if (left === "failed" || left === "busy") {
      return friendlyError(leaveRefusal("Rename", left));
    }
    try {
      const vault = root;
      // The open note, if it is the renamed one or inside it, stays open at its new path.
      await save.move(async () =>
        remapRenamed(node.path, await renameNode(vault, node.path, name), node.kind !== "leaf"),
      );
      await refreshTree();
      selectedNode = null;
      renamingPath = null; // success → exit edit mode
      return null;
    } catch (e) {
      return friendlyError(e); // keep renamingPath → input stays open, error shown inline
    }
  }

  /** Keyboard Delete: select the node and delete it (trash). */
  function handleDelete(node: TreeNode) {
    selectedNode = node;
    void deleteSelected();
  }

  async function handleSelect(node: TreeNode) {
    // Before navigating away, deterministically rename the (unnamed) note being left from its first
    // H1 — awaited so it completes before activePath changes (the blur handler may also fire it; the
    // promise coalesces, so this awaits the same run rather than racing it).
    await maybeSyncH1Filename();
    selectedNode = node; // structure-edit target (including folders)
    if (!root) return;
    pendingHeading = null; // a direct open does not scroll to a heading (cleared before the open)
    // preserve unsaved edits of the previous note before navigating anywhere
    const left = await save.beforeLeaving();
    if (left === "asking" || left === "failed") return;
    if (!node.body_path) {
      // A container with no folder note → show only its table; clear any stale open note so an
      // unrelated note doesn't linger above the folder's table.
      activePath = null;
      activeName = "";
      content = "";
      save.opened("");
      return;
    }
    const text = await readNote(root, node.body_path);
    if (activePath !== null && samePath(node.body_path, activePath)) {
      // The note already open, opened again (see decideReopen).
      const ownPending = save.pending !== null && samePath(save.pending.path, activePath);
      const action = decideReopen(ownPending, text, liveDoc);
      if (action === "keep") return;
      if (action === "rebuild") reloadVersion += 1;
    }
    content = text;
    activeName = node.name;
    activePath = node.body_path;
    save.opened(content);
  }

  // ── Structure editing (M4) ──────────────────────────────────────────────
  /**
   * The open note now lives at `path` — it, or a folder holding it, was moved or renamed. A new
   * path recreates the editor from `content`, so `content` must hold what is on screen now: left at
   * the text the note was opened with, the next keystroke would save that text over everything
   * typed since.
   */
  function followOpenNote(path: string) {
    content = liveDoc;
    activePath = path;
    activeName = noteStem(path);
  }

  async function refreshTree() {
    if (root) tree = await listTree(root);
  }

  /** Reverse-lookup a node in the tree by body_path (to confirm the rename target for inline title editing). */
  function findByBody(nodes: TreeNode[], body: string): TreeNode | null {
    for (const n of nodes) {
      if (n.body_path && samePath(n.body_path, body)) return n;
      const c = findByBody(n.children, body);
      if (c) return c;
    }
    return null;
  }

  /** Parent directory for a new item. Explicit override (createParent) takes priority; otherwise
   *  a container = inside it, a leaf = sibling (parent), no selection = root. */
  function targetParent(): string {
    if (createParent) return createParent;
    if (!selectedNode) return root as string;
    if (selectedNode.kind === "container") return selectedNode.path;
    return parentDir(selectedNode.path); // leaf selected → create as sibling
  }

  function closeEditor() {
    activePath = null;
    activeName = "";
    content = "";
    // Discards the edit context (including unsaved edits) and the errors about it, so a stale error
    // does not outlive a vault switch or a closed note.
    save.closed();
    // What the last version attempt came to was about the note being left, not this one.
    versionNotice = null;
    relatedNotes.clear();
  }

  function startMode(
    m: "new-folder",
    parentOverride: string | null = null,
  ) {
    createParent = parentOverride;
    mode = m;
    nameInput = "";
    opError = null;
  }

  /**
   * Add a new note under the selected leaf note (design §3.3 auto-promote).
   * Promotes leaf `foo.md` to container `foo/foo.md`, then immediately creates a new Untitled
   * note inside the new container. If the promoted leaf was open, follow the new body path.
   */
  async function startAddChild() {
    if (!root || !selectedNode || selectedNode.kind !== "leaf") return;
    const leaf = selectedNode.path;
    const left = await save.beforeLeaving(); // preserve current edits before promote
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Operation", left));
      return;
    }
    try {
      const vault = root;
      let newDir = leaf;
      await save.move(async () => {
        newDir = await promoteNode(vault, leaf);
        return remapPromoted(leaf, newDir);
      });
      await refreshTree();
      selectedNode = null;
      void createNewNote(newDir); // target the new container
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  function cancelMode() {
    mode = "none";
    nameInput = "";
    createParent = null;
    opError = null;
  }

  /** Create a new "Untitled" note (no dialog), open it, and focus the header title for renaming. */
  async function createNewNote(parent: string) {
    if (!root) return;
    const left = await save.beforeLeaving(); // preserve current edits before structure change
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Operation", left));
      return;
    }
    try {
      const p = await createUntitledNote(root, parent);
      await refreshTree();
      content = await readNote(root, p);
      activeName = noteStem(p);
      activePath = p;
      save.opened(content);
      selectedNode = null;
      // Wait for the .title header (and its input) to render before focusing.
      // No focus race: Editor remounts on the new docKey but Editor.svelte calls no
      // .focus()/autofocus (verified — only a `.cm-focused` style rule), and CodeMirror
      // does not autofocus by default, so a single tick is enough for .title-input focus.
      await tick();
      startTitleEdit();
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  async function confirmMode() {
    if (!root) return;
    const name = nameInput.trim();
    if (!name) return;
    const left = await save.beforeLeaving(); // preserve current edits before structure change
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Operation", left));
      return;
    }
    try {
      await createFolder(root, targetParent(), name);
      await refreshTree();
      mode = "none";
      nameInput = "";
      createParent = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  async function deleteSelected() {
    if (!root || !selectedNode) return;
    const target = selectedNode.path;
    const affectsOpen = activePath !== null && pathInside(activePath, target);
    const left = await save.beforeLeaving();
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Delete", left));
      return;
    }
    try {
      await deleteNode(root, target);
      await refreshTree();
      if (affectsOpen) closeEditor();
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  /**
   * Move a node to another folder (destDir) via drag-and-drop. Silently ignores or notifies on
   * meaningless/impossible cases; otherwise delegates to `move_node` then refreshes the tree.
   */
  async function handleMove(src: string, destDir: string) {
    if (!root) return;
    if (samePath(src, destDir)) return; // dropped onto itself — no-op
    if (samePath(parentDir(src), destDir)) return; // already in that folder — no-op
    if (pathInside(destDir, src)) {
      opError = friendlyError("Cannot move a node into its own subfolder.");
      return;
    }
    const left = await save.beforeLeaving(); // preserve current edits before move
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Move", left));
      return;
    }
    try {
      const vault = root;
      await save.move(async () => {
        return remapMoved(src, await moveNode(vault, src, destDir));
      });
      await refreshTree();
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  /**
   * Drop onto a leaf note (adopt): promote leaf to a container and move src into it as a child.
   * The backend `adopt_node` handles this atomically (rolls back the promote on failure). If the
   * open note is the promoted leaf or inside the moved src, follow the new path.
   */
  async function handleAdopt(src: string, leaf: string) {
    if (!root) return;
    if (samePath(src, leaf)) return; // dropped onto itself — no-op
    if (pathInside(leaf, src)) {
      opError = friendlyError("Cannot move a node into its own descendant.");
      return;
    }
    const left = await save.beforeLeaving();
    if (left === "asking") return;
    if (left === "failed" || left === "busy") {
      opError = friendlyError(leaveRefusal("Operation", left));
      return;
    }
    try {
      const vault = root;
      await save.move(async () => {
        return remapAdopted(src, leaf, await adoptNode(vault, src, leaf));
      });
      await refreshTree();
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = friendlyError(e);
    }
  }

  /**
   * Image paste: save the attachment to assets/ next to the current note and return the markdown link to insert.
   * On save failure, surface via saveError and return null (no insertion).
   */
  async function handleImagePaste(dataBase64: string, ext: string): Promise<string | null> {
    if (!root || !activePath) return null;
    try {
      const rel = await saveAttachment(root, activePath, dataBase64, ext);
      saveState.saveError = null;
      return `![](${rel})`;
    } catch (e) {
      saveState.saveError = friendlyError(e);
      return null;
    }
  }

  // ── External change reconciliation (sync.ts callbacks) ───────────────────────────────
  function applyReload(diskContent: string) {
    content = diskContent;
    reloadVersion += 1; // trigger Editor re-creation
    save.reloaded(diskContent);
  }

  /** Removed-note banner: put the note back where it was, holding the unsaved edits. */
  async function resolvePutBack() {
    const edit = save.pending;
    if (!root || !activePath || !edit) return;
    const vault = root;
    const path = activePath;
    const text = edit.text;
    const stem = noteStem(path);
    try {
      // Never overwrites: if something has taken the name meanwhile, the note comes back beside it.
      // When its folder went with it, the note comes back at the top of the vault instead — the
      // edits matter more than the place.
      const created = await createNoteWithContent(vault, parentDir(path), stem, text).catch(() =>
        createNoteWithContent(vault, vault, stem, text),
      );
      const moved = !pathInside(created, path);
      if (moved) {
        content = save.pending?.text ?? text; // the editor is recreated for the new path
        activePath = created;
        activeName = noteStem(created);
      }
      save.putBack(text, moved ? created : null); // after the path: it follows the open note
      removedAttention = false;
      await refreshTree();
      if (save.pending) void save.flush();
    } catch (e) {
      saveState.saveError = friendlyError(e); // the edits stay where they are; the banner stays too
    }
  }

  /** Save-failure banner: let the unsaved edits go — the note goes back to what it last saved. */
  function resolveDiscardUnsaved() {
    failureAttention = false;
    applyReload(save.discardUnsaved());
  }

  /** Removed-note banner: let the unsaved edits go. */
  function resolveDiscardRemoved() {
    save.discardRemoved();
    removedAttention = false;
  }

  /** Conflict banner: overwrite with the copy on disk, discarding my edits. */
  function resolveTakeDisk() {
    const disk = save.takeDisk();
    if (disk !== null) applyReload(disk);
    conflictAttention = false;
  }

  /**
   * Conflict banner: keep my edits — they replace the copy on disk now. The copy being replaced is
   * the one the banner showed; if it changed yet again since, the save stops and asks once more.
   */
  function resolveKeepMine() {
    save.keepMine();
    conflictAttention = false;
  }

  /** Last folder name of the vault root path (for the compact sidebar header). Full path is in the title. */
  function vaultName(p: string): string {
    return baseName(p.replace(/[/\\]+$/, "")) || p;
  }

  /** Ancestor folders of the open note (breadcrumb). Excludes the filename and the duplicate folder of a container note. */
  function breadcrumb(): string[] {
    if (!root || !activePath) return [];
    const rel = activePath.slice(root.length).replace(/^[/\\]+/, "");
    const parts = rel.split(/[/\\]/).filter(Boolean);
    const file = parts.pop() ?? "";
    const stem = file.replace(/\.md$/i, "");
    // Container note (folder/folder.md): if the last folder equals the file stem, it is a duplicate → remove.
    if (parts.length && parts[parts.length - 1] === stem) parts.pop();
    return parts;
  }

  // ── Sidebar resize ──────────────────────────────────────────
  // Adjust width via pointer capture on the drag handle. Persist once on release (avoid localStorage
  // thrashing during drag). pointermove/up are pinned to the handle via setPointerCapture, not window.
  function startResize(e: PointerEvent) {
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => layout.setWidth(ev.clientX);
    const onUp = (ev: PointerEvent) => {
      handle.releasePointerCapture(ev.pointerId);
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      layout.persistWidth();
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
  }

  // ── Unified palette (P1a Task 11) ───────────────────────────────────
  interface FileEntry {
    name: string;
    path: string;
    kind: "leaf" | "container";
  }

  function flattenTree(nodes: TreeNode[], acc: FileEntry[] = []): FileEntry[] {
    for (const n of nodes) {
      acc.push({ name: n.name, path: n.path, kind: n.kind });
      if (n.children?.length) flattenTree(n.children, acc);
    }
    return acc;
  }

  let fileIndex = $derived<FileEntry[]>(flattenTree(tree));

  // The app's tree/IPC paths are absolute; wikilinks use vault-relative POSIX paths so the text
  // written into `.md` (`[[note]]`) stays portable (data sovereignty) and matches canopy's model.
  // These helpers convert at the boundary, tolerant of mixed `/`/`\` separators.
  function toRelative(abs: string): string {
    const a = abs.replace(/\\/g, "/");
    const r = (root ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
    return r && a.startsWith(`${r}/`) ? a.slice(r.length + 1) : a;
  }
  function toAbsolute(rel: string): string {
    const r = (root ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
    return r ? `${r}/${rel}` : rel;
  }

  /** Collect every note's absolute `.md` path: leaf=own path, container=its folder note. */
  function collectNotePaths(nodes: TreeNode[], acc: string[] = []): string[] {
    for (const n of nodes) {
      if (n.kind === "leaf") acc.push(n.path);
      else if (n.body_path) acc.push(n.body_path);
      if (n.children?.length) collectNotePaths(n.children, acc);
    }
    return acc;
  }

  /** Wikilink resolution targets (vault-relative) — passed to the editor; resolve against the live tree. */
  let notePaths = $derived<string[]>(collectNotePaths(tree).map(toRelative));

  /** Find the tree node for an absolute `.md` path (leaf path or folder note), separator-tolerant. */
  function findNodeByNotePath(nodes: TreeNode[], abs: string): TreeNode | null {
    const norm = (p: string | null) => (p ?? "").replace(/\\/g, "/");
    const want = norm(abs);
    for (const n of nodes) {
      if (norm(n.body_path) === want || (n.kind === "leaf" && norm(n.path) === want)) return n;
      const found = n.children?.length ? findNodeByNotePath(n.children, abs) : null;
      if (found) return found;
    }
    return null;
  }

  /**
   * Open the note a wikilink resolves to (target is vault-relative). Reuses `handleSelect` (flush,
   * read, selection follow) so navigation behaves exactly like clicking the note in the tree.
   */
  async function handleWikiLink(relPath: string, heading: string | undefined) {
    const node = findNodeByNotePath(tree, toAbsolute(relPath));
    if (!node) return;
    await handleSelect(node); // clears pendingHeading, then opens (recreates the editor)
    // Set after handleSelect so the editor recreation picks it up and scrolls once on load.
    pendingHeading = heading ?? null;
  }

  /**
   * Rebuild the vault-wide backlink index by reading every note's body once. Frontend scan (no new
   * IPC); the simplicity beats a Rust link graph at this scale: simplicity wins until it measurably does not.
   * Reads use absolute paths (the IPC contract); the index is keyed by vault-relative paths.
   * A failed read degrades that note to empty rather than aborting the whole index.
   */
  async function rebuildBacklinks() {
    const r = root;
    if (!r) return;
    const absPaths = collectNotePaths(tree);
    const relPaths = absPaths.map(toRelative);
    const resolve = buildWikiResolver(relPaths).resolve;
    const notes = await Promise.all(
      absPaths.map(async (abs, i) => ({
        path: relPaths[i],
        body: await readNote(r, abs).catch(() => ""),
      })),
    );
    backlinks.rebuild(notes, resolve);
  }

  // Rebuild the backlink index whenever the note set changes (vault load, create/rename/move/delete).
  // Edits to the open note are handled incrementally on save (see flush); this covers structure.
  $effect(() => {
    void notePaths;
    if (root) void rebuildBacklinks();
  });

  // Refresh the related-notes panel when the active note or scope changes.
  // Keyed on activePath + content (note open) + semanticScopePath (scope shift).
  // Uses `content` (load-only) rather than `liveDoc` to avoid firing on every keystroke.
  $effect(() => {
    const path = activePath;
    const body = content;
    const scope = semanticScopePath;
    const r = root;
    if (r && path) {
      void relatedNotes.load(r, toRelative(path), body, scope);
    } else {
      relatedNotes.clear();
    }
  });

  // Stop the host generating when the user leaves Chat mode (mirror AskPanel's
  // onDestroy cancel — the shipped /ask I1 lesson: cancel must reach the host).
  $effect(() => {
    if (layout.mode !== "chat") chatStore.cancel();
  });

  // ── Frontmatter table (folder = DB, .md = row) — read-only first slice ────
  // Built from already-parsed frontmatter (no backend leakage; held in memory only). Reads each direct
  // child note's body once via the existing readNote IPC — same frontend-scan precedent as backlinks.
  let folderTable = $state<FolderTable | null>(null);
  async function buildFolderTableFor(node: TreeNode): Promise<void> {
    const r = root;
    if (!r) {
      folderTable = null;
      return;
    }
    const children = node.children.filter((c) => c.kind === "leaf" || c.body_path);
    const notes = await Promise.all(
      children.map(async (c) => {
        const notePath = c.kind === "leaf" ? c.path : (c.body_path as string);
        const body = await readNote(r, notePath).catch(() => "");
        return { name: c.name, path: notePath, frontmatter: parseFrontmatter(body).data };
      }),
    );
    // Guard a stale async result: if the selection moved on while we read, don't commit (a slower
    // earlier read could otherwise overwrite a newer folder's table).
    if (selectedNode?.path !== node.path) return;
    folderTable = buildFolderTable(notes);
  }
  // Recompute when the selection changes to a folder, or when the tree changes underneath it
  // (child added/renamed/deleted). Look the node up in the live tree to avoid a stale children ref.
  $effect(() => {
    const sel = selectedNode;
    void tree;
    if (root && sel && sel.kind === "container") {
      void buildFolderTableFor(findNode(tree, sel.path) ?? sel);
    } else {
      folderTable = null;
    }
  });

  const actions: PaletteActions = {
    openVault: () => { void chooseVault(); },
    toggleTheme: () => { theme.toggle(); },
    toggleSidebar: () => { layout.toggleCollapsed(); },
    toggleReading: () => { reading = !reading; },
    // Entering Chat must go through enterChat() so the session is started (a bare mode flip
    // would render ChatView with no active session). Leaving Chat is a plain mode switch.
    toggleMode: () => {
      if (layout.mode === "note") enterChat();
      else layout.setMode("note");
    },
    // Create at root: parentOverride=root targets the root regardless of selectedNode state.
    newNoteAtRoot: () => { if (root) void createNewNote(root); },
    newFolderAtRoot: () => { if (root) startMode("new-folder", root); },
    hasSelection: () => selectedNode !== null,
    renameSelected: () => { beginTreeRename(selectedNode); },
    deleteSelected: () => { void deleteSelected(); },
    // Promote is leaf-only — meaningful only when the selected node is a leaf (also checked inside startAddChild).
    promoteSelected: () => { void startAddChild(); },
    toggleFavoriteSelected: () => {
      const p = selectedNode?.path ?? null;
      if (p) void nav.toggleFavorite(p);
    },
    moveSelectedUp: () => reorderSelected(-1),
    moveSelectedDown: () => reorderSelected(1),
    rebuildIndex: () => {
      if (root) void rebuildIndex(root);
    },
    hasVault: () => root !== null,
    publishSite: () => { void choosePublishTarget(); },
    publishToWeb: () => { void publishToWeb(); },
    openDeletedNotes: () => { showDeletedNotes = true; },
    hasOpenNote: () => root !== null && activePath !== null,
    addVersion: () => { void startAddVersion(); },
    openVersionHistory: () => { showVersionHistory = true; },
    openLogDir: () => { void openLogDir(); },
    openSettings: () => { showSettings = true; },
  };

  /**
   * Opens the dialog that records a version, after making sure the file on disk is the one the
   * version will be taken from — a version of a state that is still only in the editor would
   * record the wrong thing while looking like it worked.
   */
  async function startAddVersion() {
    if (!root || !activePath || saveState.removed) return; // nothing on disk to record
    versionNotice = null;
    // A version of what is on disk is only the right version once the disk has it.
    if (saveState.dirty && (await save.beforeLeaving()) !== "saved") return;
    showAddVersion = true;
  }

  let commands = $derived(activeCommands(buildCommands(actions)));

  /** Search the tree by TreeNode.path (null if not found). */
  function findNode(nodes: TreeNode[], p: string): TreeNode | null {
    for (const n of nodes) {
      if (n.path === p) return n;
      if (n.children?.length) {
        const c = findNode(n.children, p);
        if (c) return c;
      }
    }
    return null;
  }

  /** TreeNode.path → POSIX relative path from the vault root (same form as search hit path). */
  function relPosixOf(absPath: string, rootDir: string): string {
    const rel = absPath.startsWith(rootDir) ? absPath.slice(rootDir.length) : absPath;
    return rel.replace(/^[\\/]+/, "").replace(/\\/g, "/");
  }

  /**
   * Palette content search → resolve each hit's POSIX relative path to the actual TreeNode.path.
   * Reconstructing the path as a string (guessing the separator) would diverge from the mixed
   * separators the backend emits (forward-slash root + OS join), making findNode exact-match fail →
   * map to the actual node path to make it robust. Folder notes (folder/folder.md) map to the
   * container node (loads the body on selection).
   */
  async function searchContentFromPalette(query: string): Promise<SearchHit[]> {
    if (!root) return [];
    const r = root;
    const hits = await searchContent(query);
    const byRel = new Map<string, string>();
    for (const e of fileIndex) {
      const rel = relPosixOf(e.path, r);
      byRel.set(rel, e.path);
      if (e.kind === "container") byRel.set(`${rel}/${e.name}.md`, e.path);
    }
    return hits.map((h) => ({ ...h, path: byRel.get(h.path) ?? h.path }));
  }

  /**
   * Scope path for semantic search — the folder the user is currently working in.
   * Priority: selected folder → parent of the open note → null (whole vault).
   * Passed as an absolute path; the backend scopes the search to documents under it.
   */
  let semanticScopePath = $derived<string | null>(
    selectedNode?.kind === "container"
      ? selectedNode.path
      : activePath
        ? parentDir(activePath)
        : null,
  );

  /** Capture the current tree selection as a pinned chat scope. */
  function chatScopeFromSelection(): ChatScope {
    const sel = selectedNode;
    if (sel?.kind === "container") return { kind: "folder", path: sel.path, label: sel.name };
    if (sel?.kind === "leaf") return { kind: "file", path: sel.path, label: sel.name };
    return { kind: "vault", path: null, label: "Whole vault" };
  }

  /** Enter Chat mode; start a session pinned to the selection only if none is active. */
  function enterChat() {
    if (!chatStore.started) chatStore.startSession(chatScopeFromSelection());
    layout.setMode("chat");
  }

  /** New chat / re-scope: pin to the current selection and start fresh. */
  function newChatFromSelection() {
    chatStore.startSession(chatScopeFromSelection());
  }

  /** Open a cited note and return to Note mode (natural reading flow). */
  function openCitedNote(path: string) {
    layout.setMode("note");
    openFileFromPalette(path);
  }

  /** Open a just-saved summary note in Note mode (refresh the tree so it appears in the sidebar). */
  async function openSavedNote(path: string) {
    await refreshTree();
    layout.setMode("note");
    openFileFromPalette(path);
  }

  /** Palette file selection → find the matching TreeNode and delegate to handleSelect.
   *  Accepts both absolute TreeNode paths (file/command mode) and vault-relative POSIX
   *  paths (semantic search hits from the sidecar) so all modes converge here. */
  function openFileFromPalette(path: string): void {
    // Try exact absolute match first (file / content modes).
    let node = findNode(tree, path);
    if (!node && root) {
      // Semantic hits arrive as vault-relative POSIX paths — resolve to absolute via byRel map.
      const r = root.replace(/\\/g, "/").replace(/\/+$/, "");
      const byRel = new Map<string, string>();
      for (const e of fileIndex) {
        const rel = relPosixOf(e.path, root);
        byRel.set(rel, e.path);
        if (e.kind === "container") byRel.set(`${rel}/${e.name}.md`, e.path);
      }
      const abs = byRel.get(path) ?? (path.startsWith(r) ? path : null);
      if (abs) node = findNode(tree, abs);
    }
    if (node) {
      void handleSelect(node);
      nav.pushRecent(node.path);
    }
  }

  /**
   * Move the selected node by delta slots (+1=down, -1=up) within its parent's sibling list and persist order.
   * The sibling array uses the same mergeOrder-applied order as the TreeView render —
   * the parentPath key also matches TreeView (root=root, otherwise the parent container path).
   */
  function reorderSelected(delta: number): void {
    if (!root || !selectedNode) return;
    const path = selectedNode.path;
    const parentPath = parentDir(path); // for a root-level node, equals root
    const parentNode = parentPath === root ? null : findNode(tree, parentPath);
    const siblings = parentNode ? parentNode.children : tree;
    const ordered = mergeOrder(siblings, nav.order[parentPath] ?? [], (n) => n.path);
    const idx = ordered.findIndex((n) => n.path === path);
    if (idx === -1) return;
    const next = moveInArray(ordered, idx, delta);
    if (next === ordered) return; // boundary — no change
    void nav.setOrder(parentPath, next.map((n) => n.path));
  }

  function onGlobalKey(e: KeyboardEvent): void {
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "p") {
      e.preventDefault();
      palette.show();
      return;
    }
    // Don't let a command accelerator fire while the user is typing in a form field (the
    // rename / new-name input) — Ctrl+N there would clobber the in-progress action. The editor
    // (contenteditable) is intentionally not excluded: starting a new note while writing is the point.
    if (e.target instanceof HTMLElement && isFormFieldTag(e.target.tagName)) return;
    // Global command accelerators (e.g. Ctrl+N new note). Only active commands are matched, so a
    // disabled command's shortcut is inert. `commands` is already when()-filtered.
    const cmd = commands.find((c) => c.keybinding && matchKeybinding(c.keybinding, e));
    if (cmd) {
      e.preventDefault();
      void cmd.run();
    }
  }

  // On window close, flush unsaved edits to disk before actually closing (guarantees "always saved").
  onMount(() => {
    // E2E test bridge (dev build only — tree-shaken out of the production bundle).
    // Bypasses the native folder dialog so Playwright can open a vault directly.
    if (import.meta.env.DEV) {
      (window as unknown as { __textreeTest?: unknown }).__textreeTest = {
        loadVault,
        publishTo: publishToDir,
        // Non-destructive read used by E2E to branch safely around the machine-global OS
        // keychain token (no test teardown exists for it — see byo_secret.rs).
        hasPublishToken,
      };
    }

    // Decide the startup vault: restore the last one, or create/open the default on first run.
    // Effects (localStorage, IPC) live here; the decision itself is the pure decideStartup().
    void (async () => {
      const plan = decideStartup(localStorage.getItem(LAST_VAULT_KEY));
      if (plan.action === "restore") {
        try {
          if (await loadVault(plan.path)) await finalizeOpenedVault();
        } catch (e) {
          // The stored vault was moved/deleted, or is on a disconnected drive. Do NOT silently
          // create a new default over the user's intended vault — that could mask a temporary
          // outage and look like their notes vanished. Surface a recovery prompt instead
          // (open another folder, or start a fresh default vault).
          root = null;
          staleVaultPath = plan.path;
          startupError = String(e);
        }
      } else {
        try {
          await openDefaultVault();
        } catch (e) {
          // First run and the default could not be created anywhere — land on the empty state.
          root = null;
          startupError = String(e);
        }
      }
    })();

    // Auto-start the local-AI host if the user previously consented (device-local flag). On first
    // run / no consent the host stays unspawned until enabled in the ? palette. If BYO is
    // configured, spawn with that config instead of the bundled local model.
    if (getAiConsent()) {
      void prepareAiModel(getByoConfig() ?? undefined);
      aiHost.startPolling(); // durable cold-download indicator, independent of which view is open
    }

    const win = getCurrentWindow();
    const unlistenClose = win.onCloseRequested(async (event) => {
      if (!save.pending && save.stranded === 0) return; // nothing to save → proceed with default close
      event.preventDefault();
      // Unsaved edits keep the window open, with the banner or warning saying why (see beforeClosing).
      if ((await save.beforeClosing()) === "close") await win.destroy();
    });

    // Subscribe to external file changes.
    const unlistenSyncP = startSync({
      root: () => root,
      activePath: () => activePath,
      activeDoc: () => liveDoc,
      isDirty: () => saveState.dirty,
      synced: () => save.synced,
      setTree: (t) => {
        tree = t;
      },
      reloadActive: applyReload,
      activeRemoved: () => save.markRemoved(),
      activePresent: () => {
        if (save.markPresent()) removedAttention = false;
      },
      conflict: (disk) => {
        save.markConflict(disk);
        removedAttention = false;
      },
    });

    checkForUpdate().then((info) => {
      updateInfo = info;
    });

    return () => {
      void unlistenClose.then((un) => un());
      void unlistenSyncP.then((un) => un());
    };
  });
</script>

<svelte:window onkeydown={onGlobalKey} />
<Palette
  files={fileIndex}
  {commands}
  onOpenFile={openFileFromPalette}
  onRunCommand={(c) => c.run()}
  onSearchContent={searchContentFromPalette}
  vaultRoot={root}
  scopePath={semanticScopePath}
/>

{#if aiHost.download}
  {@const dl = formatModelDownload(aiHost.download)}
  {#if dl}
    <div class="ai-download-bar" role="status" aria-label="Local AI model download">
      <span class="ai-dl-label">{dl.label}</span>
      <div class="ai-dl-track" aria-hidden="true">
        <div class="ai-dl-fill" style="width:{dl.ratio * 100}%"></div>
      </div>
      {#if dl.detail}<span class="ai-dl-detail">{dl.detail}</span>{/if}
      <button class="ai-dl-cancel" onclick={() => aiHost.cancel()} disabled={aiHost.cancelling}>
        {aiHost.cancelling ? "Cancelling…" : "Cancel"}
      </button>
    </div>
  {/if}
{/if}

<div class="shell">
  <TitleBar
    label={contextLabel(layout.mode, activeName, root ? vaultName(root) : "")}
    collapsed={layout.collapsed}
    onToggleSidebar={() => layout.toggleCollapsed()}
    onOpenPalette={() => palette.show()}
  />
  <div class="app" style="--sidebar-width: {layout.width}px">
  {#if !layout.collapsed}
  <aside class="sidebar">
    <div class="sidebar-head">
      {#if root}
        <button
          class="vault-name"
          onclick={chooseVault}
          title={`Switch vault — current: ${root}`}
        ><Icon name="folder" /><span class="vault-label">{vaultName(root)}</span></button>
      {/if}
      <AppMenu {commands} onRunCommand={(c) => c.run()} onOpenPalette={() => palette.show()} />
    </div>
    {#if root}
      <div class="toolbar" role="toolbar" aria-label="Note actions">
        <button onclick={() => void createNewNote(targetParent())} title="New note" aria-label="New note"
          ><Icon name="file-plus" /></button>
        <button onclick={() => startMode("new-folder")} title="New folder" aria-label="New folder"
          ><Icon name="folder-plus" /></button>
        <button
          onclick={startAddChild}
          disabled={selectedNode?.kind !== "leaf"}
          title="Promote the selected note to a folder and add a child note inside it"
          aria-label="Add child note"><Icon name="add-child" /></button>
        <span class="toolbar-sep" aria-hidden="true"></span>
        <button onclick={() => beginTreeRename(selectedNode)} disabled={!selectedNode} title="Rename"
          aria-label="Rename"><Icon name="pencil" /></button>
        <button onclick={deleteSelected} disabled={!selectedNode} title="Delete"
          aria-label="Delete"><Icon name="trash" /></button>
      </div>
      {#if mode !== "none"}
        <div class="name-edit">
          <input
            class="name-input"
            placeholder="Folder name"
            bind:value={nameInput}
            onkeydown={(e) => {
              if (e.key === "Enter") confirmMode();
              else if (e.key === "Escape") cancelMode();
            }}
          />
          <button onclick={confirmMode}>OK</button>
          <button onclick={cancelMode}>Cancel</button>
        </div>
      {/if}
      {#if opError}
        <p
          class="op-error"
          title={opError.raw !== opError.summary ? opError.raw : undefined}
        >⚠ {opError.summary}</p>
      {/if}
      <!-- Dropping outside a node (empty area) moves it to the vault root. Node drops are stopPropagation'd.
           This area is the "vault root drop zone" wrapping the tree, so it is marked as a group. -->
      <div
        class="tree-root"
        role="group"
        aria-label="Vault root — drop here to move to the root"
        ondragover={(e) => {
          e.preventDefault();
          if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
        }}
        ondrop={(e) => {
          e.preventDefault();
          const s = e.dataTransfer?.getData(DRAG_MIME);
          if (s && root) handleMove(s, root);
        }}
      >
        <TreeView
          top
          nodes={tree}
          parentPath={root!}
          onselect={handleSelect}
          onmove={handleMove}
          onadopt={handleAdopt}
          onrename={(node) => beginTreeRename(node)}
          ondelete={handleDelete}
          onfavorite={(node) => nav.toggleFavorite(node.path)}
          oncommitrename={commitTreeRename}
          oncancelrename={cancelTreeRename}
          editingPath={renamingPath}
          selectedPath={selectedNode?.path ?? null}
        />
      </div>
    {:else}
      <p class="hint">{NO_VAULT_HINT}</p>
    {/if}
  </aside>
  <div
    class="resize-handle"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize sidebar"
    onpointerdown={startResize}
  ></div>
  {/if}
  <main class="content">
    {#if updateInfo}
      <UpdateBanner info={updateInfo} />
    {/if}
    {#if publishing}
      <div class="publish-banner publishing" role="status" aria-busy="true">
        <span>{cloudConnecting ? "Connecting… (sign in in the browser window that opened)" : "Publishing…"}</span>
      </div>
    {:else if publishNotice}
      <div class="publish-banner {publishNotice.kind}" role="status">
        <span title={publishNotice.detail}>{publishNotice.kind === "ok" ? "✓" : "⚠"} {publishNotice.text}</span>
        {#if publishNotice.onRetry}
          <button
            class="banner-retry"
            onclick={() => {
              // Snapshot onRetry before clearing the notice, then invoke — order is deliberate so the
              // banner clears immediately and the closure isn't lost when publishNotice is nulled.
              const retry = publishNotice?.onRetry;
              publishNotice = null;
              retry?.();
            }}
          >Retry</button>
        {/if}
        <button
          class="banner-dismiss"
          onclick={() => (publishNotice = null)}
          aria-label="Dismiss"
        ><Icon name="x" size={14} /></button>
      </div>
    {/if}
    {#if root && showSyncConflicts}
      <div class="conflict-banner" role="status" aria-label="Possible sync conflicts">
        <div class="conflict-head">
          <span>⚠ Possible sync conflicts — your sync tool kept duplicate copies. Nothing was changed; review them.</span>
          <button
            class="banner-dismiss"
            onclick={() => (dismissedConflictSig = conflictSig)}
            aria-label="Dismiss"
          >×</button>
        </div>
        <ul class="conflict-list">
          {#each syncConflicts as c (c.path)}
            <li>
              <button class="conflict-item" onclick={() => openFileFromPalette(c.path)} title={c.path}>
                {c.name}
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if root && vaultFallbackPath}
      <div class="conflict-banner" role="status" aria-label="Vault created in a fallback location">
        <div class="conflict-head">
          <span>ⓘ Your Documents folder couldn't be used, so your vault was created here: {vaultFallbackPath}</span>
          <button
            class="banner-dismiss"
            onclick={() => (vaultFallbackPath = null)}
            aria-label="Dismiss"
          >×</button>
        </div>
        <ul class="conflict-list">
          <li>
            <button class="conflict-item" onclick={() => { vaultFallbackPath = null; void chooseVault(); }}>
              Open a different folder…
            </button>
          </li>
        </ul>
      </div>
    {/if}
    {#if root && showForeignViews}
      <div class="conflict-banner" role="status" aria-label="Saved views from another location">
        <div class="conflict-head">
          <span>⚠ {views.foreignFolders.length} saved table view group(s) were created in a different vault location and aren't shown here. Saved views are tied to the folder path, so moving the vault or opening it on another device unlinks them. Nothing was deleted.</span>
          <button
            class="banner-dismiss"
            onclick={() => (dismissedForeignViews = true)}
            aria-label="Dismiss"
          >×</button>
        </div>
      </div>
    {/if}
    {#if layout.mode === "chat" && root}
      <ChatView
        vault={root}
        tree={tree}
        onOpenNote={openCitedNote}
        onNewChat={newChatFromSelection}
        onBack={() => layout.setMode("note")}
        onSaved={openSavedNote}
      />
    {:else if !root}
      <div class="empty-state">
        <h1 class="empty-brand">Textree</h1>
        {#if staleVaultPath}
          <p class="empty-sub">Couldn't open your last vault — it may have moved, been deleted, or be on a disconnected drive.</p>
          <p class="empty-path">{staleVaultPath}</p>
          <div class="empty-actions">
            <button class="open-cta" onclick={chooseVault}>Open a folder</button>
            <button class="open-cta secondary" onclick={startWithDefaultVault}>Start with a default vault</button>
          </div>
        {:else}
          <p class="empty-sub">{NO_VAULT_PROMPT}</p>
          <button class="open-cta" onclick={chooseVault}>Open vault</button>
          {#if startupError}
            <p class="status error">⚠ Could not open vault: {startupError}</p>
          {/if}
        {/if}
      </div>
    {:else if activePath}
      <header class="title">
        <span class="crumbs">
          {#each breadcrumb() as seg}
            <span class="crumb">{seg}</span>
            <span class="sep">›</span>
          {/each}
          {#if titleEditing}
            <input
              class="title-input"
              use:focusSelect
              bind:value={titleInput}
              onkeydown={(e) => {
                if (e.key === "Enter") commitTitle();
                else if (e.key === "Escape") cancelTitleEdit();
              }}
              onblur={commitTitle}
            />
          {:else}
            <button
              class="note-name"
              onclick={startTitleEdit}
              title="Click to change the title (file name)"
            >{activeName}</button>
          {/if}
        </span>
        {#if saveState.saveError}
          <span
            class="status error"
            title={saveState.saveError.raw !== saveState.saveError.summary ? saveState.saveError.raw : undefined}
          >⚠ Save failed: {saveState.saveError.summary}</span>
        {:else if saveState.removed}
          <span class="status error">⚠ Moved/deleted externally</span>
        {:else if versionNotice}
          <span class="status" data-testid="version-notice">{versionNotice}</span>
        {/if}
        {#if backup}
          <span class="status backup-status" data-testid="backup-status" title={backup.tooltip}
          >{backup.label}</span>
        {/if}
        <div class="title-tools">
          {#if !saveState.saveError && !saveState.removed}
            <span class="status">{saveState.dirty ? "● Saving…" : "Saved"}</span>
          {/if}
          <button
            class="icon-btn read-toggle"
            onclick={() => (reading = !reading)}
            title={reading ? "Switch to editing" : "Switch to reading view"}
            aria-label={reading ? "Switch to editing" : "Switch to reading view"}
            aria-pressed={reading}
          ><Icon name={reading ? "pencil" : "book-open"} /></button>
          <button
            class="icon-btn"
            onclick={enterChat}
            title="Switch to chat"
            aria-label="Switch to chat"
          ><Icon name="message-square" /></button>
        </div>
      </header>
      {#if saveState.conflictDisk !== null}
        <div
          class="banner"
          class:attention={conflictAttention}
          role="alert"
          tabindex="-1"
          data-testid="conflict-banner"
          bind:this={conflictBanner}
        >
          <span>This note changed on disk while you have unsaved edits.</span>
          <span class="banner-actions">
            <button onclick={resolveTakeDisk}>Load the copy on disk</button>
            <button onclick={resolveKeepMine}>Keep my edits</button>
          </span>
        </div>
      {/if}
      {#if saveState.saveFailure && saveState.dirty && !saveState.removed && saveState.conflictDisk === null}
        <div
          class="banner"
          class:attention={failureAttention}
          role="alert"
          tabindex="-1"
          data-testid="save-failed-banner"
          bind:this={failureBanner}
        >
          <span>Your latest edits could not be saved: {saveState.saveFailure.summary}</span>
          <span class="banner-actions">
            <button onclick={() => void save.flush()}>Try again</button>
            <button onclick={resolveDiscardUnsaved}>Discard my edits</button>
          </span>
        </div>
      {/if}
      {#if saveState.removed && saveState.dirty}
        <div
          class="banner"
          class:attention={removedAttention}
          role="alert"
          tabindex="-1"
          data-testid="removed-banner"
          bind:this={removedBanner}
        >
          <span>This note was moved or deleted outside the app. Your unsaved edits are still here.</span>
          <span class="banner-actions">
            <button onclick={resolvePutBack}>Put it back with my edits</button>
            <button onclick={resolveDiscardRemoved}>Discard my edits</button>
          </span>
        </div>
      {/if}
      {#if saveState.removed && !saveState.dirty}
        <p class="hint">This note was moved or deleted externally. Select another note.</p>
      {:else}
        <PageHeader
          icon={getField(frontmatter.data, "icon") ?? ""}
          title={getField(frontmatter.data, "title") ?? ""}
        />
        <div class="note-body">
          <div class="editor-pane">
            <Editor
              docKey={`${activePath}@${reloadVersion}`}
              initialDoc={content}
              {reading}
              {notePaths}
              scrollToHeading={pendingHeading}
              onchange={handleEdit}
              onImagePaste={handleImagePaste}
              onWikiLink={handleWikiLink}
              onBlur={() => void maybeSyncH1Filename()}
            />
          </div>
          <Backlinks
            links={backlinks.for(activePath ? toRelative(activePath) : null)}
            onOpen={(p) => handleWikiLink(p, undefined)}
          />
          <RelatedNotes
            related={relatedNotes.items}
            onOpen={(p) => handleWikiLink(p, undefined)}
          />
        </div>
      {/if}
    {:else if selectedNode?.kind !== "container"}
      <p class="hint">{NO_NOTE_PROMPT}</p>
    {/if}
    {#if layout.mode === "note" && selectedNode?.kind === "container" && folderTable}
      <!-- Key by folder path so the (ephemeral) sort state resets when switching folders. -->
      {#key selectedNode.path}
        <FolderTableView
          table={folderTable}
          folder={selectedNode.path}
          onOpen={openFileFromPalette}
        />
      {/key}
    {/if}
    {#if showAddVersion && root && activePath}
      <AddVersion
        {root}
        paths={[activePath]}
        onclose={() => { showAddVersion = false; }}
        ondone={(message) => { versionNotice = message; }}
      />
    {/if}
    {#if showVersionHistory && root && activePath}
      <VersionHistory
        {root}
        path={activePath}
        dirty={saveState.dirty}
        onclose={() => { showVersionHistory = false; }}
        onrestored={() => { if (activePath) void openSavedNote(activePath); }}
      />
    {/if}
    {#if movedOut}
      <MigrationNotice moved={movedOut} onclose={() => { movedOut = null; }} />
    {/if}
    {#if showDeletedNotes && root}
      <!-- Keyed on the folder: what the panel said about another folder does not carry over. -->
      {#key root}
        <DeletedNotes
          {root}
          {tree}
          onclose={() => { showDeletedNotes = false; }}
          onrestored={refreshTree}
        />
      {/key}
    {/if}
    <!-- No `&& root` guard (unlike the other panels): Settings is usable with no vault open — the Vault section's "Open vault" path needs it. -->
    {#if showSettings}
      <Settings
        {root}
        onOpenVault={() => { showSettings = false; void chooseVault(); }}
        onclose={() => { showSettings = false; }}
      />
    {/if}
  </main>
  </div>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .app {
    display: flex;
    flex: 1;
    min-height: 0;
    font-family: var(--font-ui);
    color: var(--text-normal);
    background: var(--bg-primary);
  }
  .sidebar {
    width: var(--sidebar-width);
    flex-shrink: 0;
    background: var(--bg-secondary);
    overflow: auto;
    padding: var(--sp-2);
  }
  /* Drag handle between the sidebar and the content. Narrow width, generous hit area (margin). */
  .resize-handle {
    flex-shrink: 0;
    width: 1px;
    background: var(--border);
    cursor: col-resize;
    position: relative;
    /* Lift above the content pane. The editor that follows is itself positioned, so with an auto
       z-index it paints later and swallows the half of the hit area that overhangs to the right,
       leaving only the sidebar-side half grabbable. */
    z-index: 1;
  }
  .resize-handle::after {
    /* Invisible wide hit area (±3px). */
    content: "";
    position: absolute;
    inset: 0 -3px;
  }
  .resize-handle:hover {
    background: var(--accent);
  }
  .sidebar-head {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .vault-name {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font: inherit;
    font-size: var(--font-size-small);
    font-weight: var(--font-weight-semibold);
    text-align: left;
    padding: var(--sp-1) var(--sp-2);
    cursor: pointer;
    color: var(--text-normal);
    background: none;
    border: none;
    border-radius: var(--radius-s);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    transition: background var(--transition-fast);
  }
  .vault-name:hover {
    background: var(--bg-hover);
  }
  .vault-name:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .vault-name :global(svg) {
    flex-shrink: 0;
    color: var(--text-muted);
  }
  .vault-label {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-primary);
  }
  /* Editor + backlinks column. The editor pane grows and scrolls; the backlinks panel sits below. */
  .note-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .editor-pane {
    flex: 1;
    min-height: 0;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
  }
  .crumbs {
    display: flex;
    align-items: baseline;
    gap: var(--sp-1);
    min-width: 0;
    overflow: hidden;
  }
  .crumb {
    color: var(--text-muted);
    font-size: var(--font-size-small);
    white-space: nowrap;
  }
  .sep {
    color: var(--text-faint);
    font-size: var(--font-size-small);
  }
  .note-name {
    font: inherit;
    font-weight: var(--font-weight-semibold);
    color: var(--text-normal);
    background: none;
    border: none;
    padding: 2px var(--sp-1);
    margin: 0;
    cursor: text;
    border-radius: var(--radius-s);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background var(--transition-fast);
  }
  .note-name:hover {
    background: var(--bg-hover);
  }
  .title-input {
    font: inherit;
    font-weight: var(--font-weight-semibold);
    color: var(--text-normal);
    background: var(--bg-primary);
    border: 1px solid var(--accent);
    border-radius: var(--radius-s);
    padding: 1px var(--sp-1);
    min-width: 0;
  }
  .title-input:focus {
    outline: none;
  }
  /* Reading/editing toggle. */
  .read-toggle {
    align-self: center;
  }
  .read-toggle[aria-pressed="true"] {
    color: var(--accent);
  }
  /* Chrome on demand: tools stay hidden until the header is hovered/focused,
     so a captured note shows just title + breadcrumb. */
  .title-tools {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    opacity: 0;
    transition: opacity 0.12s ease;
  }
  .title:hover .title-tools,
  .title:focus-within .title-tools {
    opacity: 1;
  }
  .status {
    font-size: var(--font-size-smallest);
    font-weight: var(--font-weight-normal);
    color: var(--text-muted);
  }
  /* Outside the hover-revealed tools on purpose: a missing backup has to stay in view. */
  .backup-status {
    margin-left: auto;
  }
  .backup-status + .title-tools {
    margin-left: 0;
  }
  .status.error {
    color: var(--text-error);
  }
  .publish-banner {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    font-size: var(--font-size-small);
    border-bottom: 1px solid var(--border);
    background: var(--bg-secondary);
  }
  .publish-banner.ok {
    color: var(--accent);
  }
  .publish-banner.error {
    color: var(--text-error);
  }
  .publish-banner.publishing {
    color: var(--text-muted);
  }
  .banner-retry {
    border: 1px solid var(--border);
    background: none;
    color: inherit;
    cursor: pointer;
    font-size: var(--font-size-small);
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-s);
    transition: background var(--transition-fast);
  }
  .banner-retry:hover {
    background: var(--bg-hover);
  }
  .publish-banner span {
    flex: 1;
  }
  .banner-dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    color: inherit;
    cursor: pointer;
    line-height: 1;
    padding: var(--sp-1);
    border-radius: var(--radius-s);
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }
  .banner-dismiss:hover {
    opacity: 1;
  }
  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    background: var(--warning-bg);
    border-bottom: 1px solid var(--warning-border);
    font-size: var(--font-size-small);
    color: var(--warning-text);
  }
  /* Someone tried to leave the note before answering: the question has to be answered first. */
  .banner.attention {
    box-shadow: inset 0 0 0 2px var(--warning-border);
    animation: banner-attention 0.4s ease-in-out 2;
  }
  @keyframes banner-attention {
    50% {
      background: var(--warning-border);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .banner.attention {
      animation: none;
    }
  }
  .banner-actions {
    display: flex;
    gap: var(--sp-2);
    flex-shrink: 0;
  }
  .banner-actions button {
    font: inherit;
    padding: 3px var(--sp-2);
    cursor: pointer;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-s);
    background: var(--bg-primary);
    color: var(--text-normal);
  }
  .banner-actions button:hover {
    background: var(--bg-hover);
  }
  .conflict-banner {
    padding: var(--sp-2) var(--sp-3);
    background: var(--warning-bg);
    border-bottom: 1px solid var(--warning-border);
    font-size: var(--font-size-small);
    color: var(--warning-text);
  }
  .conflict-head {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .conflict-head span {
    flex: 1;
  }
  .conflict-list {
    list-style: none;
    margin: var(--sp-1) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-2);
  }
  .conflict-item {
    font: inherit;
    cursor: pointer;
    padding: 0 var(--sp-2);
    border: 1px solid var(--warning-border);
    border-radius: var(--radius-s);
    background: var(--bg-primary);
    color: var(--text-normal);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .conflict-item:hover {
    background: var(--bg-hover);
  }
  /* Icon buttons (note-header read-toggle / chat). Square, borderless, highlighted only on hover. */
  .icon-btn {
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: var(--font-size-ui);
    cursor: pointer;
    color: var(--text-muted);
    background: none;
    border: none;
    border-radius: var(--radius-s);
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }
  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-normal);
  }
  /* Empty state (no vault open) — centered onboarding in the content. */
  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-3);
    text-align: center;
    padding: var(--sp-8);
  }
  .empty-brand {
    margin: 0;
    font-size: 2.4em;
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.01em;
    color: var(--text-normal);
  }
  .empty-sub {
    margin: 0;
    color: var(--text-muted);
  }
  .open-cta {
    margin-top: var(--sp-2);
    padding: var(--sp-2) var(--sp-5);
    font: inherit;
    font-size: var(--font-size-ui);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    color: var(--text-on-accent);
    background: var(--accent);
    border: none;
    border-radius: var(--radius-m);
    transition: background var(--transition-fast);
  }
  .open-cta:hover {
    background: var(--accent-hover);
  }
  .empty-path {
    margin: 0;
    padding: var(--sp-1) var(--sp-2);
    max-width: 32rem;
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    word-break: break-all;
  }
  .empty-actions {
    display: flex;
    gap: var(--sp-2);
    flex-wrap: wrap;
    justify-content: center;
  }
  .open-cta.secondary {
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
  }
  .open-cta.secondary:hover {
    background: var(--bg-secondary-alt);
  }
  .ai-download-bar {
    position: fixed;
    left: 50%;
    bottom: var(--sp-4);
    transform: translateX(-50%);
    z-index: 50;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    max-width: min(90vw, 40rem);
    padding: var(--sp-2) var(--sp-3);
    font-size: var(--font-size-smaller);
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-m);
  }
  .ai-dl-label {
    white-space: nowrap;
  }
  .ai-dl-track {
    flex: 1;
    min-width: 6rem;
    height: var(--sp-1);
    background: var(--bg-secondary-alt);
    border-radius: var(--radius-s);
    overflow: hidden;
  }
  .ai-dl-fill {
    height: 100%;
    background: var(--accent);
    border-radius: var(--radius-s);
    transition: width var(--transition-normal);
  }
  .ai-dl-detail {
    white-space: nowrap;
  }
  .ai-dl-cancel {
    font: inherit;
    font-size: var(--font-size-smaller);
    cursor: pointer;
    color: var(--text-muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    padding: 2px var(--sp-2);
  }
  .ai-dl-cancel:hover:not(:disabled) {
    background: var(--bg-secondary-alt);
  }
  .ai-dl-cancel:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .toolbar button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    cursor: pointer;
    color: var(--text-muted);
    background: none;
    border: none;
    border-radius: var(--radius-s);
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }
  .toolbar button:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-normal);
  }
  .toolbar button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  /* Divides creation actions (note/folder/child) from edit actions (rename/delete). */
  .toolbar-sep {
    width: 1px;
    align-self: stretch;
    margin: var(--sp-1) var(--sp-1);
    background: var(--border);
  }
  /* Visible keyboard focus for the icon-only chrome controls (quality floor). */
  .toolbar button:focus-visible,
  .icon-btn:focus-visible,
  .banner-dismiss:focus-visible,
  .read-toggle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .name-edit {
    display: flex;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .name-input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--font-size-smaller);
    padding: 2px var(--sp-1);
    color: var(--text-normal);
    background: var(--bg-primary);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-s);
  }
  .name-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .name-edit button {
    font: inherit;
    font-size: var(--font-size-smaller);
    padding: 2px var(--sp-2);
    cursor: pointer;
    color: var(--text-normal);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
  }
  .name-edit button:hover {
    background: var(--bg-hover);
  }
  .op-error {
    color: var(--text-error);
    font-size: var(--font-size-smaller);
    margin: 0 0 var(--sp-2);
  }
  .hint {
    color: var(--text-muted);
    padding: var(--sp-3);
  }
  /* Fill the remaining height so the empty area below the tree is also a drop target (move to root). */
  .tree-root {
    min-height: 80px;
  }
  .tree-root:focus {
    outline: none;
  }
</style>
