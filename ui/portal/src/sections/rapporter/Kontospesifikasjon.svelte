<script>
  // Kontospesifikasjon (#4): løpende saldo per konto med
  // bilagshenvisning — part og dimensjoner vises på linjen. Siden hentes
  // en side av gangen (saldoen regnes likevel over hele perioden på
  // serveren), og CSV-filen er alltid hele perioden.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import Paginering from "../../components/Paginering.svelte";

  let { companyId, from, to, year, params } = $props();

  const PER_SIDE = 100;
  const base = $derived("/companies/" + companyId);

  let kontoer = $state([]);
  let filter = $state(untrack(() => params.konto || ""));
  let data = $state(null);
  let error = $state(null);
  let seq = 0;

  $effect(() => {
    api(base + "/accounts")
      .then((a) => (kontoer = a.kontoer || []))
      .catch(() => {});
  });

  // An address changed from outside moves the field with it.
  $effect(() => {
    const k = params.konto || "";
    untrack(() => {
      if (k !== filter.trim()) filter = k;
    });
  });

  // The filter applies once it names one of the company's accounts (or is
  // emptied) — not on every keystroke of a half-typed number.
  $effect(() => {
    const v = filter.trim();
    if (v && !kontoer.some((k) => k.number === v)) return;
    // Driven by the field only: a change of address must not be undone here.
    untrack(() => {
      if ((params.konto || "") === v) return;
      params.konto = v || undefined;
      delete params.side;
    });
  });

  // An edited address can say anything; a page is a whole number from 1.
  let side = $derived(Math.max(1, Math.floor(Number(params.side)) || 1));

  function load() {
    const mine = ++seq;
    const q =
      "from=" + from + "&to=" + to + (params.konto ? "&account=" + encodeURIComponent(params.konto) : "");
    data = null;
    error = null;
    api(base + "/reports/kontospesifikasjon?" + q + "&limit=" + PER_SIDE + "&offset=" + (side - 1) * PER_SIDE)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);

  function csv() {
    const q = "from=" + from + "&to=" + to + (params.konto ? "&account=" + encodeURIComponent(params.konto) : "");
    download(
      base + "/reports/kontospesifikasjon?" + q + "&format=csv",
      "kontospesifikasjon_" + year + (params.konto ? "_" + params.konto : "") + ".csv",
    );
  }

  const num = "text-right tabular-nums whitespace-nowrap";
</script>

<div class="flex gap-2 flex-wrap items-end mb-3">
  <div class="flex flex-col gap-1 text-sm">
    <label for="kontospes-konto">Konto (tom = alle)</label>
    <KontoVelger id="kontospes-konto" {kontoer} bind:value={filter} cls="input input-sm w-32" />
  </div>
  <button class="btn btn-sm btn-outline ml-auto" onclick={csv}>Last ned CSV</button>
</div>

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter kontospesifikasjonen"></span>
{:else if !data.total}
  <p class="opacity-70">Ingen posteringer{params.konto ? " på " + params.konto : ""} i {year}.</p>
{:else}
  <table class="table table-sm">
    <caption class="sr-only">Kontospesifikasjon {year}, beløp i kroner, med løpende saldo per konto.</caption>
    <thead>
      <tr>
        <th>Konto</th><th>Bilag</th><th>Dato</th><th>Tekst</th>
        <th class="text-right">Beløp</th><th class="text-right">Saldo</th>
      </tr>
    </thead>
    <tbody>
      {#each data.posts as p, i (i)}
        <tr>
          <td>
            <a class="link link-hover" href={"#/c/" + companyId + "/hovedbok/" + p.account + "?ar=" + year} title={p.account_name}>{p.account}</a>
          </td>
          <td class="whitespace-nowrap">
            <a class="link link-hover font-medium" href={"#/c/" + companyId + "/hovedbok/" + p.voucher_id}>{p.bilag}</a>
          </td>
          <td class="whitespace-nowrap">{formatDate(p.date)}</td>
          <td>
            {p.description}
            {#if p.party_no}<span class="opacity-60">({p.party_no})</span>{/if}
            {#if p.avdeling}<span class="badge badge-ghost badge-xs">{p.avdeling}</span>{/if}
            {#if p.prosjekt}<span class="badge badge-ghost badge-xs">{p.prosjekt}</span>{/if}
          </td>
          <td class={num}>{kr(p.amount_ore)}</td>
          <td class={num}>{kr(p.saldo_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  <div class="flex gap-2 items-center flex-wrap mt-2 text-sm">
    <span class="opacity-70">
      Viser {(side - 1) * PER_SIDE + 1}–{(side - 1) * PER_SIDE + data.posts.length} av {data.total} posteringer
    </span>
    <span class="ml-auto">
      <Paginering bind:side={() => side, (s) => (params.side = s > 1 ? String(s) : undefined)} antall={data.total} perSide={PER_SIDE} />
    </span>
  </div>
{/if}
