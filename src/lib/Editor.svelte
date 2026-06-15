<script lang="ts">
  import { EditorState } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { GFM } from "@lezer/markdown";
  import { livePreview } from "./livePreview";
  import { untrack } from "svelte";

  let {
    docKey = null,
    initialDoc = "",
    editable = true,
    onchange,
    onImagePaste,
  }: {
    /** 열린 노트의 정체성(경로). 이 값이 바뀔 때만 뷰를 재생성한다. */
    docKey?: string | null;
    /** 노트 로드 시 1회 주입되는 본문. 키입력으로 바뀌어도 재생성 안 함. */
    initialDoc?: string;
    editable?: boolean;
    /** 사용자 편집으로 문서가 바뀔 때 새 텍스트를 방출. */
    onchange?: (text: string) => void;
    /** 이미지 붙여넣기: base64 바이트+확장자를 받아 삽입할 마크다운 링크를 반환(실패 시 null). */
    onImagePaste?: (dataBase64: string, ext: string) => Promise<string | null>;
  } = $props();

  let host: HTMLDivElement;

  // 지원 이미지 MIME → 확장자. 백엔드(fs_ops IMAGE_EXTS) 화이트리스트를 미러링.
  // 여기에 없는 타입은 가로채지 않고 기본 붙여넣기로 통과시킨다(삼킴 방지).
  const MIME_EXT: Record<string, string> = {
    "image/png": "png",
    "image/jpeg": "jpeg",
    "image/jpg": "jpg",
    "image/gif": "gif",
    "image/webp": "webp",
    "image/svg+xml": "svg",
    "image/bmp": "bmp",
    "image/avif": "avif",
  };

  /** Uint8Array → 표준 base64(Rust STANDARD 엔진과 호환). 대용량은 청크로 처리해
   *  String.fromCharCode 스택 한계를 피한다. */
  function bytesToBase64(bytes: Uint8Array): string {
    let bin = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(bin);
  }

  /** 붙여넣기 핸들러: 클립보드에 이미지가 있으면 가로채 저장→링크 삽입. 그 외엔 기본 동작. */
  function handlePaste(
    event: ClipboardEvent,
    view: EditorView,
    ed: boolean,
    onPaste?: (b64: string, ext: string) => Promise<string | null>,
  ): boolean {
    if (!ed || !onPaste) return false;
    const items = event.clipboardData?.items;
    if (!items) return false;
    for (const item of items) {
      const ext = MIME_EXT[item.type];
      if (!ext) continue; // 미지원 타입(또는 비이미지)은 가로채지 않고 기본 동작 통과
      const file = item.getAsFile();
      if (!file) continue;
      event.preventDefault(); // 지원 이미지만 차단하고 우리가 처리
      void (async () => {
        try {
          const buf = await file.arrayBuffer();
          const link = await onPaste(bytesToBase64(new Uint8Array(buf)), ext);
          if (link) view.dispatch(view.state.replaceSelection(link));
        } catch {
          // 저장 도중 노트 전환으로 뷰가 파괴됐거나 읽기 실패 — 미처리 rejection 방지.
          // 첨부 파일은 저장됐을 수 있으나 링크 미삽입은 무해(재시도 가능).
        }
      })();
      return true; // 처리됨
    }
    return false;
  }

  // 앱 디자인 토큰(tokens.css)을 CodeMirror 에 연결하는 테마. 색을 하드코딩하지
  // 않고 var(--token)을 참조하므로 다크/라이트 전환이 에디터에도 그대로 적용된다.
  const textreeTheme = EditorView.theme({
    "&": {
      color: "var(--text-normal)",
      backgroundColor: "var(--bg-primary)",
      fontSize: "var(--font-size-editor)",
      height: "100%",
    },
    ".cm-content": {
      fontFamily: "var(--font-text)",
      lineHeight: "var(--line-height-relaxed)",
      padding: "var(--sp-5) var(--sp-6)",
      caretColor: "var(--text-normal)",
      // 넓은 창에서 줄 길이를 제한하고 중앙 정렬해 가독성 확보(lineNumbers 제거됨).
      maxWidth: "var(--content-max-width)",
      margin: "0 auto",
      width: "100%",
      boxSizing: "border-box",
    },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text-normal)" },
    "&.cm-focused": { outline: "none" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: "var(--selection-bg)" },
    ".cm-activeLine": { backgroundColor: "transparent" },
  });

  function makeState(
    doc: string,
    ed: boolean,
    emit?: (t: string) => void,
    onPaste?: (b64: string, ext: string) => Promise<string | null>,
  ) {
    return EditorState.create({
      doc,
      extensions: [
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        // GFM 확장(취소선·테이블·task list·autolink)을 켜 라이브 프리뷰가
        // ~~취소선~~·- [ ] 체크박스 등을 인식하게 한다.
        markdown({ extensions: GFM }),
        livePreview,
        textreeTheme,
        EditorView.lineWrapping,
        EditorState.readOnly.of(!ed),
        EditorView.editable.of(ed),
        EditorView.domEventHandlers({
          paste: (event, view) => handlePaste(event, view, ed, onPaste),
        }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) emit?.(u.state.doc.toString());
        }),
      ],
    });
  }

  // 노트 전환(docKey) 또는 editable 토글 시에만 뷰를 재생성한다.
  // initialDoc/onchange/onImagePaste는 untrack으로 읽어, 키입력으로 인한 prop 변화가
  // effect를 재실행시켜 커서·실행취소 이력을 날리는 것을 막는다.
  $effect(() => {
    void docKey;
    const ed = editable;
    const doc = untrack(() => initialDoc);
    const emit = untrack(() => onchange);
    const onPaste = untrack(() => onImagePaste);
    const view = new EditorView({ state: makeState(doc, ed, emit, onPaste), parent: host });
    return () => view.destroy();
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: auto;
  }
</style>
