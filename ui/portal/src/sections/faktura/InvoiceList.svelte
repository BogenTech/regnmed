<script>
  // Fakturalisten er Faktura-seksjonens forside: status, søk og
  // filtrering skjer på serveren (GET /invoices?status=&q=&limit=), så
  // nettleseren får 50 rader uansett hvor mange fakturaer selskapet har.
  // Filteret står i adressen (#/c/{id}/faktura?status=overdue) og kan
  // deles.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, krWhole, formatDate } from "../../lib/format.js";
  import InvoiceStatus from "./InvoiceStatus.svelte";

  // `party` (a party number) embeds the list on a customer's page: one
  // customer's invoices, no page heading, no address or shortcuts.
  let { companyId, query = "", party = null } = $props();
  const embedded = untrack(() => !!party);
  const newHref = untrack(() => "#/c/" + companyId + "/faktura/ny" + (party ? "?kunde=" + party : ""));

  const PAGE = 50;
  const FILTERS = [
    ["", "Alle", (c) => Object.values(c).reduce((a, b) => a + b, 0)],
    ["open", "Ubetalt", (c) => (c.open || 0) + (c.overdue || 0)],
    ["overdue", "Forfalt", (c) => c.overdue || 0],
    ["paid", "Betalt", (c) => c.paid || 0],
    ["credit", "Kreditnota", (c) => (c.credited || 0) + (c.credit_note || 0)],
  ];
  const EMPTY = {
    "": "Ingen fakturaer ennå.",
    open: "Ingen ubetalte fakturaer.",
    overdue: "Ingen fakturaer er forfalt.",
    paid: "Ingen betalte fakturaer.",
    credit: "Ingen kreditnotaer.",
  };

  const initial = new URLSearchParams(untrack(() => query));
  let status = $state(initial.get("status") || "");
  let search = $state(initial.get("q") || "");
  let rows = $state(null);
  let total = $state(0);
  let counts = $state({});
  let loadingMore = $state(false);
  let error = $state(null);
  let unbilledOre = $state(0);
  // Under Forfalt: the aging buckets and each invoice's last dunning step
  // (from /invoices/overdue), which decide the next reminder.
  let aging = $state(null);
  const STEP_LABEL = { paminnelse: "påminnelse", purring: "purring", inkassovarsel: "inkassovarsel" };
  let searchInput;

  let seq = 0;
  function path(before) {
    const p = new URLSearchParams({ limit: PAGE });
    if (before) p.set("before", before);
    if (status) p.set("status", status);
    if (search.trim()) p.set("q", search.trim());
    if (party) p.set("party_no", party);
    return "/companies/" + companyId + "/invoices?" + p;
  }

  async function load() {
    const mine = ++seq;
    error = null;
    try {
      const page = await api(path(null));
      if (mine !== seq) return; // a newer filter or keystroke won
      rows = page.invoices;
      total = page.total;
      counts = page.counts;
    } catch (e) {
      if (mine === seq) error = e.message;
    }
  }

  async function more() {
    const mine = seq; // a filter change meanwhile makes this page stale
    loadingMore = true;
    try {
      const page = await api(path(rows.at(-1).invoice_no));
      if (mine === seq) rows = [...rows, ...page.invoices];
    } catch (e) {
      if (mine === seq) error = e.message;
    }
    loadingMore = false;
  }

  // Filter and search live in the address without adding history
  // entries per keystroke; the browser's back button leaves the list.
  $effect(() => {
    if (embedded) return;
    const p = new URLSearchParams();
    if (status) p.set("status", status);
    if (search.trim()) p.set("q", search.trim());
    const hash = "#/c/" + companyId + "/faktura" + (p.size ? "?" + p : "");
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  // Search waits for a pause in typing; a filter click loads at once.
  let timer;
  $effect(() => {
    status;
    companyId;
    const q = search;
    clearTimeout(timer);
    timer = setTimeout(load, q ? 250 : 0);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (embedded) return;
    api("/companies/" + companyId + "/timesheet/unbilled")
      .then((r) => {
        unbilledOre = r.groups.reduce((s, g) => s + Math.round((g.minutter * g.timesats_ore) / 60), 0);
      })
      .catch(() => (unbilledOre = 0));
  });

  $effect(() => {
    aging = null;
    if (status !== "overdue") return;
    api("/companies/" + companyId + "/invoices/overdue")
      .then((r) => {
        aging = {
          buckets: r.buckets,
          last: Object.fromEntries(r.invoices.map((i) => [i.invoice_id, i])),
        };
      })
      .catch(() => (aging = null));
  });

  // "/" jumps to search and "n" starts a new invoice, unless the user is
  // typing somewhere.
  function onKey(e) {
    if (embedded) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target.closest("input, textarea, select, [contenteditable]")) return;
    if (e.key === "/") {
      e.preventDefault();
      searchInput?.focus();
    } else if (e.key === "n") {
      location.hash = "#/c/" + companyId + "/faktura/ny";
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if !embedded}
  <div class="flex flex-wrap items-center gap-3 mb-4">
    <h1 class="text-2xl font-bold mr-auto">Fakturaer</h1>
    <a class="btn btn-primary" href={newHref} aria-keyshortcuts="n">
      Ny faktura <kbd class="kbd kbd-sm hidden sm:inline-flex">N</kbd>
    </a>
  </div>
{/if}

{#if unbilledOre > 0}
  <div class="alert alert-info alert-soft mb-4">
    <span>{krWhole(unbilledOre)} kr i ufakturerte timer.</span>
    <a class="btn btn-sm" href={"#/c/" + companyId + "/faktura/timer"}>Fakturer timer</a>
  </div>
{/if}

<div class="card border border-base-300 bg-base-100 mb-6">
<div class="card-body overflow-x-auto">
  {#if embedded}<h2 class="card-title">Fakturaer</h2>{/if}
  <div class="flex flex-col sm:flex-row gap-3 sm:items-center">
    <label class="input w-full sm:w-72">
      <span class="sr-only">Søk i fakturaer</span>
      <svg class="h-4 w-4 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      <input
        type="search"
        placeholder={embedded ? "Nummer eller KID" : "Nummer, KID eller kunde"}
        bind:value={search}
        bind:this={searchInput}
        aria-keyshortcuts={embedded ? undefined : "/"}
      />
      {#if !embedded}<kbd class="kbd kbd-sm hidden sm:inline-flex">/</kbd>{/if}
    </label>
    <div class="flex flex-wrap gap-1" role="group" aria-label="Vis fakturaer">
      {#each FILTERS as [key, label, count] (key)}
        <button
          type="button"
          class="btn btn-sm {status === key ? 'btn-neutral' : 'btn-ghost'}"
          aria-pressed={status === key}
          onclick={() => (status = key)}
        >
          {label}
          <span class="badge badge-sm {status === key ? '' : 'badge-ghost'}">{count(counts)}</span>
        </button>
      {/each}
    </div>
  </div>

  {#if status === "overdue" && aging && !embedded}
    <p class="text-sm mt-3">
      Forfalt 1–14 dager: <b class="tabular-nums">{krWhole(aging.buckets["1-14"] || 0)}</b> ·
      15–30 dager: <b class="tabular-nums">{krWhole(aging.buckets["15-30"] || 0)}</b> ·
      over 30 dager: <b class="tabular-nums text-error">{krWhole(aging.buckets["30+"] || 0)}</b> kr
    </p>
  {/if}

  {#if error}
    <div class="alert alert-error mt-3" role="alert"><span>{error}</span></div>
  {:else if rows === null}
    <div class="flex justify-center p-6"><span class="loading loading-spinner"></span></div>
  {:else if rows.length === 0}
    <div class="py-8 text-center">
      <p class="opacity-70 mb-3">{search.trim() ? "Ingen fakturaer passer søket." : EMPTY[status]}</p>
      {#if !search.trim() && status === ""}
        <a class="btn btn-primary btn-sm" href={newHref}>Lag den første fakturaen</a>
      {/if}
    </div>
  {:else}
    <table class="table mt-2">
      <caption class="sr-only">Fakturaer, nyeste først. Velg en faktura for å åpne den.</caption>
      <thead>
        <tr>
          <th scope="col">Faktura</th>
          <th scope="col" class={embedded ? "hidden" : "hidden md:table-cell"}>Dato</th>
          <th scope="col" class="hidden md:table-cell">Forfall</th>
          <th scope="col" class="text-right">Beløp</th>
          <th scope="col" class="text-right hidden sm:table-cell">Utestående</th>
          <th scope="col" class="hidden sm:table-cell">Status</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as i (i.invoice_id)}
          <!-- The whole row is the link's hit area; the link itself sits
               in the first cell so Tab/Enter reach it in reading order. -->
          <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
            <td>
              <a
                class="font-medium after:absolute after:inset-0"
                href={"#/c/" + companyId + "/faktura/" + i.invoice_id}
              >
                {i.is_credit_note ? "Kreditnota" : "Faktura"} {i.invoice_no}
                {#if !embedded}<span class="block text-sm font-normal opacity-70">{i.party_name}</span>{/if}
              </a>
              {#if aging?.last[i.invoice_id]?.last_steg}
                <span class="block text-xs opacity-70">
                  Sist: {STEP_LABEL[aging.last[i.invoice_id].last_steg]}
                  {formatDate(aging.last[i.invoice_id].last_sent)}
                </span>
              {/if}
              <span class="block mt-1 sm:hidden"><InvoiceStatus status={i.status} dueDate={i.due_date} /></span>
            </td>
            <td class="{embedded ? 'hidden' : 'hidden md:table-cell'} whitespace-nowrap">{formatDate(i.invoice_date)}</td>
            <td class="hidden md:table-cell whitespace-nowrap">
              {i.is_credit_note ? "–" : formatDate(i.due_date)}
            </td>
            <td class="text-right tabular-nums whitespace-nowrap">{kr(i.gross_ore)}</td>
            <td class="text-right tabular-nums whitespace-nowrap hidden sm:table-cell">
              {i.remaining_ore ? kr(i.remaining_ore) : "–"}
            </td>
            <td class="hidden sm:table-cell"><InvoiceStatus status={i.status} dueDate={i.due_date} /></td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if rows.length < total}
      <div class="flex justify-center mt-3">
        <button class="btn btn-sm" onclick={more} disabled={loadingMore}>
          {#if loadingMore}<span class="loading loading-spinner loading-xs"></span>{/if}
          Vis flere ({rows.length} av {total})
        </button>
      </div>
    {/if}
  {/if}
</div>
</div>
