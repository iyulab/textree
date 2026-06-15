<script lang="ts">
  import "$lib/styles/tokens.css";
  import { onMount } from "svelte";
  import { theme } from "$lib/theme.svelte";
  import { layout } from "$lib/layout.svelte";

  let { children } = $props();

  // 앱 시작 시 저장된 앱 설정(테마·레이아웃)을 로드. 테마는 OS 선호 변화도 구독.
  onMount(() => {
    layout.init();
    return theme.init();
  });
</script>

{@render children()}
