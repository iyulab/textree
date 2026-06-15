/*
 * 레이아웃 상태 — 사이드바 폭 + 접힘.
 *
 * 표현 계층 선호이므로 localStorage 에 영속한다(앱 설정 — 볼트의 라이브러리
 * schema 와 분리). theme.svelte.ts 와 동일한 패턴.
 */

const WIDTH_KEY = "textree-sidebar-width";
const COLLAPSED_KEY = "textree-sidebar-collapsed";

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX = 480;
const SIDEBAR_DEFAULT = 280;

function clampWidth(w: number): number {
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w));
}

function readWidth(): number {
  if (typeof localStorage === "undefined") return SIDEBAR_DEFAULT;
  const v = Number(localStorage.getItem(WIDTH_KEY));
  return Number.isFinite(v) && v > 0 ? clampWidth(v) : SIDEBAR_DEFAULT;
}

function readCollapsed(): boolean {
  if (typeof localStorage === "undefined") return false;
  return localStorage.getItem(COLLAPSED_KEY) === "true";
}

class LayoutStore {
  width = $state<number>(SIDEBAR_DEFAULT);
  collapsed = $state<boolean>(false);

  /** 앱 시작 시 1회: 저장된 값 로드. */
  init(): void {
    this.width = readWidth();
    this.collapsed = readCollapsed();
  }

  /** 드래그 중 폭 설정(클램프). 영속은 드래그 종료 시 1회(persistWidth). */
  setWidth(w: number): void {
    this.width = clampWidth(w);
  }

  persistWidth(): void {
    if (typeof localStorage !== "undefined")
      localStorage.setItem(WIDTH_KEY, String(this.width));
  }

  toggleCollapsed(): void {
    this.collapsed = !this.collapsed;
    if (typeof localStorage !== "undefined")
      localStorage.setItem(COLLAPSED_KEY, String(this.collapsed));
  }
}

export const layout = new LayoutStore();
