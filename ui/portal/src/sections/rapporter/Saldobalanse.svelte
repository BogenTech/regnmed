<script>
  // Saldobalanse: every account with movement or balance, each one a
  // link to its postings in Hovedbok. The sums close the check — utgående
  // over all accounts is zero when the ledger balances.
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, from, to, year } = $props();

  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  function load() {
    const mine = ++seq;
    data = null;
    error = null;
    api("/companies/" + companyId + "/reports/saldobalanse?from=" + from + "&to=" + to)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);

  const sum = (key) => data.accounts.reduce((s, a) => s + a[key], 0);
  const num = "text-right tabular-nums whitespace-nowrap";
</script>

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter saldobalansen"></span>
{:else if !data.accounts.length}
  <p class="opacity-70">Ingen kontoer med saldo eller bevegelse i {year}.</p>
{:else}
  <table class="table table-sm">
    <caption class="sr-only">Saldobalanse {year}, beløp i kroner.</caption>
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
          <td>
            <a class="link link-hover font-medium" href={"#/c/" + companyId + "/hovedbok/" + a.number + "?ar=" + year}>{a.number}</a>
          </td>
          <td>{a.name}</td>
          <td class={num}>{kr(a.inngaende_ore)}</td>
          <td class={num}>{kr(a.debet_ore)}</td>
          <td class={num}>{kr(a.kredit_ore)}</td>
          <td class={num}>{kr(a.utgaende_ore)}</td>
        </tr>
      {/each}
    </tbody>
    <tfoot>
      <tr class="font-semibold">
        <td></td><td>Sum</td>
        <td class={num}>{kr(sum("inngaende_ore"))}</td>
        <td class={num}>{kr(sum("debet_ore"))}</td>
        <td class={num}>{kr(sum("kredit_ore"))}</td>
        <td class={num}>{kr(sum("utgaende_ore"))}</td>
      </tr>
    </tfoot>
  </table>
{/if}
