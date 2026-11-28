<script>
  // Kjør lønn (#46): fyll inn, se nøyaktig hva som bokføres, bokfør.
  // Forhåndsvisningen er serverens egen beregning (?preview=true) — den
  // samme som bokføres, uten bilag og uten bilagsnummer. Bokfør-knappen
  // finnes bare mens forhåndsvisningen svarer til det som står i skjemaet.
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import { AGA_SONER, MANEDER } from "./konstanter.js";
  import { nextPeriod, startingZone } from "./periode.js";
  import { untrack } from "svelte";

  let { companyId, ansatte, kjoringer, onDone } = $props();

  const iDag = today();
  const iAar = Number(iDag.slice(0, 4));
  let start = untrack(() => nextPeriod(kjoringer, iDag));
  // Last year for a late December run; next year for January's run once
  // December is done.
  const AAR = [iAar - 1, iAar, iAar + 1];

  let year = $state(start.year);
  let maned = $state(start.month);
  let dato = $state(iDag);
  let sone = $state(untrack(() => startingZone(kjoringer)));

  let kjort = $derived(
    new Set(kjoringer.filter((k) => k.ar === year).map((k) => k.maned)),
  );

  const krTekst = (ore) => (ore == null ? "" : (ore / 100).toFixed(2).replace(".", ","));

  // One line per employee, so changing the period keeps what was typed.
  let linjer = $state([]);
  $effect(() => {
    linjer = ansatte.map((a) => {
      // A final settlement is a deliberate choice: nothing is due by default.
      const hasLeft = !!a.ansatt_til && a.ansatt_til < iDag;
      return {
        employee_id: a.id,
        navn: a.navn,
        endDate: a.ansatt_til,
        brutto: hasLeft ? "0,00" : krTekst(a.manedslonn_ore),
        fp: "",
        canMarkExempt: !!a.ansatt_til && !a.trekk_12_maneder,
        holidayPayExempt: false,
        supplement: "",
        birthYear: a.fodselsdato ? Number(a.fodselsdato.slice(0, 4)) : null,
        timelonn: !!a.timelonn_ore,
        // Paid by the hour only: the hours are the pay, so start there.
        fra_timer: !!a.timelonn_ore && a.manedslonn_ore == null,
        med: !hasLeft,
      };
    });
  });
  // Everyone employed in the month, so a final settlement can run after the end date.
  let monthStart = $derived(`${year}-${String(maned).padStart(2, "0")}-01`);
  let aktive = $derived(linjer.filter((l) => !l.endDate || l.endDate >= monthStart));

  // The server asks for the supplement from the earning year the employee
  // turns 59, whatever the rate says today.
  const mayCarrySupplement = (l) =>
    l.birthYear == null || Number(dato.slice(0, 4)) - 1 - l.birthYear >= 59;

  function foresporsel() {
    return {
      ar: year,
      maned,
      utbetalt_dato: dato,
      sone,
      linjer: aktive
        .filter((l) => l.med)
        .map((l) => ({
          employee_id: l.employee_id,
          // Timelønn overstyrer et eventuelt bruttobeløp: serveren
          // regner det fra de låste timene.
          brutto_ore: l.fra_timer ? null : l.brutto.trim() ? parseKr(l.brutto.trim()) : null,
          feriepenger_ore: l.fp.trim() ? parseKr(l.fp.trim()) : 0,
          feriepenger_trekkfri: l.canMarkExempt && l.holidayPayExempt,
          holiday_pay_supplement_ore:
            mayCarrySupplement(l) && l.fp.trim() && l.supplement.trim()
              ? parseKr(l.supplement.trim())
              : null,
          fra_timer: l.fra_timer,
        })),
    };
  }

  // The form as text: a preview is valid only while this is unchanged.
  let skjemaNokkel = $derived(
    JSON.stringify([year, maned, dato, sone, aktive.map((l) => [l.employee_id, l.brutto, l.fp, l.supplement, l.holidayPayExempt, l.fra_timer, l.med, mayCarrySupplement(l)])]),
  );
  let forhand = $state(null); // { nokkel, svar }
  let gyldig = $derived(forhand && forhand.nokkel === skjemaNokkel);
  let arbeider = $state(false);
  let bokfort = $state(null); // the booked run's answer, warnings kept on screen

  const periode = $derived(MANEDER[maned - 1] + " " + year);
  // Every order in the preview, with the employee's name for the list.
  let garnishments = $derived(
    forhand
      ? forhand.svar.linjer.flatMap((l) => (l.trekkpalegg ?? []).map((t) => ({ ...t, navn: l.navn })))
      : [],
  );

  async function forhandsvis() {
    let body;
    try {
      body = foresporsel();
    } catch (error) {
      toast(error.message, false);
      return;
    }
    if (!body.linjer.length) {
      toast("Ingen ansatte er med i kjøringen", false);
      return;
    }
    arbeider = true;
    bokfort = null;
    try {
      const nokkel = skjemaNokkel;
      const svar = await post("/companies/" + companyId + "/payroll?preview=true", body);
      forhand = { nokkel, svar };
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
    }
  }

  async function bokfor() {
    arbeider = true;
    try {
      // The trekkpålegg the preview showed: the server refuses the booking
      // if an order changed since (docs/trekkpalegg.md §3 point 2).
      const svar = await post("/companies/" + companyId + "/payroll", {
        ...foresporsel(),
        trekkpalegg_sett: forhand.svar.trekkpalegg_sett,
      });
      bokfort = { periode, svar };
      forhand = null;
      // Straight on to the next month, so the form never sits on one
      // that has just been run.
      const neste = nextPeriod([{ ar: year, maned }], iDag);
      if (AAR.includes(neste.year)) [year, maned] = [neste.year, neste.month];
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
    }
  }
