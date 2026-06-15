<script lang="ts">
  import "$lib/styles/tokens.css";
  import { onMount } from "svelte";
  import { theme } from "$lib/theme.svelte";
  import { layout } from "$lib/layout.svelte";

  let { children } = $props();

  // On app start, load saved app settings (theme, layout). The theme also subscribes to OS preference changes.
  onMount(() => {
    layout.init();
    return theme.init();
  });
</script>

{@render children()}
