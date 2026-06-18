# Textree

> Your own local Notion, living directly on the filesystem.

**Textree** mirrors the Notion experience, but as a **local-only desktop app** instead of a SaaS.
There is no separate database — **the local filesystem itself is the single source of truth**.
The tree view on the left maps to your folder structure, and the editor on the right syncs **bidirectionally, 1:1** with the underlying markdown files.

Your notes are not trapped in some cloud table. They sit right on your disk, as ordinary `.md` files.

---

## Why Textree

**Text** — pure, unprocessed text. Not bound to any database; an `.md` file that opens in Notepad even after you delete the app.
**Tree** — mirrors your filesystem's folder/directory tree. Scattered notes grow into a single tree of knowledge.

True to its name, Textree is **plain text on top of a tree-structured filesystem**.

---

## Core Philosophy

1. **The filesystem is the database.**
   No hidden DB, no index files, no proprietary format. Folders are folders, notes are `.md` files.
2. **Full ownership.**
   Delete the app and your data remains. Open it with VS Code, Obsidian, `vim`, or `cat` — it just reads.
3. **Offline-first, privacy-first.**
   Works without a network; your data never leaves your machine.
4. **Bidirectional live sync.**
   Edit a file in an external editor and the app reflects it instantly; edit in the app and it writes to disk instantly. On conflict, **the filesystem always wins.**
5. **No lock-in.**
   You can pick up your folder and leave at any time.

---

## Filesystem Mapping Model

Every Textree operation reduces to a single rule: **a tree node is a filesystem entry.**

| Textree concept       | Filesystem counterpart                  |
| --------------------- | --------------------------------------- |
| Workspace (Vault)     | A root folder you choose                 |
| Note without children | A `note.md` file                         |
| Note with children    | A `note/` folder + a `note.md` inside it |
| Sub-note              | An `.md` file / subfolder inside the parent folder |
| Attachment (images…)  | Stored in the same folder + linked via a relative markdown path |
| Tree sort order       | `.textree/order.json` (optional metadata) |

### Example

```
my-vault/
├── project.md                  ← leaf note (no children)
├── journal/                     ← note with children
│   ├── journal.md               ← body of the "journal" note
│   ├── 2026-06-13.md
│   └── 2026-06-12.md
└── library/
    ├── library.md
    ├── meeting-notes.md
    └── assets/
        └── diagram.png          ← referenced from meeting-notes.md via ![](assets/diagram.png)
```

> **Design decision — the "folder note" pattern**
> In Notion, every page can have *both* body content *and* sub-pages.
> A pure filesystem has no "node that has both content and children", so
> Textree expresses this with the **`folder-name/folder-name.md`** convention.
> Leaf notes stay as plain `.md` files, keeping 100% compatibility with ordinary markdown tools.
> (An `index.md` convention is also possible — selectable in settings, planned.)

---

## Features

### Done — MVP + design foundation + local search

- [x] Open / switch workspace (root folder)
- [x] Real-time bidirectional sync between folders and the tree view (file watcher)
- [x] Markdown editor (CodeMirror 6 live preview)
- [x] Create / rename / move / delete / promote notes → instantly reflected on the filesystem
- [x] Drag-and-drop tree reordering
- [x] Paste an image → stored locally + linked via relative path
- [x] Full-text search (local index, no DB) and a unified command palette / quick switcher
- [x] Dark/light theme, keyboard navigation, accessibility (ARIA)

### Done — pretty-by-default

- [x] **Frontmatter page header** — `title` + emoji `icon` render as a tidy page header above the editor; the raw YAML folds into a compact "properties" pill while you edit (the source `.md` is never rewritten)
- [x] **Opinionated typography** — a modular heading scale (h1–h6) with spacing-token vertical rhythm, so an unstyled note looks tidy by default
- [x] **Reading view** — a one-toggle, clean read-only render (all markdown markers hidden, frontmatter folded)
- [x] **Favorites** — star a note straight from the tree; favorites surface in the tree and at the top of the command palette

> Cover banners and image (non-emoji) icons are planned — they need the Tauri asset protocol to display local files.

### Done — wikilinks & Obsidian interoperability

- [x] **Wikilinks** — `[[note]]`, aliases `[[note|label]]`, headings `[[note#heading]]`, block anchors `[[note#^id]]` (Obsidian-compatible syntax); rendered in live preview, click to navigate, with `[[` autocomplete
- [x] **Backlinks** — a panel listing every note that links to the current one
- [x] **Obsidian vault interoperability** — open a standard `.md` vault as-is; `.obsidian/` and `.canvas` files are left untouched, and editing is byte-lossless (CRLF line endings are preserved), so two apps can take turns on the same vault
- [x] **Sync-folder safety** — atomic writes stage in `.textree/tmp/` (out of your content folders, swept on open) so a sync client isn't churned by transient files; and conflicted copies a sync tool leaves behind (OneDrive / Dropbox / Syncthing) are surfaced non-destructively — nothing is changed or deleted, so a divergent edit isn't silently lost

### In progress — frictionless publish

- [x] **Publishing renderer** ([canopy](https://github.com/iyulab/canopy), a separate MIT tool) — `npx canopy build <vault>` turns your tree into a deployable static site today (self-host on GitHub / Cloudflare Pages)
- [ ] **One-click in-app publish** — a "Publish site…" command (renders via canopy, injecting the app's theme) is wired and verified in development; bundling the renderer into packaged builds for offline one-click publish is in progress

### Done — frontmatter table (read-only)

- [x] **Frontmatter table view** — select a folder to see its child notes as a read-only table: frontmatter keys become sortable columns, one row per note (folder = database, `.md` = row). Built in memory from the notes themselves — no separate database

### Next — free local AI

- [ ] **Free local AI + bring-your-own API key** — a local model by default (graceful degradation: editing/tree/search work without it), cloud elevation with your own key

### Later

- [ ] Tree-topology AI (amplification layer) — search/write scoped by where you are in the tree
- [ ] frontmatter database — saved/custom views, filters, board & calendar, inline cell editing
- [ ] Slash commands / rich block editing

---

## How Textree Differs from Notion

| Aspect          | Notion              | Textree                          |
| --------------- | ------------------- | -------------------------------- |
| Delivery        | Cloud SaaS          | Local desktop app                |
| Storage         | Proprietary DB (server) | Local filesystem (`.md`)     |
| Offline         | Limited             | Fully supported                  |
| Data ownership  | Vendor-locked       | 100% owned by you                |
| Format          | Proprietary / needs export | Standard markdown, portable as-is |
| Collaboration   | Real-time multi-user | Single user (work around with Git, etc.) |
| External tools  | Low compatibility   | Compatible with every markdown tool |

> Textree trades collaboration for **ownership, portability, and transparency** — a clear positioning of "Notion, just for me."

---

## Architecture

- **Shell:** [Tauri 2](https://tauri.app) — Rust backend + webview. Lightweight (~10MB-class install), fast file I/O.
- **Frontend:** Svelte 5 + TypeScript + Vite
- **Editor core:** [CodeMirror 6](https://codemirror.net) — live preview decorations
- **File watching:** Rust `notify` (+ debouncer)
- **Search:** Rust `tantivy` local index — no external DB required
- **Sidecar metadata:** `.textree/` (order.json, favorites.json, etc.) — travels with the vault

---

## Getting Started

```bash
git clone https://github.com/iyulab/textree.git
cd textree
npm install
npm run tauri dev      # development run
npm run tauri build    # production build
```

Requirements: Node 24+, Rust (stable). Windows/macOS/Linux desktop.

---

## License

MIT

---

*Textree — your notes, on your disk, as plain files.*
