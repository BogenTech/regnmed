<script>
  // Bokføringsspesifikasjon (#4): bilagene i kjederekkefølge, med
  // linjene sine. En side med bilag av gangen; CSV-filen er hele
  // perioden.
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import LoadError from "../../components/LoadError.svelte";
  import Paginering from "../../components/Paginering.svelte";

  let { companyId, from, to, params } = $props();

  const PER_SIDE = 50;
  const base = $derived("/companies/" + companyId);

  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  // An edited address can say anything; a page is a whole number from 1.
  let side = $derived(Math.max(1, Math.floor(Number(params.side)) || 1));

  function load() {
    const mine = ++seq;
    data = null;
    error = null;
    api(
      base + "/reports/bokforingsspesifikasjon?from=" + from + "&to=" + to +
        "&limit=" + PER_SIDE + "&offset=" + (side - 1) * PER_SIDE,
    )
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);

  const num = "text-right tabular-nums whitespace-nowrap";
</script>

<div class="flex justify-end mb-3">
  <button
    class="btn btn-sm btn-outline"
    onclick={() =>
      download(
        base + "/reports/bokforingsspesifikasjon?from=" + from + "&to=" + to + "&format=csv",
        "bokforingsspesifikasjon_" + from.slice(0, 4) + ".csv",
      )}
  >
    Last ned CSV
  </button>
</div>

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter bokføringsspesifikasjonen"></span>
{:else if !data.total}
  <p class="opacity-70">Ingen bilag i perioden.</p>
{:else}
  {#each data.vouchers as v (v.voucher_id)}
    <section class="mb-3" aria-labelledby={"bilag-" + v.voucher_id}>
      <h3 id={"bilag-" + v.voucher_id} class="text-sm">
        <a class="link link-hover font-semibold" href={"#/c/" + companyId + "/hovedbok/" + v.voucher_id}>{v.bilag}</a>
        <span class="whitespace-nowrap">{formatDate(v.date)}</span> — {v.description}
      </h3>
      <table class="table table-sm">
        <tbody>
          {#each v.lines as l (l.line_no)}
            <tr>
              <td>{l.account} {l.account_name}</td>
              <td class="whitespace-nowrap">{l.vat_code ? "mva " + l.vat_code : ""}</td>
              <td class={num}>{kr(l.amount_ore)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/each}
  <div class="flex gap-2 items-center flex-wrap mt-2 text-sm">
    <span class="opacity-70">
      Viser bilag {(side - 1) * PER_SIDE + 1}–{(side - 1) * PER_SIDE + data.vouchers.length} av {data.total}
    </span>
    <span class="ml-auto">
      <Paginering bind:side={() => side, (s) => (params.side = s > 1 ? String(s) : undefined)} antall={data.total} perSide={PER_SIDE} />
    </span>
  </div>
{/if}
