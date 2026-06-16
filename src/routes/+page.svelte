<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import {
    openVault,
    listTree,
    readNote,
    writeNote,
    createNote,
    createFolder,
    renameNode,
    deleteNode,
    moveNode,
    promoteNode,
    adoptNode,
    saveAttachment,
    searchContent,
    rebuildIndex,
    type TreeNode,
    type SearchHit,
  } from "$lib/ipc";
  import { startSync } from "$lib/sync";
  import { buildWikiResolver } from "$lib/wikilink.helpers";
  import { backlinks } from "$lib/backlinkStore.svelte";
  import { theme } from "$lib/theme.svelte";
  import { layout } from "$lib/layout.svelte";
  import TreeView, { DRAG_MIME } from "$lib/TreeView.svelte";
  import Editor from "$lib/Editor.svelte";
  import Backlinks from "$lib/Backlinks.svelte";
  import PageHeader from "$lib/PageHeader.svelte";
  import { parseFrontmatter, getField } from "$lib/frontmatter.helpers";
  import { palette } from "$lib/paletteStore.svelte";
  import Palette from "$lib/Palette.svelte";
  import { buildCommands, activeCommands, type PaletteActions } from "$lib/commands";
  import { mergeOrder, nav } from "$lib/nav.svelte";
  import { moveInArray } from "$lib/nav.helpers";

  let root = $state<string | null>(null);
  let tree = $state<TreeNode[]>([]);
  let content = $state("");
  // Live document text — mirrors `content` on note load, then tracks edits so the page header
  // reflects frontmatter changes immediately (the editor owns its own doc; `content` is load-only).
  let liveDoc = $state("");
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
  let dirty = $state(false);
  let saveError = $state<string | null>(null);

  // External change (M3) state.
  let reloadVersion = $state(0); // bump on external reload → force Editor re-creation
  let removed = $state(false); // open note was externally deleted/moved
  let conflictDisk = $state<string | null>(null); // disk version on conflict (for banner)

  // Structure editing (M4) state.
  let selectedNode = $state<TreeNode | null>(null);
  let mode = $state<"none" | "new-note" | "new-folder" | "rename">("none");
  let nameInput = $state("");
  let opError = $state<string | null>(null);
  // Explicit create-target override (e.g. new container after leaf promote). When set, used instead of selection-based inference.
  let createParent = $state<string | null>(null);

  // Debounced autosave state (no reactivity needed — holds timer/latest draft).
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let pending: { path: string; text: string } | null = null;
  const DEBOUNCE_MS = 500;

  /**
   * Write the pending save to disk immediately. Called before switching notes or changing vault.
   * Does not throw on failure; surfaces it via `saveError` (so it does not block the caller's switch).
   * On failure, preserves `pending` so the next flush can retry.
   */
  async function flush() {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    if (!pending || !root) return;
    const job = pending;
    try {
      await writeNote(root, job.path, job.text);
      backlinks.updateSource(job.path, job.text); // refresh links from the just-saved note (cheap, no re-read)
      // Switch to clean state only if no newer edit accumulated during the save.
      if (pending === job) {
        pending = null;
        dirty = false;
      }
      saveError = null;
    } catch (e) {
      // Keep pending → retryable. Surface to the user.
      saveError = String(e);
    }
  }

  function scheduleSave(path: string, text: string) {
    pending = { path, text };
    dirty = true;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void flush();
    }, DEBOUNCE_MS);
  }

  function handleEdit(text: string) {
    liveDoc = text; // keep the page header in sync with in-editor frontmatter edits
    if (activePath) scheduleSave(activePath, text);
  }

  /** Open a vault path and load the tree (separate from the dialog — reused by the test bridge). */
  async function loadVault(path: string) {
    await flush(); // preserve unsaved edits before switching vault
    root = path;
    tree = await openVault(path);
    // When the vault changes, the previous vault's selection, open note, and edit-mode context are invalid.
    // If not cleared, a stale selectedNode would wrongly target the previous vault's path as the
    // parent for creation/move, sending operations to the wrong location or failing.
    closeEditor();
    selectedNode = null;
    cancelMode();
    backlinks.clear(); // drop the previous vault's index (the $effect below rebuilds it)
    await nav.load(path); // load favorites/order sidecar
  }

  async function chooseVault() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await loadVault(selected);
  }

  // ── Inline title editing (D5) ──────────────────────────────────────
  let titleEditing = $state(false);
  let titleInput = $state("");
  // On Escape cancel, suppress once the commit from the blur that fires as the input disappears.
  let suppressTitleBlur = false;

  function startTitleEdit() {
    if (!activePath) return;
    titleInput = activeName;
    titleEditing = true;
  }

  /** Focus + select-all on mount (inline title input). */
  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
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
    await flush(); // preserve unsaved edits before rename
    if (pending) {
      saveError = "Rename canceled — could not save your unsaved edits.";
      return;
    }
    try {
      const newNodePath = await renameNode(root, node.path, name);
      await refreshTree();
      // Follow the new body path: leaf=new node path, container note=newfolder/newname.md.
      activePath =
        node.kind === "leaf"
          ? newNodePath
          : joinPath(newNodePath, `${baseName(newNodePath)}.md`);
      activeName = name;
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = String(e);
    }
  }

  /** Keyboard F2: select the node and enter rename mode. */
  function handleRename(node: TreeNode) {
    selectedNode = node;
    startMode("rename");
  }

  /** Keyboard Delete: select the node and delete it (trash). */
  function handleDelete(node: TreeNode) {
    selectedNode = node;
    void deleteSelected();
  }

  async function handleSelect(node: TreeNode) {
    selectedNode = node; // structure-edit target (including folders)
    if (!root || !node.body_path) return; // do not open containers without a body
    pendingHeading = null; // a direct open does not scroll to a heading (cleared before the open)
    await flush(); // preserve unsaved edits of the previous note
    content = await readNote(root, node.body_path);
    activeName = node.name;
    activePath = node.body_path;
    dirty = false;
    removed = false;
    conflictDisk = null;
  }

  // ── Structure editing (M4) ──────────────────────────────────────────────
  /** Normalized path comparison: is child equal to or below ancestor.
   *  Windows is case-insensitive, so normalize to lowercase (Windows-first). */
  function pathInside(child: string, ancestor: string): boolean {
    const n = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    const c = n(child);
    const a = n(ancestor);
    return c === a || c.startsWith(a + "/");
  }

  /** Parent directory of a path (separator preserved). */
  function parentDir(p: string): string {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(0, i) : p;
  }

  /** Last component of a path (file/folder name). */
  function baseName(p: string): string {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  /** Join child onto dir (preserving dir's existing separator style). */
  function joinPath(dir: string, child: string): string {
    const sep = dir.includes("\\") ? "\\" : "/";
    return `${dir}${sep}${child}`;
  }

  /** Normalized equality comparison (absorbs separator, trailing slash, case; same policy as pathInside). */
  function samePath(a: string, b: string): boolean {
    const n = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    return n(a) === n(b);
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
    dirty = false;
    pending = null;
    // Since we discard the edit context (including unsaved pending), the previous save error is no
    // longer valid either. If not cleared, a stale error persists after vault switch/note close.
    saveError = null;
    removed = false;
    conflictDisk = null;
  }

  function startMode(
    m: "new-note" | "new-folder" | "rename",
    parentOverride: string | null = null,
  ) {
    if (m === "rename" && !selectedNode) return;
    createParent = parentOverride;
    mode = m;
    nameInput = m === "rename" && selectedNode ? selectedNode.name : "";
    opError = null;
  }

  /**
   * Add a new note under the selected leaf note (design §3.3 auto-promote).
   * Promotes leaf `foo.md` to container `foo/foo.md`, then enters new-note mode with the
   * new container as the create target. If the promoted leaf was open, follow the new body path.
   */
  async function startAddChild() {
    if (!root || !selectedNode || selectedNode.kind !== "leaf") return;
    const leaf = selectedNode.path;
    await flush(); // preserve current edits before promote
    if (pending) {
      opError = "Operation canceled — could not save your unsaved edits.";
      return;
    }
    const wasActive = activePath !== null && samePath(activePath, leaf);
    try {
      const newDir = await promoteNode(root, leaf);
      await refreshTree();
      // If the promoted leaf was the open note, its body moved to newDir/<stem>.md → follow it.
      if (wasActive) activePath = joinPath(newDir, `${baseName(newDir)}.md`);
      selectedNode = null;
      startMode("new-note", newDir); // target the new container
    } catch (e) {
      opError = String(e);
    }
  }

  function cancelMode() {
    mode = "none";
    nameInput = "";
    createParent = null;
    opError = null;
  }

  async function confirmMode() {
    if (!root) return;
    const name = nameInput.trim();
    if (!name) return;
    await flush(); // preserve current edits before structure change
    if (pending) {
      opError = "Operation canceled — could not save your unsaved edits.";
      return;
    }
    try {
      if (mode === "new-note") {
        const p = await createNote(root, targetParent(), name);
        await refreshTree();
        // open the new note immediately
        content = await readNote(root, p);
        activeName = name;
        activePath = p;
        dirty = false;
        removed = false;
      } else if (mode === "new-folder") {
        await createFolder(root, targetParent(), name);
        await refreshTree();
      } else if (mode === "rename" && selectedNode) {
        const target = selectedNode.path;
        const affectsOpen = activePath !== null && pathInside(activePath, target);
        await renameNode(root, target, name);
        await refreshTree();
        // If the open note is inside the renamed node, close it and prompt re-selection (path changed).
        if (affectsOpen) closeEditor();
        selectedNode = null;
      }
      mode = "none";
      nameInput = "";
      createParent = null;
    } catch (e) {
      opError = String(e); // keep mode/createParent → retryable with the same target
    }
  }

  async function deleteSelected() {
    if (!root || !selectedNode) return;
    const target = selectedNode.path;
    const affectsOpen = activePath !== null && pathInside(activePath, target);
    await flush();
    if (pending) {
      opError = "Delete canceled — could not save your unsaved edits.";
      return;
    }
    try {
      await deleteNode(root, target);
      await refreshTree();
      if (affectsOpen) closeEditor();
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = String(e);
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
      opError = "Cannot move a node into its own subfolder.";
      return;
    }
    await flush(); // preserve current edits before move
    if (pending) {
      opError = "Move canceled — could not save your unsaved edits.";
      return;
    }
    const affectsOpen = activePath !== null && pathInside(activePath, src);
    try {
      const newPath = await moveNode(root, src, destDir);
      await refreshTree();
      // If the open note is inside the moved subtree, follow the new path (content same, path only changed).
      if (affectsOpen && activePath) activePath = newPath + activePath.slice(src.length);
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = String(e);
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
      opError = "Cannot move a node into its own descendant.";
      return;
    }
    await flush();
    if (pending) {
      opError = "Operation canceled — could not save your unsaved edits.";
      return;
    }
    const wasActiveLeaf = activePath !== null && samePath(activePath, leaf);
    const wasActiveSrc = activePath !== null && pathInside(activePath, src);
    try {
      const movedPath = await adoptNode(root, src, leaf);
      await refreshTree();
      // The promoted new container = the parent of the moved node.
      const newDir = parentDir(movedPath);
      if (wasActiveLeaf) {
        // The promoted leaf body moved to newDir/<stem>.md.
        activePath = joinPath(newDir, `${baseName(newDir)}.md`);
      } else if (wasActiveSrc && activePath) {
        activePath = movedPath + activePath.slice(src.length);
      }
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = String(e);
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
      saveError = null;
      return `![](${rel})`;
    } catch (e) {
      saveError = String(e);
      return null;
    }
  }

  // ── External change reconciliation (sync.ts callbacks) ───────────────────────────────
  function applyReload(diskContent: string) {
    content = diskContent;
    reloadVersion += 1; // trigger Editor re-creation
    dirty = false;
    pending = null;
    saveError = null;
    removed = false; // return to normal state if re-created after external deletion
  }

  /** Conflict banner: overwrite with the disk version, discarding my edits. */
  function resolveTakeDisk() {
    if (conflictDisk !== null) applyReload(conflictDisk);
    conflictDisk = null;
  }

  /** Conflict banner: keep my edits (overwrites disk on the next save). */
  function resolveKeepMine() {
    conflictDisk = null;
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

  // ── Sidebar resize (D2) ─────────────────────────────────────
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

  /** Collect every note's vault-relative `.md` path: leaf=own path, container=its folder note. */
  function collectNotePaths(nodes: TreeNode[], acc: string[] = []): string[] {
    for (const n of nodes) {
      if (n.kind === "leaf") acc.push(n.path);
      else if (n.body_path) acc.push(n.body_path);
      if (n.children?.length) collectNotePaths(n.children, acc);
    }
    return acc;
  }

  /** Wikilink resolution targets — passed to the editor so `[[links]]` resolve against the live tree. */
  let notePaths = $derived<string[]>(collectNotePaths(tree));

  /** Find the tree node whose body is the given vault-relative `.md` path (leaf path or folder note). */
  function findNodeByNotePath(nodes: TreeNode[], notePath: string): TreeNode | null {
    for (const n of nodes) {
      if (n.body_path === notePath || (n.kind === "leaf" && n.path === notePath)) return n;
      const found = n.children?.length ? findNodeByNotePath(n.children, notePath) : null;
      if (found) return found;
    }
    return null;
  }

  /**
   * Open the note a wikilink resolves to. Reuses `handleSelect` (flush, read, selection follow) so
   * navigation behaves exactly like clicking the note in the tree. Heading scroll is a follow-up.
   */
  async function handleWikiLink(path: string, heading: string | undefined) {
    const node = findNodeByNotePath(tree, path);
    if (!node) return;
    await handleSelect(node); // clears pendingHeading, then opens (recreates the editor)
    // Set after handleSelect so the editor recreation picks it up and scrolls once on load.
    pendingHeading = heading ?? null;
  }

  /**
   * Rebuild the vault-wide backlink index by reading every note's body once. Frontend scan (no new
   * IPC); the simplicity beats a Rust link graph at this scale (constitution: simplicity > perf).
   * A failed read degrades that note to empty rather than aborting the whole index.
   */
  async function rebuildBacklinks() {
    const r = root;
    if (!r) return;
    const paths = notePaths;
    const resolve = buildWikiResolver(paths).resolve;
    const notes = await Promise.all(
      paths.map(async (p) => ({ path: p, body: await readNote(r, p).catch(() => "") })),
    );
    backlinks.rebuild(notes, resolve);
  }

  // Rebuild the backlink index whenever the note set changes (vault load, create/rename/move/delete).
  // Edits to the open note are handled incrementally on save (see flush); this covers structure.
  $effect(() => {
    void notePaths;
    if (root) void rebuildBacklinks();
  });

  const actions: PaletteActions = {
    openVault: () => { void chooseVault(); },
    toggleTheme: () => { theme.toggle(); },
    toggleSidebar: () => { layout.toggleCollapsed(); },
    toggleReading: () => { reading = !reading; },
    // Create at root: parentOverride=root targets the root regardless of selectedNode state.
    newNoteAtRoot: () => { if (root) startMode("new-note", root); },
    newFolderAtRoot: () => { if (root) startMode("new-folder", root); },
    hasSelection: () => selectedNode !== null,
    renameSelected: () => { startMode("rename"); },
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
  };

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

  /** Palette file selection → find the matching TreeNode and delegate to handleSelect. */
  function openFileFromPalette(path: string): void {
    const node = findNode(tree, path);
    if (node) {
      void handleSelect(node);
      nav.pushRecent(path);
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
    }
  }

  // On window close, flush unsaved edits to disk before actually closing (guarantees "always saved").
  onMount(() => {
    // E2E test bridge (dev build only — tree-shaken out of the production bundle).
    // Bypasses the native folder dialog so Playwright can open a vault directly.
    if (import.meta.env.DEV) {
      (window as unknown as { __textreeTest?: unknown }).__textreeTest = {
        loadVault,
      };
    }

    const win = getCurrentWindow();
    const unlistenClose = win.onCloseRequested(async (event) => {
      if (!pending) return; // nothing to save → proceed with default close
      event.preventDefault();
      await flush();
      // If flush fails, pending remains (saveError shown). In that case do not close
      // and keep the window open to prevent data loss. Only close on success.
      if (!pending) await win.destroy();
    });

    // Subscribe to external file changes.
    const unlistenSyncP = startSync({
      root: () => root,
      activePath: () => activePath,
      isDirty: () => dirty,
      setTree: (t) => {
        tree = t;
      },
      reloadActive: applyReload,
      activeRemoved: () => {
        removed = true;
        pending = null; // prevent save attempts to a vanished file
        dirty = false;
        conflictDisk = null; // avoid showing alongside the conflict banner (deletion takes priority)
      },
      conflict: (disk) => {
        conflictDisk = disk;
      },
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
        >📁 {vaultName(root)}</button>
      {:else}
        <span class="brand">Textree</span>
      {/if}
      <button
        class="icon-btn"
        onclick={() => theme.toggle()}
        title={theme.resolved === "dark" ? "Switch to light theme" : "Switch to dark theme"}
        aria-label="Toggle theme"
      >{theme.resolved === "dark" ? "☀" : "☾"}</button>
      <button
        class="icon-btn"
        onclick={() => layout.toggleCollapsed()}
        title="Collapse sidebar"
        aria-label="Collapse sidebar"
      >⟨</button>
    </div>
    {#if root}
      <div class="toolbar">
        <button onclick={() => startMode("new-note")} title="New note">＋Note</button>
        <button onclick={() => startMode("new-folder")} title="New folder">＋Folder</button>
        <button
          onclick={startAddChild}
          disabled={selectedNode?.kind !== "leaf"}
          title="Promote the selected note to a folder and add a child note inside it"
        >＋Child</button>
        <button onclick={() => startMode("rename")} disabled={!selectedNode}>Rename</button>
        <button onclick={deleteSelected} disabled={!selectedNode}>Delete</button>
      </div>
      {#if mode !== "none"}
        <div class="name-edit">
          <input
            class="name-input"
            placeholder={mode === "new-folder" ||
            (mode === "rename" && selectedNode?.kind === "container")
              ? "Folder name"
              : "Note name"}
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
        <p class="op-error">⚠ {opError}</p>
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
          onrename={handleRename}
          ondelete={handleDelete}
          onfavorite={(node) => nav.toggleFavorite(node.path)}
          selectedPath={selectedNode?.path ?? null}
        />
      </div>
    {:else}
      <p class="hint">No vault is open.</p>
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
    {#if !root}
      <div class="empty-state">
        <h1 class="empty-brand">Textree</h1>
        <p class="empty-sub">Open a local Markdown vault to get started.</p>
        <button class="open-cta" onclick={chooseVault}>Open vault</button>
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
        {#if saveError}
          <span class="status error">⚠ Save failed: {saveError}</span>
        {:else if removed}
          <span class="status error">⚠ Moved/deleted externally</span>
        {:else}
          <span class="status">{dirty ? "● Saving…" : "Saved"}</span>
        {/if}
        <button
          class="icon-btn read-toggle"
          onclick={() => (reading = !reading)}
          title={reading ? "Switch to editing" : "Switch to reading view"}
          aria-label={reading ? "Switch to editing" : "Switch to reading view"}
          aria-pressed={reading}
        >{reading ? "✏" : "📖"}</button>
      </header>
      {#if conflictDisk !== null}
        <div class="banner" role="alert">
          <span>This note changed on disk while you have unsaved edits.</span>
          <span class="banner-actions">
            <button onclick={resolveTakeDisk}>Load disk version</button>
            <button onclick={resolveKeepMine}>Keep my edits</button>
          </span>
        </div>
      {/if}
      {#if removed}
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
            />
          </div>
          <Backlinks links={backlinks.for(activePath)} onOpen={(p) => handleWikiLink(p, undefined)} />
        </div>
      {/if}
    {:else}
      <p class="hint">Select a note.</p>
    {/if}
  </main>
  {#if layout.collapsed}
    <button
      class="expand-btn"
      onclick={() => layout.toggleCollapsed()}
      title="Expand sidebar"
      aria-label="Expand sidebar"
    >⟩</button>
  {/if}
</div>

<style>
  .app {
    display: flex;
    height: 100vh;
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
  /* Reading/editing toggle — pushed to the right edge of the title bar. */
  .read-toggle {
    margin-left: auto;
    align-self: center;
  }
  .read-toggle[aria-pressed="true"] {
    color: var(--accent);
  }
  .status {
    font-size: var(--font-size-smallest);
    font-weight: var(--font-weight-normal);
    color: var(--text-muted);
  }
  .status.error {
    color: var(--text-error);
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
  /* Vault name (compact header) — click to switch vault. Full path is the title tooltip. */
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
  .brand {
    flex: 1;
    font-size: var(--font-size-small);
    font-weight: var(--font-weight-semibold);
    color: var(--text-muted);
    padding: var(--sp-1) var(--sp-2);
  }
  /* Header icon buttons (theme/collapse). Square, borderless, highlighted only on hover. */
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
  /* Floating expand button shown while collapsed (top-left). */
  .expand-btn {
    position: fixed;
    top: var(--sp-2);
    left: var(--sp-2);
    z-index: 10;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: var(--font-size-ui);
    cursor: pointer;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    box-shadow: var(--shadow-s);
  }
  .expand-btn:hover {
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
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .toolbar button {
    font: inherit;
    font-size: var(--font-size-smaller);
    padding: 2px var(--sp-2);
    cursor: pointer;
    color: var(--text-normal);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    transition: background var(--transition-fast);
  }
  .toolbar button:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .toolbar button:disabled {
    opacity: 0.4;
    cursor: default;
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
