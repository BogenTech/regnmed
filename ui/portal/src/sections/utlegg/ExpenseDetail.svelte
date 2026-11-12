<script>
  // Ett krav: kvitteringen ved siden av beslutningen (stablet på telefon).
  // Den som har sendt inn ser hvor kravet står; den som godkjenner ser
  // kvitteringen og hva som bokføres før det skjer. Etter godkjenning
  // eller avvisning går siden rett til neste krav som venter.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { skjema, sporsmal } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { previewOf } from "../../lib/preview.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import { INPUT_VAT, splitGross } from "../bilag/bookingMath.js";
  import { STATUS, REPRESENTATION_ACCOUNTS } from "./expenseMath.js";

  let { companyId, expenseId } = $props();

  const VAT_CHOICES = [["", "Ingen fradrag"], ...["1", "11", "13"].map((c) => [c, c + " — " + INPUT_VAT[c].label])];

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");

  let e = $state(null);
  let error = $state(null);
  let preview = $state(null);
  let accounts = $state({ kontoer: [], standard: [] });
  let account = $state("");
  let vatCode = $state("");
  let busy = $state(false);

  let canApprove = $derived(harRett(companyId, "UTLEGG_GODKJENN"));
  let canPay = $derived(harRett(companyId, "UTLEGG_UTBETAL"));
  let receiptUrl = $derived(base + "/" + expenseId + "/receipt");

  function load() {
    return api(base + "/" + expenseId)
      .then((r) => {
        e = r;
        if (!account) account = r.kind === "kjoring" ? "7100" : "7790";
      })
      .catch((err) => (error = err.message));
  }

  $effect(() => {
    load();
    if (canApprove) {
      api("/companies/" + companyId + "/accounts")
        .then((a) => (accounts = { kontoer: a.kontoer || [], standard: a.standard || [] }))
        .catch(() => {});
    }
  });

  // Only an utlegg has a receipt; the preview trusts the bytes, not the name.
  $effect(() => {
    if (e?.kind !== "utlegg") return;
    let url = null;
    api(receiptUrl)
      .then((r) => r.blob())
      .then(previewOf)
      .then((p) => {
        url = p.url || null;
        preview = p;
      })
      .catch(() => (preview = { kind: "none" }));
    return () => url && URL.revokeObjectURL(url);
  });

  let accountName = $derived(
    [...accounts.kontoer, ...accounts.standard].find((k) => k.number === account.trim())?.name,
  );

  // What approval will post, in the reader's terms. The server computes
  // the VAT with the rate valid on the claim's date; for these codes that
  // equals today's.
  let lines = $derived.by(() => {
    if (!e) return [];
    const rate = e.kind === "utlegg" && vatCode ? INPUT_VAT[vatCode].bp : 0;
    const { base: net, vat } = splitGross(e.belop_ore, rate);
    const out = [{ account: account || "?", text: accountName || "Kostnad", debit: rate ? net : e.belop_ore }];
    if (rate) out.push({ account: "2710", text: "Inngående mva", debit: vat });
    out.push({ account: "2910", text: "Skyldig til " + e.person, credit: e.belop_ore });
    return out;
  });
  let representation = $derived(!!vatCode && REPRESENTATION_ACCOUNTS.includes(account.trim()));

  async function goNext() {
    let next = null;
    try {
      const r = await api(base + "?status=innsendt&limit=2");
      next = r.expenses.find((x) => x.expense_id !== expenseId) || null;
    } catch {}
    location.hash = next ? list + "/" + next.expense_id : list;
  }

  async function approve(event) {
    event.preventDefault();
    if (busy || !account.trim()) return;
    busy = true;
    try {
      const nr = account.trim();
      // A catalog account becomes the company's the first time it is used.
      if (!accounts.kontoer.some((k) => k.number === nr) && accounts.standard.some((s) => s.number === nr)) {
        await post("/companies/" + companyId + "/accounts", { number: nr });
      }
      const r = await post(base + "/" + expenseId + "/approve", {
        konto: nr,
        ...(e.kind === "utlegg" && vatCode ? { mva_kode: vatCode } : {}),
      });
      toast("Godkjent og bokført som bilag " + r.voucher, true);
      if (r.warning) toast(r.warning, false);
      goNext();
    } catch (err) {
      toast(err.message, false);
      busy = false;
    }
  }

  async function reject() {
    const note = await sporsmal("Hvorfor avvises kravet?", {
      type: "textarea",
      ok: "Avvis kravet",
      farlig: true,
      melding: e.person + " ser begrunnelsen og kan sende inn på nytt.",
    });
    if (!note) return;
    try {
      await post(base + "/" + expenseId + "/reject", { note });
      toast("Kravet er avvist", true);
      goNext();
    } catch (err) {
      toast(err.message, false);
    }
  }

  async function pay() {
    const answer = await skjema(
      "Utbetal " + kr(e.belop_ore) + " kr til " + e.person,
      [
        { navn: "dato", etikett: "Utbetalingsdato", type: "date", standard: today() },
        { navn: "konto", etikett: "Betalt fra bankkonto", standard: "1920", hjelp: "Kontonummer i kontoplanen" },
      ],
      {
        ok: "Bokfør utbetalingen",
        melding:
          "Bokfør dette når pengene er overført. Gjelden til " + e.person + " på 2910 nulles mot banken, og det kan ikke angres.",
      },
    );
    if (!answer) return;
    busy = true;
    try {
      const r = await post(base + "/" + expenseId + "/pay", { dato: answer.dato, konto: answer.konto });
      toast("Utbetalingen er bokført som bilag " + r.voucher, true);
      await load();
    } catch (err) {
      toast(err.message, false);
    }
    busy = false;
  }
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={list}>← Alle utlegg</a>
</nav>

