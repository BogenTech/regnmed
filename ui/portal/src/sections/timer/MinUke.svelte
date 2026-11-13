<script>
  // Ukegrid for timeføring (#38-oppfølger): rader er prosjekter, kolonner
  // er dager, en celle er timene den dagen. Hver celle lagrer seg selv
  // (POST/PUT/DELETE på de vanlige endepunktene) i det du forlater den;
  // Enter og piltaster flytter mellom cellene som i et regneark. Serveren
  // avgjør hvem som ser hva (TIMER_LES_ALLE) — «Hele laget» under viser
  // bare det svaret faktisk inneholdt.
  import { onMount, tick, untrack } from "svelte";
  import { api, post, send } from "../../lib/api.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { formatDate, kr, minutterTilTimer, parseKr, today } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import DimRegisterLenke from "../../components/DimRegisterLenke.svelte";
  import DimSelect from "../../components/DimSelect.svelte";
  import { addDays, dayName, dayOrder, parseHours } from "./week.js";

  let {
    companyId,
    uke,
    forrigeUke,
    from,
    to,
    dims,
    onDone,
    onWeek,
    startFokus = null,
    onFokusBrukt = () => {},
  } = $props();

  // Satsen kommer fra prosjektregisteret (docs/timer.md) — bare den som
  // holder TIMER_SATS_SKRIV ser og setter satser her; alle andre får
  // registerets sats uansett hva klienten sender, så feltet skjules.
  let kanSats = $derived(harRett(companyId, "TIMER_SATS_SKRIV"));

  const DAGNAVN = ["Ma", "Ti", "On", "To", "Fr", "Lø", "Sø"];
  const IDAG = today();

  let dager = $derived(
    DAGNAVN.map((navn, i) => {
      const dato = addDays(from, i);
      return { navn, dato, dag: Number(dato.slice(8, 10)), helg: i >= 5, idag: dato === IDAG };
    }),
  );

  let egne = $derived(uke.entries.filter((e) => e.own));
  let andre = $derived(uke.entries.filter((e) => !e.own));
  let egneForrige = $derived((forrigeUke?.entries || []).filter((e) => e.own));

  // Radene: prosjektene med egne timer denne uken, i forrige uke (tomme
  // rader klare til utfylling), pluss rader lagt til for hånd. "" er
  // raden uten prosjekt.
  let ekstraRader = $state([]);
  let rader = $derived.by(() => {
    const sett = [];
    for (const e of [...egne, ...egneForrige]) {
      const nokkel = e.prosjekt || "";
      if (!sett.includes(nokkel)) sett.push(nokkel);
    }
    for (const nokkel of ekstraRader) {
      if (!sett.includes(nokkel)) sett.push(nokkel);
    }
    return sett.sort((a, b) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)));
  });

  function iCelle(prosjekt, dato) {
    return egne.filter((e) => (e.prosjekt || "") === prosjekt && e.dato === dato);
  }

  // Radens standard kommer fra PROSJEKTET (0052): fakturerbar_default og
  // min egen sats i registeret. Uten prosjekt finnes ingen standard.
  function radStandard(prosjekt) {
    const d = prosjekt ? dims.find((x) => x.kind === "prosjekt" && x.code === prosjekt) : null;
    return d
      ? { fakturerbar: d.fakturerbar_default, timesats_ore: d.min_timesats_ore }
      : { fakturerbar: false, timesats_ore: null };
  }

  function dimNavn(kode) {
    const d = dims.find((x) => x.kind === "prosjekt" && x.code === kode);
    return d ? d.name : "";
  }

  function radNavn(prosjekt) {
    return prosjekt ? (prosjekt + " " + dimNavn(prosjekt)).trim() : "Uten prosjekt";
  }

  function laast(dato) {
    return uke.locked_through && dato <= uke.locked_through;
  }

  function sumMin(entries) {
    return entries.reduce((sum, e) => sum + e.minutter, 0);
  }

  function satsTekst(ore) {
    return ore == null ? "" : String(ore / 100).replace(".", ",");
  }

  // What a screen reader says for a cell: where it is, and what a sighted
  // user sees as styling (locked, several lines, a description dot).
  function celleEtikett(prosjekt, dato, entries) {
    let s = radNavn(prosjekt) + ", " + dayName(dato) + ", timer";
    if (laast(dato)) s += ", låst";
    else if (entries.length > 1) s += ", flere linjer, redigeres under tabellen";
    if (entries.some((e) => e.beskrivelse)) s += ", har beskrivelse";
    return s;
  }

  let lagrer = $state("");
  let aktiv = $state(null);

  // One save at a time per cell: a second edit of the same cell waits for
  // the first save AND the reload after it, so it sees the line the
  // first one created and updates it instead of creating a second one.
  const pagaende = new Map();
  function lagreCelle(prosjekt, dato, input) {
    const key = prosjekt + "|" + dato;
    const tekst = input.value;
    const forrige = pagaende.get(key);
    const jobb = (async () => {
      if (forrige) await forrige;
      await lagreCelleNa(prosjekt, dato, input, tekst);
    })();
    pagaende.set(key, jobb);
    jobb.finally(() => pagaende.get(key) === jobb && pagaende.delete(key));
    return jobb;
  }

  async function lagreCelleNa(prosjekt, dato, input, tekst) {
    const entries = iCelle(prosjekt, dato);
    const gamle = sumMin(entries);
    const tilbake = () => (input.value = gamle ? minutterTilTimer(gamle) : "");
    let minutter;
    try {
      minutter = parseHours(tekst);
    } catch (error) {
      toast(error.message, false);
      tilbake();
      return;
    }
    if (minutter === gamle) return;
    const beskrevet = entries.filter((e) => e.beskrivelse);
    if (minutter === 0 && beskrevet.length) {
      const ok = await bekreft(
        "Linjen «" + beskrevet[0].beskrivelse + "» slettes, med beskrivelsen.",
        { tittel: "Slette timene?", ok: "Slett", farlig: true },
      );
      // The question was asked from a blur, when nothing had focus yet:
      // give the keyboard back the cell it was answering about.
      setTimeout(() => document.activeElement === document.body && input.focus(), 0);
      if (!ok) return tilbake();
    }
    lagrer = "lagrer";
    try {
      if (entries.length === 0) {
        // fakturerbar og sats sendes IKKE: prosjektets standard og
        // registerets sats avgjør på serveren.
        await post("/companies/" + companyId + "/timesheet", {
          dato,
          minutter,
          beskrivelse: "",
          prosjekt: prosjekt || null,
        });
      } else if (minutter > 0) {
        await oppdater(entries[0], { minutter });
      } else {
        for (const e of entries) {
          await api("/companies/" + companyId + "/timesheet/" + e.entry_id, { method: "DELETE" });
        }
      }
      lagrer = "lagret";
      await onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
      tilbake();
    }
  }

  function oppdater(e, endring) {
    const neste = { ...e, ...endring };
    return send("PUT", "/companies/" + companyId + "/timesheet/" + e.entry_id, {
      dato: e.dato,
      minutter: neste.minutter,
      beskrivelse: neste.beskrivelse,
      prosjekt: e.prosjekt || null,
      fakturerbar: neste.fakturerbar,
      // Uten TIMER_SATS_SKRIV avgjør registeret — feltet betyr ingenting.
      timesats_ore: kanSats && neste.fakturerbar ? neste.timesats_ore : null,
    });
  }

  // Piltaster og Enter flytter mellom cellene — regnearkfølelsen. Låste
  // og fakturerte celler hoppes over, og en piltast forbi søndag (eller
  // før mandag) går videre inn i neste (forrige) uke, i samme rad.
  // PageDown/PageUp bytter uke og blir stående på samme rad og dag.
  function celle(rad, dag) {
    const el = document.querySelector('input[data-rad="' + rad + '"][data-dag="' + dag + '"]');
    return el && !el.disabled && el.offsetParent ? el : null;
  }

  function flytt(el) {
    el.focus();
    el.select();
  }

  function byttUke(input, delta, dag, retning) {
    const prosjekt = rader[Number(input.dataset.rad)];
    input.blur(); // saves the cell before its week leaves the screen
    onWeek(delta, { prosjekt, dag, retning });
  }

  function tastNav(event) {
    const input = event.target;
    if (!input.dataset || input.dataset.rad === undefined) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const rad = Number(input.dataset.rad);
    const dag = Number(input.dataset.dag);
    const mobil = dag < 0;
    const key = event.key;

    if (key === "Escape") {
      const dato = mobil ? mobilVisning.dato : dager[dag].dato;
      const entries = iCelle(rader[rad], dato);
      input.value = entries.length ? minutterTilTimer(sumMin(entries)) : "";
      return;
    }
    if (key === "PageDown" || key === "PageUp") {
      event.preventDefault();
      byttUke(input, key === "PageDown" ? 1 : -1, mobil ? 0 : dag, 0);
      return;
    }
    if (!mobil && (key === "ArrowRight" || key === "ArrowLeft")) {
      event.preventDefault();
      const retning = key === "ArrowRight" ? 1 : -1;
      for (let d = dag + retning; d >= 0 && d <= 6; d += retning) {
        const el = celle(rad, d);
        if (el) return flytt(el);
      }
      // Back into a week that is locked all through: nothing there to
      // type in, so the cursor stays put rather than landing nowhere.
      if (retning < 0 && laast(addDays(from, -1))) return;
      byttUke(input, retning, retning > 0 ? 0 : 6, retning);
      return;
    }
    if (key === "ArrowDown" || key === "ArrowUp" || key === "Enter") {
      const retning = key === "ArrowUp" ? -1 : 1;
      for (let r = rad + retning; r >= 0 && r < rader.length; r += retning) {
        const el = celle(r, dag);
        if (el) {
          event.preventDefault();
          return flytt(el);
        }
      }
      // Siste rad: Enter har ingen celle å gå til, men skal fortsatt
      // bekrefte cellen — lagringen ligger på blur.
      if (key === "Enter") input.blur();
    }
  }

  // Arriving from the week next door by keyboard: put the cursor where
  // the keys were heading, in the same project row (added if this week
  // has no hours on it yet).
  onMount(async () => {
    const f = untrack(() => startFokus);
    if (!f) return;
    onFokusBrukt();
    if (!rader.includes(f.prosjekt)) ekstraRader = [...ekstraRader, f.prosjekt];
    await tick();
    const rad = rader.indexOf(f.prosjekt);
    for (const d of dayOrder(f.dag, f.retning)) {
      const el = celle(rad, d);
      if (el) return flytt(el);
    }
  });

  // A new row gets focus in its first open cell, so adding a project and
  // typing its hours never needs the mouse.
  let nyProsjekt = $state("");
  async function leggTilRad() {
    const nokkel = nyProsjekt || "";
    if (!rader.includes(nokkel)) ekstraRader = [...ekstraRader, nokkel];
    nyProsjekt = "";
    await tick();
    const rad = rader.indexOf(nokkel);
    const celle = [...document.querySelectorAll('input[data-rad="' + rad + '"]')].find(
      (i) => i.offsetParent && !i.disabled && !i.readOnly,
    );
    celle?.focus();
  }

  // Kopierer forrige ukes egne linjer til samme ukedag denne uken.
  // Tilbys bare når uken står tom, så en dobbelttrykk ikke dobler
  // timene. Satsen sendes ikke med — registeret priser de nye linjene
  // på sin dato — og en linje som feiler stopper ikke resten.
  let kopierer = $state(false);
  async function kopierForrigeUke() {
    kopierer = true;
    let feilet = 0;
    try {
      for (const e of egneForrige) {
        const dato = addDays(e.dato, 7);
        if (laast(dato) || e.invoice_no) continue;
        try {
          await post("/companies/" + companyId + "/timesheet", {
            dato,
            minutter: e.minutter,
            beskrivelse: e.beskrivelse,
            prosjekt: e.prosjekt || null,
            fakturerbar: e.fakturerbar,
          });
        } catch {
          feilet += 1;
        }
      }
      if (feilet) toast(feilet + " linjer kunne ikke kopieres", false);
      await onDone();
    } finally {
      kopierer = false;
    }
  }

  // Detaljpanelet under gridet: linjene i den valgte cellen. Hvert felt
  // lagrer seg selv når du forlater det, som cellene — det finnes ingen
  // «Lagre» å glemme, og ingen utkast som forsvinner når du går videre.
  // En ny linje skrives ferdig før panelet følger en annen celle.
  let nyLinje = $state(null);
  let panel = $derived(nyLinje ? { prosjekt: nyLinje.prosjekt, dato: nyLinje.dato } : aktiv);
  let panelLinjer = $derived(panel ? iCelle(panel.prosjekt, panel.dato) : []);

  async function endreFelt(e, endring, input, gammel) {
    if (endring.minutter === 0) {
      if (!(await slettLinje(e))) input.value = gammel;
      return;
    }
    lagrer = "lagrer";
    try {
      await oppdater(e, endring);
      lagrer = "lagret";
      await onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
      if (input.type === "checkbox") input.checked = gammel;
      else input.value = gammel;
    }
  }

  function endreTimer(e, input) {
    const gammel = minutterTilTimer(e.minutter);
    try {
      endreFelt(e, { minutter: parseHours(input.value) }, input, gammel);
    } catch (error) {
      toast(error.message, false);
      input.value = gammel;
    }
  }

  function endreSats(e, input) {
    const gammel = satsTekst(e.timesats_ore);
    try {
      const ore = input.value.trim() ? parseKr(input.value) : null;
      endreFelt(e, { timesats_ore: ore }, input, gammel);
    } catch (error) {
      toast(error.message, false);
      input.value = gammel;
    }
  }

  async function slettLinje(e) {
    const ok = await bekreft(
      (e.beskrivelse ? "«" + e.beskrivelse + "», " : "") +
        minutterTilTimer(e.minutter) +
        " t " +
        dayName(e.dato) +
        ".",
      { tittel: "Slette linjen?", ok: "Slett", farlig: true },
    );
    if (!ok) return false;
    try {
      await api("/companies/" + companyId + "/timesheet/" + e.entry_id, { method: "DELETE" });
      await onDone();
      return true;
    } catch (error) {
      toast(error.message, false);
      return false;
    }
  }

  function startNyLinje() {
    const standard = radStandard(panel.prosjekt);
    nyLinje = {
      prosjekt: panel.prosjekt,
      dato: panel.dato,
      beskrivelse: "",
      timer: "",
      fakturerbar: standard.fakturerbar,
      sats: satsTekst(standard.timesats_ore),
    };
  }

  async function lagreNyLinje(event) {
    event.preventDefault();
    lagrer = "lagrer";
    try {
      const minutter = parseHours(nyLinje.timer);
      if (!minutter) throw new Error("skriv inn timene");
      await post("/companies/" + companyId + "/timesheet", {
        dato: nyLinje.dato,
        minutter,
        beskrivelse: nyLinje.beskrivelse,
        prosjekt: nyLinje.prosjekt || null,
        fakturerbar: nyLinje.fakturerbar,
        timesats_ore:
          kanSats && nyLinje.fakturerbar && nyLinje.sats ? parseKr(nyLinje.sats) : null,
      });
      lagrer = "lagret";
      nyLinje = null;
      await onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
    }
  }

  // Mobil: gridet er uleselig på 375 px — der vises én dag om gangen,
  // valgt med dagknappene, med de samme radene og samme lagringsvei.
  // The component is keyed on the week (Timer.svelte), so the week is
  // fixed for its lifetime and the initial value is the right one.
  let mobilDag = $state(untrack(() => (IDAG >= from && IDAG <= to ? IDAG : from)));
  let mobilVisning = $derived(dager.find((d) => d.dato === mobilDag) || dager[0]);
  function velgDag(dato) {
    mobilDag = dato;
    aktiv = null; // the panel must not keep showing another day's lines
  }

  let radSum = $derived(
    Object.fromEntries(
      rader.map((r) => [r, sumMin(egne.filter((e) => (e.prosjekt || "") === r))]),
    ),
  );
  let dagSum = $derived(
    Object.fromEntries(dager.map((d) => [d.dato, sumMin(egne.filter((e) => e.dato === d.dato))])),
  );
  let ukeSum = $derived(sumMin(egne));
  let laasVises = $derived(uke.locked_through && uke.locked_through >= from);
