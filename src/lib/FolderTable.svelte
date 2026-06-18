<script lang="ts">
  import { sortRows, type FolderTable, type SortDir } from "./folderTable.helpers";

  let {
    table,
    onOpen,
  }: {
    /** The folder's notes as a table model (columns = frontmatter keys, rows = notes). */
    table: FolderTable;
    /** Open a row's note. */
    onOpen: (path: string) => void;
  } = $props();

  // Ephemeral sort state (not persisted — no view-definition format yet). `null` = the Name column;
  // a string = a frontmatter field; `undefined` = unsorted (natural tree order).
  let sortKey = $state<string | null | undefined>(undefined);
  let sortDir = $state<SortDir>("asc");

  let sortedRows = $derived(
    sortKey === undefined ? table.rows : sortRows(table.rows, sortKey, sortDir),
  );

  function toggleSort(key: string | null): void {
    if (sortKey === key) sortDir = sortDir === "asc" ? "desc" : "asc";
    else {
      sortKey = key;
      sortDir = "asc";
    }
  }

  const ariaSort = (key: string | null): "ascending" | "descending" | "none" =>
    sortKey === key ? (sortDir === "asc" ? "ascending" : "descending") : "none";
  const arrow = (key: string | null): string =>
    sortKey === key ? (sortDir === "asc" ? " ▲" : " ▼") : "";
</script>

{#if table.rows.length}
  <section class="folder-table" aria-label="Folder table">
    <table>
      <thead>
        <tr>
          <th scope="col" aria-sort={ariaSort(null)}>
            <button class="sort" onclick={() => toggleSort(null)}>Name{arrow(null)}</button>
          </th>
          {#each table.columns as col (col)}
            <th scope="col" aria-sort={ariaSort(col)}>
              <button class="sort" onclick={() => toggleSort(col)}>{col}{arrow(col)}</button>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each sortedRows as row (row.path)}
          <tr>
            <td>
              <button class="row-open" onclick={() => onOpen(row.path)} title={row.path}>
                {row.name}
              </button>
            </td>
            {#each table.columns as col (col)}
              <td>{row.fields[col] ?? ""}</td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

<style>
  .folder-table {
    overflow: auto;
    padding: var(--sp-4) var(--sp-6);
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--font-size-small);
  }
  th,
  td {
    text-align: left;
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
    vertical-align: top;
    white-space: nowrap;
  }
  td {
    color: var(--text-normal);
  }
  .sort {
    border: none;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-weight: var(--font-weight-bold);
    cursor: pointer;
    padding: 0;
  }
  .sort:hover {
    color: var(--text-normal);
  }
  .row-open {
    border: none;
    background: none;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
    padding: 0;
  }
  .row-open:hover {
    text-decoration: underline;
  }
</style>
