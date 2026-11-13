<script>
  // Balansen har BEVISST ingen dimensjonsfilter — bare resultatet (#37).
  // Udisponert resultat holder differansen på null; er den ikke null,
  // er det en feil som skal sies høyt, ikke en linje å lese forbi.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import LoadError from "../../components/LoadError.svelte";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, to } = $props();

  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  function load() {
    const mine = ++seq;
    data = null;
    error = null;
    api("/companies/" + companyId + "/reports/balanse?date=" + to)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);
</script>

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter balansen"></span>
{:else}
  {#if data.differanse_ore}
    <div role="alert" class="alert alert-error mb-3">
      Balansen går ikke opp — differansen er {kr(data.differanse_ore)} kr. Kjør
      verifikasjonsrapporten under Revisjon for å finne årsaken.
    </div>
  {/if}
  <table class="table table-sm">
    <caption class="sr-only">Balanse, beløp i kroner.</caption>
    <tbody>
      <SeksjonRader seksjon={data.eiendeler} />
      <SeksjonRader seksjon={data.egenkapital_gjeld} />
      <tr>
        <td></td><td>Udisponert resultat</td>
        <td class="text-right tabular-nums whitespace-nowrap">{kr(data.udisponert_resultat_ore)}</td>
      </tr>
    </tbody>
  </table>
  {#if !data.differanse_ore}
    <p class="text-sm text-success mt-2">✓ I balanse</p>
  {/if}
{/if}
