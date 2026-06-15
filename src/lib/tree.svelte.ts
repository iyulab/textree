/*
 * 트리 UI 상태 — 접힘 집합 + 키보드 포커스 경로.
 *
 * 접힘은 표현 계층 선호이므로 localStorage 영속(절대경로 기반 → 볼트 무충돌).
 * 포커스(roving tabindex)는 세션 상태라 영속하지 않는다.
 */

const COLLAPSE_KEY = "textree-tree-collapsed";

function loadCollapsed(): Set<string> {
  if (typeof localStorage === "undefined") return new Set();
  try {
    const arr = JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? "[]");
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

class TreeStore {
  collapsed = $state<Set<string>>(loadCollapsed());
  /** 키보드 포커스 중인 노드 경로(roving tabindex 기준). null=미설정(첫 항목 폴백). */
  focused = $state<string | null>(null);
  /** 키보드 이동을 위해 "잘라낸" 노드 경로(Ctrl+X). 붙여넣기(Ctrl+V) 대상에 이동/adopt. */
  cut = $state<string | null>(null);

  isCollapsed(path: string): boolean {
    return this.collapsed.has(path);
  }

  private persist(): void {
    if (typeof localStorage !== "undefined")
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...this.collapsed]));
  }

  collapse(path: string): void {
    if (this.collapsed.has(path)) return;
    const next = new Set(this.collapsed);
    next.add(path);
    this.collapsed = next;
    this.persist();
  }

  expand(path: string): void {
    if (!this.collapsed.has(path)) return;
    const next = new Set(this.collapsed);
    next.delete(path);
    this.collapsed = next;
    this.persist();
  }

  toggle(path: string): void {
    if (this.collapsed.has(path)) this.expand(path);
    else this.collapse(path);
  }

  setFocused(path: string | null): void {
    this.focused = path;
  }

  setCut(path: string | null): void {
    this.cut = path;
  }
}

export const tree = new TreeStore();
