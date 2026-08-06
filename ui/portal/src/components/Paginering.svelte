<script>
  // Client-side paging control (daisyUI join). The lists it pages are
  // year-scoped and already in memory — this bounds the DOM, not the
  // fetch; server-side paging is a later step if volume ever demands it.
  let { side = $bindable(1), antall, perSide } = $props();

  let sider = $derived(Math.max(1, Math.ceil(antall / perSide)));

  $effect(() => {
    if (side > sider) side = sider;
    if (side < 1) side = 1;
  });
</script>

{#if sider > 1}
  <div class="join">
    <button class="btn btn-xs join-item" disabled={side <= 1} onclick={() => (side -= 1)}>«</button>
    <span class="btn btn-xs join-item pointer-events-none">Side {side} av {sider}</span>
    <button class="btn btn-xs join-item" disabled={side >= sider} onclick={() => (side += 1)}>
      »
    </button>
  </div>
{/if}
