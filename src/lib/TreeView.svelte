<script module lang="ts">
  // 트리 내부 드래그앤드롭 전송 키(단일 출처). OS 파일 드롭과 구분되는 앱 전용 MIME.
  export const DRAG_MIME = "application/x-textree-path";

  // 드롭 대상으로 강조된 노드 경로. 재귀 인스턴스 전체가 공유하는 단일 상태 —
  // 드래그 중 강조는 항상 하나뿐이고, 어느 인스턴스의 dragend로도 확실히 해제된다.
  let dragOverPath = $state<string | null>(null);
</script>

<script lang="ts">
  import type { TreeNode } from "./ipc";
  import { tree } from "./tree.svelte";
  import { mergeOrder, nav } from "./nav.svelte";
  import Self from "./TreeView.svelte";

  let {
    nodes,
    parentPath,
    onselect,
    onmove,
    onadopt,
    onrename,
    ondelete,
    selectedPath = null,
    top = false,
  }: {
    nodes: TreeNode[];
    /** 이 노드 목록의 부모 경로(수동 정렬 키). 최상위는 볼트 루트. */
    parentPath: string;
    onselect: (node: TreeNode) => void;
    /** 노드 이동(폴더 간). srcPath를 destDir(컨테이너 디렉터리)로 옮긴다. */
    onmove: (srcPath: string, destDir: string) => void;
    /** 리프 위에 드롭: leafPath를 컨테이너로 승격하고 srcPath를 그 자식으로 이동. */
    onadopt: (srcPath: string, leafPath: string) => void;
    /** 키보드 F2: 해당 노드를 이름변경. */
    onrename: (node: TreeNode) => void;
    /** 키보드 Delete: 해당 노드를 삭제. */
    ondelete: (node: TreeNode) => void;
    /** 현재 선택된 노드 경로(하이라이트용). 모든 하위에 전파. */
    selectedPath?: string | null;
    /** 최상위 인스턴스 여부(role=tree vs group, 폴백 포커스). */
    top?: boolean;
  } = $props();

  /** nav.order[parentPath]에 따라 형제 노드를 정렬한 파생 배열. order 미등재 시 원래 순서. */
  let ordered = $derived(mergeOrder(nodes, nav.order[parentPath] ?? [], (n) => n.path));

  function onDragStart(e: DragEvent, node: TreeNode) {
    if (!e.dataTransfer) return;
    e.dataTransfer.setData(DRAG_MIME, node.path);
    e.dataTransfer.effectAllowed = "move";
  }

  function onDragOver(e: DragEvent, _node: TreeNode) {
    // 컨테이너=폴더 간 이동, 리프=adopt(승격 후 자식). 둘 다 드롭 대상.
    // preventDefault해야 drop이 허용된다. stopPropagation으로 최내곽 노드만 반응.
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverPath = _node.path;
  }

  function onDragLeave(node: TreeNode) {
    if (dragOverPath === node.path) dragOverPath = null;
  }

  function onDrop(e: DragEvent, node: TreeNode) {
    e.preventDefault();
    e.stopPropagation();
    dragOverPath = null;
    const src = e.dataTransfer?.getData(DRAG_MIME);
    if (!src) return;
    if (node.kind === "container") onmove(src, node.path); // 컨테이너 디렉터리로 이동
    else onadopt(src, node.path); // 리프 → 승격 후 자식으로
  }

  // ── 키보드 네비게이션(WAI-ARIA tree) ──────────────────────────
  // 이동은 실제 DOM 문서순서(접힌 노드는 렌더 안 됨)로 처리해 평면 모델 불필요.
  function items(from: HTMLElement): HTMLElement[] {
    const root = from.closest('[role="tree"]');
    if (!root) return [];
    return [...root.querySelectorAll<HTMLElement>('[role="treeitem"]')];
  }

  function focusItem(el: HTMLElement | null | undefined, path?: string) {
    if (!el) return;
    el.focus();
    if (path) tree.setFocused(path);
  }

  function siblingItem(el: HTMLElement, delta: number) {
    const all = items(el);
    const i = all.indexOf(el);
    return i >= 0 ? all[i + delta] : undefined;
  }

  function firstChildItem(el: HTMLElement): HTMLElement | null {
    const li = el.closest("li");
    return (
      li?.querySelector<HTMLElement>(
        ':scope > ul[role="group"] > li > .row > [role="treeitem"]',
      ) ?? null
    );
  }

  function parentItem(el: HTMLElement): HTMLElement | null {
    const ul = el.closest('ul[role="group"]');
    const parentLi = ul?.parentElement; // group ul 의 부모 = 부모 li
    return (
      parentLi?.querySelector<HTMLElement>(
        ':scope > .row > [role="treeitem"]',
      ) ?? null
    );
  }

  function onKeydown(e: KeyboardEvent, node: TreeNode) {
    const el = e.currentTarget as HTMLElement;
    const isContainer = node.kind === "container" && node.children.length > 0;
    const open = isContainer && !tree.isCollapsed(node.path);

    // 키보드 이동: Ctrl+X 잘라내기 → 대상서 Ctrl+V(컨테이너=이동, 리프=adopt).
    // DnD 와 동등한 접근성 경로(WCAG 2.1.1).
    if (e.ctrlKey && (e.key === "x" || e.key === "X")) {
      e.preventDefault();
      tree.setCut(node.path);
      return;
    }
    if (e.ctrlKey && (e.key === "v" || e.key === "V")) {
      e.preventDefault();
      const src = tree.cut;
      if (!src) return;
      tree.setCut(null);
      if (node.kind === "container") onmove(src, node.path);
      else onadopt(src, node.path);
      return;
    }

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusItem(siblingItem(el, 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        focusItem(siblingItem(el, -1));
        break;
      case "ArrowRight":
        e.preventDefault();
        if (isContainer && !open) tree.expand(node.path);
        else if (open) focusItem(firstChildItem(el));
        break;
      case "ArrowLeft":
        e.preventDefault();
        if (open) tree.collapse(node.path);
        else focusItem(parentItem(el));
        break;
      case "Home":
        e.preventDefault();
        focusItem(items(el)[0]);
        break;
      case "End": {
        e.preventDefault();
        const all = items(el);
        focusItem(all[all.length - 1]);
        break;
      }
      case "Enter":
      case " ":
        e.preventDefault();
        onselect(node);
        break;
      case "F2":
        e.preventDefault();
        onrename(node);
        break;
      case "Delete":
        e.preventDefault();
        ondelete(node);
        break;
      case "Escape":
        if (tree.cut) {
          e.preventDefault();
          tree.setCut(null);
        }
        break;
    }
  }

  /** roving tabindex: 포커스 노드만 0, 미설정 시 최상위 첫 항목이 0. */
  function tabFor(path: string, index: number): number {
    if (tree.focused === path) return 0;
    if (tree.focused === null && top && index === 0) return 0;
    return -1;
  }
</script>

<ul class="tree" role={top ? "tree" : "group"}>
  {#each ordered as node, i}
    {@const hasChildren = node.children.length > 0}
    {@const open = !tree.isCollapsed(node.path)}
    {@const isContainer = node.kind === "container" && hasChildren}
    <li>
      <div
        class="row"
        class:selected={node.path === selectedPath}
        class:drop-over={node.path === dragOverPath}
        class:drop-leaf={node.path === dragOverPath && node.kind === "leaf"}
        class:cut={node.path === tree.cut}
      >
        {#if isContainer}
          <button
            class="chevron"
            class:open
            onclick={(e) => {
              e.stopPropagation();
              tree.toggle(node.path);
            }}
            aria-label={open ? "접기" : "펼치기"}
            tabindex="-1"
          >▸</button>
        {:else}
          <span class="chevron-spacer"></span>
        {/if}
        <button
          class="node"
          class:no-body={!node.body_path}
          role="treeitem"
          aria-selected={node.path === selectedPath}
          aria-expanded={isContainer ? open : undefined}
          tabindex={tabFor(node.path, i)}
          draggable="true"
          ondragstart={(e) => onDragStart(e, node)}
          ondragend={() => (dragOverPath = null)}
          ondragover={(e) => onDragOver(e, node)}
          ondragleave={() => onDragLeave(node)}
          ondrop={(e) => onDrop(e, node)}
          onclick={() => onselect(node)}
          onfocus={() => tree.setFocused(node.path)}
          onkeydown={(e) => onKeydown(e, node)}
        >
          <span class="icon">{node.kind === "container" ? "📁" : "📄"}</span>
          <span class="label">{node.name}</span>
        </button>
      </div>
      {#if hasChildren && open}
        <Self
          nodes={node.children}
          parentPath={node.path}
          {onselect}
          {onmove}
          {onadopt}
          {onrename}
          {ondelete}
          {selectedPath}
        />
      {/if}
    </li>
  {/each}
</ul>

<style>
  .tree {
    list-style: none;
    margin: 0;
    padding-left: var(--sp-3);
  }
  /* 노드 행: chevron + 노드 버튼. 선택/드롭 강조는 행 전체에. */
  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius-s);
    transition: background var(--transition-fast);
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.selected {
    background: var(--selection-bg);
  }
  /* 드롭 대상 — 컨테이너(이동): 실선 테두리 + accent 배경. */
  .row.drop-over {
    outline: 2px solid var(--drop-border);
    outline-offset: -2px;
    background: var(--drop-bg);
  }
  /* 드롭 대상 — 리프(adopt, 승격 후 자식): 점선 테두리로 "이동"과 시각 구분. */
  .row.drop-leaf {
    outline-style: dashed;
  }
  /* 키보드로 "잘라낸" 노드: 흐리게 + 점선 테두리(붙여넣기 대기 표시). */
  .row.cut {
    opacity: 0.5;
    outline: 1px dashed var(--border-strong);
    outline-offset: -1px;
  }
  .chevron {
    flex-shrink: 0;
    width: 16px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    font-size: 10px;
    line-height: 1;
    color: var(--text-faint);
    background: none;
    border: none;
    cursor: pointer;
    transition: transform var(--transition-fast);
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .chevron-spacer {
    flex-shrink: 0;
    width: 16px;
  }
  .node {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    padding: 3px var(--sp-1);
    font: inherit;
    font-size: var(--font-size-small);
    color: var(--text-normal);
    border-radius: var(--radius-s);
  }
  /* 키보드 포커스 링(마우스 클릭에는 안 뜨도록 focus-visible). */
  .node:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .icon {
    flex-shrink: 0;
    font-size: 0.9em;
  }
  .label {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  /* body 없는 컨테이너: 본문은 못 열지만 구조 편집 대상으로 선택은 가능 */
  .no-body .label {
    color: var(--text-muted);
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
  }
</style>
