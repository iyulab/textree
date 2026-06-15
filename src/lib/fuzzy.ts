/*
 * 순수 함수 fuzzy 매처 — subsequence 매칭 + 가중 스코어링.
 * 외부 의존성 없음(미니멀). 점수 가중: 연속 매치, 단어 경계 시작, 앞쪽 위치.
 */

export interface Match<T> {
  item: T;
  score: number;
  /** 매칭 문자 위치 범위(하이라이트용), [start, end) 반(half-open). */
  ranges: [number, number][];
}

function isBoundary(text: string, i: number): boolean {
  if (i === 0) return true;
  const prev = text[i - 1];
  const cur = text[i];
  // 구분자 다음이거나, 소문자→대문자 전이(camelCase 경계).
  return (
    prev === "/" ||
    prev === "-" ||
    prev === "_" ||
    prev === " " ||
    prev === "." ||
    (prev === prev.toLowerCase() && cur === cur.toUpperCase() && cur !== cur.toLowerCase())
  );
}

/** 단일 후보 문자열에 대한 점수. 매칭 실패 시 null. */
function scoreOne(query: string, text: string): { score: number; ranges: [number, number][] } | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let score = 0;
  const positions: number[] = [];
  let ti = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    let found = -1;
    for (let k = ti; k < t.length; k++) {
      if (t[k] === ch) {
        found = k;
        break;
      }
    }
    if (found === -1) return null; // subsequence 불성립
    // 가중: 앞쪽일수록(작은 found) 가산, 경계면 가산, 직전 매치와 연속이면 가산.
    // 위치 보너스는 앞 20자 윈도우 근사. 그 밖(긴 이름 뒤쪽)은 경계·연속 가중이
    // 순위를 주도한다(YAGNI: 실측 불만 시 상대 가중치로 튜닝).
    score += Math.max(0, 20 - found);
    if (isBoundary(text, found)) score += 15;
    if (positions.length && found === positions[positions.length - 1] + 1) score += 10;
    positions.push(found);
    ti = found + 1;
  }
  // 연속 위치를 [start,end) 범위로 압축.
  const ranges: [number, number][] = [];
  for (const p of positions) {
    const last = ranges[ranges.length - 1];
    if (last && last[1] === p) last[1] = p + 1;
    else ranges.push([p, p + 1]);
  }
  return { score, ranges };
}

/**
 * 후보 배열을 fuzzy 매칭해 점수 내림차순으로 반환. 매칭 실패 후보는 제외.
 * 동점이면 후보 문자열이 짧은 쪽을 우선(더 정확한 매치로 간주).
 * 빈 쿼리(`""`)면 전체 항목을 score 0·ranges 빈 배열로 원래 순서대로 반환한다.
 */
export function fuzzyMatch<T>(
  query: string,
  items: T[],
  key: (item: T) => string,
): Match<T>[] {
  if (query === "") return items.map((item) => ({ item, score: 0, ranges: [] }));
  const out: Match<T>[] = [];
  for (const item of items) {
    const text = key(item);
    const s = scoreOne(query, text);
    if (s) out.push({ item, score: s.score, ranges: s.ranges });
  }
  out.sort((a, b) => b.score - a.score || key(a.item).length - key(b.item).length);
  return out;
}
