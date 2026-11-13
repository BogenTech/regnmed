<script>
  // One account's postings for a year, with the running balance from the
  // opening balance (the kontospesifikasjon, filtered on the server), and
  // each posting linked to its voucher.
  import { untrack } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import Paginering from "../../components/Paginering.svelte";
  import YearPicker from "../../components/YearPicker.svelte";

  let { companyId, account, query = "" } = $props();

  const PAGE = 50;
  let year = $state(Number(new URLSearchParams(untrack(() => query)).get("ar")) || new Date().getFullYear());
  let posts = $state(null);
  let name = $state("");
  let balance = $state(null);
  let error = $state(null);
  let page = $state(1);

  $effect(() => {
    api("/companies/" + companyId + "/accounts")
      .then((r) => {
        const k = r.kontoer.find((k) => k.number === account);
        name = k?.name || "";
        balance = k?.saldo_ore ?? null;
      })
      .catch(() => {});
  });
  $effect(() => {
    const y = year;
    posts = null;
    page = 1;
    api("/companies/" + companyId + "/reports/kontospesifikasjon?from=" + y + "-01-01&to=" + y + "-12-31&account=" + account)
      .then((r) => y === year && (posts = r.posts))
      .catch((e) => (error = e.message));
  });

  // The running balance includes the opening balance: what stood before
  // the first posting of the year.
  let opening = $derived(posts?.length ? posts[0].saldo_ore - posts[0].amount_ore : null);
  let shown = $derived((posts || []).slice((page - 1) * PAGE, page * PAGE));
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/hovedbok/kontoplan"}>← Kontoplan</a>
</nav>
<div class="flex flex-wrap items-end gap-3 mb-4">
  <div class="mr-auto">
    <h1 class="text-2xl font-bold"><span class="font-mono">{account}</span> {name}</h1>
    {#if balance !== null}<p class="opacity-70 tabular-nums">Saldo i dag {kr(balance)} kr</p>{/if}
  </div>
  <YearPicker bind:year />
</div>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if posts === null}
  <div class="flex justify-center p-8"><span class="loading loading-spinner"></span></div>
{:else if posts.length === 0}
  <p class="opacity-70">Ingen posteringer på {account} i {year}.</p>
{:else}
  <div class="card border border-base-300 bg-base-100">
    <div class="card-body p-0 sm:p-2 overflow-x-auto">
      <table class="table table-sm">
        <caption class="sr-only">Posteringer på konto {account} i {year} med løpende saldo.</caption>
        <thead>
          <tr>
            <th scope="col">Bilag</th>
            <th scope="col" class="text-right">Beløp</th>
            <th scope="col" class="text-right hidden sm:table-cell">Saldo</th>
          </tr>
        </thead>
        <tbody>
          {#if page === 1}
            <tr>
              <td class="opacity-70">Inngående saldo 01.01.{year}</td>
              <td></td>
              <td class="text-right tabular-nums hidden sm:table-cell">{kr(opening)}</td>
            </tr>
          {/if}
          {#each shown as p, i (i)}
            <tr>
              <td>
                <a class="link link-hover font-medium" href={"#/c/" + companyId + "/hovedbok/" + p.voucher_id}>{p.bilag.replace(/^GL-/, "")}</a>
                <span class="opacity-70 whitespace-nowrap">{formatDate(p.date)}</span>
                <span class="block text-sm opacity-80">
                  {p.description}{p.party_no ? " · part " + p.party_no : ""}{p.avdeling ? " · " + p.avdeling : ""}{p.prosjekt ? " · " + p.prosjekt : ""}
                </span>
              </td>
              <td class="text-right tabular-nums whitespace-nowrap">{kr(p.amount_ore)}</td>
              <td class="text-right tabular-nums whitespace-nowrap hidden sm:table-cell">{kr(p.saldo_ore)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="p-2"><Paginering bind:side={page} antall={posts.length} perSide={PAGE} /></div>
    </div>
  </div>
{/if}
