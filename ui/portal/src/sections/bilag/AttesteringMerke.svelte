<script>
  import { attesteringMerke } from "./attestering.js";

  let { document: doc, policyAktiv } = $props();

  let merke = $derived(attesteringMerke(doc, policyAktiv));
</script>

{#if merke}
  <span class="badge badge-sm {merke.klasse}">{merke.tekst}</span>
  {#if merke.av}<span class="text-xs opacity-60">{merke.av}</span>{/if}
{:else}
  —
{/if}