</script>

{#snippet celleInput(prosjekt, radIdx, dag, dagIdx, ekstraCls)}
  {@const entries = iCelle(prosjekt, dag.dato)}
  {@const fakturert = entries.length > 0 && entries.every((e) => e.invoice_no)}
  {@const flere = entries.length > 1}
  {#if fakturert}
    <span class="badge badge-ghost badge-sm" title={"Fakturert på faktura " + entries[0].invoice_no}>
      {minutterTilTimer(sumMin(entries))}
      <span class="sr-only">
        timer, {radNavn(prosjekt)}, {dayName(dag.dato)}, fakturert på faktura {entries[0].invoice_no}
      </span>
    </span>
  {:else}
    <input
      class={"input w-full text-center " + (ekstraCls || "input-xs")}
      inputmode="decimal"
      autocomplete="off"
      aria-label={celleEtikett(prosjekt, dag.dato, entries)}
      value={entries.length ? minutterTilTimer(sumMin(entries)) : ""}
      disabled={laast(dag.dato)}
      readonly={flere}
      title={laast(dag.dato)
        ? "Låst t.o.m. " + formatDate(uke.locked_through)
        : flere
          ? "Flere linjer — rediger dem under tabellen"
          : ""}
      data-rad={radIdx}
      data-dag={dagIdx}
      onfocus={(ev) => {
        aktiv = { prosjekt, dato: dag.dato };
        ev.target.select();
      }}
      onblur={(ev) => !flere && lagreCelle(prosjekt, dag.dato, ev.target)}
      onkeydown={tastNav}
    />
    {#if entries.some((e) => e.beskrivelse)}
      <div class="w-1 h-1 rounded-full bg-primary mx-auto mt-0.5" aria-hidden="true"></div>
    {/if}
  {/if}
{/snippet}

<Card title="Mine timer">
  <div class="flex flex-wrap gap-2 items-center mb-2">
    {#if egne.length === 0 && egneForrige.length > 0}
      <button class="btn btn-sm" disabled={kopierer} onclick={kopierForrigeUke}>
        {kopierer ? "Kopierer…" : "Kopier forrige uke"}
      </button>
    {/if}
    <span class="text-xs opacity-60 ml-auto" aria-live="polite">
      {#if lagrer === "lagrer"}Lagrer…{:else if lagrer === "lagret"}Lagret{:else if lagrer === "feil"}Ikke lagret{/if}
    </span>
  </div>
  {#if laasVises}
    <p class="text-xs opacity-70 mb-2">
      Timene er låst t.o.m. {formatDate(uke.locked_through)} og kan ikke endres.
    </p>
  {/if}

  {#if rader.length === 0}
    <p class="text-sm opacity-70 mb-2">
      Ingen timer denne uken. Velg et prosjekt under og trykk «Legg til rad», eller før timer uten
      prosjekt.
    </p>
  {/if}

  {#if rader.length > 0}
    <!-- Uke-grid (skjult på mobil, der dagvisningen tar over) -->
    <div class="hidden sm:block">
      <table class="table table-sm w-full">
        <thead>
          <tr>
            <th class="w-44">Prosjekt</th>
            {#each dager as dag (dag.dato)}
              <th
                class={"text-center " + (dag.idag ? "text-primary" : dag.helg ? "opacity-50" : "")}
                aria-current={dag.idag ? "date" : undefined}
              >
                {dag.navn} {dag.dag}
              </th>
            {/each}
            <th class="text-right w-14">Sum</th>
          </tr>
        </thead>
        <tbody>
          {#each rader as prosjekt, radIdx (prosjekt)}
            <tr>
              <td class="whitespace-nowrap overflow-hidden text-ellipsis max-w-44">
                {#if prosjekt}
                  <span class="font-medium">{prosjekt}</span>
                  <span class="text-xs opacity-60">{dimNavn(prosjekt)}</span>
                {:else}
                  <span class="opacity-60">Uten prosjekt</span>
                {/if}
                {#if radStandard(prosjekt).fakturerbar}
                  <div class="text-xs opacity-50">{kr(radStandard(prosjekt).timesats_ore)}/t</div>
                {/if}
              </td>
              {#each dager as dag, dagIdx (dag.dato)}
                <td class={"p-1 text-center " + (dag.idag ? "bg-base-200" : "")}>
                  {@render celleInput(prosjekt, radIdx, dag, dagIdx, "")}
                </td>
              {/each}
              <td class="text-right font-medium">
                {radSum[prosjekt] ? minutterTilTimer(radSum[prosjekt]) : ""}
              </td>
            </tr>
          {/each}
        </tbody>
        <tfoot>
          <tr>
            <td class="font-medium">Sum dag</td>
            {#each dager as dag (dag.dato)}
              <td class="text-center font-medium">
                {dagSum[dag.dato] ? minutterTilTimer(dagSum[dag.dato]) : ""}
              </td>
            {/each}
            <td class="text-right font-medium">{minutterTilTimer(ukeSum)} t</td>
          </tr>
        </tfoot>
      </table>
      <p class="text-xs opacity-50 mt-1">
        Skriv 7,5 eller 7:30. Tab, Enter og piltastene flytter mellom cellene, og en piltast forbi
        søndag eller mandag går videre til neste eller forrige uke. Page Down og Page Up bytter uke.
        Beskrivelse og sats står under tabellen for cellen du står i.
      </p>
    </div>

    <!-- Dagvisning på mobil: samme rader, én dag -->
    <div class="sm:hidden">
      <div class="grid grid-cols-7 gap-1 mb-2" role="group" aria-label="Velg dag">
        {#each dager as dag (dag.dato)}
          <button
            class={"btn btn-sm h-auto min-h-11 flex-col gap-0 px-0 " +
              (dag.dato === mobilDag ? "btn-primary" : "btn-ghost")}
            aria-pressed={dag.dato === mobilDag}
            aria-label={dayName(dag.dato) +
              (dagSum[dag.dato] ? ", " + minutterTilTimer(dagSum[dag.dato]) + " timer" : "")}
            onclick={() => velgDag(dag.dato)}
          >
            <span>{dag.navn} {dag.dag}</span>
            <span class="text-xs opacity-70">
              {dagSum[dag.dato] ? minutterTilTimer(dagSum[dag.dato]) : "–"}
            </span>
          </button>
        {/each}
      </div>
      <table class="table table-sm">
        <tbody>
          {#each rader as prosjekt (prosjekt)}
            <tr>
              <td class="max-w-0 w-full truncate">{radNavn(prosjekt)}</td>
              <td class="w-24">
                {@render celleInput(prosjekt, rader.indexOf(prosjekt), mobilVisning, -1, "input-md min-w-20")}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="text-sm mt-1">
        Sum {dayName(mobilVisning.dato)}: <b>{minutterTilTimer(dagSum[mobilVisning.dato] || 0)} t</b>
        · uken: <b>{minutterTilTimer(ukeSum)} t</b>
      </p>
    </div>
  {/if}

  <!-- Ny rad -->
  <div class="flex flex-wrap gap-2 items-center mt-3">
    <DimSelect
      {dims}
      kind="prosjekt"
      label="Prosjekt for ny rad"
      alleLabel="Velg prosjekt…"
      cls="select select-sm"
      bind:value={nyProsjekt}
    >
      {#snippet tomHint()}
        <DimRegisterLenke
          {companyId}
          tekst="Ingen prosjekter ennå — opprett dem i"
          ansattTekst="Ingen prosjekter er opprettet ennå."
        />
      {/snippet}
    </DimSelect>
    <button class="btn btn-sm" onclick={leggTilRad}>Legg til rad</button>
  </div>

  <!-- Linjene i valgt celle -->
  {#if panel}
    <section class="mt-3 p-3 rounded-lg bg-base-200" aria-label="Linjene i valgt celle">
      <p class="text-sm font-medium mb-2">{radNavn(panel.prosjekt)} · {dayName(panel.dato)}</p>
      {#each panelLinjer as e (e.entry_id)}
        <div class="flex flex-wrap gap-2 items-end mb-3">
          {#if e.invoice_no || laast(e.dato)}
            <span class="text-sm">
              {e.beskrivelse || "(uten beskrivelse)"} — {minutterTilTimer(e.minutter)} t
            </span>
            <span class="badge badge-ghost badge-sm">
              {e.invoice_no ? "faktura " + e.invoice_no : "låst"}
            </span>
          {:else}
            <label class="flex flex-col gap-1 text-xs flex-1 min-w-48">
              Beskrivelse
              <input
                class="input input-sm w-full"
                placeholder="Hva jobbet du med?"
                value={e.beskrivelse}
                onchange={(ev) =>
                  endreFelt(e, { beskrivelse: ev.target.value }, ev.target, e.beskrivelse)}
              />
            </label>
            <label class="flex flex-col gap-1 text-xs w-20">
              Timer
              <input
                class="input input-sm w-full text-right"
                inputmode="decimal"
                value={minutterTilTimer(e.minutter)}
                onchange={(ev) => endreTimer(e, ev.target)}
              />
            </label>
            <label class="label cursor-pointer gap-2 h-8">
              <input
                type="checkbox"
                class="checkbox checkbox-sm"
                checked={e.fakturerbar}
                onchange={(ev) =>
                  endreFelt(e, { fakturerbar: ev.target.checked }, ev.target, e.fakturerbar)}
              />
              <span class="text-sm">Fakturerbar</span>
            </label>
            {#if e.fakturerbar && kanSats}
              <label class="flex flex-col gap-1 text-xs w-28">
                Sats (kr/t)
                <input
                  class="input input-sm w-full text-right"
                  inputmode="decimal"
                  value={satsTekst(e.timesats_ore)}
                  onchange={(ev) => endreSats(e, ev.target)}
                />
              </label>
            {:else if e.fakturerbar && e.timesats_ore != null}
              <span class="text-sm opacity-70 h-8 leading-8" title="Satsen kommer fra prosjektet">
                {kr(e.timesats_ore)}/t
              </span>
            {/if}
            <button class="btn btn-sm btn-ghost" onclick={() => slettLinje(e)}>Slett</button>
          {/if}
        </div>
      {:else}
        {#if !nyLinje}
          <p class="text-sm opacity-70 mb-2">
            {laast(panel.dato)
              ? "Dagen er låst."
              : "Ingen timer ført. Skriv timene i cellen, eller legg til en linje med beskrivelse."}
          </p>
        {/if}
      {/each}

      {#if nyLinje}
        <form class="flex flex-wrap gap-2 items-end" onsubmit={lagreNyLinje}>
          <label class="flex flex-col gap-1 text-xs flex-1 min-w-48">
            Beskrivelse
            <input
              class="input input-sm w-full"
              placeholder="Hva jobbet du med?"
              bind:value={nyLinje.beskrivelse}
            />
          </label>
          <label class="flex flex-col gap-1 text-xs w-20">
            Timer
            <input class="input input-sm w-full text-right" inputmode="decimal" bind:value={nyLinje.timer} />
          </label>
          <label class="label cursor-pointer gap-2 h-8">
            <input type="checkbox" class="checkbox checkbox-sm" bind:checked={nyLinje.fakturerbar} />
            <span class="text-sm">Fakturerbar</span>
          </label>
          {#if nyLinje.fakturerbar && kanSats}
            <label class="flex flex-col gap-1 text-xs w-28">
              Sats (kr/t)
              <input class="input input-sm w-full text-right" inputmode="decimal" bind:value={nyLinje.sats} />
            </label>
          {/if}
          <button class="btn btn-sm btn-primary">Lagre linjen</button>
          <button type="button" class="btn btn-sm btn-ghost" onclick={() => (nyLinje = null)}>
            Avbryt
          </button>
        </form>
      {:else if !laast(panel.dato)}
        <button class="btn btn-sm btn-ghost" onclick={startNyLinje}>+ Linje med beskrivelse</button>
      {/if}
    </section>
  {/if}

  <!-- Kollegers timer — bare i svaret når TIMER_LES_ALLE -->
  {#if andre.length}
    <h3 class="font-medium mt-4 mb-1">Hele laget</h3>
    <div class="overflow-x-auto">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>Dato</th><th>Hvem</th><th>Beskrivelse</th><th>Prosjekt</th>
            <th class="text-right">Timer</th><th>Sats</th><th><span class="sr-only">Faktura</span></th>
          </tr>
        </thead>
        <tbody>
          {#each andre as e (e.entry_id)}
            <tr>
              <td class="whitespace-nowrap">{formatDate(e.dato)}</td>
              <td>{e.person}</td>
              <td>{e.beskrivelse}</td>
              <td>{e.prosjekt ? e.prosjekt : "–"}</td>
              <td class="text-right whitespace-nowrap">{minutterTilTimer(e.minutter)} t</td>
              <td class="whitespace-nowrap">{e.fakturerbar ? kr(e.timesats_ore) + "/t" : "–"}</td>
              <td>
                {#if e.invoice_no}
                  <span class="badge badge-ghost badge-sm">faktura {e.invoice_no}</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</Card>
