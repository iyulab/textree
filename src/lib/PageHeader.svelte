<script lang="ts">
  /*
   * Pretty-by-default page header (P1) — renders the frontmatter title and a text/emoji icon
   * above the editor body. Image icons and cover banners require the Tauri asset protocol
   * (a new capability, proposed separately); until then a path-like icon is not rendered as
   * raw text (which would be ugly) — it is simply skipped. The frontmatter source stays intact
   * in the `.md`; this is a derived, read-only presentation.
   */
  let { icon = "", title = "" }: { icon?: string; title?: string } = $props();

  // Render the icon as text only when it is not a file path / image reference. Image icons are
  // deferred until the asset protocol lands; rendering their raw path here would defeat the
  // "pretty" intent.
  const IMAGE_EXT = /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i;
  let textIcon = $derived(
    icon && !/[\\/]/.test(icon) && !IMAGE_EXT.test(icon) ? icon : "",
  );
</script>

{#if textIcon || title}
  <header class="page-header">
    {#if textIcon}
      <div class="page-icon" aria-hidden="true">{textIcon}</div>
    {/if}
    {#if title}
      <h1 class="page-title">{title}</h1>
    {/if}
  </header>
{/if}

<style>
  .page-header {
    max-width: var(--content-max-width);
    width: 100%;
    margin: 0 auto;
    box-sizing: border-box;
    padding: var(--sp-6) var(--sp-6) 0;
  }
  .page-icon {
    font-size: 3rem;
    line-height: 1.1;
    margin-bottom: var(--sp-2);
  }
  .page-title {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-bold);
    line-height: var(--line-height-tight);
    letter-spacing: -0.01em;
    color: var(--text-normal);
    word-break: break-word;
  }
</style>
