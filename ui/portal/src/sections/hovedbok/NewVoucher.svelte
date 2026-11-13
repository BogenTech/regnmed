<script>
  // A manual voucher: debit and credit columns, a VAT code that splits the
  // gross amount out onto 2700/2710 (voucherMath.js), the lines that will
  // be posted shown before posting, and the button busy until the answer
  // arrives — a posted voucher is never deleted, only reversed, so one
  // press must be one voucher. A document belongs in Bilag, where the
  // voucher gets it as an attachment.
  import { api, post } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import DimSelect from "../../components/DimSelect.svelte";
  import { VAT, lineOre, postingLines } from "./voucherMath.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId);
  const blank = () => ({ account: "", debit: "", credit: "", vat: "", avdeling: "", prosjekt: "" });

  let chart = $state({ kontoer: [], standard: [] });
  let dims = $state([]);
  let policy = $state(null);
  let date = $state(today());
  let description = $state("");
  let lines = $state([blank(), blank()]);
  let busy = $state(false);
  let submitted = $state(false);

  $effect(() => {
    api(base + "/accounts")
      .then((a) => (chart = { kontoer: a.kontoer || [], standard: a.standard || [] }))
      .catch(() => {});
    api(base + "/dimensions")
      .then((d) => (dims = d.dimensions || []))
      .catch(() => {});
    api(base + "/attestering/policy")
      .then((p) => (policy = p.policy?.aktiv ? p.policy : null))
      .catch(() => {});
  });

  const own = (nr) => chart.kontoer.find((k) => k.number === nr);
  const known = (nr) => own(nr)?.active === true;
  const inCatalog = (nr) => chart.standard.some((s) => s.number === nr);
  const nameOf = (nr) => [...chart.kontoer, ...chart.standard].find((k) => k.number === nr)?.name;

  let posting = $derived(postingLines(lines));
  let debit = $derived(posting.reduce((s, l) => s + Math.max(l.amount_ore, 0), 0));
  let credit = $derived(posting.reduce((s, l) => s + Math.max(-l.amount_ore, 0), 0));
  let hasVat = $derived(lines.some((l) => VAT[l.vat]));

  // Per line: what is wrong with it, if anything.
  let lineProblems = $derived(
    lines.map((l) => {
      const nr = l.account.trim();
      const ore = lineOre(l);
      if (!nr && ore === null) return null;
      if (!nr) return "Velg konto";
      if (own(nr) && !own(nr).active) return "Konto " + nr + " er deaktivert. Aktiver den under Kontoplan først.";
      if (!own(nr) && !inCatalog(nr)) return "Konto " + nr + " finnes ikke. Legg den til under Kontoplan.";
      if (ore === null) return "Skriv et beløp i debet eller kredit";
      if (Number.isNaN(ore)) return "Ett positivt beløp, i debet eller kredit";
      return null;
    }),
  );
  let problem = $derived(
    !description.trim()
      ? "Skriv hva bilaget gjelder"
      : lineProblems.find(Boolean) ||
        (posting.length < 2
          ? "Et bilag trenger minst to linjer"
          : debit !== credit
            ? "Debet og kredit må være like (differanse " + kr(debit - credit) + " kr)"
            : null),
  );

  function remove(i) {
    lines = lines.filter((_, j) => j !== i);
    if (lines.length < 2) lines = [...lines, blank()];
  }

  async function submit(event) {
    event.preventDefault();
    submitted = true;
    if (problem || busy) return;
    busy = true;
    try {
      // A standard account joins the company's chart the first time it is used.
      for (const nr of new Set(posting.map((l) => l.account))) {
        if (!own(nr) && inCatalog(nr)) await post(base + "/accounts", { number: nr });
      }
      const r = await post(base + "/vouchers", {
        journal_code: "GL",
        date,
        description: description.trim(),
        lines: posting,
      });
      toast("Bokført som bilag " + r.voucher, true);
      location.hash = "#/c/" + companyId + "/hovedbok/" + r.voucher_id;
    } catch (e) {
      toast(e.message, false);
      busy = false;
    }
  }
</script>

