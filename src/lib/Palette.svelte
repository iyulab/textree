<script lang="ts">
  import { palette } from "./paletteStore.svelte";
  import { nav } from "./nav.svelte";
  import { fuzzyMatch, type Match } from "./fuzzy";
  import type { Command } from "./commands";
  import type { SearchHit } from "./ipc";

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
  }: {
    files: FileEntry[];
    commands: Command[];
    onOpenFile: (path: string) => void;
    onRunCommand: (cmd: Command) => void;
    onSearchContent: (query: string) => Promise<SearchHit[]>;
  } = $props();

  let fileMatches = $derived<Match<FileEntry>[]>(
    palette.mode !== "file"
      ? []
      : palette.term === ""
        ? nav.recent
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

  $effect(() => {
    if (palette.mode !== "content") {
      contentHits = [];
      return;
    }
    const term = palette.term;
    if (term === "") {
      contentHits = [];
      return;
    }
    const seq = ++searchSeq;
    const t = setTimeout(() => {
      void onSearchContent(term).then((hits) => {
        if (seq === searchSeq) contentHits = hits; // apply only the latest request
      });
    }, 120);
    return () => clearTimeout(t);
  });

  let count = $derived(
    palette.mode === "file"
      ? fileMatches.length
      : palette.mode === "command"
        ? cmdMatches.length
        : contentHits.length,
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
          ? "명령 실행…"
          : palette.mode === "content"
            ? "본문 검색…"
            : "파일 검색…  ('>'=명령, '/'=본문)"}
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
                {#each highlight(m.item.name, m.ranges) as seg}<span class:hl={seg.on}>{seg.t}</span
                  >{/each}
              </span>
              <span class="sub">{m.item.path}</span>
            </li>
          {/each}
        {:else if palette.mode === "command"}
          {#each cmdMatches as m, i (m.item.id)}
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
                {#each highlight(m.item.title, m.ranges) as seg}<span class:hl={seg.on}>{seg.t}</span
                  >{/each}
              </span>
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
  .title { color: var(--text-normal); }
  /* --accent: match highlight */
  .hl { color: var(--accent); font-weight: 600; }
  /* --text-muted: secondary path text */
  .sub { color: var(--text-muted); font-size: 0.8rem; }
  .snippet { color: var(--text-normal); font-size: 0.85rem; }
</style>
