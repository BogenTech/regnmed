<script>
  // Kontospesifikasjon (#4): løpende saldo per konto med
  // bilagshenvisning — part og dimensjoner vises på linjen.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, from, to } = $props();

  let data = $state(null);

  $effect(() => {
    data = null;
    api("/companies/" + companyId + "/reports/kontospesifikasjon?from=" + from + "&to=" + to)
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
        <th>Konto</th><th>Bilag</th><th>Dato</th><th>Tekst</th>
        <th class="text-right">Beløp</th><th class="text-right">Saldo</th>
      </tr>
    </thead>
    <tbody>
      {#each data.posts as p}
        <tr>
          <td>{p.account}</td>
          <td>{p.bilag}</td>
          <td>{p.date}</td>
          <td>
            {p.description}
            {#if p.party_no}<span class="opacity-60">({p.party_no})</span>{/if}
            {#if p.avdeling}<span class="badge badge-ghost badge-xs">{p.avdeling}</span>{/if}
            {#if p.prosjekt}<span class="badge badge-ghost badge-xs">{p.prosjekt}</span>{/if}
          </td>
          <td class="text-right">{kr(p.amount_ore)}</td>
          <td class="text-right">{kr(p.saldo_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