{#if error}
  <div class="alert alert-error" role="alert">
    <span>{error}</span>
    <a class="btn btn-sm" href={list}>Til utleggene</a>
  </div>
{:else if !e}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  {@const s = STATUS[e.status]}
  <div class="flex flex-wrap items-start gap-3 mb-4">
    <div class="mr-auto min-w-0">
      <h1 class="text-2xl font-bold break-words">{e.beskrivelse}</h1>
      <p class="opacity-70">
        {e.kind === "kjoring" ? "Kjøring" : "Utlegg"} {formatDate(e.dato)}{e.own ? "" : ", sendt inn av " + e.person}
      </p>
    </div>
    <p class="text-2xl font-bold tabular-nums whitespace-nowrap">{kr(e.belop_ore)} kr</p>
  </div>

  <div class="alert alert-soft mb-4 {e.status === 'avvist' ? 'alert-error' : e.status === 'utbetalt' ? 'alert-success' : 'alert-info'}" role="status">
    <div>
      <strong>{s?.text || e.status}.</strong>
      {#if e.status === "innsendt"}
        {e.own ? "Den som godkjenner utlegg ser kravet nå." : ""}
      {:else if e.status === "godkjent"}
        Godkjent{e.decided_by ? " av " + e.decided_by : ""}{e.decided_at ? " " + formatDate(e.decided_at) : ""} og bokført som bilag {e.voucher}.
      {:else if e.status === "utbetalt"}
        Utbetalt {formatDate(e.utbetalt_at)}, bilag {e.utbetalt_voucher}. Godkjent som bilag {e.voucher}.
      {:else if e.status === "avvist"}
        {e.decided_by ? e.decided_by + " skrev: " : ""}«{e.avvist_note}»
        {#if e.own}<a class="link" href={list + "/ny"}>Send inn på nytt</a>{/if}
      {/if}
    </div>
  </div>

  <div class="grid gap-6 items-start {e.kind === 'utlegg' ? 'lg:grid-cols-2' : ''}">
    {#if e.kind === "utlegg"}
      <section class="card border border-base-300 bg-base-100 lg:sticky lg:top-4" aria-label="Kvitteringen">
        <div class="card-body p-2">
          {#if preview === null}
            <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
          {:else if preview.kind === "pdf"}
            <iframe src={preview.url} title={"Kvittering: " + e.receipt_filename} class="w-full h-[60vh] lg:h-[75vh] rounded"></iframe>
          {:else if preview.kind === "image"}
            <img src={preview.url} alt={"Kvittering: " + e.receipt_filename} class="w-full max-h-[75vh] object-contain rounded" />
          {:else}
            <p class="opacity-70 p-6 text-center">Kvitteringen kan ikke vises her.</p>
          {/if}
          <button class="btn btn-ghost btn-sm w-fit" onclick={() => download(receiptUrl, e.receipt_filename)}>
            Last ned {e.receipt_filename}
          </button>
        </div>
      </section>
    {/if}

    <div class="grid gap-6">
      {#if e.kind === "kjoring"}
        <section class="card border border-base-300 bg-base-100" aria-labelledby="kjoring-tittel">
          <div class="card-body">
            <h2 id="kjoring-tittel" class="card-title">Beregning</h2>
            <p>{e.km} km × {kr(e.sats_ore_per_km)} kr (statens sats {formatDate(e.dato)}) = {kr(e.belop_ore)} kr</p>
            {#if e.trekkpliktig_ore > 0}
              <div class="alert alert-warning alert-soft text-sm">
                <span>
                  {kr(e.trekkfri_ore)} kr er trekkfritt. De resterende {kr(e.trekkpliktig_ore)} kr er trekkpliktige og
                  skal lønnsinnberettes. De kommer ikke med i a-meldingen av seg selv ennå, så de må meldes særskilt.
                </span>
              </div>
            {/if}
          </div>
        </section>
      {/if}

      {#if e.status === "innsendt" && canApprove}
        <form class="card border border-base-300 bg-base-100" aria-labelledby="godkjenn-tittel" onsubmit={approve}>
          <div class="card-body grid gap-4">
            <h2 id="godkjenn-tittel" class="card-title">Godkjenn og bokfør</h2>
            <div class="grid gap-3 sm:grid-cols-2">
              <div class="fieldset">
                <label class="fieldset-legend" for="godkjenn-konto">Kostnadskonto</label>
                <KontoVelger id="godkjenn-konto" kontoer={accounts.kontoer} standard={accounts.standard} bind:value={account} cls="input w-full" />
                <span class="label whitespace-normal">{accountName || "Skriv nummer eller navn"}</span>
              </div>
              {#if e.kind === "utlegg"}
                <label class="fieldset">
                  <span class="fieldset-legend">Inngående mva</span>
                  <select class="select w-full" bind:value={vatCode}>
                    {#each VAT_CHOICES as [code, label] (code)}
                      <option value={code}>{label}</option>
                    {/each}
                  </select>
                  <span class="label whitespace-normal">Bare når kvitteringen viser mva og selskapet kan trekke den fra</span>
                </label>
              {/if}
            </div>
            {#if representation}
              <div class="alert alert-warning alert-soft text-sm" role="status">
                <span>Mva på representasjon gir ikke fradrag (mval. § 8-3). Velg «Ingen fradrag» om dette er representasjon.</span>
              </div>
            {/if}
            <table class="table table-sm">
              <caption class="text-left text-sm opacity-70 pb-1">Dette bokføres</caption>
              <thead><tr><th scope="col">Konto</th><th scope="col" class="text-right">Debet</th><th scope="col" class="text-right">Kredit</th></tr></thead>
              <tbody>
                {#each lines as l (l.account + l.text)}
                  <tr>
                    <td><span class="font-mono">{l.account}</span> <span class="opacity-70">{l.text}</span></td>
                    <td class="text-right tabular-nums">{l.debit ? kr(l.debit) : ""}</td>
                    <td class="text-right tabular-nums">{l.credit ? kr(l.credit) : ""}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <div class="flex flex-wrap gap-2">
              <button class="btn btn-primary" type="submit" disabled={busy || !account.trim()}>
                {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
                Godkjenn og bokfør
              </button>
              <button class="btn btn-ghost text-error" type="button" onclick={reject}>Avvis kravet</button>
            </div>
          </div>
        </form>
      {:else if e.status === "godkjent" && canPay}
        <section class="card border border-base-300 bg-base-100" aria-labelledby="utbetal-tittel">
          <div class="card-body">
            <h2 id="utbetal-tittel" class="card-title">Utbetaling</h2>
            <p>Overfør {kr(e.belop_ore)} kr til {e.person}, og bokfør utbetalingen når pengene er sendt.</p>
            <button class="btn btn-primary w-fit" disabled={busy} onclick={pay}>Bokfør utbetalingen</button>
          </div>
        </section>
      {/if}
    </div>
  </div>
{/if}
