<script>
  // Daily cash settlement (#89, docs/kontantsalg.md): the day's Z-report
  // as one voucher with sales split per VAT rate, and the cash difference
  // as a voucher of its own. regnmed is not a cash register — this posts
  // the settlement from one. Amounts go through oreOrNull: "12 500,00" is
  // twelve and a half thousand, never twelve.
  import { api, post } from "../../lib/api.js";
  import { kr, oreOrNull, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import VatSelect from "../../components/VatSelect.svelte";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId);
  let chart = $state({ kontoer: [], standard: [] });
  let date = $state(today());
  let zNumber = $state("");
  let sales = $state([{ konto: "3000", vat_code: "3", belop: "" }]);
  let payments = $state([{ konto: "1900", belop: "" }]);
  let cashAccount = $state("1900");
  let counted = $state("");
  let file = $state(null);
  let busy = $state(false);
  let submitted = $state(false);
  let canPost = $derived(harRett(companyId, "BILAG_BOKFOR"));

  $effect(() => {
    api(base + "/accounts")
      .then((a) => (chart = { kontoer: a.kontoer || [], standard: a.standard || [] }))
      .catch(() => {});
  });

  const sum = (rows) => rows.reduce((s, r) => s + (oreOrNull(r.belop) ?? 0), 0);
  let salesSum = $derived(sum(sales));
  let paymentSum = $derived(sum(payments));
  let badAmount = $derived([...sales, ...payments].some((r) => r.belop.trim() && oreOrNull(r.belop) === null));
  let countedOre = $derived(counted.trim() ? oreOrNull(counted) : null);
  let problem = $derived(
    !zNumber.trim()
      ? "Skriv Z-nummeret fra kassasystemet"
      : badAmount || (counted.trim() && countedOre === null)
        ? "Et beløp er ikke et tall"
        : salesSum <= 0
          ? "Skriv salget per sats"
          : salesSum !== paymentSum
            ? "Salget (" + kr(salesSum) + ") og betalingsmidlene (" + kr(paymentSum) + ") må være like. En differanse i kassen føres som opptalt kontant, ikke her."
            : null,
  );

  async function submit(event) {
    event.preventDefault();
    submitted = true;
    if (problem || busy) return;
    busy = true;
    try {
      const body = {
        dato: date,
        z_nummer: zNumber.trim(),
        salg: sales
          .filter((l) => oreOrNull(l.belop))
          .map((l) => ({ konto: l.konto, vat_code: l.vat_code || null, brutto_ore: oreOrNull(l.belop) })),
        betaling: payments.filter((b) => oreOrNull(b.belop)).map((b) => ({ konto: b.konto, belop_ore: oreOrNull(b.belop) })),
        kontantkonto: countedOre === null ? null : cashAccount,
        opptalt_kontant_ore: countedOre,
      };
      const r = file
        ? await api(base + "/kassaoppgjor/rapport?filename=" + encodeURIComponent(file.name), {
            method: "POST",
            headers: { "content-type": file.type || "application/octet-stream", "x-dagsoppgjor": JSON.stringify(body) },
            body: file,
          })
        : await post(base + "/kassaoppgjor", body);
      toast(
        r.differansebilag
          ? "Bokført som bilag " + r.bilag + "; kassedifferanse " + kr(r.differanse_ore) + " kr som bilag " + r.differansebilag
          : "Bokført som bilag " + r.bilag,
        true,
      );
      zNumber = "";
      sales = [{ konto: "3000", vat_code: "3", belop: "" }];
      payments = [{ konto: "1900", belop: "" }];
      counted = "";
      file = null;
      submitted = false;
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<form class="card border border-base-300 bg-base-100 max-w-4xl" novalidate onsubmit={submit}>
  <div class="card-body gap-4">
    <h2 class="card-title">Kassaoppgjør</h2>
    <p class="text-sm opacity-70">
      Dagsoppgjøret fra kassasystemet som ett bilag, med salget splittet per mva-sats (bokføringsforskriften § 5-3 og § 5-4).
      Z-nummeret knytter bilaget til kassasystemets egen nummererte rapport. <strong>regnmed er ikke et kassasystem</strong>; dette
      bokfører oppgjøret fra ett.
    </p>

    <div class="grid gap-3 sm:grid-cols-[11rem_12rem]">
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="kas-dato">Dato</label>
        <input id="kas-dato" type="date" class="input w-full" bind:value={date} />
      </div>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="kas-z">Z-nummer</label>
        <input id="kas-z" class="input w-full" autocomplete="off" bind:value={zNumber} />
      </div>
    </div>

    <fieldset class="grid gap-2">
      <legend class="font-semibold mb-1">Salg per mva-sats (brutto, med mva)</legend>
      {#each sales as row, i (i)}
        <div class="grid gap-2 grid-cols-2 sm:grid-cols-[8rem_12rem_10rem_auto] items-end">
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"kas-sk-" + i}>Salgskonto</label>
            <KontoVelger id={"kas-sk-" + i} kontoer={chart.kontoer} standard={chart.standard} bind:value={row.konto} cls="input w-full" />
          </div>
          <label class="fieldset py-0">
            <span class="fieldset-legend pt-0">Mva</span>
            <VatSelect cls="select w-full" bind:value={row.vat_code} />
          </label>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"kas-sb-" + i}>Beløp (kr)</label>
            <input id={"kas-sb-" + i} class="input w-full text-right" inputmode="decimal" bind:value={row.belop} />
          </div>
          {#if sales.length > 1}
            <button type="button" class="btn btn-ghost btn-square" aria-label={"Fjern salgslinje " + (i + 1)} onclick={() => (sales = sales.filter((_, j) => j !== i))}>✕</button>
          {/if}
        </div>
      {/each}
      <button type="button" class="btn btn-sm w-fit" onclick={() => (sales = [...sales, { konto: "3000", vat_code: "31", belop: "" }])}>Legg til sats</button>
    </fieldset>

    <fieldset class="grid gap-2">
      <legend class="font-semibold mb-1">Betalingsmidler</legend>
      {#each payments as row, i (i)}
        <div class="grid gap-2 grid-cols-2 sm:grid-cols-[8rem_10rem_auto] items-end">
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"kas-bk-" + i}>Konto</label>
            <KontoVelger id={"kas-bk-" + i} kontoer={chart.kontoer} standard={chart.standard} bind:value={row.konto} cls="input w-full" />
          </div>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"kas-bb-" + i}>Beløp (kr)</label>
            <input id={"kas-bb-" + i} class="input w-full text-right" inputmode="decimal" bind:value={row.belop} />
          </div>
          {#if payments.length > 1}
            <button type="button" class="btn btn-ghost btn-square" aria-label={"Fjern betalingsmiddel " + (i + 1)} onclick={() => (payments = payments.filter((_, j) => j !== i))}>✕</button>
          {/if}
        </div>
      {/each}
      <button type="button" class="btn btn-sm w-fit" onclick={() => (payments = [...payments, { konto: "1920", belop: "" }])}>Legg til betalingsmiddel</button>
      <span class="text-sm opacity-70">Kontant 1900, kort 1571, bank 1920.</span>
    </fieldset>

    <p class="tabular-nums {salesSum === paymentSum ? 'opacity-70' : 'text-error'}" role="status" aria-live="polite">
      Salg {kr(salesSum)} kr · betalingsmidler {kr(paymentSum)} kr
    </p>

    <fieldset class="grid gap-2 sm:grid-cols-[8rem_10rem]">
      <legend class="font-semibold mb-1">Opptalt kontant (valgfritt)</legend>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="kas-kk">Kontantkonto</label>
        <KontoVelger id="kas-kk" kontoer={chart.kontoer} standard={chart.standard} bind:value={cashAccount} cls="input w-full" />
      </div>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="kas-opp">Opptalt (kr)</label>
        <input id="kas-opp" class="input w-full text-right" inputmode="decimal" bind:value={counted} />
      </div>
      <span class="text-sm opacity-70 sm:col-span-2">Tomt = kassen ble ikke talt. Da bokføres ingen differanse; vi antar aldri at den stemte.</span>
    </fieldset>

    <div class="fieldset py-0">
      <label class="fieldset-legend pt-0" for="kas-fil">Z-rapporten (valgfritt)</label>
      <input id="kas-fil" type="file" class="file-input w-full max-w-md" onchange={(e) => (file = e.currentTarget.files[0] || null)} />
      <span class="label">Legges ved bilaget som dokumentasjon.</span>
    </div>

    {#if submitted && problem}<p class="text-error text-sm" role="alert">{problem}</p>{/if}
    {#if canPost}
      <button class="btn btn-primary w-fit" type="submit" disabled={busy}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        Bokfør dagsoppgjøret
      </button>
    {/if}
  </div>
</form>