</script>

<Card title="Kjør lønn">
  {#if bokfort}
    <div role="status" class="alert alert-success alert-soft mb-3">
      <div>
        <p>
          Lønn for {bokfort.periode} er bokført. Netto til utbetaling
          {kr(bokfort.svar.kjoring.netto_ore)} står på <b>2930 Skyldig lønn</b> til den betales i
          betalingslisten.
        </p>
        <a class="link" href={"#/c/" + companyId + "/hovedbok/" + bokfort.svar.kjoring.voucher_id}>
          Se bilaget
        </a>
      </div>
    </div>
    {#if bokfort.svar.advarsler?.length}
      <div role="alert" class="alert alert-warning alert-soft mb-3">
        <ul class="list-disc ml-4 text-sm">
          {#each bokfort.svar.advarsler as a, i (i)}<li>{a}</li>{/each}
        </ul>
      </div>
    {/if}
  {/if}

  {#if aktive.length}
    <div class="grid grid-cols-2 sm:grid-cols-4 gap-2 mb-3 max-w-2xl">
      <label class="flex flex-col gap-1 text-xs">
        År
        <select class="select select-sm" bind:value={year}>
          {#each AAR as a (a)}<option value={a}>{a}</option>{/each}
        </select>
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Måned
        <select class="select select-sm" bind:value={maned}>
          {#each MANEDER as navn, i (navn)}
            <option value={i + 1} disabled={kjort.has(i + 1)}>
              {navn}{kjort.has(i + 1) ? " (kjørt)" : ""}
            </option>
          {/each}
        </select>
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Utbetalingsdato
        <input type="date" class="input input-sm" bind:value={dato} />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Arbeidsgiveravgiftssone
        <select
          class="select select-sm"
          required
          aria-describedby={sone ? undefined : "aga-sone-hjelp"}
          bind:value={sone}
        >
          <option value="" disabled>Velg sone</option>
          {#each AGA_SONER as s (s[0])}<option value={s[0]}>{s[1]}</option>{/each}
        </select>
      </label>
    </div>
    {#if !sone}
      <p id="aga-sone-hjelp" class="text-sm text-warning mb-2">
        Velg arbeidsgiveravgiftssone før du går videre. Sonen følger normalt kommunen underenheten
        er registrert i, og en feil sone kan bare rettes med et reverserende bilag og en ny kjøring.
      </p>
    {/if}
    {#if kjort.has(maned)}
      <p role="alert" class="text-sm text-warning mb-2">
        Lønn for {periode} er allerede kjørt. En retting er et reverserende bilag og en ny kjøring.
      </p>
    {/if}

    <div class="overflow-x-auto">
      <table class="table table-sm mb-2">
        <thead>
          <tr>
            <th>Med</th>
            <th>Ansatt</th>
            <th>Brutto (kr)</th>
            <th>Feriepenger utbetalt (kr)</th>
            <th>Timelønn</th>
          </tr>
        </thead>
        <tbody>
          {#each aktive as l (l.employee_id)}
            <tr class={l.med ? "" : "opacity-50"}>
              <td>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  aria-label={"Ta med " + l.navn}
                  bind:checked={l.med}
                />
              </td>
              <td class="whitespace-nowrap">{l.navn}</td>
              <td>
                <input
                  class="input input-sm w-32 text-right"
                  inputmode="decimal"
                  aria-label={"Brutto for " + l.navn + " i kroner"}
                  disabled={l.fra_timer || !l.med}
                  bind:value={l.brutto}
                />
              </td>
              <td>
                <input
                  class="input input-sm w-32 text-right"
                  inputmode="decimal"
                  placeholder="0"
                  aria-label={"Feriepenger utbetalt til " + l.navn + " i kroner"}
                  disabled={!l.med}
                  bind:value={l.fp}
                />
                {#if l.canMarkExempt && l.fp.trim()}
                  <label class="label cursor-pointer justify-start gap-2 text-xs mt-1">
                    <input type="checkbox" class="checkbox checkbox-xs" bind:checked={l.holidayPayExempt} />
                    Trekkfri: dødsfall eller løpende utbetaling
                  </label>
                {/if}
                {#if mayCarrySupplement(l) && l.fp.trim()}
                  <label class="block mt-1 text-xs">
                    <span class="block opacity-70">Herav tillegg over 60 (kr)</span>
                    <input
                      class="input input-xs w-32 text-right"
                      inputmode="decimal"
                      placeholder="beregnes"
                      aria-label={"Tillegg for over 60 år i feriepengene til " + l.navn + " i kroner"}
                      disabled={!l.med}
                      bind:value={l.supplement}
                    />
                  </label>
                {/if}
              </td>
              <td>
                {#if l.timelonn}
                  <label class="label cursor-pointer gap-2 text-sm">
                    <input type="checkbox" class="checkbox checkbox-sm" bind:checked={l.fra_timer} />
                    Fra låste timer
                  </label>
                {:else}
                  <span class="opacity-50 text-xs">ingen timesats</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <details class="text-sm opacity-80 mb-3">
      <summary class="cursor-pointer">Slik fungerer lønnskjøringen</summary>
      <div class="mt-2 grid gap-2">
        <p>
          En kjøring blir <b>ett bilag</b>: lønn, forskuddstrekk, netto til utbetaling,
          feriepengeavsetning og arbeidsgiveravgift i én transaksjon. Netto føres mot
          <b>2930 Skyldig lønn</b> — selve utbetalingen gjøres i betalingslisten. En måned kan bare
          kjøres én gang; en retting er et reverserende bilag og en ny kjøring.
        </p>
        <p>
          Arbeidsgiveravgift på feriepenger som er avsatt, men ikke utbetalt, kostnadsføres når de
          opptjenes (<b>5405</b> mot <b>2780</b>) og føres tilbake når de utbetales. Beløpet er
          alltid satsen av det som faktisk skyldes, så en utbetaling, en satsendring eller en gjeld
          uten avsetning retter seg selv ved neste kjøring.
        </p>
        <p>
          Timelønn hentes fra timeføringen, men bare når <b>måneden er låst</b> i Timer — ulåste
          timer kan endres etter at lønnen er bokført. Godkjent kjøregodtgjørelse og diett datert
          til og med utbetalingsdatoen utbetales med kjøringen.
        </p>
      </div>
    </details>

    {#if gyldig}
      {@const s = forhand.svar}
      <section class="border border-base-300 rounded-box p-3 mb-3" aria-label="Forhåndsvisning">
        <h3 class="font-semibold mb-2">Dette bokføres for {periode}</h3>
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>Ansatt</th>
                <th class="text-right">Brutto</th>
                <th class="text-right">Feriepenger</th>
                <th class="text-right">Trekk</th>
                <th class="text-right">Kjøring og diett</th>
                {#if garnishments.length}<th class="text-right">Trekkpålegg</th>{/if}
                <th class="text-right">Netto</th>
              </tr>
            </thead>
            <tbody class="whitespace-nowrap">
              {#each s.linjer as l (l.employee_id)}
                <tr>
                  <td class="whitespace-nowrap">
                    {l.navn}{#if l.halv_trekk}<span class="badge badge-sm ml-1">halv skatt</span>{/if}
                  </td>
                  <td class="text-right">{kr(l.brutto_ore)}</td>
                  <td class="text-right">{kr(l.feriepenger_ore)}</td>
                  <td class="text-right">{kr(l.forskuddstrekk_ore)}</td>
                  <td class="text-right">{kr(l.kjoregodtgjorelse_ore)}</td>
                  {#if garnishments.length}
                    <td class="text-right">{kr(l.trekkpalegg.reduce((sum, t) => sum + t.trukket_ore, 0))}</td>
                  {/if}
                  <td class="text-right font-medium">{kr(l.netto_ore)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <dl class="grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-1 text-sm mt-3">
          <dt class="opacity-70">Netto til utbetaling</dt>
          <dd class="font-semibold">{kr(s.kjoring.netto_ore)}</dd>
          <dt class="opacity-70">Arbeidsgiveravgift</dt>
          <dd>{kr(s.kjoring.aga_ore)}</dd>
          <dt class="opacity-70">Feriepenger avsatt</dt>
          <dd>{kr(s.kjoring.feriepengeavsetning_ore)}</dd>
          <dt class="opacity-70">Aga på feriepenger</dt>
          <dd>{kr(s.kjoring.aga_feriepenger_ore)}</dd>
          <dt class="opacity-70">Lønnskostnad i alt</dt>
          <dd>{kr(s.kjoring.lonnskostnad_ore)}</dd>
        </dl>
        {#if garnishments.length}
          <!-- Each order with amount, KID and its letter, so the person
               running pay can check them against the letters before booking
               (docs/trekkpalegg.md §6, Avgjørelser). -->
          <h4 class="font-semibold text-sm mt-3">Trekkpålegg</h4>
          <ul class="text-sm grid gap-1 mt-1">
            {#each garnishments as t (t.registrering ?? t.trekkid)}
              <li>
                {t.navn}: {kr(t.trukket_ore)} kr
                {#if t.trukket_ore < t.onsket_ore}(av {kr(t.onsket_ore)}){/if}
                — {t.type === "skatt" ? "skatt, selskapets KID" : "KID " + t.kid}, versjon {t.versjon}
                {#if t.trekkid}
                  (fra Skatteetaten, trekkpålegg {t.trekkid})
                {:else}
                  <button class="link text-xs" onclick={() => download(t.brev_url, t.brev_filnavn)}>
                    brevet
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
          {#if s.trekkpalegg_konto}
            <p class="text-xs opacity-70 mt-1">Føres på konto {s.trekkpalegg_konto} som leverandørpost og betales fra betalingslisten.</p>
          {/if}
        {/if}
        {#if s.advarsler?.length}
          <div role="alert" class="alert alert-warning alert-soft mt-3">
            <ul class="list-disc ml-4 text-sm">
              {#each s.advarsler as a, i (i)}<li>{a}</li>{/each}
            </ul>
          </div>
        {/if}
        <p class="text-xs opacity-70 mt-3">
          Det som er hentet fra Skatteetaten eller registrert under «Trekkpålegg» når du forhåndsviser,
          er det som trekkes.
        </p>
        <div class="flex flex-wrap gap-2 mt-3">
          <button class="btn btn-primary btn-sm" disabled={arbeider} onclick={bokfor}>
            Bokfør lønn for {periode}
          </button>
          <button class="btn btn-ghost btn-sm" onclick={() => (forhand = null)}>Endre</button>
        </div>
        <p class="text-xs opacity-70 mt-2">
          Et bokført bilag kan ikke endres — en retting er et reverserende bilag og en ny kjøring.
        </p>
      </section>
    {:else}
      <button
        class="btn btn-primary btn-sm w-fit"
        disabled={arbeider || kjort.has(maned) || !sone}
        onclick={forhandsvis}
      >
        Se hva som bokføres for {periode}
      </button>
    {/if}
  {:else}
    <p class="text-sm opacity-70">Registrer minst én ansatt under for å kjøre lønn.</p>
  {/if}
</Card>
