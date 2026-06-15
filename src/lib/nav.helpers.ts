/*
 * 네비게이션 순수 헬퍼 — runes 없는 순수 함수/상수.
 * nav.svelte.ts(runes 스토어)와 분리하여 vitest(node 환경)에서 안전하게 테스트한다.
 */

/** 최근 목록 상한. */
export const RECENT_MAX = 20;

/**
 * 형제 노드를 order 배열에 따라 병합 정렬한다(순수 함수).
 * order에 등재된 항목을 지정 순서대로 앞에, 나머지는 원래 순서로 뒤에 둔다.
 * order에 있으나 siblings에 없는 경로는 무시한다(삭제된 경로 자기치유).
 */
export function mergeOrder<T>(
  siblings: T[],
  order: string[],
  key: (item: T) => string,
): T[] {
  const bySib = new Map(siblings.map((s) => [key(s), s]));
  const placed = new Set<string>();
  const head: T[] = [];
  for (const k of order) {
    const item = bySib.get(k);
    if (item !== undefined && !placed.has(k)) {
      head.push(item);
      placed.add(k);
    }
  }
  const tail = siblings.filter((s) => !placed.has(key(s)));
  return [...head, ...tail];
}

/** value를 맨 앞에 넣고 중복 제거 후 max개로 자른다(순수). */
export function dedupePushFront(list: string[], value: string, max: number): string[] {
  return [value, ...list.filter((v) => v !== value)].slice(0, max);
}

/**
 * 배열에서 index 항목을 delta칸(+1=아래, -1=위) 이동한 새 배열을 반환(순수).
 * 범위를 벗어나면(맨 위에서 위로, 맨 아래에서 아래로) 원본을 그대로 반환.
 */
export function moveInArray<T>(arr: T[], index: number, delta: number): T[] {
  const to = index + delta;
  if (index < 0 || index >= arr.length || to < 0 || to >= arr.length) return arr;
  const next = arr.slice();
  const [item] = next.splice(index, 1);
  next.splice(to, 0, item);
  return next;
}
