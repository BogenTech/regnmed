<script>
  // Kunderegisteret: søk, sortering og sider på serveren
  // (GET /parties?kind=kunde&q=&sort=&limit=), så nettleseren får 50
  // rader uansett hvor mange kunder selskapet har. Søk og sortering står
  // i adressen og kan deles.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import SortHeader from "../../components/SortHeader.svelte";

  let { companyId, query = "" } = $props();

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
    const p = new URLSearchParams({ kind: "kunde", limit, sort, desc });
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
    const hash = "#/c/" + companyId + "/kunder" + (p.size ? "?" + p : "");
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  // Amounts sort biggest first on the first click; text A–Å.
  function onsort(key) {
    if (sort === key) desc = !desc;
    else {
      sort = key;
      desc = key === "balance" || key === "overdue";
    }
  }

  function onKey(e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target.closest("input, textarea, select, [contenteditable]")) return;
    if (e.key === "/") {
      e.preventDefault();
      searchInput?.focus();
    } else if (e.key === "n") {
      location.hash = "#/c/" + companyId + "/kunder/ny";
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="flex flex-wrap items-center gap-3 mb-4">
  <h1 class="text-2xl font-bold mr-auto">Kunder</h1>
  <a class="btn btn-primary" href={"#/c/" + companyId + "/kunder/ny"} aria-keyshortcuts="n">
    Ny kunde <kbd class="kbd kbd-sm hidden sm:inline-flex">N</kbd>
  </a>
</div>

<div class="card border border-base-300 bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <label class="input w-full sm:w-96">
      <span class="sr-only">Søk i kunder</span>
      <svg class="h-4 w-4 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      <input
        type="search"
        placeholder="Navn, kundenummer, orgnr eller e-post"
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
        <p class="opacity-70 mb-3">{search.trim() ? "Ingen kunder passer søket." : "Ingen kunder ennå."}</p>
        {#if !search.trim()}
          <a class="btn btn-primary btn-sm" href={"#/c/" + companyId + "/kunder/ny"}>Legg til den første kunden</a>
        {/if}
      </div>
    {:else}
      <table class="table mt-2">
        <caption class="sr-only">Kunder. Velg en kunde for å åpne den.</caption>
        <thead>
          <tr>
            <SortHeader label="Kunde" key="name" {sort} {desc} {onsort} />
            <SortHeader label="Nr" key="number" {sort} {desc} {onsort} class="hidden md:table-cell" />
            <th scope="col" class="hidden md:table-cell">E-post</th>
            <SortHeader label="Utestående" key="balance" {sort} {desc} {onsort} class="text-right" />
            <SortHeader label="Forfalt" key="overdue" {sort} {desc} {onsort} class="text-right hidden sm:table-cell" />
          </tr>
        </thead>
        <tbody>
          {#each rows as p (p.party_id)}
            <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
              <td>
                <a class="font-medium after:absolute after:inset-0" href={"#/c/" + companyId + "/kunder/" + p.party_id}>
                  {p.name}
                </a>
                <span class="block text-sm opacity-70 md:hidden">Nr {p.party_no}</span>
                {#if p.overdue_ore}
                  <span class="block text-sm text-error sm:hidden">{kr(p.overdue_ore)} forfalt</span>
                {/if}
              </td>
              <td class="hidden md:table-cell">{p.party_no}</td>
              <td class="hidden md:table-cell">
                {#if p.email}{p.email}{:else}<span class="opacity-60">mangler</span>{/if}
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{p.saldo_ore ? kr(p.saldo_ore) : "–"}</td>
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