<form class="card border border-base-300 bg-base-100 max-w-4xl" novalidate onsubmit={submit}>
  <div class="card-body gap-4">
    <p class="text-sm opacity-70">
      For posteringer uten eget dokument: avskrivninger, omposteringer, korrigeringer. Har bilaget et dokument (kvittering,
      faktura), last det opp under <a class="link" href={"#/c/" + companyId + "/bilag"}>Bilag</a>, så følger dokumentet bilaget.
    </p>
    {#if policy}
      <div class="alert alert-info alert-soft text-sm" role="status">
        <span>
          Attestering er på:
          {policy.belopsgrense_ore != null
            ? "bilag på " + kr(policy.belopsgrense_ore) + " kr eller mer kan ikke føres her,"
            : "manuelle bilag kan ikke føres her,"}
          de må lastes opp under Bilag og attesteres av en annen.
        </span>
      </div>
    {/if}

    <div class="grid gap-3 sm:grid-cols-[11rem_1fr]">
      <div class="fieldset py-0">
        <label class="fieldset-legend" for="nb-dato">Dato</label>
        <input id="nb-dato" type="date" class="input w-full" bind:value={date} />
      </div>
      <div class="fieldset py-0">
        <label class="fieldset-legend" for="nb-tekst">Tekst</label>
        <input
          id="nb-tekst"
          class="input w-full {submitted && !description.trim() ? 'input-error' : ''}"
          placeholder="F.eks. Avskrivning varebil november"
          bind:value={description}
        />
      </div>
    </div>

    <fieldset class="grid gap-3">
      <legend class="font-semibold mb-1">Linjer</legend>
      {#each lines as line, i (i)}
        {@const lineProblem = lineProblems[i]}
        <div class="grid gap-2 grid-cols-2 sm:grid-cols-[8rem_1fr_1fr_11rem_auto] items-end border-b border-base-200 pb-3">
          <div class="fieldset py-0 col-span-2 sm:col-span-1">
            <label class="fieldset-legend pt-0" for={"nb-konto-" + i}>Konto</label>
            <KontoVelger id={"nb-konto-" + i} kontoer={chart.kontoer} standard={chart.standard} bind:value={line.account} cls="input w-full" />
          </div>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"nb-debet-" + i}>Debet</label>
            <input id={"nb-debet-" + i} class="input w-full text-right" inputmode="decimal" autocomplete="off" bind:value={line.debit} />
          </div>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"nb-kredit-" + i}>Kredit</label>
            <input id={"nb-kredit-" + i} class="input w-full text-right" inputmode="decimal" autocomplete="off" bind:value={line.credit} />
          </div>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for={"nb-mva-" + i}>Mva</label>
            <select id={"nb-mva-" + i} class="select w-full" bind:value={line.vat}>
              <option value="">Ingen mva</option>
              {#each Object.entries(VAT) as [code, v] (code)}
                <option value={code}>{v.label}</option>
              {/each}
            </select>
          </div>
          <button
            type="button"
            class="btn btn-ghost btn-square"
            aria-label={"Fjern linje " + (i + 1)}
            title="Fjern linjen"
            onclick={() => remove(i)}
          >
            ✕
          </button>
          {#if dims.length}
            <div class="col-span-2 sm:col-span-5 flex flex-wrap gap-2">
              <label class="flex items-center gap-2 text-sm">
                <span class="opacity-70">Avdeling</span>
                <DimSelect {dims} kind="avdeling" cls="select select-sm" alleLabel="Ingen" bind:value={line.avdeling} />
              </label>
              <label class="flex items-center gap-2 text-sm">
                <span class="opacity-70">Prosjekt</span>
                <DimSelect {dims} kind="prosjekt" cls="select select-sm" alleLabel="Ingen" bind:value={line.prosjekt} />
              </label>
            </div>
          {/if}
          <p class="text-sm col-span-2 sm:col-span-5 {lineProblem && (submitted || line.account.trim()) ? 'text-error' : 'opacity-70'}">
            {#if lineProblem && (submitted || line.account.trim())}
              {lineProblem}
            {:else if line.account.trim() && nameOf(line.account.trim())}
              {nameOf(line.account.trim())}{VAT[line.vat] ? ". Beløpet er inkludert mva." : ""}
            {/if}
          </p>
        </div>
      {/each}
      <button type="button" class="btn btn-sm w-fit" onclick={() => (lines = [...lines, blank()])}>Legg til linje</button>
    </fieldset>

    {#if hasVat && posting.length}
      <div class="overflow-x-auto">
        <table class="table table-sm">
          <caption class="text-left text-sm opacity-70 pb-1">Dette bokføres (mva skilt ut)</caption>
          <thead><tr><th scope="col">Konto</th><th scope="col" class="text-right">Debet</th><th scope="col" class="text-right">Kredit</th></tr></thead>
          <tbody>
            {#each posting as l, i (i)}
              <tr>
                <td><span class="font-mono">{l.account}</span> {nameOf(l.account) || ""}{l.vat_code ? " · mva-kode " + l.vat_code : ""}</td>
                <td class="text-right tabular-nums">{l.amount_ore > 0 ? kr(l.amount_ore) : ""}</td>
                <td class="text-right tabular-nums">{l.amount_ore < 0 ? kr(-l.amount_ore) : ""}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    <div class="flex flex-wrap items-center gap-3">
      <p class="tabular-nums mr-auto" role="status" aria-live="polite">
        Debet {kr(debit)} · Kredit {kr(credit)}
        {#if debit !== credit}<span class="text-error"> · Differanse {kr(debit - credit)}</span>{/if}
      </p>
      <button class="btn btn-primary" type="submit" disabled={busy}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        Bokfør
      </button>
    </div>
    {#if submitted && problem}
      <p class="text-error text-sm" role="alert">{problem}</p>
    {/if}
  </div>
</form>
