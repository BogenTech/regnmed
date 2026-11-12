<script>
  // Exchange rates: daily rates from Norges Bank, or entered by hand with
  // their date. Invoices in foreign currency are posted in NOK at the
  // rate on the invoice date, and the rate is part of the voucher.
  import { api, post } from "../../lib/api.js";
  import { today, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { skjema } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/currency/rates");
  const CURRENCIES = ["EUR", "USD", "SEK", "DKK", "GBP"];

  let rates = $state(null);
  let currency = $state("");
  let date = $state(today());
  let rate = $state("");
  let busy = $state(false);
  let canWrite = $derived(harRett(companyId, "VALUTA_SKRIV"));

  function load() {
    api(base)
      .then((r) => (rates = r.rates))
      .catch(() => (rates = []));
  }
  $effect(load);

  // The API lists per currency, newest date first: the first row of each
  // currency is its current rate.
  let latest = $derived((rates || []).filter((r, i, all) => i === 0 || all[i - 1].valuta !== r.valuta));

  async function fetchRates() {
    const answer = await skjema(
      "Hent dagskurser fra Norges Bank",
      CURRENCIES.map((c) => ({ navn: c, etikett: c, type: "checkbox", standard: c !== "DKK" && c !== "GBP" })),
      { ok: "Hent", melding: "Kursene for de siste dagene hentes og lagres med Norges Bank som kilde." },
    );
    if (!answer) return;
    const chosen = CURRENCIES.filter((c) => answer[c]);
    if (!chosen.length) return toast("Velg minst én valuta", false);
    busy = true;
    try {
      const r = await post(base + "/fetch", { valutaer: chosen });
      toast(r.fetched + " kurser hentet", true);
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function addRate(event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    try {
      await post(base, { valuta: currency.trim().toUpperCase(), dato: date, kurs: rate.trim().replace(",", ".") });
      toast("Kursen er lagret", true);
      currency = "";
      rate = "";
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="kurs-tittel">
  <div class="card-body">
    <div class="flex flex-wrap items-center gap-3">
      <h2 id="kurs-tittel" class="card-title mr-auto">Valutakurser</h2>
      {#if canWrite}
        <button class="btn btn-primary btn-sm" disabled={busy} onclick={fetchRates}>Hent fra Norges Bank</button>
      {/if}
    </div>
    <p class="text-sm opacity-70">
      Fakturaer i valuta bokføres i kroner til kursen på fakturadatoen, og kursen blir en del av bilaget.
    </p>

    {#if rates === null}
      <span class="loading loading-spinner"></span>
    {:else if latest.length === 0}
      <p class="opacity-70">Ingen kurser ennå.</p>
    {:else}
      <table class="table table-sm mt-2">
        <caption class="text-left text-sm opacity-70 pb-1">Siste kurs per valuta ({rates.length} {rates.length === 1 ? "kurs" : "kurser"} lagret)</caption>
        <thead>
          <tr><th scope="col">Valuta</th><th scope="col" class="text-right">Kurs</th><th scope="col">Dato</th><th scope="col" class="hidden sm:table-cell">Kilde</th></tr>
        </thead>
        <tbody>
          {#each latest as r (r.valuta)}
            <tr>
              <td class="font-mono">{r.valuta}</td>
              <td class="text-right tabular-nums">{String(r.kurs).replace(".", ",")}</td>
              <td class="whitespace-nowrap">{formatDate(r.dato)}</td>
              <td class="hidden sm:table-cell text-sm opacity-70">{r.kilde}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    {#if canWrite}
      <form class="grid gap-3 sm:grid-cols-[6rem_11rem_9rem_auto] items-end mt-2" onsubmit={addRate} aria-label="Legg inn kurs for hånd">
        <div class="fieldset py-0">
          <label class="fieldset-legend" for="kurs-valuta">Valuta</label>
          <input id="kurs-valuta" class="input w-full uppercase" maxlength="3" autocomplete="off" placeholder="EUR" bind:value={currency} />
        </div>
        <div class="fieldset py-0">
          <label class="fieldset-legend" for="kurs-dato">Dato</label>
          <input id="kurs-dato" type="date" class="input w-full" bind:value={date} />
        </div>
        <div class="fieldset py-0">
          <label class="fieldset-legend" for="kurs-kurs">Kurs i kroner</label>
          <input id="kurs-kurs" class="input w-full" inputmode="decimal" autocomplete="off" placeholder="11,6543" bind:value={rate} />
        </div>
        <button class="btn" type="submit" disabled={busy || !currency.trim() || !rate.trim()}>Lagre kurs</button>
      </form>
    {/if}
  </div>
</section>
