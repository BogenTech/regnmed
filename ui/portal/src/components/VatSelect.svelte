<script>
  // Mva-kodevelgeren — standardkodene med dagens satser; med
  // produktvalg kan koden stå tom («mva fra produkt»); med uten kan et
  // produkt stå uten mva-kode i det hele tatt.
  let { cls = "select", produktvalg = false, uten = false, value = $bindable("3") } = $props();
</script>

<select class={cls} bind:value>
  {#if produktvalg}
    <option value="">mva fra produkt</option>
  {:else if uten}
    <option value="">ingen mva-kode</option>
  {/if}
  <option value="3">3 — 25 %</option>
  <option value="31">31 — 15 %</option>
  <option value="33">33 — 12 %</option>
  <option value="5">5 — fritatt</option>
  <option value="6">6 — utenfor</option>
</select>
