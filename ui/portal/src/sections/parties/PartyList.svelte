<script>
  // Kunde- og leverandørregisteret: søk, sortering og sider på serveren
  // (GET /parties?kind=&q=&sort=&limit=), så nettleseren får 50 rader
  // uansett hvor mange parter selskapet har. Søk og sortering står i
  // adressen og kan deles. Én side for begge, fordi de skal oppføre seg
  // likt; ordene og beløpets fortegn følger `kind`.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import SortHeader from "../../components/SortHeader.svelte";

  // Reskontro → Parter shows the same list under its own address and
  // heading: `listSection` is where search/sort are written back to, and
  // `heading` drops the page title and "new" button it does not need.
  let { companyId, kind = "kunde", query = "", listSection = null, heading = true } = $props();

  const T = {
    kunde: {
      title: "Kunder", section: "kunder", one: "kunde", newLabel: "Ny kunde",
      first: "Legg til den første kunden", none: "Ingen kunder ennå.", noMatch: "Ingen kunder passer søket.",
      search: "Søk i kunder", col: "Kunde", owed: "Utestående", missing: "email",
    },
    leverandor: {
      title: "Leverandører", section: "leverandorer", one: "leverandør", newLabel: "Ny leverandør",
      first: "Legg til den første leverandøren", none: "Ingen leverandører ennå.",
      noMatch: "Ingen leverandører passer søket.", search: "Søk i leverandører", col: "Leverandør",
      owed: "Skyldig", missing: "bank_account",
    },
  }[untrack(() => kind)];
  // The ledger sign: a customer owes us (debit, positive), we owe a
  // supplier (credit, negative). Shown the way each is read.
  const owed = (p) => (kind === "kunde" ? p.saldo_ore : -p.saldo_ore);
  const PAGE = 50;
  const initial = new URLSearchParams(untrack(() => query));
  let search = $state(initial.get("q") || "");
  let sort = $state(initial.get("sort") || "name");
  let desc = $state(initial.get("desc") === "true");
  let rows = $state(null);
  let total = $state(0);
  let error = $state(null);
  let loadingMore = $state(false);
  let searchInput;

  let seq = 0;
  let limit = $state(PAGE);
  function path() {
    const p = new URLSearchParams({ kind, limit, sort, desc });
    if (search.trim()) p.set("q", search.trim());
    return "/companies/" + companyId + "/parties?" + p;
  }

  async function load() {
    const mine = ++seq;
    error = null;
    try {
      const page = await api(path());
      if (mine !== seq) return;
      rows = page.parties;
      total = page.total;
    } catch (e) {
      if (mine === seq) error = e.message;
    }
  }

  // One longer page rather than an offset: sorted by amount, a payment
  // between two pages would move a customer across the boundary and it
  // would be shown twice or not at all.
  async function more() {
    limit = Math.min(limit + PAGE, 200);
    loadingMore = true;
    await load();
    loadingMore = false;
  }

  let timer;
  $effect(() => {
    sort;
    desc;
    const q = search;
    limit = PAGE;
    clearTimeout(timer);
    timer = setTimeout(load, q ? 250 : 0);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const p = new URLSearchParams();
    if (search.trim()) p.set("q", search.trim());
    if (sort !== "name") p.set("sort", sort);
    if (desc) p.set("desc", "true");
    if (listSection) p.set("kind", kind);
    const hash = "#/c/" + companyId + "/" + (listSection || T.section) + (p.size ? "?" + p : "");
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  // Amounts sort biggest first on the first click; text A–Å.
  // Amounts sort biggest first on the first click. A supplier's balance
  // is credit (negative), so "biggest debt first" is ascending there.
  function onsort(key, inverted = false) {
    if (sort === key) desc = !desc;
    else {
      sort = key;
      desc = key === "overdue" || (key === "balance" && !inverted);
    }
  }

  function onKey(e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target.closest("input, textarea, select, [contenteditable]")) return;
    if (e.key === "/") {
      e.preventDefault();
      searchInput?.focus();
    } else if (e.key === "n") {
      location.hash = "#/c/" + companyId + "/" + T.section + "/ny";
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if heading}
  <div class="flex flex-wrap items-center gap-3 mb-4">
    <h1 class="text-2xl font-bold mr-auto">{T.title}</h1>
    <a class="btn btn-primary" href={"#/c/" + companyId + "/" + T.section + "/ny"} aria-keyshortcuts="n">
      {T.newLabel} <kbd class="kbd kbd-sm hidden sm:inline-flex">N</kbd>
    </a>
  </div>
{/if}

<div class="card border border-base-300 bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <label class="input w-full sm:w-96">
      <span class="sr-only">{T.search}</span>
      <svg class="h-4 w-4 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      <input
        type="search"
        placeholder="Navn, nummer, orgnr eller e-post"
        bind:value={search}
        bind:this={searchInput}
        aria-keyshortcuts="/"
      />
      <kbd class="kbd kbd-sm hidden sm:inline-flex">/</kbd>
    </label>

    {#if error}
      <div class="alert alert-error mt-3" role="alert"><span>{error}</span></div>
    {:else if rows === null}
      <div class="flex justify-center p-6"><span class="loading loading-spinner"></span></div>
    {:else if rows.length === 0}
      <div class="py-8 text-center">
        <p class="opacity-70 mb-3">{search.trim() ? T.noMatch : T.none}</p>
        {#if !search.trim()}
          <a class="btn btn-primary btn-sm" href={"#/c/" + companyId + "/" + T.section + "/ny"}>{T.first}</a>
        {/if}
      </div>
    {:else}
      <table class="table mt-2">
        <caption class="sr-only">{T.title}. Velg en {T.one} for å åpne den.</caption>
        <thead>
          <tr>
            <SortHeader label={T.col} key="name" {sort} {desc} {onsort} />
            <SortHeader label="Nr" key="number" {sort} {desc} {onsort} class="hidden md:table-cell" />
            <th scope="col" class="hidden md:table-cell">{kind === "kunde" ? "E-post" : "Kontonummer"}</th>
            <!-- A supplier's balance is credit: the ledger sorts it the other
                 way round from the positive amount shown, so the arrow and
                 aria-sort follow what the user sees. -->
            <SortHeader label={T.owed} key="balance" {sort} desc={kind === "leverandor" ? !desc : desc} onsort={(k) => onsort(k, kind === "leverandor")} class="text-right" />
            <SortHeader label="Forfalt" key="overdue" {sort} {desc} {onsort} class="text-right hidden sm:table-cell" />
          </tr>
        </thead>
        <tbody>
          {#each rows as p (p.party_id)}
            <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
              <td>
                <a class="font-medium after:absolute after:inset-0" href={"#/c/" + companyId + "/" + T.section + "/" + p.party_id}>
                  {p.name}
                </a>
                <span class="block text-sm opacity-70 md:hidden">Nr {p.party_no}</span>
                {#if p.overdue_ore}
                  <span class="block text-sm text-error sm:hidden">{kr(p.overdue_ore)} forfalt</span>
                {/if}
              </td>
              <td class="hidden md:table-cell">{p.party_no}</td>
              <td class="hidden md:table-cell">
                {#if p[T.missing]}{p[T.missing]}{:else}<span class="opacity-60">mangler</span>{/if}
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{p.saldo_ore ? kr(owed(p)) : "–"}</td>
              <td class="text-right tabular-nums whitespace-nowrap hidden sm:table-cell {p.overdue_ore ? 'text-error' : ''}">
                {p.overdue_ore ? kr(p.overdue_ore) : "–"}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if rows.length < total && limit >= 200}
        <p class="text-sm opacity-70 mt-3 text-center">
          Viser {rows.length} av {total}. Søk for å finne resten.
        </p>
      {:else if rows.length < total}
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
