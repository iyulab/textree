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
    /** Identity (path) of the open note. The view is recreated only when this value changes. */
    docKey?: string | null;
    /** Body injected once on note load. Not recreated even when changed by keystrokes. */
    initialDoc?: string;
    editable?: boolean;
    /** Emits the new text when the document changes due to user editing. */
    onchange?: (text: string) => void;
    /** Image paste: receives base64 bytes + extension and returns the markdown link to insert (null on failure). */
    onImagePaste?: (dataBase64: string, ext: string) => Promise<string | null>;
  } = $props();

  let host: HTMLDivElement;

  // Supported image MIME → extension. Mirrors the backend (fs_ops IMAGE_EXTS) whitelist.
  // Types not listed here are not intercepted and pass through to the default paste (prevents swallowing).
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

  /** Uint8Array → standard base64 (compatible with Rust STANDARD engine). Large data is
   *  processed in chunks to avoid the String.fromCharCode stack limit. */
  function bytesToBase64(bytes: Uint8Array): string {
    let bin = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(bin);
  }

  /** Paste handler: if the clipboard has an image, intercept it, save it, and insert a link. Otherwise default behavior. */
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
      if (!ext) continue; // Unsupported types (or non-images) are not intercepted and pass through to default behavior
      const file = item.getAsFile();
      if (!file) continue;
      event.preventDefault(); // Only block supported images and handle them ourselves
      void (async () => {
        try {
          const buf = await file.arrayBuffer();
          const link = await onPaste(bytesToBase64(new Uint8Array(buf)), ext);
          if (link) view.dispatch(view.state.replaceSelection(link));
        } catch {
          // The view was destroyed by a note switch during save, or a read failed — prevents unhandled rejection.
          // The attachment may have been saved, but not inserting the link is harmless (can retry).
        }
      })();
      return true; // handled
    }
    return false;
  }

  // Theme that connects the app design tokens (tokens.css) to CodeMirror. Colors are not
  // hardcoded but reference var(--token), so dark/light switching applies to the editor as well.
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
      // On wide windows, limit line length and center it for readability (lineNumbers removed).
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
        // Enable GFM extensions (strikethrough, tables, task lists, autolink) so live preview
        // recognizes ~~strikethrough~~, - [ ] checkboxes, etc.
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

  // Recreate the view only on note switch (docKey) or editable toggle.
  // initialDoc/onchange/onImagePaste are read via untrack so that prop changes caused by keystrokes
  // don't re-run the effect and wipe the cursor and undo history.
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
