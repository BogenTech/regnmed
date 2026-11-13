<script>
  // A party's reskontro postings and the matching of its open items:
  // suggested pairs first (exact amounts, then oldest first — matching.js),
  // a manual pick for anything else, and the history where a wrong match
  // is undone. Matching always takes the positive remainder (entry_a)
  // against the negative one (entry_b), but WHAT the two sides are flips
  // with the party's kind: a customer's invoice is debit and the payment
  // credit, a supplier's invoice credit and our payment debit. So the
  // words follow `kind`.
  import { api, post, send } from "../../lib/api.js";
  import { kr, parseKr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { suggestPairs } from "./matching.js";

  let { companyId, party, onChanged = () => {} } = $props();

  const base = $derived("/companies/" + companyId);
  let supplier = $derived(party.kind === "leverandor");
  // Names for the positive (entry_a) and negative (entry_b) side.
  let posWord = $derived(supplier ? "Betaling" : "Faktura");
  let negWord = $derived(supplier ? "Faktura" : "Innbetaling");

  let items = $state(null);
  let matches = $state(null);
  let openOnly = $state(true);
  let picked = $state([]);
  let amount = $state("");
  let busy = $state(false);
  let canMatch = $derived(harRett(companyId, "RESKONTRO_SKRIV"));

  function loadItems() {
    api(base + "/parties/" + party.party_id + "/items" + (openOnly ? "?open=true" : ""))
      .then((r) => (items = r.items))
      .catch((e) => toast(e.message, false));
  }
  function loadMatches() {
    api(base + "/parties/" + party.party_id + "/matches")
      .then((r) => (matches = r.matches))
      .catch(() => (matches = []));
  }
  const load = () => {
    loadItems();
    loadMatches();
  };
  $effect(loadItems);
  $effect(loadMatches);

  let open = $derived((items || []).filter((i) => i.remaining_ore !== 0));
  let suggestions = $derived(suggestPairs(open));
  let pickedItems = $derived(open.filter((i) => picked.includes(i.entry_id)));
  let pickedPos = $derived(pickedItems.filter((i) => i.remaining_ore > 0));
  let pickedNeg = $derived(pickedItems.filter((i) => i.remaining_ore < 0));
  let pairReady = $derived(pickedPos.length === 1 && pickedNeg.length === 1);
  let maxAmount = $derived(pairReady ? Math.min(pickedPos[0].remaining_ore, -pickedNeg[0].remaining_ore) : 0);
  let amountOre = $derived.by(() => {
    if (!amount.trim()) return maxAmount;
    try {
      return parseKr(amount);
    } catch {
      return NaN;
    }
  });
  let amountProblem = $derived(
    !pairReady ? null : !(amountOre > 0) ? "Skriv et positivt beløp" : amountOre > maxAmount ? "Høyst " + kr(maxAmount) + " kr" : null,
  );

  function toggle(id) {
    picked = picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id];
    amount = "";
  }

  async function matchOne(a, b, ore) {
    await post(base + "/reskontro/matches", { entry_a: a.entry_id, entry_b: b.entry_id, amount_ore: ore });
  }

  async function applyPair(p) {
    if (busy) return;
    busy = true;
    try {
      await matchOne(p.a, p.b, p.amount_ore);
      toast(kr(p.amount_ore) + " kr avstemt", true);
      done();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function applyAll() {
    const sum = suggestions.reduce((s, p) => s + p.amount_ore, 0);
    const ok = await bekreft(
      suggestions.length + " avstemminger på til sammen " + kr(sum) + " kr. De kan løses opp igjen under «Avstemt».",
      { tittel: "Avstem alle forslagene?", ok: "Avstem" },
    );
    if (!ok) return;
    busy = true;
    let done_ = 0;
    try {
      for (const p of suggestions) {
        await matchOne(p.a, p.b, p.amount_ore);
        done_++;
      }
      toast(done_ + " avstemminger lagret", true);
    } catch (e) {
      toast((done_ ? done_ + " lagret, så stoppet det: " : "") + e.message, false);
    }
    busy = false;
    done();
  }

  async function matchPicked(event) {
    event.preventDefault();
    if (!pairReady || amountProblem || busy) return;
    busy = true;
    try {
      await matchOne(pickedPos[0], pickedNeg[0], amountOre);
      toast(kr(amountOre) + " kr avstemt", true);
      done();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function undo(m) {
    const ok = await bekreft(
      "Avstemmingen mellom bilag " + m.voucher_a + " og " + m.voucher_b + " på " + kr(m.amount_ore) +
        " kr fjernes, og begge postene blir åpne igjen. Bilagene står urørt.",
      { tittel: "Løs opp avstemmingen?", ok: "Løs opp" },
    );
    if (!ok) return;
    try {
      await send("DELETE", base + "/reskontro/matches/" + m.match_id);
      toast("Avstemmingen er løst opp", true);
      done();
    } catch (e) {
      toast(e.message, false);
    }
  }

  function done() {
    picked = [];
    amount = "";
    load();
    onChanged();
  }

  const sideWord = (i) => (i.remaining_ore > 0 || (i.remaining_ore === 0 && i.amount_ore > 0) ? posWord : negWord);
</script>

{#if canMatch && suggestions.length}
  <section class="card border border-primary/40 bg-base-100 mb-4" aria-labelledby={"forslag-" + party.party_id}>
    <div class="card-body p-4 gap-2">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id={"forslag-" + party.party_id} class="font-semibold mr-auto">Forslag til avstemming</h3>
        {#if suggestions.length > 1}
          <button class="btn btn-sm" disabled={busy} onclick={applyAll}>Avstem alle ({suggestions.length})</button>
        {/if}
      </div>
      <ul class="grid gap-2">
        {#each suggestions as p (p.a.entry_id + p.b.entry_id)}
          <li class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
            <span class="mr-auto">
              {posWord} {p.a.voucher} ({formatDate(p.a.date)}) mot {negWord.toLowerCase()} {p.b.voucher} ({formatDate(p.b.date)})
              {#if !p.exact}<span class="opacity-70">— eldste først</span>{/if}
            </span>
            <span class="tabular-nums font-medium">{kr(p.amount_ore)} kr</span>
            <button class="btn btn-sm btn-primary" disabled={busy} onclick={() => applyPair(p)}>Avstem</button>
          </li>
        {/each}
      </ul>
    </div>
  </section>
{/if}

<div class="flex flex-wrap items-center gap-3 mb-2">
  <label class="label cursor-pointer gap-2">
    <input type="checkbox" class="checkbox checkbox-sm" bind:checked={openOnly} />
    <span class="text-base-content">Bare åpne poster</span>
  </label>
  {#if canMatch && open.length > 1}
    <span class="text-sm opacity-70">Huk av én {posWord.toLowerCase()} og én {negWord.toLowerCase()} for å avstemme selv.</span>
  {/if}
</div>

{#if items === null}
  <span class="loading loading-spinner loading-sm"></span>
{:else if items.length === 0}
  <p class="opacity-70">{openOnly ? "Ingen åpne poster." : "Ingen poster."}</p>
{:else}
  <div class="overflow-x-auto">
    <table class="table table-sm">
      <caption class="sr-only">Poster i reskontroen for {party.name}, eldste først.</caption>
      <thead>
        <tr>
          {#if canMatch}<th scope="col" class="w-px px-1 sm:px-3"><span class="sr-only">Velg</span></th>{/if}
          <th scope="col">Bilag</th>
          <th scope="col" class="hidden sm:table-cell">Dato</th>
          <th scope="col" class="text-right hidden md:table-cell">Beløp</th>
          <th scope="col" class="text-right">Åpent</th>
        </tr>
      </thead>
      <tbody>
        {#each items as i (i.entry_id)}
          <tr>
            {#if canMatch}
              <td class="px-1 sm:px-3">
                {#if i.remaining_ore !== 0}
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm"
                    aria-label={"Velg " + sideWord(i).toLowerCase() + " " + i.voucher}
                    checked={picked.includes(i.entry_id)}
                    onchange={() => toggle(i.entry_id)}
                  />
                {/if}
              </td>
            {/if}
            <td>
              <span class="font-medium">{sideWord(i)} {i.voucher}</span>
              <span class="block text-sm opacity-70">{i.description || ""}<span class="sm:hidden">{i.description ? ", " : ""}{formatDate(i.date)}</span></span>
            </td>
            <td class="hidden sm:table-cell whitespace-nowrap">{formatDate(i.date)}</td>
            <td class="text-right tabular-nums whitespace-nowrap hidden md:table-cell">{kr(i.amount_ore)}</td>
            <td class="text-right tabular-nums whitespace-nowrap">{i.remaining_ore ? kr(i.remaining_ore) : "avstemt"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if canMatch && pairReady}
  <form class="flex flex-wrap items-end gap-3 mt-3" onsubmit={matchPicked}>
    <div class="fieldset py-0">
      <label class="fieldset-legend pt-0" for={"belop-" + party.party_id}>Beløp å avstemme (kr)</label>
      <input
        id={"belop-" + party.party_id}
        class="input w-40 {amountProblem ? 'input-error' : ''}"
        inputmode="decimal"
        placeholder={kr(maxAmount)}
        bind:value={amount}
        aria-invalid={!!amountProblem}
        aria-describedby={"belop-hjelp-" + party.party_id}
      />
    </div>
    <button class="btn btn-primary" type="submit" disabled={busy || !!amountProblem}>Avstem</button>
    <p id={"belop-hjelp-" + party.party_id} class="text-sm w-full {amountProblem ? 'text-error' : 'opacity-70'}">
      {#if amountProblem}
        {amountProblem}
      {:else}
        {posWord} {pickedPos[0].voucher} mot {negWord.toLowerCase()} {pickedNeg[0].voucher}. Åpent etterpå:
        {kr(pickedPos[0].remaining_ore - amountOre)} og {kr(-(pickedNeg[0].remaining_ore + amountOre))} kr.
      {/if}
    </p>
  </form>
{:else if canMatch && pickedItems.length}
  <p class="text-sm opacity-70 mt-2">Velg nøyaktig én {posWord.toLowerCase()} og én {negWord.toLowerCase()}.</p>
{/if}

{#if matches?.length}
  <details class="mt-4">
    <summary class="cursor-pointer font-medium">Avstemt ({matches.length})</summary>
    <ul class="grid gap-1 text-sm mt-2">
      {#each matches as m (m.match_id)}
        <li class="flex flex-wrap items-center gap-x-3">
          <span class="mr-auto">Bilag {m.voucher_a} mot {m.voucher_b}, avstemt av {m.matched_by} {formatDate(m.created_at)}</span>
          <span class="tabular-nums">{kr(m.amount_ore)} kr</span>
          {#if canMatch && !m.currency_settlement}
            <button class="btn btn-ghost btn-xs" onclick={() => undo(m)}>Løs opp</button>
          {:else if m.currency_settlement}
            <span class="text-xs opacity-70" title="Rettes ved å reversere kursdifferansebilaget">valutaoppgjør</span>
          {/if}
        </li>
      {/each}
    </ul>
    {#if matches.length >= 200}
      <p class="text-sm opacity-70 mt-1">Viser de 200 nyeste.</p>
    {/if}
  </details>
{/if}
