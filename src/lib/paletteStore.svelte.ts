/*
 * 통합 팔레트 상태 — 열림/쿼리/선택 인덱스. 모드는 쿼리에서 파생('>'=명령).
 */

import { paletteMode, paletteTerm, type PaletteMode } from "./palette.helpers";

class PaletteStore {
  open = $state(false);
  query = $state("");
  /** 결과 리스트 내 키보드 선택 인덱스. */
  selected = $state(0);

  /** 쿼리 접두로 모드 파생('>'=명령, '/'=본문검색, 그 외=파일). */
  get mode(): PaletteMode {
    return paletteMode(this.query);
  }

  /** 모드별 실제 검색어(접두 제거). */
  get term(): string {
    return paletteTerm(this.query);
  }

  show(): void {
    this.open = true;
    this.query = "";
    this.selected = 0;
  }

  hide(): void {
    this.open = false;
  }

  setQuery(q: string): void {
    this.query = q;
    this.selected = 0; // 입력 변경 시 선택 리셋
  }

  move(delta: number, count: number): void {
    if (count === 0) return;
    this.selected = (this.selected + delta + count) % count;
  }
}

export const palette = new PaletteStore();
