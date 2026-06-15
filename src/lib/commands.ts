/*
 * Command registry — targets the unified palette's '>' command mode.
 * Builds the command list from injected component-owned actions (minimizing coupling).
 */

export interface Command {
  id: string;
  title: string;
  run: () => void | Promise<void>;
  /** If false, inactive in the current context (excluded from the list). Always active if omitted. */
  when?: () => boolean;
}

/** Bundle of app actions the palette invokes. +page.svelte implements and injects them. */
export interface PaletteActions {
  openVault: () => void;
  toggleTheme: () => void;
  toggleSidebar: () => void;
  newNoteAtRoot: () => void;
  newFolderAtRoot: () => void;
  hasSelection: () => boolean;
  renameSelected: () => void;
  deleteSelected: () => void;
  promoteSelected: () => void;
  toggleFavoriteSelected: () => void;
  moveSelectedUp: () => void;
  moveSelectedDown: () => void;
  rebuildIndex: () => void;
}

export function buildCommands(a: PaletteActions): Command[] {
  const sel = a.hasSelection;
  return [
    { id: "vault.open", title: "볼트 열기/전환", run: a.openVault },
    { id: "view.theme", title: "테마 전환(라이트↔다크)", run: a.toggleTheme },
    { id: "view.sidebar", title: "사이드바 접기/펼치기", run: a.toggleSidebar },
    { id: "note.new", title: "새 노트(루트)", run: a.newNoteAtRoot },
    { id: "folder.new", title: "새 폴더(루트)", run: a.newFolderAtRoot },
    { id: "node.rename", title: "선택 노드 이름 변경", run: a.renameSelected, when: sel },
    { id: "node.delete", title: "선택 노드 삭제", run: a.deleteSelected, when: sel },
    { id: "node.promote", title: "선택 노드 승격", run: a.promoteSelected, when: sel },
    { id: "node.favorite", title: "선택 노드 즐겨찾기 토글", run: a.toggleFavoriteSelected, when: sel },
    { id: "node.moveUp", title: "선택 노드 위로 이동", run: a.moveSelectedUp, when: sel },
    { id: "node.moveDown", title: "선택 노드 아래로 이동", run: a.moveSelectedDown, when: sel },
    { id: "search.rebuild", title: "본문 인덱스 재색인", run: a.rebuildIndex },
  ];
}

/** Evaluates when() and returns only currently active commands. */
export function activeCommands(cmds: Command[]): Command[] {
  return cmds.filter((c) => c.when === undefined || c.when());
}
