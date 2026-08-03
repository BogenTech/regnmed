<script>
  // Resultatregnskapet er den ENESTE rapporten med dimensjonsfilter
  // (#37): avdeling/prosjekt filtrerer en periode med bevegelser.
  // Balansen har det bevisst ikke — en beholdning per dato lar seg
  // ikke meningsfullt splittes på dimensjon.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, from, to } = $props();

  let avdeling = $state("");
  let prosjekt = $state("");
  let dims = $state([]);
  let data = $state(null);

  $effect(() => {
    const query =
      (avdeling ? "&avdeling=" + encodeURIComponent(avdeling) : "") +
      (prosjekt ? "&prosjekt=" + encodeURIComponent(prosjekt) : "");
    data = null;
    Promise.all([
      api("/companies/" + companyId + "/reports/resultat?from=" + from + "&to=" + to + query),
      api("/companies/" + companyId + "/dimensions").catch(() => ({ dimensions: [] })),
    ])
      .then(([r, d]) => {
        data = r;
        dims = d.dimensions;
      })
      .catch((error) => toast(error.message, false));
  });
</script>

{#if dims.length}
  <div class="flex gap-2 mb-3">
    <!-- Rapportfilter: en nedlagt avdeling har fortsatt historikk, så
         den må kunne velges her selv om den ikke kan posteres på. -->
    <DimSelect
      {dims}
      kind="avdeling"
      cls="select select-sm"
      inkluderAvsluttede
      alleLabel="Alle avdelinger"
      bind:value={avdeling}
    />
    <DimSelect
      {dims}
      kind="prosjekt"
      cls="select select-sm"
      inkluderAvsluttede
      alleLabel="Alle prosjekter"
      bind:value={prosjekt}
    />
  </div>
{/if}

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <table class="table table-sm">
    <tbody>
      {#each data.seksjoner as s (s.heading)}
        <SeksjonRader seksjon={s} />
      {/each}
      <tr class="font-bold">
        <td></td><td>Driftsresultat</td>
        <td class="text-right">{kr(data.driftsresultat_ore)}</td>
      </tr>
      <tr class="font-bold">
        <td></td><td>Årsresultat</td>
        <td class="text-right">{kr(data.arsresultat_ore)}</td>
      </tr>
    </tbody>
  </table>
{/if}
