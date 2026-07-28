<script>
  // Balansen har BEVISST ingen dimensjonsfilter — bare resultatet (#37).
  // Udisponert resultat holder differansen på null.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, to } = $props();

  let data = $state(null);

  $effect(() => {
    data = null;
    api("/companies/" + companyId + "/reports/balanse?date=" + to)
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <table class="table table-sm">
    <tbody>
      <SeksjonRader seksjon={data.eiendeler} />
      <SeksjonRader seksjon={data.egenkapital_gjeld} />
      <tr>
        <td></td><td>Udisponert resultat</td>
        <td class="text-right">{kr(data.udisponert_resultat_ore)}</td>
      </tr>
      <tr class="font-bold">
        <td></td><td>Differanse (skal være 0)</td>
        <td class="text-right">{kr(data.differanse_ore)}</td>
      </tr>
    </tbody>
  </table>
{/if}
