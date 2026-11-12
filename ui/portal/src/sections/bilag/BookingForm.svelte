<script>
  // Bokføring av ett innboksdokument. Den ENKLE formen spør om det et
  // menneske ser på kvitteringen — hvem, hvor mye inkl. mva, hvilken
  // sats, hva det var, hvordan det ble betalt — og viser linjene den
  // bokfører. «Avansert» gir regnskapsføreren linjene direkte. Begge
  // går til samme endepunkt, som bokfører bilag, vedlegg og status i én
  // transaksjon (docs/bilagsinnboks.md). Et forslag (#34) fyller ut, men
  // ingenting bokføres før noen trykker.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { isValidKid } from "../../lib/kid.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import { INPUT_VAT, PAYMENT, simpleLines, guessVatCode } from "./bookingMath.js";

  let {
    companyId,
    documentId,
    dims = [],
    kontoer = [],
    standard = [],
    suppliers = [],
    forslag = null,
    onBooked,
  } = $props();

  // ---- Seeded once from the suggestion ----
  const f = untrack(() => forslag);
  let date = $state(f?.dato || today());
  let description = $state(
    f ? (f.selger_navn || f.leverandor_navn || "Bilag") + (f.fakturanr ? " faktura " + f.fakturanr : "") : "",
  );
  let supplierNo = $state(f?.leverandor_no || "");
  let payment = $state(f && (f.leverandor_no || f.forfall || f.kid) ? "faktura" : "bank");
  let gross = $state(f?.brutto_ore ? kr(f.brutto_ore) : "");
  // A VAT figure that is not part of the total was misread; ignore it.
  const vatOk = f && f.mva_ore > 0 && f.brutto_ore > f.mva_ore;
  let vatCode = $state(f ? (vatOk ? guessVatCode(f.brutto_ore - f.mva_ore, f.mva_ore) : "1") : "1");
  let costAccount = $state(f?.konto || "");
  let avdeling = $state("");
  let prosjekt = $state("");
  let dueDate = $state(f?.forfall || "");
  let kid = $state(f?.kid || "");

  let advanced = $state(false);
  let rows = $state([]); // advanced lines: { account, amount, vat, party_no, avdeling, prosjekt }

  // ---- Simple mode ----
  let grossOre = $derived.by(() => {
    try {
      return gross.trim() ? parseKr(gross) : null;
    } catch {
      return null;
    }
  });
  let computed = $derived(
    grossOre && costAccount.trim()
      ? simpleLines({
          grossOre,
          vatCode,
          costAccount: costAccount.trim(),
          payment,
          partyNo: supplierNo,
          avdeling,
          prosjekt,
        })
      : [],
  );

  function toAdvanced() {
    rows = (computed.length ? computed : [{ account: "", amount_ore: 0, vat_code: null, party_no: null }]).map((l) => ({
      account: l.account,
      amount: l.amount_ore ? kr(l.amount_ore).replace("−", "-") : "",
      vat: l.vat_code || "",
      party_no: l.party_no || "",
      avdeling: l.avdeling || "",
      prosjekt: l.prosjekt || "",
    }));
    if (rows.length < 2) rows.push({ account: "", amount: "", vat: "", party_no: "", avdeling: "", prosjekt: "" });
    advanced = true;
  }

  // ---- Advanced mode ----
  let advancedLines = $derived(
    rows
      .filter((r) => r.account.trim() && r.amount.trim())
      .map((r) => {
        let amount = null;
        try {
          amount = parseKr(r.amount.replace("−", "-"));
        } catch {}
        return {
          account: r.account.trim(),
          amount_ore: amount,
          vat_code: r.vat.trim() || null,
          party_no: r.party_no || null,
          avdeling: r.avdeling || null,
          prosjekt: r.prosjekt || null,
        };
      }),
  );
  let difference = $derived(advancedLines.reduce((s, l) => s + (l.amount_ore ?? 0), 0));

  // ---- Shared ----
  let lines = $derived(advanced ? advancedLines : computed);
  let hasSupplier = $derived(lines.some((l) => l.party_no));
  let kidError = $derived(hasSupplier && kid.trim() !== "" && !isValidKid(kid.trim()));
  // An account the server would refuse: not the company's, and not in
  // the standard catalog it can be added from. Said here, by number.
  const known = (nr) => kontoer.some((k) => k.number === nr) || standard.some((s) => s.number === nr);
  let problems = $derived.by(() => {
    const p = [];
    const unknown = [...new Set(lines.map((l) => l.account).filter((nr) => nr && !known(nr)))];
    if (unknown.length) {
      p.push(
        "Konto " + unknown.join(", ") + " finnes ikke i kontoplanen — velg en fra listen, eller opprett den under Hovedbok",
      );
    }
    if (!description.trim()) p.push("Skriv en tekst for bilaget");
    if (!advanced) {
      if (grossOre === null) p.push("Skriv beløpet inkl. mva");
      if (!costAccount.trim()) p.push("Velg hva det var (konto)");
      if (payment === "faktura" && !supplierNo) p.push("Velg leverandøren fakturaen er fra");
    } else {
      if (advancedLines.some((l) => l.amount_ore === null)) p.push("Et beløp kan ikke leses");
      if (advancedLines.length < 2) p.push("Et bilag trenger minst to linjer");
      if (difference !== 0) p.push("Debet og kredit går ikke opp (differanse " + kr(difference) + ")");
    }
    if (kidError) p.push("KID-ens kontrollsiffer stemmer ikke — rett den eller tøm feltet");
    return p;
  });

  let submitted = $state(false);
  let busy = $state(false);

  // A standard account used for the first time joins the company's chart
  // of accounts first, as in Hovedbok.
  async function ensureAccounts() {
    for (const l of lines) {
      if (!kontoer.some((k) => k.number === l.account) && standard.some((s) => s.number === l.account)) {
        await post("/companies/" + companyId + "/accounts", { number: l.account });
      }
    }
  }

  async function book(event) {
    event?.preventDefault();
    submitted = true;
    if (problems.length || busy) return;
    busy = true;
    try {
      await ensureAccounts();
      const posted = await post("/companies/" + companyId + "/inbox/" + documentId + "/bokfor", {
        journal_code: "GL",
        date,
        description: description.trim(),
        lines,
        due_date: hasSupplier && dueDate ? dueDate : null,
        kid: hasSupplier && kid.trim() ? kid.trim() : null,
      });
      toast("Bokført som bilag " + posted.voucher, true);
      onBooked(posted);
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  const supplierName = (no) => suppliers.find((s) => s.party_no === no)?.name || no;
</script>

<form class="grid gap-4" novalidate onsubmit={book}>
  <div class="grid gap-3 sm:grid-cols-2">
    <label class="fieldset">
      <span class="fieldset-legend">Dato på bilaget</span>
      <input type="date" class="input w-full" bind:value={date} />
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">Tekst</span>
      <input class="input w-full" bind:value={description} placeholder="F.eks. Kontorrekvisita, Clas Ohlson" />
    </label>
  </div>

  {#if !advanced}
    <fieldset class="fieldset">
      <legend class="fieldset-legend">Hvordan ble det betalt?</legend>
      <div class="flex flex-wrap gap-2">
        {#each Object.entries(PAYMENT) as [key, p] (key)}
          <label class="label cursor-pointer gap-2 border border-base-300 rounded-box px-3 py-2 {payment === key ? 'border-primary' : ''}">
            <input type="radio" class="radio radio-sm" name="payment" value={key} bind:group={payment} />
            <span class="text-sm">{p.label}</span>
          </label>
        {/each}
      </div>
    </fieldset>

    <div class="grid gap-3 sm:grid-cols-2">
      {#if payment === "faktura"}
        <label class="fieldset sm:col-span-2">
          <span class="fieldset-legend">Leverandør</span>
          <select class="select w-full" bind:value={supplierNo}>
            <option value="">Velg leverandør</option>
            {#each suppliers as s (s.party_no)}
              <option value={s.party_no}>{s.name}</option>
            {/each}
          </select>
          <span class="label whitespace-normal">
            Mangler den? <a class="link" href={"#/c/" + companyId + "/leverandorer/ny"} target="_blank" rel="noopener">Legg til leverandør</a>
          </span>
        </label>
      {/if}
      <label class="fieldset">
        <span class="fieldset-legend">Beløp inkl. mva (kr)</span>
        <input class="input w-full text-right" inputmode="decimal" bind:value={gross} />
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">Mva</span>
        <select class="select w-full" bind:value={vatCode}>
          {#each Object.entries(INPUT_VAT) as [code, v] (code)}
            <option value={code}>{v.label}</option>
          {/each}
        </select>
      </label>
      <label class="fieldset sm:col-span-2">
        <span class="fieldset-legend">Hva var det? (konto)</span>
        <KontoVelger {kontoer} {standard} bind:value={costAccount} cls="input w-full" />
        <span class="label whitespace-normal">Skriv f.eks. «kontor», «reise» eller et kontonummer</span>
      </label>
      {#if dims.length}
        <div class="flex gap-2 sm:col-span-2">
          <DimSelect {dims} kind="avdeling" bind:value={avdeling} />
          <DimSelect {dims} kind="prosjekt" bind:value={prosjekt} />
        </div>
      {/if}
    </div>
  {:else}
    <fieldset class="fieldset">
      <legend class="fieldset-legend">Linjer (debet positivt, kredit negativt)</legend>
      {#each rows as r, i (i)}
        <div class="grid grid-cols-2 sm:grid-cols-[7rem_8rem_5rem_minmax(0,1fr)_2rem] gap-2 mb-2 items-center">
          <label>
            <span class="sr-only">Konto, linje {i + 1}</span>
            <KontoVelger {kontoer} {standard} bind:value={r.account} cls="input input-sm w-full" />
          </label>
          <label>
            <span class="sr-only">Beløp, linje {i + 1}</span>
            <input class="input input-sm w-full text-right" inputmode="decimal" bind:value={r.amount} placeholder="Beløp" />
          </label>
          <label>
            <span class="sr-only">Mva-kode, linje {i + 1}</span>
            <input class="input input-sm w-full" bind:value={r.vat} placeholder="Mva" />
          </label>
          <label>
            <span class="sr-only">Leverandør, linje {i + 1}</span>
            <select class="select select-sm w-full" bind:value={r.party_no}>
              <option value="">Ingen part</option>
              {#each suppliers as s (s.party_no)}
                <option value={s.party_no}>{s.name}</option>
              {/each}
            </select>
          </label>
          <button type="button" class="btn btn-ghost btn-xs" aria-label={"Fjern linje " + (i + 1)} onclick={() => rows.splice(i, 1)}>✕</button>
        </div>
      {/each}
      <div class="flex items-center gap-3">
        <button type="button" class="btn btn-xs" onclick={() => rows.push({ account: "", amount: "", vat: "", party_no: "", avdeling: "", prosjekt: "" })}>
          + linje
        </button>
        <span class="text-sm {difference ? 'text-error' : 'opacity-70'}" aria-live="polite">
          {difference ? "Differanse " + kr(difference) : "Går opp"}
        </span>
      </div>
    </fieldset>
  {/if}

  {#if hasSupplier}
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="fieldset">
        <span class="fieldset-legend">Forfall</span>
        <input type="date" class="input w-full" bind:value={dueDate} />
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">KID</span>
        <input
          class="input w-full {kidError ? 'input-error' : ''}"
          inputmode="numeric"
          bind:value={kid}
          aria-invalid={kidError}
          aria-describedby="booking-kid-help"
        />
        <span id="booking-kid-help" class="label whitespace-normal {kidError ? 'text-error' : ''}">
          {kidError ? "Kontrollsifferet stemmer ikke. Rett KID-en, eller tøm feltet." : "Brukes når fakturaen betales"}
        </span>
      </label>
    </div>
  {/if}

  {#if !advanced && computed.length}
    <div class="text-sm">
      <p class="opacity-70 mb-1">Dette bokføres:</p>
      <table class="table table-xs">
        <tbody>
          {#each computed as l, i (i)}
            <tr>
              <td class="font-mono">{l.account}</td>
              <td>
                {l.account === "2710" ? "Inngående mva" : l.party_no ? supplierName(l.party_no) : i === 0 ? "Kostnad" : "Betalt"}
              </td>
              <td class="text-right tabular-nums">{kr(l.amount_ore)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#if submitted && problems.length}
    <ul class="text-sm text-error list-disc ps-5" role="alert">
      {#each problems as p (p)}<li>{p}</li>{/each}
    </ul>
  {/if}

  <div class="flex flex-wrap items-center gap-2">
    <button class="btn btn-primary" disabled={busy}>
      {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
      Bokfør
    </button>
    {#if !advanced}
      <button type="button" class="btn btn-ghost btn-sm" onclick={toAdvanced}>Avansert: rediger linjene</button>
    {/if}
  </div>
</form>
