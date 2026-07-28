<script>
  // Dimensjonsvelger (#37). To bruk, én komponent:
  //
  // - POSTERING (bilag, faktura): bare AKTIVE dimensjoner. En avsluttet
  //   dimensjon avviser nye posteringer, så den skal ikke kunne velges.
  // - RAPPORTFILTER: også avsluttede, merket «(avsluttet)». Historikken
  //   deres finnes, og et regnskap må kunne leses per avdeling selv om
  //   avdelingen er lagt ned.
  //
  // Rendrer ingenting når registeret ikke har dimensjoner av slaget.
  let {
    dims,
    kind,
    cls = "select select-bordered flex-1",
    inkluderAvsluttede = false,
    alleLabel = null,
    value = $bindable(""),
  } = $props();

  let valgbare = $derived(
    dims.filter((d) => d.kind === kind && (inkluderAvsluttede || d.active)),
  );
</script>

{#if valgbare.length}
  <select class={cls} title={kind} bind:value>
    <option value="">{alleLabel || "(" + kind + ")"}</option>
    {#each valgbare as d (d.code)}
      <option value={d.code}>{d.code} {d.name}{d.active ? "" : " (avsluttet)"}</option>
    {/each}
  </select>
{/if}
