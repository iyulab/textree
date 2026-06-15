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
  import { theme } from "$lib/theme.svelte";
  import { layout } from "$lib/layout.svelte";
  import TreeView, { DRAG_MIME } from "$lib/TreeView.svelte";
  import Editor from "$lib/Editor.svelte";
  import { palette } from "$lib/paletteStore.svelte";
  import Palette from "$lib/Palette.svelte";
  import { buildCommands, activeCommands, type PaletteActions } from "$lib/commands";
  import { mergeOrder, nav } from "$lib/nav.svelte";
  import { moveInArray } from "$lib/nav.helpers";

  let root = $state<string | null>(null);
  let tree = $state<TreeNode[]>([]);
  let content = $state("");
  let activeName = $state("");
  let activePath = $state<string | null>(null);
  let dirty = $state(false);
  let saveError = $state<string | null>(null);

  // 외부 변경(M3) 상태.
  let reloadVersion = $state(0); // 외부 재로드 시 bump → Editor 강제 재생성
  let removed = $state(false); // 열린 노트가 외부 삭제/이동됨
  let conflictDisk = $state<string | null>(null); // 충돌 시 디스크 버전(배너용)

  // 구조 편집(M4) 상태.
  let selectedNode = $state<TreeNode | null>(null);
  let mode = $state<"none" | "new-note" | "new-folder" | "rename">("none");
  let nameInput = $state("");
  let opError = $state<string | null>(null);
  // 생성 타깃 명시 오버라이드(예: 리프 승격 후 새 컨테이너). 설정 시 선택 기반 추론 대신 사용.
  let createParent = $state<string | null>(null);

  // 디바운스 자동저장 상태(반응형 불필요 — 타이머/최신 드래프트 보관용).
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let pending: { path: string; text: string } | null = null;
  const DEBOUNCE_MS = 500;

  /**
   * 대기 중인 저장을 즉시 디스크에 기록. 노트 전환·볼트 변경 전에 호출.
   * 실패해도 throw하지 않고 `saveError`로 표면화한다(호출자의 전환을 막지 않음).
   * 실패 시 `pending`을 보존해 다음 flush에서 재시도할 수 있게 한다.
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
      // 저장 중 더 새 편집이 안 쌓였을 때만 깨끗 상태로 전환.
      if (pending === job) {
        pending = null;
        dirty = false;
      }
      saveError = null;
    } catch (e) {
      // pending 유지 → 재시도 가능. 사용자에게 표면화.
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
    if (activePath) scheduleSave(activePath, text);
  }

  /** 볼트 경로를 열어 트리를 로드(다이얼로그와 분리 — 테스트 브리지가 재사용). */
  async function loadVault(path: string) {
    await flush(); // 볼트 전환 전 미저장 편집 보존
    root = path;
    tree = await openVault(path);
    // 볼트가 바뀌면 이전 볼트의 선택·열린 노트·편집 모드 컨텍스트는 무효다.
    // 정리하지 않으면 stale한 selectedNode가 생성/이동의 타깃 부모를 이전 볼트
    // 경로로 잘못 잡아 작업이 엉뚱한 위치를 향하거나 실패한다.
    closeEditor();
    selectedNode = null;
    cancelMode();
    await nav.load(path); // 즐겨찾기·정렬 사이드카 로드
  }

  async function chooseVault() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await loadVault(selected);
  }

  // ── 인라인 제목 편집(D5) ──────────────────────────────────────
  let titleEditing = $state(false);
  let titleInput = $state("");
  // Escape 취소 시, 입력창이 사라지며 발생하는 blur 의 commit 을 1회 억제.
  let suppressTitleBlur = false;

  function startTitleEdit() {
    if (!activePath) return;
    titleInput = activeName;
    titleEditing = true;
  }

  /** 마운트 시 포커스+전체선택(인라인 제목 입력창). */
  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function cancelTitleEdit() {
    suppressTitleBlur = true; // 뒤따르는 blur 가 commit 하지 않도록
    titleEditing = false;
  }

  /** 제목 편집 확정 → 활성 노트를 rename 하고 새 경로를 추종. */
  async function commitTitle() {
    // Escape 취소 직후의 blur 는 무시.
    if (suppressTitleBlur) {
      suppressTitleBlur = false;
      return;
    }
    // 재진입 가드: Enter 가 먼저 처리하면 titleEditing=false → 뒤따르는 blur 는 무시.
    if (!titleEditing) return;
    const name = titleInput.trim();
    titleEditing = false;
    if (!root || !activePath || !name || name === activeName) return;
    const node = findByBody(tree, activePath);
    if (!node) return;
    await flush(); // rename 전 미저장 편집 보존
    if (pending) {
      saveError = "미저장 편집을 저장하지 못해 이름변경을 취소했습니다.";
      return;
    }
    try {
      const newNodePath = await renameNode(root, node.path, name);
      await refreshTree();
      // 새 본문 경로 추종: 리프=새 노드경로, 컨테이너노트=새폴더/새이름.md.
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

  /** 키보드 F2: 노드를 선택하고 이름변경 모드 진입. */
  function handleRename(node: TreeNode) {
    selectedNode = node;
    startMode("rename");
  }

  /** 키보드 Delete: 노드를 선택하고 삭제(휴지통). */
  function handleDelete(node: TreeNode) {
    selectedNode = node;
    void deleteSelected();
  }

  async function handleSelect(node: TreeNode) {
    selectedNode = node; // 구조 편집 대상(폴더 포함)
    if (!root || !node.body_path) return; // body 없는 컨테이너는 열지 않음
    await flush(); // 이전 노트의 미저장 편집 보존
    content = await readNote(root, node.body_path);
    activeName = node.name;
    activePath = node.body_path;
    dirty = false;
    removed = false;
    conflictDisk = null;
  }

  // ── 구조 편집(M4) ──────────────────────────────────────────────
  /** 경로 정규화 비교: child가 ancestor와 같거나 그 하위인가.
   *  Windows는 대소문자 비구분이므로 소문자로 정규화(Windows-first). */
  function pathInside(child: string, ancestor: string): boolean {
    const n = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    const c = n(child);
    const a = n(ancestor);
    return c === a || c.startsWith(a + "/");
  }

  /** 경로의 부모 디렉터리(구분자 보존). */
  function parentDir(p: string): string {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(0, i) : p;
  }

  /** 경로의 마지막 구성요소(파일/폴더명). */
  function baseName(p: string): string {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  /** dir에 child를 잇는다(dir의 기존 구분자 스타일 보존). */
  function joinPath(dir: string, child: string): string {
    const sep = dir.includes("\\") ? "\\" : "/";
    return `${dir}${sep}${child}`;
  }

  /** 정규화 동등 비교(구분자·후행 슬래시·대소문자 흡수; pathInside와 동일 정책). */
  function samePath(a: string, b: string): boolean {
    const n = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    return n(a) === n(b);
  }

  async function refreshTree() {
    if (root) tree = await listTree(root);
  }

  /** body_path로 트리에서 노드를 역탐색(인라인 제목 편집의 rename 타깃 확인용). */
  function findByBody(nodes: TreeNode[], body: string): TreeNode | null {
    for (const n of nodes) {
      if (n.body_path && samePath(n.body_path, body)) return n;
      const c = findByBody(n.children, body);
      if (c) return c;
    }
    return null;
  }

  /** 새 항목이 들어갈 부모 디렉터리. 명시 오버라이드(createParent) 우선, 없으면
   *  컨테이너는 그 안, 리프는 형제(부모), 선택 없으면 루트. */
  function targetParent(): string {
    if (createParent) return createParent;
    if (!selectedNode) return root as string;
    if (selectedNode.kind === "container") return selectedNode.path;
    return parentDir(selectedNode.path); // 리프 선택 → 형제로 생성
  }

  function closeEditor() {
    activePath = null;
    activeName = "";
    content = "";
    dirty = false;
    pending = null;
    // 편집 컨텍스트(미저장 pending 포함)를 버리므로 직전 저장 오류도 더는 유효하지
    // 않다. 정리하지 않으면 볼트 전환/노트 닫기 후에도 stale 오류가 잔존한다.
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
   * 선택된 리프 노트 하위에 새 노트를 추가(설계 §3.3 자동 승격).
   * 리프 `foo.md`를 컨테이너 `foo/foo.md`로 승격한 뒤, 새 컨테이너를 생성 타깃으로
   * new-note 모드 진입. 승격된 리프가 열려 있었다면 새 본문 경로로 추종.
   */
  async function startAddChild() {
    if (!root || !selectedNode || selectedNode.kind !== "leaf") return;
    const leaf = selectedNode.path;
    await flush(); // 승격 전 현재 편집 보존
    if (pending) {
      opError = "미저장 편집을 저장하지 못해 작업을 취소했습니다.";
      return;
    }
    const wasActive = activePath !== null && samePath(activePath, leaf);
    try {
      const newDir = await promoteNode(root, leaf);
      await refreshTree();
      // 승격된 리프가 열린 노트였으면 본문이 newDir/<stem>.md로 이동됨 → 추종.
      if (wasActive) activePath = joinPath(newDir, `${baseName(newDir)}.md`);
      selectedNode = null;
      startMode("new-note", newDir); // 새 컨테이너를 타깃으로
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
    await flush(); // 구조 변경 전 현재 편집 보존
    if (pending) {
      opError = "미저장 편집을 저장하지 못해 작업을 취소했습니다.";
      return;
    }
    try {
      if (mode === "new-note") {
        const p = await createNote(root, targetParent(), name);
        await refreshTree();
        // 새 노트 바로 열기
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
        // 열린 노트가 이름변경된 노드 안에 있으면 닫고 재선택 유도(경로 변동).
        if (affectsOpen) closeEditor();
        selectedNode = null;
      }
      mode = "none";
      nameInput = "";
      createParent = null;
    } catch (e) {
      opError = String(e); // mode/createParent 유지 → 같은 타깃으로 재시도 가능
    }
  }

  async function deleteSelected() {
    if (!root || !selectedNode) return;
    const target = selectedNode.path;
    const affectsOpen = activePath !== null && pathInside(activePath, target);
    await flush();
    if (pending) {
      opError = "미저장 편집을 저장하지 못해 삭제를 취소했습니다.";
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
   * 노드를 다른 폴더(destDir)로 이동(드래그앤드롭). 무의미/불가 케이스는 조용히
   * 무시하거나 안내하고, 그 외엔 `move_node` 위임 후 트리 갱신.
   */
  async function handleMove(src: string, destDir: string) {
    if (!root) return;
    if (samePath(src, destDir)) return; // 자기 자신 위에 드롭 — no-op
    if (samePath(parentDir(src), destDir)) return; // 이미 그 폴더에 있음 — no-op
    if (pathInside(destDir, src)) {
      opError = "자기 하위 폴더로는 이동할 수 없습니다.";
      return;
    }
    await flush(); // 이동 전 현재 편집 보존
    if (pending) {
      opError = "미저장 편집을 저장하지 못해 이동을 취소했습니다.";
      return;
    }
    const affectsOpen = activePath !== null && pathInside(activePath, src);
    try {
      const newPath = await moveNode(root, src, destDir);
      await refreshTree();
      // 열린 노트가 이동된 서브트리 안이면 새 경로로 추종(내용 동일, 경로만 변경).
      if (affectsOpen && activePath) activePath = newPath + activePath.slice(src.length);
      selectedNode = null;
      opError = null;
    } catch (e) {
      opError = String(e);
    }
  }

  /**
   * 리프 노트 위에 드롭(adopt): leaf를 컨테이너로 승격하고 src를 그 자식으로 이동.
   * 백엔드 `adopt_node`가 원자적으로 처리(실패 시 승격 롤백). 열린 노트가 승격된
   * 리프 또는 이동된 src 안이면 새 경로로 추종.
   */
  async function handleAdopt(src: string, leaf: string) {
    if (!root) return;
    if (samePath(src, leaf)) return; // 자기 자신 위에 드롭 — no-op
    if (pathInside(leaf, src)) {
      opError = "자기 하위로는 이동할 수 없습니다.";
      return;
    }
    await flush();
    if (pending) {
      opError = "미저장 편집을 저장하지 못해 작업을 취소했습니다.";
      return;
    }
    const wasActiveLeaf = activePath !== null && samePath(activePath, leaf);
    const wasActiveSrc = activePath !== null && pathInside(activePath, src);
    try {
      const movedPath = await adoptNode(root, src, leaf);
      await refreshTree();
      // 승격된 새 컨테이너 = 이동된 노드의 부모.
      const newDir = parentDir(movedPath);
      if (wasActiveLeaf) {
        // 승격된 리프 본문은 newDir/<stem>.md로 이동됨.
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
   * 이미지 붙여넣기: 첨부를 현재 노트 옆 assets/에 저장하고 삽입할 마크다운 링크 반환.
   * 저장 실패는 saveError로 표면화하고 null 반환(삽입 안 함).
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

  // ── 외부 변경 조정(sync.ts 콜백) ───────────────────────────────
  function applyReload(diskContent: string) {
    content = diskContent;
    reloadVersion += 1; // Editor 재생성 트리거
    dirty = false;
    pending = null;
    saveError = null;
    removed = false; // 외부 삭제 후 재생성된 경우 정상 상태로 복귀
  }

  /** 충돌 배너: 디스크 버전으로 덮어 내 편집 폐기. */
  function resolveTakeDisk() {
    if (conflictDisk !== null) applyReload(conflictDisk);
    conflictDisk = null;
  }

  /** 충돌 배너: 내 편집 유지(다음 저장 때 디스크를 덮어씀). */
  function resolveKeepMine() {
    conflictDisk = null;
  }

  /** 볼트 루트 경로의 마지막 폴더명(사이드바 헤더 컴팩트 표시용). 전체 경로는 title. */
  function vaultName(p: string): string {
    return baseName(p.replace(/[/\\]+$/, "")) || p;
  }

  /** 열린 노트의 조상 폴더들(브레드크럼). 파일명·컨테이너 노트의 중복 폴더는 제외. */
  function breadcrumb(): string[] {
    if (!root || !activePath) return [];
    const rel = activePath.slice(root.length).replace(/^[/\\]+/, "");
    const parts = rel.split(/[/\\]/).filter(Boolean);
    const file = parts.pop() ?? "";
    const stem = file.replace(/\.md$/i, "");
    // 컨테이너 노트(folder/folder.md): 마지막 폴더가 파일 stem과 같으면 중복 → 제거.
    if (parts.length && parts[parts.length - 1] === stem) parts.pop();
    return parts;
  }

  // ── 사이드바 리사이즈(D2) ─────────────────────────────────────
  // 드래그 핸들에서 pointer 캡처로 폭을 조정. 종료 시 1회 영속(드래그 중 localStorage
  // 폭주 방지). pointermove/up은 window가 아니라 setPointerCapture로 핸들에 고정.
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

  // ── 통합 팔레트(P1a Task 11) ───────────────────────────────────
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

  const actions: PaletteActions = {
    openVault: () => { void chooseVault(); },
    toggleTheme: () => { theme.toggle(); },
    toggleSidebar: () => { layout.toggleCollapsed(); },
    // 루트 생성: parentOverride=root 로 selectedNode 상태와 무관하게 루트를 타깃으로.
    newNoteAtRoot: () => { if (root) startMode("new-note", root); },
    newFolderAtRoot: () => { if (root) startMode("new-folder", root); },
    hasSelection: () => selectedNode !== null,
    renameSelected: () => { startMode("rename"); },
    deleteSelected: () => { void deleteSelected(); },
    // 승격은 리프 전용 — 선택 노드가 리프일 때만 의미 있음(startAddChild 내부에서도 확인).
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

  /** TreeNode.path 기준 트리 탐색(없으면 null). */
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

  /** 볼트 루트 + POSIX 상대경로 → TreeNode.path 형식(절대, OS 구분자). */
  function joinVaultPath(rootDir: string, rel: string): string {
    const sep = rootDir.includes("\\") ? "\\" : "/";
    return rootDir + sep + rel.split("/").join(sep);
  }

  /** 팔레트 본문검색 → 상대경로 히트를 절대경로로 변환(기존 onOpenFile 호환). */
  async function searchContentFromPalette(query: string): Promise<SearchHit[]> {
    if (!root) return [];
    const r = root;
    const hits = await searchContent(query);
    return hits.map((h) => ({ ...h, path: joinVaultPath(r, h.path) }));
  }

  /** 팔레트 파일 선택 → 해당 TreeNode를 찾아 handleSelect 위임. */
  function openFileFromPalette(path: string): void {
    const node = findNode(tree, path);
    if (node) {
      void handleSelect(node);
      nav.pushRecent(path);
    }
  }

  /**
   * 선택 노드를 부모 형제 목록 내에서 delta칸(+1=아래, -1=위) 옮기고 order를 영속.
   * 형제 배열은 TreeView 렌더와 동일하게 mergeOrder 적용 순서를 기준으로 한다 —
   * parentPath 키도 TreeView와 일치(루트=root, 그 외=부모 컨테이너 path).
   */
  function reorderSelected(delta: number): void {
    if (!root || !selectedNode) return;
    const path = selectedNode.path;
    const parentPath = parentDir(path); // 루트 레벨 노드면 root와 일치
    const parentNode = parentPath === root ? null : findNode(tree, parentPath);
    const siblings = parentNode ? parentNode.children : tree;
    const ordered = mergeOrder(siblings, nav.order[parentPath] ?? [], (n) => n.path);
    const idx = ordered.findIndex((n) => n.path === path);
    if (idx === -1) return;
    const next = moveInArray(ordered, idx, delta);
    if (next === ordered) return; // 경계 — 변화 없음
    void nav.setOrder(parentPath, next.map((n) => n.path));
  }

  function onGlobalKey(e: KeyboardEvent): void {
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "p") {
      e.preventDefault();
      palette.show();
    }
  }

  // 창 닫기 시 미저장 편집을 디스크에 flush한 뒤 실제로 닫는다("항상 저장됨" 보장).
  onMount(() => {
    // E2E 테스트 브리지(dev 빌드 전용 — 프로덕션 번들에서 트리쉐이크 제거).
    // 네이티브 폴더 다이얼로그를 우회해 Playwright가 볼트를 직접 열 수 있게 한다.
    if (import.meta.env.DEV) {
      (window as unknown as { __textreeTest?: unknown }).__textreeTest = {
        loadVault,
      };
    }

    const win = getCurrentWindow();
    const unlistenClose = win.onCloseRequested(async (event) => {
      if (!pending) return; // 저장할 것 없으면 기본 닫기 진행
      event.preventDefault();
      await flush();
      // flush가 실패하면 pending이 남는다(saveError 표시됨). 이 경우 닫지
      // 않고 창을 유지해 데이터 유실을 막는다. 성공 시에만 실제로 닫는다.
      if (!pending) await win.destroy();
    });

    // 외부 파일 변경 구독.
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
        pending = null; // 사라진 파일에 저장 시도 방지
        dirty = false;
        conflictDisk = null; // 충돌 배너와 동시 표시 방지(삭제가 우선)
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
          title={`볼트 전환 — 현재: ${root}`}
        >📁 {vaultName(root)}</button>
      {:else}
        <span class="brand">Textree</span>
      {/if}
      <button
        class="icon-btn"
        onclick={() => theme.toggle()}
        title={theme.resolved === "dark" ? "라이트 테마로 전환" : "다크 테마로 전환"}
        aria-label="테마 전환"
      >{theme.resolved === "dark" ? "☀" : "☾"}</button>
      <button
        class="icon-btn"
        onclick={() => layout.toggleCollapsed()}
        title="사이드바 접기"
        aria-label="사이드바 접기"
      >⟨</button>
    </div>
    {#if root}
      <div class="toolbar">
        <button onclick={() => startMode("new-note")} title="새 노트">＋노트</button>
        <button onclick={() => startMode("new-folder")} title="새 폴더">＋폴더</button>
        <button
          onclick={startAddChild}
          disabled={selectedNode?.kind !== "leaf"}
          title="선택한 노트를 폴더로 승격하고 그 안에 새 노트 추가"
        >＋하위</button>
        <button onclick={() => startMode("rename")} disabled={!selectedNode}>이름변경</button>
        <button onclick={deleteSelected} disabled={!selectedNode}>삭제</button>
      </div>
      {#if mode !== "none"}
        <div class="name-edit">
          <input
            class="name-input"
            placeholder={mode === "new-folder" ||
            (mode === "rename" && selectedNode?.kind === "container")
              ? "폴더 이름"
              : "노트 이름"}
            bind:value={nameInput}
            onkeydown={(e) => {
              if (e.key === "Enter") confirmMode();
              else if (e.key === "Escape") cancelMode();
            }}
          />
          <button onclick={confirmMode}>확인</button>
          <button onclick={cancelMode}>취소</button>
        </div>
      {/if}
      {#if opError}
        <p class="op-error">⚠ {opError}</p>
      {/if}
      <!-- 노드 밖(빈 영역)에 드롭하면 볼트 루트로 이동. 노드 드롭은 stopPropagation됨.
           이 영역은 트리를 감싸는 "볼트 루트 드롭 영역"이므로 group으로 표기. -->
      <div
        class="tree-root"
        role="group"
        aria-label="볼트 루트 — 여기에 드롭하면 루트로 이동"
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
          selectedPath={selectedNode?.path ?? null}
        />
      </div>
    {:else}
      <p class="hint">볼트가 열려 있지 않습니다.</p>
    {/if}
  </aside>
  <div
    class="resize-handle"
    role="separator"
    aria-orientation="vertical"
    aria-label="사이드바 크기 조절"
    onpointerdown={startResize}
  ></div>
  {/if}
  <main class="content">
    {#if !root}
      <div class="empty-state">
        <h1 class="empty-brand">Textree</h1>
        <p class="empty-sub">로컬 마크다운 볼트를 열어 시작하세요.</p>
        <button class="open-cta" onclick={chooseVault}>볼트 열기</button>
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
              title="클릭하여 제목(파일명) 변경"
            >{activeName}</button>
          {/if}
        </span>
        {#if saveError}
          <span class="status error">⚠ 저장 실패: {saveError}</span>
        {:else if removed}
          <span class="status error">⚠ 외부에서 이동/삭제됨</span>
        {:else}
          <span class="status">{dirty ? "● 저장 중…" : "저장됨"}</span>
        {/if}
      </header>
      {#if conflictDisk !== null}
        <div class="banner" role="alert">
          <span>이 노트가 외부에서 변경되었습니다. 미저장 편집이 있습니다.</span>
          <span class="banner-actions">
            <button onclick={resolveTakeDisk}>디스크 버전 불러오기</button>
            <button onclick={resolveKeepMine}>내 편집 유지</button>
          </span>
        </div>
      {/if}
      {#if removed}
        <p class="hint">이 노트는 외부에서 이동되거나 삭제되었습니다. 다른 노트를 선택하세요.</p>
      {:else}
        <Editor
          docKey={`${activePath}@${reloadVersion}`}
          initialDoc={content}
          onchange={handleEdit}
          onImagePaste={handleImagePaste}
        />
      {/if}
    {:else}
      <p class="hint">노트를 선택하세요.</p>
    {/if}
  </main>
  {#if layout.collapsed}
    <button
      class="expand-btn"
      onclick={() => layout.toggleCollapsed()}
      title="사이드바 펼치기"
      aria-label="사이드바 펼치기"
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
  /* 사이드바와 본문 사이 드래그 핸들. 폭은 좁게, 히트영역은 넉넉히(여백). */
  .resize-handle {
    flex-shrink: 0;
    width: 1px;
    background: var(--border);
    cursor: col-resize;
    position: relative;
  }
  .resize-handle::after {
    /* 보이지 않는 넓은 히트 영역(±3px). */
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
  /* 볼트명(컴팩트 헤더) — 클릭 시 볼트 전환. 전체 경로는 title 툴팁. */
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
  /* 헤더 아이콘 버튼(테마·접기). 정사각, 무테, 호버 시만 강조. */
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
  /* 접힌 상태에서 떠 있는 펼치기 버튼(좌상단). */
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
  /* 빈 상태(볼트 미열림) — 본문 중앙 온보딩. */
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
  /* 트리 아래 빈 영역도 드롭 대상이 되도록 남은 높이를 채운다(루트로 이동). */
  .tree-root {
    min-height: 80px;
  }
  .tree-root:focus {
    outline: none;
  }
</style>
