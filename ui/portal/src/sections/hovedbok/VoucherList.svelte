<script>
  // The book: vouchers newest first with their lines, paged and filtered
  // on the server (GET /vouchers with from/to/sok/limit/offset/lines), so
  // the view scales with the ledger. The filter reads the whole voucher:
  // number, date, text, and every line's account number or name.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Paginering from "../../components/Paginering.svelte";
  import YearPicker from "./YearPicker.svelte";

  let { companyId, query = "" } = $props();

  const PAGE = 20;
  const initial = new URLSearchParams(untrack(() => query));
  let year = $state(Number(initial.get("ar")) || new Date().getFullYear());
  let search = $state(initial.get("sok") || "");
  let active = $state(search);
  let withoutDoc = $state(initial.get("uten_vedlegg") === "true");
  let page = $state(1);
  let data = $state(null);
  let error = $state(null);
  let canPost = $derived(harRett(companyId, "BILAG_BOKFOR"));

  let timer;
  $effect(() => {
    const v = search;
    clearTimeout(timer);
    timer = setTimeout(() => (active = v), 300);
    return () => clearTimeout(timer);
  });
  // A new filter starts on page 1. Done inside the fetch effect, not in
  // an effect of its own, so a filter change on page 3 asks once, for
  // page 1 — not first for page 3 of the new filter and then again.
  let lastFilter = "";
  let seq = 0;
  $effect(() => {
    const filter = year + "|" + active + "|" + withoutDoc;
    if (filter !== lastFilter) {
      const first = lastFilter === "";
      lastFilter = filter;
      if (!first && page !== 1) {
        page = 1;
        return;
      }
    }
    const mine = ++seq;
    const p = new URLSearchParams({
      lines: "true",
      from: year + "-01-01",
      to: year + "-12-31",
      limit: PAGE,
      offset: (page - 1) * PAGE,
    });
    if (active.trim()) p.set("sok", active.trim());
    if (withoutDoc) p.set("uten_vedlegg", "true");
    data = null;
    api("/companies/" + companyId + "/vouchers?" + p)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  });

  // Search and year in the address, so a filtered book can be shared.
  $effect(() => {
    const p = new URLSearchParams();
    if (year !== new Date().getFullYear()) p.set("ar", year);
    if (active.trim()) p.set("sok", active.trim());
    if (withoutDoc) p.set("uten_vedlegg", "true");
    const hash = "#/c/" + companyId + "/hovedbok" + (p.size ? "?" + p : "");
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  const label = (v) => (v.journal === "GL" ? "" : v.journal + " ") + v.voucher;
  const voucherHref = (v) => "#/c/" + companyId + "/hovedbok/" + v.voucher_id;
</script>

<div class="flex flex-wrap items-end gap-3 mb-4">
  <YearPicker bind:year />
  <label class="input w-full sm:w-80">
    <span class="sr-only">Søk i bilagene</span>
    <svg class="h-4 w-4 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
    <input type="search" placeholder="Bilagsnummer, tekst, dato eller konto" bind:value={search} />
  </label>
  <label class="label cursor-pointer gap-2">
    <input type="checkbox" class="checkbox checkbox-sm" bind:checked={withoutDoc} />
    <span class="text-base-content">Bare bilag uten vedlegg</span>
  </label>
  {#if canPost}
    <a class="btn btn-primary ml-auto" href={"#/c/" + companyId + "/hovedbok/ny"}>Nytt bilag</a>
  {/if}
</div>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if data === null}
  <div class="flex justify-center p-8"><span class="loading loading-spinner"></span></div>
{:else if data.vouchers.length === 0}
  <p class="opacity-70">
    {withoutDoc
      ? "Alle bilag i " + year + " har vedlegg."
      : active.trim()
        ? "Ingen bilag i " + year + " passer søket."
        : "Ingen bilag i " + year + " ennå."}
  </p>
{:else}
  <p class="text-sm opacity-70 mb-2" role="status">
    {data.total} bilag i {year}{active.trim() || withoutDoc ? " som passer" : ""}, nyeste først
  </p>
  <ul class="grid gap-2">
    {#each data.vouchers as v (v.voucher_id)}
      <li class="card border border-base-300 bg-base-100 relative hover:bg-base-200 focus-within:bg-base-200">
        <div class="card-body p-3 sm:p-4 gap-1">
          <div class="flex flex-wrap items-baseline gap-x-3">
            <a class="font-semibold after:absolute after:inset-0" href={voucherHref(v)}>Bilag {label(v)}</a>
            <span class="opacity-70 whitespace-nowrap">{formatDate(v.date)}</span>
            <span class="min-w-0 break-words">{v.description}</span>
          </div>
          <table class="table table-xs">
            <caption class="sr-only">Linjene på bilag {label(v)}</caption>
            <tbody>
              {#each v.lines as l, i (i)}
                <tr>
                  <td class="w-full">
                    <span class="font-mono">{l.account}</span>
                    <span class="opacity-80">{l.account_name}</span>
                    {#if l.party_no}<span class="opacity-60">· part {l.party_no}</span>{/if}
                    {#if l.vat_code}<span class="opacity-60">· mva {l.vat_code}</span>{/if}
                  </td>
                  <td class="text-right tabular-nums whitespace-nowrap">{l.amount_ore > 0 ? kr(l.amount_ore) : ""}</td>
                  <td class="text-right tabular-nums whitespace-nowrap">{l.amount_ore < 0 ? kr(-l.amount_ore) : ""}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </li>
    {/each}
  </ul>
  <div class="mt-3"><Paginering bind:side={page} antall={data.total} perSide={PAGE} /></div>
{/if}
