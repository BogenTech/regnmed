<script>
  // The chart of accounts — the book's index. The company's own accounts
  // with their balance; the standard catalog is searched alongside, and a
  // standard account joins the company's chart the first time it is used
  // or added. A custom account needs its own name. Numbers are permanent;
  // names and active can change, and a deactivated account refuses new
  // postings while its history stays.
  import { api, post, send } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { sporsmal, bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Paginering from "../../components/Paginering.svelte";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/accounts");
  const PAGE = 50;
  const KIND = { kunde: "kundereskontro", leverandor: "leverandørreskontro" };

  let data = $state(null);
  let error = $state(null);
  let search = $state("");
  let showInactive = $state(false);
  let page = $state(1);
  let newNumber = $state("");
  let newName = $state("");
  let canEdit = $derived(harRett(companyId, "BILAG_BOKFOR"));

  function load() {
    api(base)
      .then((r) => (data = r))
      .catch((e) => (error = e.message));
  }
  $effect(load);

  const q = $derived(search.trim().toLowerCase());
  const hit = (k) => !q || k.number.startsWith(q) || k.name.toLowerCase().includes(q);
  let own = $derived((data?.kontoer || []).filter((k) => (showInactive || k.active) && hit(k)));
  let shown = $derived(own.slice((page - 1) * PAGE, page * PAGE));
  let catalog = $derived.by(() => {
    if (!data || !q) return [];
    const mine = new Set(data.kontoer.map((k) => k.number));
    return data.standard.filter((s) => !mine.has(s.number) && hit(s)).slice(0, 15);
  });
  $effect(() => {
    void q;
    void showInactive;
    page = 1;
  });

  async function add(number, name) {
    try {
      const r = await post(base, { number, name: name || null });
      toast("Konto " + r.number + " " + r.name + " er lagt til", true);
      newNumber = "";
      newName = "";
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function rename(k) {
    const name = await sporsmal("Nytt navn på konto " + k.number, { standard: k.name, ok: "Lagre" });
    if (!name) return;
    try {
      await send("PUT", base + "/" + k.number, { name });
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function toggle(k) {
    if (
      k.active &&
      !(await bekreft("Nye posteringer på " + k.number + " avvises. Historikken står.", {
        tittel: "Deaktiver konto " + k.number + "?",
        ok: "Deaktiver",
        farlig: true,
      }))
    )
      return;
    try {
      await send("PUT", base + "/" + k.number, { active: !k.active });
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }
</script>

<div class="flex flex-wrap items-center gap-3 mb-4">
  <label class="input w-full sm:w-96">
    <span class="sr-only">Søk i kontoplanen</span>
    <svg class="h-4 w-4 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
    <input type="search" placeholder="Nummer eller navn, også standardkontoer" bind:value={search} />
  </label>
  <label class="label cursor-pointer gap-2">
    <input type="checkbox" class="checkbox checkbox-sm" bind:checked={showInactive} />
    <span class="text-base-content">Vis deaktiverte</span>
  </label>
</div>

{#if error}
  <div class="alert alert-error" role="alert"><span>{error}</span></div>
{:else if data === null}
  <div class="flex justify-center p-8"><span class="loading loading-spinner"></span></div>
{:else}
  <div class="card border border-base-300 bg-base-100 mb-4">
    <div class="card-body p-0 sm:p-2 overflow-x-auto">
      {#if own.length === 0}
        <p class="opacity-70 p-4">{q ? "Ingen av selskapets kontoer passer søket." : "Selskapet har ingen kontoer ennå."}</p>
      {:else}
        <table class="table table-sm sm:table-md">
          <caption class="sr-only">Selskapets kontoer med saldo. Velg en konto for å se posteringene.</caption>
          <thead>
            <tr>
              <th scope="col">Konto</th>
              <th scope="col" class="text-right">Saldo</th>
              <th scope="col" class="hidden sm:table-cell text-right">Posteringer</th>
              {#if canEdit}<th scope="col"><span class="sr-only">Endre</span></th>{/if}
            </tr>
          </thead>
          <tbody>
            {#each shown as k (k.number)}
              <tr class={k.active ? "" : "opacity-60"}>
                <td>
                  <a class="link link-hover" href={"#/c/" + companyId + "/hovedbok/" + k.number}>
                    <span class="font-mono">{k.number}</span> {k.name}
                  </a>
                  <span class="block text-sm opacity-70">
                    {[k.reskontro_kind && KIND[k.reskontro_kind], !k.active && "deaktivert"].filter(Boolean).join(", ")}
                    <span class="sm:hidden">{k.posteringer} posteringer</span>
                  </span>
                </td>
                <td class="text-right tabular-nums whitespace-nowrap">{kr(k.saldo_ore)}</td>
                <td class="hidden sm:table-cell text-right tabular-nums">{k.posteringer}</td>
                {#if canEdit}
                  <td class="text-right px-1 sm:px-4">
                    <div class="flex flex-col sm:flex-row items-end sm:justify-end gap-1">
                      <button class="btn btn-ghost btn-xs" aria-label={"Endre navn på " + k.number} onclick={() => rename(k)}>Navn</button>
                      <button class="btn btn-ghost btn-xs" onclick={() => toggle(k)}>{k.active ? "Deaktiver" : "Aktiver"}</button>
                    </div>
                  </td>
                {/if}
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="p-2"><Paginering bind:side={page} antall={own.length} perSide={PAGE} /></div>
      {/if}
    </div>
  </div>

  {#if canEdit && catalog.length}
    <section class="mb-4" aria-labelledby="std-tittel">
      <h2 id="std-tittel" class="font-semibold mb-1">Fra standardkontoplanen</h2>
      <ul class="grid gap-1">
        {#each catalog as s (s.number)}
          <li class="flex items-center gap-3">
            <span class="mr-auto"><span class="font-mono">{s.number}</span> {s.name}</span>
            <button class="btn btn-sm" onclick={() => add(s.number, null)}>Legg til</button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if canEdit}
    <form
      class="grid gap-3 sm:grid-cols-[8rem_1fr_auto] items-end max-w-2xl"
      onsubmit={(e) => {
        e.preventDefault();
        add(newNumber.trim(), newName.trim() || null);
      }}
    >
      <h2 class="font-semibold sm:col-span-3">Ny konto</h2>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="ny-konto-nr">Nummer</label>
        <input id="ny-konto-nr" class="input w-full" inputmode="numeric" maxlength="4" bind:value={newNumber} />
      </div>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="ny-konto-navn">Navn</label>
        <input id="ny-konto-navn" class="input w-full" placeholder="Tomt = standardnavnet" bind:value={newName} />
      </div>
      <button class="btn" type="submit" disabled={!/^\d{4}$/.test(newNumber.trim())}>Legg til konto</button>
    </form>
  {/if}
{/if}
