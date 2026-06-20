<script lang="ts">
  import { palette } from "./paletteStore.svelte";
  import { nav } from "./nav.svelte";
  import { fuzzyMatch, type Match } from "./fuzzy";
  import { formatKeybinding } from "./keybinding.helpers";
  import { paletteListState } from "./palette.helpers";
  import type { Command } from "./commands";
  import { semanticSearch, hostStatus, type SearchHit, type SemanticHit, type HostStatus } from "./ipc";

  interface FileEntry {
    name: string;
    path: string;
    kind: "leaf" | "container";
  }

  let {
    files,
    commands,
    onOpenFile,
    onRunCommand,
    onSearchContent,
    vaultRoot = null,
    scopePath = null,
  }: {
    files: FileEntry[];
    commands: Command[];
    onOpenFile: (path: string) => void;
    onRunCommand: (cmd: Command) => void;
    onSearchContent: (query: string) => Promise<SearchHit[]>;
    vaultRoot?: string | null;
    scopePath?: string | null;
  } = $props();

  let fileMatches = $derived<Match<FileEntry>[]>(
    palette.mode !== "file"
      ? []
      : palette.term === ""
        ? // Empty query: surface favorites first, then recent (excluding ones already favorited).
          [
            ...nav.favorites,
            ...nav.recent.filter((p) => !nav.favorites.includes(p)),
          ]
            .map((p) => files.find((f) => f.path === p))
            .filter((f): f is FileEntry => f !== undefined)
            .map((item) => ({ item, score: 0, ranges: [] as [number, number][] }))
        : fuzzyMatch(palette.term, files, (f) => f.name),
  );
  let cmdMatches = $derived<Match<Command>[]>(
    palette.mode === "command" ? fuzzyMatch(palette.term, commands, (c) => c.title) : [],
  );
  // Content search uses the backend IPC (async) — debounce + apply only the last response (race prevention).
  let contentHits = $state<SearchHit[]>([]);
  let searchSeq = 0;
  // True while a content query is debouncing/in flight — lets the list distinguish "searching"
  // from a settled "no matches" (both would otherwise render zero rows).
  let searching = $state(false);

  $effect(() => {
    if (palette.mode !== "content") {
      contentHits = [];
      searching = false;
      return;
    }
    const term = palette.term;
    if (term === "") {
      contentHits = [];
      searching = false;
      return;
    }
    searching = true;
    const seq = ++searchSeq;
    const t = setTimeout(() => {
      void onSearchContent(term)
        .then((hits) => {
          if (seq === searchSeq) {
            contentHits = hits; // apply only the latest request
            searching = false;
          }
        })
        .catch(() => {
          // On a search failure, clear the in-flight flag so the row doesn't stick on "Searching…".
          if (seq === searchSeq) searching = false;
        });
    }, 120);
    return () => clearTimeout(t);
  });

  // Semantic search — mirrors content search pattern; guarded by hostState and vaultRoot availability.
  let semanticHits = $state<SemanticHit[]>([]);
  let hostState = $state<HostStatus | null>(null);

  $effect(() => {
    if (palette.mode !== "semantic") {
      semanticHits = [];
      searching = false;
      return;
    }
    const term = palette.term;
    if (term === "") {
      semanticHits = [];
      searching = false;
      return;
    }
    // Poll host status once when the user enters semantic mode; refresh on each query change.
    void hostStatus().then((s) => { hostState = s; });
    if (!vaultRoot) {
      searching = false;
      return;
    }
    searching = true;
    const seq = ++searchSeq;
    const vault = vaultRoot;
    const scope = scopePath;
    const t = setTimeout(() => {
      void semanticSearch(vault, term, scope, 20)
        .then((hits) => {
          if (seq === searchSeq) {
            semanticHits = hits;
            searching = false;
          }
        })
        .catch(() => {
          if (seq === searchSeq) {
            semanticHits = [];
            searching = false;
          }
        });
    }, 120);
    return () => clearTimeout(t);
  });

  let count = $derived(
    palette.mode === "file"
      ? fileMatches.length
      : palette.mode === "command"
        ? cmdMatches.length
        : palette.mode === "semantic"
          ? semanticHits.length
          : contentHits.length,
  );
  let listState = $derived(
    paletteListState({ mode: palette.mode, term: palette.term, count, searching }),
  );

  function highlight(text: string, ranges: [number, number][]): { t: string; on: boolean }[] {
    const out: { t: string; on: boolean }[] = [];
    let i = 0;
    for (const [s, e] of ranges) {
      if (s > i) out.push({ t: text.slice(i, s), on: false });
      out.push({ t: text.slice(s, e), on: true });
      i = e;
    }
    if (i < text.length) out.push({ t: text.slice(i), on: false });
    return out;
  }

  function commit(): void {
    if (count === 0) return;
    if (palette.mode === "file") onOpenFile(fileMatches[palette.selected].item.path);
    else if (palette.mode === "command") onRunCommand(cmdMatches[palette.selected].item);
    else if (palette.mode === "semantic") onOpenFile(semanticHits[palette.selected].path);
    else onOpenFile(contentHits[palette.selected].path);
    palette.hide();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      palette.move(1, count);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      palette.move(-1, count);
    } else if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      palette.hide();
    }
  }
</script>

