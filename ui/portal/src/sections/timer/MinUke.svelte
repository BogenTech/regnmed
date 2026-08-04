<script>
  // Ukegrid for timeføring (#38-oppfølger): regnearkfølelsen fra
  // Tripletex/Harvest — rader er prosjekter, kolonner er dager, en celle
  // er timene den dagen. Hver celle lagrer seg selv (POST/PUT/DELETE på
  // de vanlige endepunktene) i det du forlater den; Enter og piltaster
  // flytter mellom cellene som i et regneark. Serveren avgjør hvem som
  // ser hva (TIMER_LES_ALLE) — «Hele laget» under viser bare det svaret
  // faktisk inneholdt.
  import { api, post, send } from "../../lib/api.js";
  import { kr, parseKr, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import DimRegisterLenke from "../../components/DimRegisterLenke.svelte";
  import DimSelect from "../../components/DimSelect.svelte";

  let { companyId, uke, forrigeUke, from, to, forrige, neste, dims, onDone } = $props();

  const DAGNAVN = ["Ma", "Ti", "On", "To", "Fr", "Lø", "Sø"];
  const IDAG = new Date().toISOString().slice(0, 10);

  function datoPluss(iso, dager) {
    const d = new Date(iso + "T00:00:00Z");
    d.setUTCDate(d.getUTCDate() + dager);
    return d.toISOString().slice(0, 10);
  }

  let dager = $derived(
    DAGNAVN.map((navn, i) => {
      const dato = datoPluss(from, i);
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
    for (const e of egne) {
      const nokkel = e.prosjekt || "";
      if (!sett.includes(nokkel)) sett.push(nokkel);
    }
    for (const e of egneForrige) {
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

  // Ny celle i en rad arver fakturerbar/sats fra radens nyeste linje —
  // denne uken først, ellers forrige uke.
  function radStandard(prosjekt) {
    const kilder = [...egne, ...egneForrige].filter((e) => (e.prosjekt || "") === prosjekt);
    const siste = kilder[kilder.length - 1];
    return siste
      ? { fakturerbar: siste.fakturerbar, timesats_ore: siste.timesats_ore }
      : { fakturerbar: false, timesats_ore: null };
  }

  function dimNavn(kode) {
    const d = dims.find((x) => x.kind === "prosjekt" && x.code === kode);
    return d ? d.name : "";
  }

  function laast(dato) {
    return uke.locked_through && dato <= uke.locked_through;
  }

  // "7,5", "7.5" og "7:30" er alle gyldige — tom celle betyr null timer.
  function parseTimer(tekst) {
    const t = String(tekst).trim();
    if (!t) return 0;
    if (t.includes(":")) {
      const [h, m] = t.split(":");
      const min = Math.round(Number(h || 0) * 60 + Number(m || 0));
      if (!isFinite(min) || min < 0) throw new Error("ugyldige timer: " + tekst);
      return min;
    }
    const verdi = Number(t.replace(/\s/g, "").replace(",", "."));
    if (!isFinite(verdi) || verdi < 0) throw new Error("ugyldige timer: " + tekst);
    return Math.round(verdi * 60);
  }

  function sumMin(entries) {
    return entries.reduce((sum, e) => sum + e.minutter, 0);
  }

  let lagrer = $state("");
  let aktiv = $state(null);

  async function lagreCelle(prosjekt, dato, input) {
    const entries = iCelle(prosjekt, dato);
    const gamle = sumMin(entries);
    let minutter;
    try {
      minutter = parseTimer(input.value);
    } catch (error) {
      toast(error.message, false);
      input.value = gamle ? minutterTilTimer(gamle) : "";
      return;
    }
    if (minutter === gamle) return;
    lagrer = "lagrer";
    try {
      if (entries.length === 0 && minutter > 0) {
        const standard = radStandard(prosjekt);
        await post("/companies/" + companyId + "/timesheet", {
          dato,
          minutter,
          beskrivelse: "",
          prosjekt: prosjekt || null,
          fakturerbar: standard.fakturerbar,
          timesats_ore: standard.fakturerbar ? standard.timesats_ore : null,
        });
      } else if (entries.length === 1 && minutter > 0) {
        const e = entries[0];
        await send("PUT", "/companies/" + companyId + "/timesheet/" + e.entry_id, {
          dato,
          minutter,
          beskrivelse: e.beskrivelse,
          prosjekt: prosjekt || null,
          fakturerbar: e.fakturerbar,
          timesats_ore: e.timesats_ore,
        });
      } else if (minutter === 0) {
        for (const e of entries) {
          await api("/companies/" + companyId + "/timesheet/" + e.entry_id, {
            method: "DELETE",
          });
        }
      }
      lagrer = "lagret";
      onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
      input.value = gamle ? minutterTilTimer(gamle) : "";
    }
  }

  // Piltaster og Enter flytter mellom cellene — regnearkfølelsen.
  function tastNav(event) {
    const input = event.target;
    if (!input.dataset || input.dataset.rad === undefined) return;
    const rad = Number(input.dataset.rad);
    const dag = Number(input.dataset.dag);
    let nesteRad = rad;
    let nesteDag = dag;
    if (event.key === "ArrowRight") nesteDag += 1;
    else if (event.key === "ArrowLeft") nesteDag -= 1;
    else if (event.key === "ArrowDown" || event.key === "Enter") nesteRad += 1;
    else if (event.key === "ArrowUp") nesteRad -= 1;
    else if (event.key === "Escape") {
      const entries = iCelle(rader[rad], dager[dag].dato);
      input.value = entries.length ? minutterTilTimer(sumMin(entries)) : "";
      return;
    } else return;
    const målet = document.querySelector(
      'input[data-rad="' + nesteRad + '"][data-dag="' + nesteDag + '"]',
    );
    if (målet) {
      event.preventDefault();
      målet.focus();
      målet.select();
    } else if (event.key === "Enter") {
      // Siste rad: Enter har ingen celle å gå til, men skal fortsatt
      // bekrefte cellen — lagringen ligger på blur.
      input.blur();
    }
  }

  let nyProsjekt = $state("");
  function leggTilRad() {
    const nokkel = nyProsjekt || "";
    if (!rader.includes(nokkel)) ekstraRader = [...ekstraRader, nokkel];
    nyProsjekt = "";
  }

  // Kopierer forrige ukes egne linjer til samme ukedag denne uken.
  // Tilbys bare når uken står tom, så en dobbelttrykk ikke dobler timene.
  let kopierer = $state(false);
  async function kopierForrigeUke() {
    kopierer = true;
    try {
      for (const e of egneForrige) {
        const dato = datoPluss(e.dato, 7);
        if (laast(dato) || e.invoice_no) continue;
        await post("/companies/" + companyId + "/timesheet", {
          dato,
          minutter: e.minutter,
          beskrivelse: e.beskrivelse,
          prosjekt: e.prosjekt || null,
          fakturerbar: e.fakturerbar,
          timesats_ore: e.timesats_ore,
        });
      }
      onDone();
    } catch (error) {
      toast(error.message, false);
      onDone();
    } finally {
      kopierer = false;
    }
  }

  // Detaljfeltet under gridet: cellen som sist hadde fokus, med én
  // redigerbar linje per føring (beskrivelse, fakturerbar, sats).
  let detaljer = $state([]);
  $effect(() => {
    const celle = aktiv;
    const entries = celle ? iCelle(celle.prosjekt, celle.dato) : [];
    detaljer = entries.map((e) => ({
      entry_id: e.entry_id,
      beskrivelse: e.beskrivelse,
      timer: minutterTilTimer(e.minutter),
      fakturerbar: e.fakturerbar,
      sats: e.timesats_ore == null ? "" : String(e.timesats_ore / 100).replace(".", ","),
      invoice_no: e.invoice_no,
      dato: e.dato,
    }));
  });

  async function lagreDetalj(d) {
    lagrer = "lagrer";
    try {
      await send("PUT", "/companies/" + companyId + "/timesheet/" + d.entry_id, {
        dato: d.dato,
        minutter: parseTimer(d.timer),
        beskrivelse: d.beskrivelse,
        prosjekt: aktiv.prosjekt || null,
        fakturerbar: d.fakturerbar,
        timesats_ore: d.fakturerbar ? parseKr(d.sats) : null,
      });
      lagrer = "lagret";
      onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
    }
  }

  async function slettDetalj(d) {
    try {
      await api("/companies/" + companyId + "/timesheet/" + d.entry_id, { method: "DELETE" });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  let nyLinje = $state(null);
  async function lagreNyLinje() {
    lagrer = "lagrer";
    try {
      await post("/companies/" + companyId + "/timesheet", {
        dato: aktiv.dato,
        minutter: parseTimer(nyLinje.timer),
        beskrivelse: nyLinje.beskrivelse,
        prosjekt: aktiv.prosjekt || null,
        fakturerbar: nyLinje.fakturerbar,
        timesats_ore: nyLinje.fakturerbar ? parseKr(nyLinje.sats) : null,
      });
      lagrer = "lagret";
      nyLinje = null;
      onDone();
    } catch (error) {
      lagrer = "feil";
      toast(error.message, false);
    }
  }

  // Mobil: gridet er uleselig på 375 px — der vises én dag om gangen,
  // valgt med dagknappene, med de samme radene og samme lagringsvei.
  // mobilDag kan peke utenfor uken i et øyeblikk (ukebytte før effekten
  // har løpt) — visningen faller da tilbake på mandag i stedet for å dø.
  let mobilDag = $state(IDAG);
  $effect(() => {
    // Uken byttet: pek på i dag når den er i uken, ellers mandag.
    mobilDag = IDAG >= from && IDAG <= to ? IDAG : from;
  });
  let mobilVisning = $derived(dager.find((d) => d.dato === mobilDag) || dager[0]);

  let radSum = $derived(
    Object.fromEntries(
      rader.map((r) => [r, sumMin(egne.filter((e) => (e.prosjekt || "") === r))]),
    ),
  );
  let dagSum = $derived(
    Object.fromEntries(dager.map((d) => [d.dato, sumMin(egne.filter((e) => e.dato === d.dato))])),
  );
  let ukeSum = $derived(sumMin(egne));
</script>

{#snippet celleInput(prosjekt, radIdx, dag, dagIdx, ekstraCls)}
  {@const entries = iCelle(prosjekt, dag.dato)}
  {@const fakturert = entries.length > 0 && entries.every((e) => e.invoice_no)}
  {@const flere = entries.length > 1}
  {#if fakturert}
    <span class="badge badge-ghost badge-xs" title={"faktura " + entries[0].invoice_no}>
      {minutterTilTimer(sumMin(entries))}
    </span>
  {:else}
    <input
      class={"input input-xs w-full text-center " + (ekstraCls || "")}
      value={entries.length ? minutterTilTimer(sumMin(entries)) : ""}
      disabled={laast(dag.dato)}
      readonly={flere}
      title={laast(dag.dato)
        ? "låst t.o.m. " + uke.locked_through
        : flere
          ? "flere linjer — rediger i detaljene under"
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
      <div class="w-1 h-1 rounded-full bg-primary mx-auto mt-0.5" title="har beskrivelse"></div>
    {/if}
  {/if}
{/snippet}

<Card title={"Min uke " + from + " – " + to}>
  <div class="flex flex-wrap gap-2 items-center mb-2">
    <a class="btn btn-ghost btn-xs" href={"#/c/" + companyId + "/timer?uke=" + forrige}>«</a>
    <a class="btn btn-ghost btn-xs" href={"#/c/" + companyId + "/timer?uke=" + neste}>»</a>
    {#if egne.length === 0 && egneForrige.length > 0}
      <button class="btn btn-xs" disabled={kopierer} onclick={kopierForrigeUke}>
        {kopierer ? "Kopierer…" : "Kopier forrige uke"}
      </button>
    {/if}
    <span class="text-xs opacity-60 ml-auto">
      {#if lagrer === "lagrer"}Lagrer…{:else if lagrer === "lagret"}Lagret{:else if lagrer === "feil"}Feil ved lagring{/if}
    </span>
  </div>
  {#if uke.locked_through}
    <p class="text-xs opacity-70 mb-2">Låst t.o.m. {uke.locked_through}</p>
  {/if}

  {#if rader.length === 0}
    <p class="text-sm opacity-70">
      Ingen rader ennå — legg til et prosjekt under, eller før timer uten prosjekt.
    </p>
  {/if}

  <!-- Uke-grid (skjult på mobil, der dagvisningen tar over) -->
  {#if rader.length > 0}
    <div class="hidden sm:block">
      <table class="table table-sm w-full">
        <thead>
          <tr>
            <th class="w-44">Prosjekt</th>
            {#each dager as dag (dag.dato)}
              <th class={"text-center " + (dag.idag ? "text-primary" : dag.helg ? "opacity-50" : "")}>
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
        Skriv 7,5 eller 7:30 — Enter og piltaster flytter mellom cellene. Beskrivelse og sats
        redigeres i detaljene under når en celle er valgt.
      </p>
    </div>

    <!-- Dagvisning på mobil: samme rader, én dag -->
    <div class="sm:hidden">
      <div class="flex gap-1 mb-2">
        {#each dager as dag (dag.dato)}
          <button
            class={"btn btn-xs flex-1 " + (dag.dato === mobilDag ? "btn-primary" : "btn-ghost")}
            onclick={() => (mobilDag = dag.dato)}
          >
            {dag.navn}<br />{dag.dag}
          </button>
        {/each}
      </div>
      <table class="table table-sm">
        <tbody>
          {#each rader as prosjekt (prosjekt)}
            <tr>
              <td>{prosjekt || "Uten prosjekt"}</td>
              <td class="w-24">
                {@render celleInput(prosjekt, rader.indexOf(prosjekt), mobilVisning, -1, "input-sm")}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="text-sm">
        Sum {mobilVisning.dato}: <b>{minutterTilTimer(dagSum[mobilVisning.dato] || 0)} t</b> · uke:
        <b>{minutterTilTimer(ukeSum)} t</b>
      </p>
    </div>
  {/if}

  <!-- Ny rad -->
  <div class="flex flex-wrap gap-2 items-center mt-3">
    <DimSelect {dims} kind="prosjekt" cls="select select-sm" bind:value={nyProsjekt}>
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

  <!-- Detaljer for valgt celle -->
  {#if aktiv && (detaljer.length > 0 || nyLinje)}
    <div class="mt-3 p-3 rounded-lg bg-base-200">
      <p class="text-sm font-medium mb-2">
        {aktiv.prosjekt || "Uten prosjekt"} · {aktiv.dato}
      </p>
      {#each detaljer as d (d.entry_id)}
        <div class="flex flex-wrap gap-2 items-center mb-2">
          {#if d.invoice_no}
            <span class="text-sm">{d.beskrivelse || "(uten beskrivelse)"} — {d.timer} t</span>
            <span class="badge badge-ghost badge-xs">faktura {d.invoice_no}</span>
          {:else}
            <input
              class="input input-sm flex-1 min-w-40"
              placeholder="Hva jobbet du med?"
              bind:value={d.beskrivelse}
            />
            <input class="input input-sm w-16 text-right" bind:value={d.timer} />
            <label class="label cursor-pointer gap-1">
              <input type="checkbox" class="checkbox checkbox-xs" bind:checked={d.fakturerbar} />
              <span class="text-xs">Fakturerbar</span>
            </label>
            {#if d.fakturerbar}
              <input class="input input-sm w-20" placeholder="Sats (kr/t)" bind:value={d.sats} />
            {/if}
            <button class="btn btn-sm btn-primary" onclick={() => lagreDetalj(d)}>Lagre</button>
            <button class="btn btn-sm btn-ghost" onclick={() => slettDetalj(d)}>Slett</button>
          {/if}
        </div>
      {/each}
      {#if nyLinje}
        <div class="flex flex-wrap gap-2 items-center mb-2">
          <input
            class="input input-sm flex-1 min-w-40"
            placeholder="Hva jobbet du med?"
            bind:value={nyLinje.beskrivelse}
          />
          <input
            class="input input-sm w-16 text-right"
            placeholder="Timer"
            bind:value={nyLinje.timer}
          />
          <label class="label cursor-pointer gap-1">
            <input type="checkbox" class="checkbox checkbox-xs" bind:checked={nyLinje.fakturerbar} />
            <span class="text-xs">Fakturerbar</span>
          </label>
          {#if nyLinje.fakturerbar}
            <input
              class="input input-sm w-20"
              placeholder="Sats (kr/t)"
              bind:value={nyLinje.sats}
            />
          {/if}
          <button class="btn btn-sm btn-primary" onclick={lagreNyLinje}>Lagre</button>
          <button class="btn btn-sm btn-ghost" onclick={() => (nyLinje = null)}>Avbryt</button>
        </div>
      {:else if !laast(aktiv.dato)}
        <button
          class="btn btn-xs btn-ghost"
          onclick={() => {
            const standard = radStandard(aktiv.prosjekt);
            nyLinje = {
              beskrivelse: "",
              timer: "",
              fakturerbar: standard.fakturerbar,
              sats:
                standard.timesats_ore == null
                  ? ""
                  : String(standard.timesats_ore / 100).replace(".", ","),
            };
          }}
        >
          + Ny linje samme dag
        </button>
      {/if}
    </div>
  {/if}

  <!-- Kollegers timer — bare i svaret når TIMER_LES_ALLE -->
  {#if andre.length}
    <h3 class="font-medium mt-4 mb-1">Hele laget</h3>
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Dato</th><th>Hvem</th><th>Beskrivelse</th><th>Prosjekt</th>
          <th class="text-right">Timer</th><th>Sats</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each andre as e (e.entry_id)}
          <tr>
            <td>{e.dato}</td>
            <td>{e.person}</td>
            <td>{e.beskrivelse}</td>
            <td>{e.prosjekt ? e.prosjekt : "–"}</td>
            <td class="text-right">{minutterTilTimer(e.minutter)} t</td>
            <td>{e.fakturerbar ? kr(e.timesats_ore) + "/t" : "–"}</td>
            <td>
              {#if e.invoice_no}
                <span class="badge badge-ghost badge-xs">faktura {e.invoice_no}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
