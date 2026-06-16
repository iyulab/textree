<script lang="ts">
  import { Compartment, EditorState } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { GFM } from "@lezer/markdown";
  import { livePreview, readingMode, wikiResolver } from "./livePreview";
  import { parseFrontmatter } from "./frontmatter.helpers";
  import { detectLineEnding, normalizeLineEndings } from "./eol.helpers";
  import { buildWikiResolver, findHeadingOffset } from "./wikilink.helpers";
  import { wikiAutocomplete } from "./wikiComplete";
  import { untrack } from "svelte";

  let {
    docKey = null,
    initialDoc = "",
    editable = true,
    reading = false,
    notePaths = [],
    scrollToHeading = null,
    onchange,
    onImagePaste,
    onWikiLink,
  }: {
    /** Identity (path) of the open note. The view is recreated only when this value changes. */
    docKey?: string | null;
    /** Body injected once on note load. Not recreated even when changed by keystrokes. */
    initialDoc?: string;
    editable?: boolean;
    /** Reading mode: clean read-only render (all markers hidden, frontmatter folded). */
    reading?: boolean;
    /** Vault-relative `.md` note paths — wikilink resolution targets. Reconfigured in place on change. */
    notePaths?: string[];
    /** Heading to scroll to once this note loads (from a `[[note#heading]]` click). Null = top. */
    scrollToHeading?: string | null;
    /** Emits the new text when the document changes due to user editing. */
    onchange?: (text: string) => void;
    /** Image paste: receives base64 bytes + extension and returns the markdown link to insert (null on failure). */
    onImagePaste?: (dataBase64: string, ext: string) => Promise<string | null>;
    /** Wikilink navigation: called with the resolved note path (+ optional heading) when a link is clicked. */
    onWikiLink?: (path: string, heading: string | undefined) => void;
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

  /** Move the cursor to a heading within this view and scroll it into view. */
  function scrollToHeadingInView(view: EditorView, heading: string): void {
    const off = findHeadingOffset(view.state.doc.toString(), heading);
    if (off === null) return;
    view.dispatch({
      selection: { anchor: off },
      effects: EditorView.scrollIntoView(off, { y: "start" }),
    });
  }

  /**
   * Wikilink click: navigate to the resolved note. Reads the destination from the widget's
   * `data-wikilink-path` (set by livePreview). A same-document heading link (empty path) scrolls
   * within this view; an unresolved link (no attribute) falls through to default — clicking moves
   * the cursor, revealing the raw source for editing.
   */
  function handleWikiClick(
    event: MouseEvent,
    view: EditorView,
    onLink?: (path: string, heading: string | undefined) => void,
  ): boolean {
    if (!onLink) return false;
    const start = event.target as HTMLElement | null;
    const el = start?.closest?.(".cm-lp-wikilink") as HTMLElement | null;
    if (!el) return false;
    const path = el.dataset.wikilinkPath;
    if (path === undefined) return false; // unresolved → fall through (reveal source)
    event.preventDefault(); // do not place the cursor inside the widget (which would reveal source)
    const heading = el.dataset.wikilinkHeading;
    if (path === "") {
      if (heading) scrollToHeadingInView(view, heading); // same-document heading link
      return true;
    }
    onLink(path, heading);
    return true;
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

  // Compartment for the wikilink resolver — reconfigured in place when the vault tree (notePaths)
  // changes, so links re-resolve without recreating the view (preserving doc + undo history).
  const wikiConf = new Compartment();
  function wikiExtension(paths: string[]) {
    return [wikiResolver.of(buildWikiResolver(paths).resolve), wikiAutocomplete(paths)];
  }

  function makeState(
    doc: string,
    ed: boolean,
    rd: boolean,
    paths: string[],
    emit?: (t: string) => void,
    onPaste?: (b64: string, ext: string) => Promise<string | null>,
    onLink?: (path: string, heading: string | undefined) => void,
  ) {
    // Start the cursor at the body (after any frontmatter) so the frontmatter folds by default
    // (a cursor inside the block keeps it expanded). Notes without frontmatter start at offset 0.
    const fmEnd = parseFrontmatter(doc).end;
    // CodeMirror normalizes every line break to `\n`; re-apply the source's ending on emit so a
    // CRLF note (e.g. a Windows Obsidian vault under git) round-trips byte-for-byte.
    const eol = detectLineEnding(doc);
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
        wikiConf.of(wikiExtension(paths)),
        EditorView.domEventHandlers({
          paste: (event, view) => handlePaste(event, view, onPaste),
          mousedown: (event, view) => handleWikiClick(event, view, onLink),
        }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) emit?.(normalizeLineEndings(u.state.doc.toString(), eol));
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
    const paths = untrack(() => notePaths);
    const emit = untrack(() => onchange);
    const onPaste = untrack(() => onImagePaste);
    const onLink = untrack(() => onWikiLink);
    const v = new EditorView({
      state: makeState(doc, ed, rd, paths, emit, onPaste, onLink),
      parent: host,
    });
    view = v;
    // Scroll to the requested heading once on load (cross-note `[[note#heading]]` navigation).
    const heading = untrack(() => scrollToHeading);
    if (heading) {
      const off = findHeadingOffset(doc, heading);
      if (off !== null)
        v.dispatch({ selection: { anchor: off }, effects: EditorView.scrollIntoView(off, { y: "start" }) });
    }
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

  // Reconfigure the wikilink resolver when the vault tree changes (notes added/renamed/removed), so
  // links re-resolve live without recreating the view.
  $effect(() => {
    const paths = notePaths;
    view?.dispatch({ effects: wikiConf.reconfigure(wikiExtension(paths)) });
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: auto;
  }
</style>
