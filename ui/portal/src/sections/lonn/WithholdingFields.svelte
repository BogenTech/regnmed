<script>
  // How tax is withheld: from the skattekort, or set by hand.
  let { value = $bindable(), canFetch = false } = $props();
</script>

<div class="grid grid-cols-2 gap-2">
  <label class="flex flex-col gap-1 text-xs">
    Trekk
    <select class="select select-sm" bind:value={value.kind}>
      {#if canFetch}<option value="card">Hent fra skattekort</option>{/if}
      <option value="prosent">Prosent</option>
      <option value="tabell">Tabell</option>
      <option value="ingen">Ingen trekk</option>
    </select>
  </label>
  {#if value.kind === "prosent"}
    <label class="flex flex-col gap-1 text-xs">
      Trekkprosent
      <input class="input input-sm" inputmode="decimal" placeholder="f.eks. 35" bind:value={value.percent} />
      {#if value.percent === "50"}
        <span class="opacity-70">Uten skattekort skal arbeidsgiver trekke 50 %</span>
      {/if}
    </label>
  {:else if value.kind === "tabell"}
    <label class="flex flex-col gap-1 text-xs">
      Tabellnummer
      <input class="input input-sm" inputmode="numeric" placeholder="f.eks. 8010" bind:value={value.table} />
    </label>
  {:else if value.kind === "card"}
    <p class="text-xs opacity-70 self-end">Trekker 50 % til kortet er hentet.</p>
  {/if}
</div>
