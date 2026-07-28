<script>
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, from, to } = $props();

  let data = $state(null);

  $effect(() => {
    data = null;
    api("/companies/" + companyId + "/reports/saldobalanse?from=" + from + "&to=" + to)
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Konto</th><th>Navn</th>
        <th class="text-right">Inngående</th><th class="text-right">Debet</th>
        <th class="text-right">Kredit</th><th class="text-right">Utgående</th>
      </tr>
    </thead>
    <tbody>
      {#each data.accounts as a (a.number)}
        <tr>
          <td>{a.number}</td>
          <td>{a.name}</td>
          <td class="text-right">{kr(a.inngaende_ore)}</td>
          <td class="text-right">{kr(a.debet_ore)}</td>
          <td class="text-right">{kr(a.kredit_ore)}</td>
          <td class="text-right">{kr(a.utgaende_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
