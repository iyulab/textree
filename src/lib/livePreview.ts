/*
 * 라이브 프리뷰(옵시디언 LP 모델) — CodeMirror 6 인라인 데코레이션.
 *
 * 별도 프리뷰 패널이 아니라 에디터 위에서 마크다운을 렌더한다:
 *  - 헤딩은 크기를, 강조/코드 등은 스타일을 항상 적용(line/mark 데코).
 *  - 마커(#, **, *, `, ~~)는 **커서가 놓이지 않은 줄에서만** 숨긴다(replace 데코).
 *    커서가 그 줄에 오면 마커가 다시 보여 원본 마크다운을 그대로 편집할 수 있다.
 *
 * 성능: 가시 범위(visibleRanges)만 syntaxTree 로 순회하고, 문서·뷰포트·선택 변경
 * 시에만 데코를 재계산한다.
 *
 * 스코프(D4): 헤딩·강조(볼드/이탤릭)·취소선·인라인 코드. 링크·체크박스·인용/구분선·
 * 이미지 인라인은 후속(D4 계속/P4).
 */

import { syntaxTree } from "@codemirror/language";
import type { Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";

/** task list 체크박스 위젯 — 클릭하면 디스크 원문의 [ ]↔[x]를 토글한다. */
class CheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly from: number,
    readonly to: number,
  ) {
    super();
  }
  eq(other: CheckboxWidget) {
    return other.checked === this.checked && other.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = this.checked;
    box.className = "cm-lp-checkbox";
    // mousedown 차단: 커서가 줄로 이동해 위젯이 사라지는 것을 막고 토글만 수행.
    box.addEventListener("mousedown", (e) => {
      e.preventDefault();
      view.dispatch({
        changes: { from: this.from, to: this.to, insert: this.checked ? "[ ]" : "[x]" },
      });
    });
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

/** 구분선(---) 위젯 — 비활성 줄에서 원문 대신 가로줄로 렌더. */
class HrWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM(): HTMLElement {
    const hr = document.createElement("span");
    hr.className = "cm-lp-hr";
    return hr;
  }
}

/** 커서/선택이 걸친 줄 번호 집합 — 이 줄들은 마커를 숨기지 않는다(소스 노출). */
function activeLines(view: EditorView): Set<number> {
  const s = new Set<number>();
  const { doc } = view.state;
  for (const r of view.state.selection.ranges) {
    const a = doc.lineAt(r.from).number;
    const b = doc.lineAt(r.to).number;
    for (let l = a; l <= b; l++) s.add(l);
  }
  return s;
}

// 줄 단위 헤딩 크기 데코(클래스는 lpTheme 에서 정의).
const headingLine = [
  null,
  Decoration.line({ class: "cm-lp-h1" }),
  Decoration.line({ class: "cm-lp-h2" }),
  Decoration.line({ class: "cm-lp-h3" }),
  Decoration.line({ class: "cm-lp-h4" }),
  Decoration.line({ class: "cm-lp-h5" }),
  Decoration.line({ class: "cm-lp-h6" }),
];

const strongMark = Decoration.mark({ class: "cm-lp-strong" });
const emMark = Decoration.mark({ class: "cm-lp-em" });
const strikeMark = Decoration.mark({ class: "cm-lp-strike" });
const codeMark = Decoration.mark({ class: "cm-lp-code" });
const linkMark = Decoration.mark({ class: "cm-lp-link" });
const quoteLine = Decoration.line({ class: "cm-lp-quote" });
const hideMark = Decoration.replace({});

// 숨김 대상 마커 노드(lezer-markdown). URL 은 인라인 링크일 때만 숨겨야 해서
// (autolink·bare url 은 URL 이 유일한 가시 콘텐츠) 일반 숨김 집합에 넣지 않는다.
const MARKER_NODES = new Set([
  "HeaderMark",
  "EmphasisMark",
  "CodeMark",
  "StrikethroughMark",
  "LinkMark",
  "QuoteMark",
]);

