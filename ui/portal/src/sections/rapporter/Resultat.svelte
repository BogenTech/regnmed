<script>
  // Resultatregnskapet er den ENESTE rapporten med dimensjonsfilter
  // (#37): avdeling/prosjekt filtrerer en periode med bevegelser.
  // Balansen har det bevisst ikke — en beholdning per dato lar seg
  // ikke meningsfullt splittes på dimensjon. Filtrene står i adressen.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, from, to, params } = $props();

  let dims = $state([]);
  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  $effect(() => {
    api("/companies/" + companyId + "/dimensions")
      .then((d) => (dims = d.dimensions))
      .catch(() => (dims = []));
  });

  function load() {
    const mine = ++seq;
    const filter =
      (params.avdeling ? "&avdeling=" + encodeURIComponent(params.avdeling) : "") +
      (params.prosjekt ? "&prosjekt=" + encodeURIComponent(params.prosjekt) : "");
    data = null;
    error = null;
    api("/companies/" + companyId + "/reports/resultat?from=" + from + "&to=" + to + filter)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);
</script>

{#if dims.length}
  <div class="flex gap-2 flex-wrap mb-3">
    <!-- Rapportfilter: en nedlagt avdeling har fortsatt historikk, så
         den må kunne velges her selv om den ikke kan posteres på. -->
    <DimSelect {dims} kind="avdeling" label="Avdeling" cls="select select-sm" inkluderAvsluttede alleLabel="Alle avdelinger" bind:value={() => params.avdeling || "", (v) => (params.avdeling = v || undefined)} />
    <DimSelect {dims} kind="prosjekt" label="Prosjekt" cls="select select-sm" inkluderAvsluttede alleLabel="Alle prosjekter" bind:value={() => params.prosjekt || "", (v) => (params.prosjekt = v || undefined)} />
  </div>
{/if}

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter resultatregnskapet"></span>
{:else}
  <table class="table table-sm">
    <caption class="sr-only">Resultatregnskap, beløp i kroner.</caption>
    <tbody>
      {#each data.seksjoner as s (s.heading)}
        <SeksjonRader seksjon={s} />
      {/each}
      <tr class="font-bold">
        <td></td><td>Driftsresultat</td>
        <td class="text-right tabular-nums whitespace-nowrap">{kr(data.driftsresultat_ore)}</td>
      </tr>
      <tr class="font-bold">
        <td></td><td>Årsresultat</td>
        <td class="text-right tabular-nums whitespace-nowrap">{kr(data.arsresultat_ore)}</td>
      </tr>
    </tbody>
  </table>
{/if}