{#if palette.open}
  <div
    class="overlay"
    data-testid="palette-overlay"
    role="presentation"
    onclick={() => palette.hide()}
    onkeydown={() => {}}
  >
    <div
      class="panel"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="input"
        data-testid="palette-input"
        autofocus
        placeholder={palette.mode === "command"
          ? "Run a command…"
          : palette.mode === "content"
            ? "Search content…"
            : palette.mode === "semantic"
              ? "Semantic search…  (scoped to current folder)"
              : "Search files…  ('>' = commands, '/' = content, '?' = semantic)"}
        value={palette.query}
        oninput={(e) => palette.setQuery(e.currentTarget.value)}
        onkeydown={onKey}
      />
      <ul class="results" data-testid="palette-results">
        {#if palette.mode === "file"}
          {#each fileMatches as m, i (m.item.path)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <li
              class="row"
              class:sel={i === palette.selected}
              data-testid="palette-item"
              onmousedown={() => {
                palette.selected = i;
                commit();
              }}
            >
              <span class="title">
                {#if nav.isFavorite(m.item.path)}<span class="fav-mark" aria-hidden="true">★</span
                  >{/if}{#each highlight(m.item.name, m.ranges) as seg}<span class:hl={seg.on}
                    >{seg.t}</span
                  >{/each}
              </span>
              <span class="sub">{m.item.path}</span>
            </li>
          {/each}
        {:else if palette.mode === "command"}
          {#each cmdMatches as m, i (m.item.id)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <li
              class="row cmd"
              class:sel={i === palette.selected}
              data-testid="palette-item"
              onmousedown={() => {
                palette.selected = i;
                commit();
              }}
            >
              <span class="title">
                {#each highlight(m.item.title, m.ranges) as seg}<span class:hl={seg.on}>{seg.t}</span
                  >{/each}
              </span>
              {#if m.item.keybinding}
                <kbd class="kbd">{formatKeybinding(m.item.keybinding)}</kbd>
              {/if}
            </li>
          {/each}
        {:else if palette.mode === "content"}
          {#each contentHits as h, i (h.path)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <li
              class="row"
              class:sel={i === palette.selected}
              data-testid="palette-item"
              onmousedown={() => {
                palette.selected = i;
                commit();
              }}
            >
              <span class="title">{h.title}</span>
              <span class="snippet">
                {#each highlight(h.snippet, h.ranges) as seg}<span class:hl={seg.on}>{seg.t}</span
                  >{/each}
              </span>
              <span class="sub">{h.path}</span>
            </li>
          {/each}
        {:else if palette.mode === "semantic"}
          {#if hostState !== null && hostState !== "ready"}
            <!-- Host not ready: show a calm muted status row instead of results (graceful degradation). -->
            <li class="status-row ai-unavailable" role="status">
              {hostState === "starting" ? "Local AI is indexing…" : "Local AI is unavailable"}
            </li>
          {:else}
            {#each semanticHits as h, i (h.path)}
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <li
                class="row"
                class:sel={i === palette.selected}
                data-testid="palette-item"
                onmousedown={() => {
                  palette.selected = i;
                  commit();
                }}
              >
                <span class="title">{h.path}</span>
                <span class="snippet">{h.snippet}</span>
                <span class="sub">score: {h.score.toFixed(3)}</span>
              </li>
            {/each}
          {/if}
        {/if}
        {#if listState === "searching"}
          <li class="status-row" role="status">Searching…</li>
        {:else if listState === "no-results"}
          <li class="status-row" role="status" data-testid="palette-empty">No matches found</li>
        {/if}
      </ul>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    /* --bg-primary: background-based translucent overlay */
    background: color-mix(in srgb, var(--bg-primary) 60%, transparent);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    z-index: 50;
  }
  .panel {
    width: min(40rem, 92vw);
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    /* --bg-secondary: panel surface */
    background: var(--bg-secondary);
    /* --border: border line */
    border: 1px solid var(--border);
    /* --radius-m: panel radius */
    border-radius: var(--radius-m);
    /* --shadow-m: strongest shadow token */
    box-shadow: var(--shadow-m);
    overflow: hidden;
  }
  .input {
    padding: 0.75rem 1rem;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    /* --text-normal: default text */
    color: var(--text-normal);
    font-size: 1rem;
    outline: none;
  }
  .results {
    margin: 0;
    padding: 0.25rem;
    list-style: none;
    overflow-y: auto;
  }
  .row {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    padding: 0.4rem 0.6rem;
    /* --radius-s: row radius */
    border-radius: var(--radius-s);
    cursor: pointer;
  }
  .row.sel {
    /* --selection-bg: derived from accent alpha (selection highlight) */
    background: var(--selection-bg);
  }
  /* Command rows lay the title and shortcut hint out horizontally (no .sub second line). */
  .row.cmd { flex-direction: row; align-items: center; justify-content: space-between; }
  .title { color: var(--text-normal); }
  /* Keyboard shortcut hint shown next to a command (discoverability). */
  .kbd {
    flex-shrink: 0;
    margin-left: 0.6rem;
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    color: var(--text-muted);
    font-size: 0.72rem;
    font-family: inherit;
    white-space: nowrap;
  }
  /* Favorite marker in the empty-query file list. */
  .fav-mark { color: var(--accent); margin-right: 0.35em; }
  /* --accent: match highlight */
  .hl { color: var(--accent); font-weight: 600; }
  /* --text-muted: secondary path text */
  .sub { color: var(--text-muted); font-size: 0.8rem; }
  .snippet { color: var(--text-normal); font-size: 0.85rem; }
  /* Non-interactive status row (searching / no matches) — muted, not selectable. */
  .status-row {
    padding: 0.4rem 0.6rem;
    color: var(--text-muted);
    font-size: 0.85rem;
    cursor: default;
  }
  /* AI unavailable / indexing — same muted treatment, distinct from error. */
  .ai-unavailable {
    font-style: italic;
  }
</style>
