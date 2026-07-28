<script>
  // Bokføringsspesifikasjon (#4): bilagene i kjederekkefølge, med
  // linjene sine.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, from, to } = $props();

  let data = $state(null);

  $effect(() => {
    data = null;
    api("/companies/" + companyId + "/reports/bokforingsspesifikasjon?from=" + from + "&to=" + to)
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-sm"></span>
{:else if !data.vouchers.length}
  <p class="opacity-70">Ingen bilag i perioden.</p>
{:else}
  {#each data.vouchers as v}
    <div class="mb-3">
      <span class="font-semibold">{v.bilag}</span>
      {v.date} — {v.description}
      <table class="table table-sm">
        <tbody>
          {#each v.lines as l}
            <tr>
              <td>{l.account} {l.account_name}</td>
              <td>{l.vat_code ? "mva " + l.vat_code : ""}</td>
              <td class="text-right">{kr(l.amount_ore)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/each}
{/if}