function buildDecorations(view: EditorView): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const { state } = view;
  const active = activeLines(view);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;

        // 헤딩: 줄 전체에 크기 데코(커서 줄이어도 크기는 유지 — 옵시디언과 동일).
        const h = /^ATXHeading([1-6])$/.exec(name);
        if (h) {
          const deco = headingLine[Number(h[1])];
          if (deco) ranges.push(deco.range(state.doc.lineAt(node.from).from));
          return;
        }

        // 인용: 줄마다 좌측 바 스타일.
        if (name === "Blockquote") {
          const startLn = state.doc.lineAt(node.from).number;
          const endLn = state.doc.lineAt(node.to).number;
          for (let l = startLn; l <= endLn; l++) {
            ranges.push(quoteLine.range(state.doc.line(l).from));
          }
          return;
        }

        // 구분선(---): 비활성 줄에서 원문 대신 가로줄 위젯.
        if (name === "HorizontalRule") {
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          const line = state.doc.lineAt(node.from);
          if (line.from < line.to)
            ranges.push(
              Decoration.replace({ widget: new HrWidget() }).range(line.from, line.to),
            );
          return;
        }

        // task list 체크박스: 비활성 줄에서 마커를 토글 위젯으로 치환.
        if (name === "TaskMarker") {
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          const checked = /[xX]/.test(state.doc.sliceString(node.from, node.to));
          ranges.push(
            Decoration.replace({
              widget: new CheckboxWidget(checked, node.from, node.to),
            }).range(node.from, node.to),
          );
          return;
        }

        // 인라인 스타일: 내용 범위에 항상 적용.
        if (name === "StrongEmphasis") ranges.push(strongMark.range(node.from, node.to));
        else if (name === "Emphasis") ranges.push(emMark.range(node.from, node.to));
        else if (name === "Strikethrough") ranges.push(strikeMark.range(node.from, node.to));
        else if (name === "InlineCode") ranges.push(codeMark.range(node.from, node.to));
        else if (name === "Link") ranges.push(linkMark.range(node.from, node.to));
        else if (name === "URL") {
          // 인라인 링크 [text](url) 의 url 만 숨긴다(앞 문자가 '('). autolink·bare
          // url 은 url 자체가 표시 콘텐츠이므로 보존.
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          if (state.doc.sliceString(node.from - 1, node.from) === "(")
            ranges.push(hideMark.range(node.from, node.to));
        } else if (MARKER_NODES.has(name)) {
          // 마커 숨김 — 단, 커서가 놓인 줄은 소스를 보여준다.
          const ln = state.doc.lineAt(node.from).number;
          if (active.has(ln)) return;
          let end = node.to;
          // 헤딩 마커는 뒤따르는 공백 1칸까지 함께 숨겨 들여쓰기 잔여를 없앤다.
          if (name === "HeaderMark" && state.doc.sliceString(end, end + 1) === " ") end += 1;
          if (node.from < end) ranges.push(hideMark.range(node.from, end));
        }
      },
    });
  }

  // 정렬 위임(sort=true) — line/mark/replace 가 섞여도 안전하게 정렬된다.
  return Decoration.set(ranges, true);
}

/** 라이브 프리뷰 ViewPlugin — 데코를 보유하고 변경 시 재계산. */
const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || u.selectionSet) {
        this.decorations = buildDecorations(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

/** 라이브 프리뷰 타이포 — 토큰 기반(테마 추종). */
const lpTheme = EditorView.theme({
  ".cm-lp-h1": { fontSize: "var(--font-size-h1)", fontWeight: "var(--font-weight-semibold)", lineHeight: "1.3" },
  ".cm-lp-h2": { fontSize: "var(--font-size-h2)", fontWeight: "var(--font-weight-semibold)", lineHeight: "1.3" },
  ".cm-lp-h3": { fontSize: "var(--font-size-h3)", fontWeight: "var(--font-weight-semibold)", lineHeight: "1.35" },
  ".cm-lp-h4": { fontWeight: "var(--font-weight-semibold)" },
  ".cm-lp-h5": { fontWeight: "var(--font-weight-semibold)", color: "var(--text-muted)" },
  ".cm-lp-h6": { fontWeight: "var(--font-weight-semibold)", color: "var(--text-muted)" },
  ".cm-lp-strong": { fontWeight: "700" },
  ".cm-lp-em": { fontStyle: "italic" },
  ".cm-lp-strike": { textDecoration: "line-through", color: "var(--text-muted)" },
  ".cm-lp-code": {
    fontFamily: "var(--font-monospace)",
    fontSize: "0.9em",
    background: "var(--bg-secondary-alt)",
    padding: "0.1em 0.35em",
    borderRadius: "var(--radius-s)",
  },
  ".cm-lp-link": { color: "var(--accent)", textDecoration: "underline", cursor: "pointer" },
  ".cm-lp-quote": {
    borderLeft: "3px solid var(--border-strong)",
    paddingLeft: "var(--sp-3)",
    color: "var(--text-muted)",
  },
  ".cm-lp-checkbox": { cursor: "pointer", marginRight: "0.4em", verticalAlign: "middle" },
  ".cm-lp-hr": {
    display: "inline-block",
    width: "100%",
    borderTop: "1px solid var(--border-strong)",
    verticalAlign: "middle",
  },
});

/** 에디터에 추가할 라이브 프리뷰 확장 묶음. */
export const livePreview = [livePreviewPlugin, lpTheme];
