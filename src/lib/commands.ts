/*
 * 명령 레지스트리 — 통합 팔레트의 '>' 명령 모드 대상.
 * 컴포넌트가 보유한 액션을 주입받아 명령 목록을 만든다(결합도 최소화).
 */

export interface Command {
  id: string;
  title: string;
  run: () => void | Promise<void>;
  /** false면 현재 컨텍스트에서 비활성(목록 제외). 생략 시 항상 활성. */
  when?: () => boolean;
}

/** 팔레트가 호출할 앱 액션 묶음. +page.svelte가 구현해 주입한다. */
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

/** when()을 평가해 현재 활성 명령만 반환. */
export function activeCommands(cmds: Command[]): Command[] {
  return cmds.filter((c) => c.when === undefined || c.when());
}
