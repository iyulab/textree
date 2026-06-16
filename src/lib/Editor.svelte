<script lang="ts">
  import { Compartment, EditorState } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { GFM } from "@lezer/markdown";
  import { livePreview, readingMode } from "./livePreview";
  import { parseFrontmatter } from "./frontmatter.helpers";
  import { untrack } from "svelte";

  let {
    docKey = null,
    initialDoc = "",
    editable = true,
    reading = false,
    onchange,
    onImagePaste,
  }: {
    /** Identity (path) of the open note. The view is recreated only when this value changes. */
    docKey?: string | null;
    /** Body injected once on note load. Not recreated even when changed by keystrokes. */
    initialDoc?: string;
    editable?: boolean;
    /** Reading mode: clean read-only render (all markers hidden, frontmatter folded). */
    reading?: boolean;
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
    onPaste?: (b64: string, ext: string) => Promise<string | null>,
  ): boolean {
    // Read the live read-only state (not a captured flag) so reading mode reliably disables paste
    // even though the view is reconfigured in place rather than recreated.
    if (view.state.readOnly || !onPaste) return false;
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

  // Compartment for the mode-dependent extensions (reading flag + read-only + editable). Reconfigured
  // in place when `reading`/`editable` change so the view is never recreated — preserving the live
  // document and undo history (recreating with the load-time initialDoc would drop unsaved edits).
  const modeConf = new Compartment();
  function modeExtensions(ed: boolean, rd: boolean) {
    return [
      readingMode.of(rd),
      EditorState.readOnly.of(!ed),
      EditorView.editable.of(ed),
    ];
  }

  function makeState(
    doc: string,
    ed: boolean,
    rd: boolean,
    emit?: (t: string) => void,
    onPaste?: (b64: string, ext: string) => Promise<string | null>,
  ) {
    // Start the cursor at the body (after any frontmatter) so the frontmatter folds by default
    // (a cursor inside the block keeps it expanded). Notes without frontmatter start at offset 0.
    const fmEnd = parseFrontmatter(doc).end;
    return EditorState.create({
      doc,
      selection: fmEnd > 0 ? { anchor: fmEnd } : undefined,
      extensions: [
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        // Enable GFM extensions (strikethrough, tables, task lists, autolink) so live preview
        // recognizes ~~strikethrough~~, - [ ] checkboxes, etc.
        markdown({ extensions: GFM }),
        livePreview,
        textreeTheme,
        EditorView.lineWrapping,
        modeConf.of(modeExtensions(ed, rd)),
        EditorView.domEventHandlers({
          paste: (event, view) => handlePaste(event, view, onPaste),
        }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) emit?.(u.state.doc.toString());
        }),
      ],
    });
  }

  let view: EditorView | undefined;

  // Create the view on note switch (docKey). Mode/editable are read untracked and applied via the
  // compartment effect below, so toggling reading does not re-run this effect (no recreation).
  // initialDoc/onchange/onImagePaste are read via untrack so keystroke-driven prop changes don't
  // wipe the cursor and undo history.
  $effect(() => {
    void docKey;
    const rd = untrack(() => reading);
    const ed = untrack(() => editable) && !rd;
    const doc = untrack(() => initialDoc);
    const emit = untrack(() => onchange);
    const onPaste = untrack(() => onImagePaste);
    const v = new EditorView({ state: makeState(doc, ed, rd, emit, onPaste), parent: host });
    view = v;
    return () => {
      v.destroy();
      if (view === v) view = undefined;
    };
  });

  // Reconfigure the mode compartment on the existing view when reading/editable change — no
  // recreation, so the document and undo history are preserved across the toggle.
  $effect(() => {
    const rd = reading;
    const ed = editable && !rd;
    view?.dispatch({ effects: modeConf.reconfigure(modeExtensions(ed, rd)) });
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: auto;
  }
</style>
