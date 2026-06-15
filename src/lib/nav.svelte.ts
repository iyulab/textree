/*
 * 네비게이션 스토어 — 즐겨찾기·최근·수동정렬.
 * 즐겨찾기/정렬은 .textree/(볼트 귀속), 최근은 localStorage(기기 귀속).
 *
 * 순수 로직(mergeOrder/dedupePushFront/RECENT_MAX)은 nav.helpers.ts에 있다 —
 * runes 없는 모듈로 분리해 vitest(node 환경)에서 직접 테스트한다.
 */

import { readSidecar, writeSidecar } from "./ipc";
import { dedupePushFront, RECENT_MAX } from "./nav.helpers";

// 컴포넌트가 `$lib/nav.svelte`에서 정렬 헬퍼를 함께 import할 수 있도록 re-export.
export { mergeOrder } from "./nav.helpers";

const RECENT_KEY = "textree-recent";

class NavStore {
  favorites = $state<string[]>([]);
  recent = $state<string[]>(loadRecent());
  /** parentPath → 자식 경로 순서. */
  order = $state<Record<string, string[]>>({});
  /** 현재 볼트 루트(영속 IPC 대상). null이면 미로드. */
  private root: string | null = null;

  /** 볼트 열기/전환 시 호출 — 사이드카에서 favorites/order 재로드. */
  async load(root: string): Promise<void> {
    this.root = root;
    this.favorites = (await readJson<string[]>(root, "favorites.json")) ?? [];
    this.order = (await readJson<Record<string, string[]>>(root, "order.json")) ?? {};
  }

  isFavorite(path: string): boolean {
    return this.favorites.includes(path);
  }

  async toggleFavorite(path: string): Promise<void> {
    this.favorites = this.isFavorite(path)
      ? this.favorites.filter((p) => p !== path)
      : [...this.favorites, path];
    await this.persist("favorites.json", this.favorites);
  }

  pushRecent(path: string): void {
    this.recent = dedupePushFront(this.recent, path, RECENT_MAX);
    if (typeof localStorage !== "undefined")
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
  }

  async setOrder(parent: string, paths: string[]): Promise<void> {
    this.order = { ...this.order, [parent]: paths };
    await this.persist("order.json", this.order);
  }

  private async persist(rel: string, value: unknown): Promise<void> {
    if (this.root === null) return;
    try {
      await writeSidecar(this.root, rel, JSON.stringify(value));
    } catch (e) {
      // 비차단: 인메모리 상태는 유지. 다음 쓰기에서 복구 시도.
      console.warn(`사이드카 쓰기 실패(${rel}):`, e);
    }
  }
}

function loadRecent(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const arr = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

/** 사이드카 JSON 읽기 — 부재/손상 시 null(호출부가 기본값 폴백). */
async function readJson<T>(root: string, rel: string): Promise<T | null> {
  try {
    const raw = await readSidecar(root, rel);
    return raw === null ? null : (JSON.parse(raw) as T);
  } catch (e) {
    console.warn(`사이드카 읽기/파싱 실패(${rel}):`, e);
    return null;
  }
}

export const nav = new NavStore();
