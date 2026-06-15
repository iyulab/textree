/*
 * 테마 상태 — 라이트/다크/시스템(auto).
 *
 * 선택은 localStorage("textree-theme")에 영속한다. 이는 **앱 설정**이며 볼트의
 * 라이브러리 schema(.textree/, frontmatter)와 분리된다 — 표현 계층 선호이지
 * 노트 데이터가 아니다.
 *
 * "auto"는 OS 선호(prefers-color-scheme)를 실시간 추종한다. 명시적 light/dark는
 * OS 변화를 무시한다. 실제 적용은 <html data-theme> 속성으로, 토큰(tokens.css)의
 * [data-theme="dark"] 셀렉터가 색을 전환한다.
 */

export type ThemeMode = "auto" | "light" | "dark";

const STORAGE_KEY = "textree-theme";

function readStored(): ThemeMode {
  if (typeof localStorage === "undefined") return "auto";
  const v = localStorage.getItem(STORAGE_KEY);
  return v === "light" || v === "dark" || v === "auto" ? v : "auto";
}

function systemPrefersDark(): boolean {
  return (
    typeof matchMedia !== "undefined" &&
    matchMedia("(prefers-color-scheme: dark)").matches
  );
}

/** mode 를 실제 다크/라이트로 해석. */
function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "auto") return systemPrefersDark() ? "dark" : "light";
  return mode;
}

/** <html data-theme> 에 해석된 테마를 반영. */
function apply(mode: ThemeMode): void {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", resolve(mode));
}

class ThemeStore {
  /** 사용자 선택 모드(auto/light/dark). */
  mode = $state<ThemeMode>("auto");
  /** 현재 실제 적용된 테마(파생). */
  resolved = $derived<"light" | "dark">(resolve(this.mode));

  private mql: MediaQueryList | null = null;
  private onSystemChange = () => {
    // auto 일 때만 OS 변화를 반영(명시 선택은 고정).
    if (this.mode === "auto") apply(this.mode);
  };

  /** 앱 시작 시 1회: 저장된 선택을 로드하고 DOM 에 반영, OS 변화 구독.
   *  cleanup 함수를 반환(구독 해제). */
  init(): () => void {
    this.mode = readStored();
    apply(this.mode);
    if (typeof matchMedia !== "undefined") {
      this.mql = matchMedia("(prefers-color-scheme: dark)");
      this.mql.addEventListener("change", this.onSystemChange);
    }
    return () => this.mql?.removeEventListener("change", this.onSystemChange);
  }

  /** 모드 설정 + 영속 + 적용. */
  set(mode: ThemeMode): void {
    this.mode = mode;
    if (typeof localStorage !== "undefined") localStorage.setItem(STORAGE_KEY, mode);
    apply(mode);
  }

  /** 라이트 ↔ 다크 토글. auto 였다면 현재 해석값의 반대로 고정 전환. */
  toggle(): void {
    this.set(this.resolved === "dark" ? "light" : "dark");
  }
}

export const theme = new ThemeStore();
