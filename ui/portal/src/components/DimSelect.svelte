<script>
  // Dimensjonsvelger (#37): bare aktive dimensjoner av gitt slag;
  // rendrer ingenting når registeret ikke har noen.
  let { dims, kind, cls = "select select-bordered flex-1", value = $bindable("") } = $props();

  let active = $derived(dims.filter((d) => d.kind === kind && d.active));
</script>

{#if active.length}
  <select class={cls} title={kind} bind:value>
    <option value="">({kind})</option>
    {#each active as d (d.code)}
      <option value={d.code}>{d.code} {d.name}</option>
    {/each}
  </select>
{/if}
