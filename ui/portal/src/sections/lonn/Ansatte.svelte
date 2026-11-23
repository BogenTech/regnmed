<script>
  import { api, post, send } from "../../lib/api.js";
  import { naadd } from "../../lib/pakke.js";
  import { formatDate, kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, sporsmal } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import { untrack } from "svelte";
  import Arbeidsforhold from "./Arbeidsforhold.svelte";
  import WithholdingFields from "./WithholdingFields.svelte";
  import {
    commonRate,
    fillBlanks,
    fraAnsatt,
    newWithholding,
    rateEmployees,
    suggest,
    tilPayload,
    withholdingOf,
    withholdingPayload,
  } from "./employment.js";

  let { companyId, ansatte, koder, canFetchCard, invitasjoner, medlemmer, onDone } = $props();

  let navn = $state("");
  let fnr = $state("");
  let stilling = $state("");
  let fra = $state(today());
  let previousStart = today();
  let lonn = $state("");
  let withholding = $state(untrack(() => newWithholding(canFetchCard)));
  let saving = $state(false);
  let fp = $state(untrack(() => commonRate(ansatte)));
  let epost = $state("");
  let units = $state([]);
  let forhold = $state(untrack(() => suggest(ansatte, [], today())));
  // A suggestion may be replaced by a better one, a user's value never.
  let suggestedUnit = untrack(() => forhold.virksomhet);
  // Suggestions keep updating only until the user starts typing.
  let touched = $state(false);

  $effect(() => {
    const id = companyId;
    units = [];
    api("/companies/" + id + "/business-units")
      .then((s) => {
        if (companyId === id) units = s.units;
      })
      .catch(() => {});
  });

  $effect(() => {
    const f = suggest(ansatte, units, untrack(() => fra));
    const sats = commonRate(ansatte);
    untrack(() => {
      if (!touched) {
        fp = sats;
        forhold = { ...f, yrkeskode: forhold.yrkeskode };
        suggestedUnit = f.virksomhet;
      } else if (forhold.virksomhet === suggestedUnit) {
        forhold.virksomhet = f.virksomhet;
        suggestedUnit = f.virksomhet;
      }
      if (redigering?.suggestedUnit != null && redigering.verdi.virksomhet === redigering.suggestedUnit) {
        redigering.verdi.virksomhet = f.virksomhet;
        redigering.suggestedUnit = f.virksomhet;
      }
      // The card may have changed under an open editor: never save the old value over it.
      if (editingWithholding) {
        const now = ansatte.find((a) => a.id === editingWithholding.ansatt.id);
        if (now && withholdingText(now) !== withholdingText(editingWithholding.ansatt)) {
          editingWithholding = null;
          toast("Trekket ble endret mens skjemaet var åpent. Åpne det på nytt.", false);
        }
      }
    });
  });

  // The last pay change follows the start date until someone sets their own.
  function startChanged(ny) {
    if (forhold.sisteLonnsendring === previousStart) forhold.sisteLonnsendring = ny;
    previousStart = ny;
  }

  // The package limit shown up front; the server refuses regardless.
  let pakke = $state(null);
  $effect(() => {
    const id = companyId;
    void ansatte?.length; // read again after the list changes: the count is what fills up
    pakke = null;
    api("/companies/" + id + "/subscription")
      .then((s) => {
        if (companyId === id) pakke = s;
      })
      .catch(() => {});
  });
  let fullt = $derived(!!pakke?.bruk && naadd(pakke.bruk.ansatte, pakke.bruk.ansatte_grense));

  function nullstill() {
    navn = "";
    fnr = "";
    stilling = "";
    fra = today();
    previousStart = fra;
    lonn = "";
    withholding = newWithholding(canFetchCard);
    fp = commonRate(ansatte);
    epost = "";
    forhold = suggest(ansatte, units, fra);
    suggestedUnit = forhold.virksomhet;
    touched = false;
  }

  async function opprett() {
    const fetchCard = withholding.kind === "card";
    const name = navn.trim();
    saving = true;
    try {
      const created = await post("/companies/" + companyId + "/employees", {
        navn: navn.trim(),
        fodselsnummer: fnr.trim(),
        stilling: stilling.trim() || null,
        ansatt_fra: fra.trim(),
        manedslonn_ore: lonn.trim() ? parseKr(lonn.trim()) : null,
        ...withholdingPayload(withholding),
        feriepenger_bp: Math.round(parseFloat(fp.trim().replace(",", ".")) * 100),
        ...tilPayload(forhold),
      });
      // Invitasjonen kobler den ansatte til portalbrukeren når hun selv
      // logger inn med adressen — admin velger aldri en person fra en
      // liste (docs/lonn.md).
      if (epost.trim()) {
        // The employee exists now, so a refused invitation must not stop the rest.
        await inviterAnsatt(created.employee_id, epost.trim()).catch((e) =>
          toast("Ansatt registrert, men invitasjonen feilet: " + e.message, false),
        );
      }
      toast("Ansatt registrert", true);
      nullstill();
      if (fetchCard) await fetchTaxCard(created.employee_id, name);
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      saving = false;
    }
  }

  // A failed fetch leaves the withholding as it was and says what that is.
  async function fetchTaxCard(employeeId, name, kept = "står med 50 % trekk") {
    const year = new Date().getFullYear();
    try {
      const b = await post(
        "/companies/" + companyId + "/tax-cards?year=" + year + "&employee=" + employeeId,
        {},
      );
      const card = b.skattekort?.[0];
      if (b.besvart && !card?.satt) {
        toast(
          "Skattekortet ble ikke satt for " + name + ", som beholder trekket. " + (card?.merknad ?? ""),
          false,
        );
      } else {
        toast(
          b.besvart
            ? "Skattekortet er hentet"
            : "Skattekortet er bestilt, men svaret er ikke klart. Hent svaret under Skattekort.",
          b.besvart,
        );
      }
    } catch (error) {
      toast(name + " " + kept + " fordi skattekortet ikke ble hentet: " + error.message, false);
    }
  }

  function withholdingText(w) {
    if (w.trekk_type === "prosent") {
      return prosent(w.trekk_prosent_bp, 1) + (w.trekk_12_maneder ? " (12 mnd)" : "");
    }
    if (w.trekk_type === "tabell") return "tabell " + w.trekk_tabell;
    if (w.trekk_type === "frikort") return "frikort, grense " + kr(w.frikort_grense_ore);
    return "ingen trekk";
  }

  // One employee's withholding at a time, with its trail.
  let editingWithholding = $state(null); // { ansatt, value, reason, history }

  async function openWithholding(a) {
    editingWithholding = { ansatt: a, value: withholdingOf(a), reason: "", history: [] };
    try {
      const h = await api("/companies/" + companyId + "/employees/" + a.id + "/withholding");
      if (editingWithholding?.ansatt.id === a.id) editingWithholding.history = h.history;
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Another employee's editor may have been opened while a request ran.
  function closeWithholding(id) {
    if (editingWithholding?.ansatt.id === id) editingWithholding = null;
  }

  async function saveWithholding() {
    const id = editingWithholding.ansatt.id;
    saving = true;
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/employees/" + id + "/withholding",
        { ...withholdingPayload(editingWithholding.value), reason: editingWithholding.reason },
      );
      toast("Trekket er endret, og endringen er logget", true);
      closeWithholding(id);
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      saving = false;
    }
  }

  async function fetchFromEditor() {
    const a = editingWithholding.ansatt;
    saving = true;
    await fetchTaxCard(a.id, a.navn, "beholder trekket (" + withholdingText(a) + ")");
    saving = false;
    closeWithholding(a.id);
    onDone();
  }

  async function inviterAnsatt(employeeId, adresse) {
    const svar = await post("/companies/" + companyId + "/invitations", {
      epost: adresse,
      rolle: "ansatt",
      employee_id: employeeId,
    });
    toast(
      svar.epost_sendt
        ? "Invitasjonen er sendt — koblingen skjer når den ansatte logger inn"
        : "Invitasjonen er registrert, men e-posten gikk ikke ut — si fra til vedkommende selv",
      svar.epost_sendt,
    );
  }

  // Per-ansatt koblingsskjema, ett om gangen (samme mønster som
  // tildelingsskjemaet i plattformvisningen).
  let kobling = $state(null); // { ansatt, epost, personId }

  function apneKobling(a) {
    kobling = { ansatt: a, epost: "", personId: "" };
  }

  // Arbeidsforholdet for a-meldingen, ett om gangen.
  let redigering = $state(null); // { ansatt, verdi }

  async function lagreArbeidsforhold() {
    try {
      await send(
        "PUT",
        "/companies/" + companyId + "/employees/" + redigering.ansatt.id + "/employment",
        tilPayload(redigering.verdi),
      );
      toast("Arbeidsforholdet er lagret", true);
      redigering = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  const prosent = (bp, desimaler) => (bp / 100).toFixed(desimaler).replace(".", ",") + " %";

  const invitasjonFor = (a) => (invitasjoner || []).find((i) => i.employee_id === a.id);

  // Bare direkte, aktive medlemmer kan velges; de som alt er koblet til
  // en annen ansatt filtreres bort så listen ikke tilbyr valg serveren
  // uansett nekter.
  const koblbareMedlemmer = $derived.by(() => {
    if (!medlemmer) return null;
    const koblet = new Set(ansatte.filter((a) => a.person).map((a) => a.person.person_id));
    return medlemmer.filter((m) => m.aktiv && m.kan_endres && !koblet.has(m.person_id));
  });

  async function inviterFraKobling() {
    try {
      await inviterAnsatt(kobling.ansatt.id, kobling.epost.trim());
      kobling = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function kobleTilMedlem() {
    try {
      await post("/companies/" + companyId + "/employees/" + kobling.ansatt.id + "/link", {
        person_id: kobling.personId,
      });
      toast("Koblet — handlingen er logget", true);
      kobling = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Sluttdatoen stopper a-meldingen i å rapportere arbeidsforholdet hver
  // måned (F108). Tomt felt fjerner en feilført dato.
  async function sluttdato(a) {
    const svar = await sporsmal("Siste dag i arbeidsforholdet for " + a.navn, {
      type: "date",
      standard: a.ansatt_til ?? "",
      valgfri: true,
      ok: "Lagre",
      melding:
        "A-meldingen rapporterer arbeidsforholdet hver måned til og med måneden det slutter. " +
        "La feltet stå tomt for å fjerne en sluttdato som er feil.",
    });
    if (svar === null) return;
    try {
      await send("PUT", "/companies/" + companyId + "/employees/" + a.id + "/end", {
        ansatt_til: svar || null,
      });
      toast(svar ? "Sluttdato lagret" : "Sluttdatoen er fjernet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function feriesats(a) {
    const svar = await sporsmal("Feriepengesats for " + a.navn + " (%)", {
      standard: String((a.feriepenger_krevd_bp ?? a.feriepenger_bp) / 100).replace(".", ","),
      ok: "Lagre",
      melding:
        "Gjelder fra neste lønnskjøring. Får satsen tillegget for arbeidstakere over 60, tar neste " +
        "kjøring igjen tillegget på det som alt er utbetalt i år.",
    });
    if (svar === null) return;
    const bp = Math.round(parseFloat(svar.replace(",", ".")) * 100);
    if (!Number.isFinite(bp)) {
      toast("Skriv satsen som et tall, for eksempel 12,5", false);
      return;
    }
    try {
      await send("PUT", "/companies/" + companyId + "/employees/" + a.id + "/holiday-rate", {
        feriepenger_bp: bp,
      });
      toast("Feriepengesatsen er lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Koblingen avgjør hvem som leser lønnsslippen, så den fjernes ikke
  // på et feiltrykk.
  async function kobleFra(a) {
    const ok = await bekreft(
      a.person.navn + " mister tilgangen til lønnsslippene for " + a.navn + ". Handlingen logges.",
      { tittel: "Koble fra portalbrukeren?", ok: "Koble fra", farlig: true },
    );
    if (!ok) return;
    try {
      await send("DELETE", "/companies/" + companyId + "/employees/" + a.id + "/link");
      toast("Koblingen er fjernet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekkTilbake(inv) {
    const ok = await bekreft("Invitasjonen til " + inv.epost + " slutter å virke.", {
      tittel: "Trekke tilbake invitasjonen?",
      ok: "Trekk tilbake",
    });
    if (!ok) return;
    try {
      await send("DELETE", "/companies/" + companyId + "/invitations/" + inv.id);
      toast("Invitasjonen er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Ansatte">
  <details class="text-sm opacity-80 mb-2">
    <summary class="cursor-pointer">Om ansattregisteret</summary>
    <p class="mt-2">
      Identiteten (fødselsnummeret) er permanent og vises ikke her — listen viser
      <b>fødselsdato</b>, som er nok til å kjenne igjen en ansatt. Feriepengesatsen står per ansatt
      fordi den er et faktum om arbeidsforholdet: 10,2 % etter ferieloven §10, 12,5 % fra
      opptjeningsåret man fyller 59 (tillegget på 2,3 prosentpoeng bare opp til 6G), høyere på
      tariff. <b>Portalbruker</b>-kolonnen styrer selvbetjeningen: den koblede
      brukeren ser sin egen lønnsslipp, og timelønn hentes fra dennes timeføring. Tryggest er å
      invitere på e-post — da kobler den ansatte seg selv ved å logge inn.
    </p>
  </details>
  {#if ansatte.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Navn</th>
            <th>Stilling</th>
            <th>Født</th>
            <th class="text-right">Månedslønn</th>
            <th class="text-right">Trekk</th>
            <th class="text-right">Feriepenger</th>
            <th>Portalbruker</th>
            <th>A-melding</th>
          </tr>
        </thead>
        <tbody>
          {#each ansatte as a (a.id)}
            {@const inv = invitasjonFor(a)}
            <tr class={a.ansatt_til && a.ansatt_til < today() ? "opacity-50" : ""}>
              <td>
                {a.navn}
                {#if a.ansatt_til}
                  <div class="text-xs opacity-70 whitespace-nowrap">sluttdato {formatDate(a.ansatt_til)}</div>
                {/if}
              </td>
              <td class="text-xs opacity-70">{a.stilling || ""}</td>
              <!-- Ferieloven krever ikke fnr her; datoen er nok for å kjenne igjen. -->
              <td class="text-xs opacity-70 whitespace-nowrap">{formatDate(a.fodselsdato || "")}</td>
              <td class="text-right">{a.manedslonn_ore == null ? "" : kr(a.manedslonn_ore)}</td>
              <td class="text-right">
                {withholdingText(a)}
                <button
                  class="btn btn-ghost btn-xs"
                  aria-label={"Endre trekk for " + a.navn}
                  onclick={() => openWithholding(a)}
                >
                  Endre …
                </button>
              </td>
              <td class="text-right whitespace-nowrap">
                {prosent(a.feriepenger_bp, 1)}
                <button
                  class="btn btn-ghost btn-xs"
                  aria-label={"Endre feriepengesats for " + a.navn}
                  onclick={() => feriesats(a)}
                >
                  Endre …
                </button>
                {#if a.feriepenger_krevd_bp}
                  <div class="text-xs text-warning">skal være {prosent(a.feriepenger_krevd_bp, 1)}</div>
                {/if}
              </td>
              <td class="text-xs">
                {#if a.person}
                  <span title={a.person.epost || ""}>{a.person.navn}</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => kobleFra(a)}>
                    Koble fra
                  </button>
                {:else if inv}
                  <span class="opacity-70">invitert: {inv.epost}</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => trekkTilbake(inv)}>
                    Trekk tilbake
                  </button>
                {:else}
                  <span class="opacity-50">ikke koblet</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => apneKobling(a)}>
                    Koble …
                  </button>
                {/if}
              </td>
              <td class="text-xs">
                {#if a.amelding_mangler.length}
                  <span class="text-warning">mangler {a.amelding_mangler.length}</span>
                {:else}
                  <span class="opacity-70">komplett</span>
                {/if}
                <button
                  class="btn btn-ghost btn-xs whitespace-nowrap"
                  aria-label={"Arbeidsforhold for " + a.navn}
                  onclick={() => {
                    const f = suggest(ansatte, units);
                    redigering = {
                      ansatt: a,
                      verdi: fillBlanks(fraAnsatt(a), f),
                      suggestedUnit: a.arbeidsforhold?.virksomhet_orgnr ? null : f.virksomhet,
                    };
                  }}
                >
                  {a.amelding_mangler.length ? "Fyll ut …" : "Arbeidsforhold …"}
                </button>
                <button
                  class="btn btn-ghost btn-xs whitespace-nowrap"
                  aria-label={"Sluttdato for " + a.navn}
                  onclick={() => sluttdato(a)}
                >
                  Sluttdato …
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if redigering}
      <div class="card border border-base-300 card-sm mb-4 max-w-md">
        <div class="card-body">
          <h4 class="font-semibold text-sm">Arbeidsforhold for {redigering.ansatt.navn}</h4>
          <p class="text-xs opacity-70 mb-2">
            Tomme felt har fått forslag fra Enhetsregisteret og forrige ansatte. Sjekk dem før du
            lagrer. A-meldingen nektes så lenge et felt mangler.
          </p>
          {#if redigering.ansatt.amelding_mangler.length}
            <p class="text-xs text-warning mb-2">
              Mangler: {redigering.ansatt.amelding_mangler.join(", ")}
            </p>
          {/if}
          <Arbeidsforhold
            bind:value={redigering.verdi}
            {koder}
            {companyId}
            {units}
            onunitinput={() => (redigering.suggestedUnit = null)}
          />
          <div class="flex gap-2 mt-2">
            <button class="btn btn-sm btn-primary" onclick={lagreArbeidsforhold}>Lagre</button>
            <button class="btn btn-ghost btn-sm" onclick={() => (redigering = null)}>Avbryt</button>
          </div>
        </div>
      </div>
    {/if}
    {#if editingWithholding}
      <div class="card border border-base-300 card-sm mb-4 max-w-md">
        <div class="card-body">
          <h4 class="font-semibold text-sm">Trekk for {editingWithholding.ansatt.navn}</h4>
          <p class="text-xs opacity-70 mb-2">
            Nå: {withholdingText(editingWithholding.ansatt)}. Endringen gjelder neste lønnskjøring og
            logges. Et nytt skattekort erstatter det du setter her. Frikort og trekk over 12 måneder
            kommer bare fra skattekortet.
          </p>
          <WithholdingFields bind:value={editingWithholding.value} />
          <label class="flex flex-col gap-1 text-xs mt-2">
            Begrunnelse
            <input
              class="input input-sm"
              placeholder="f.eks. tabellnummer fra skattekortet"
              bind:value={editingWithholding.reason}
            />
          </label>
          {#if editingWithholding.ansatt.trekk_12_maneder || editingWithholding.ansatt.trekk_type === "frikort"}
            <div role="alert" class="alert alert-warning alert-soft text-xs mt-2">
              <span>
                Lagrer du, forsvinner vilkårene fra skattekortet. Prosent og tabell uten 12 måneder
                følger de vanlige reglene for desember og feriepenger, og frikortets beløpsgrense
                gjelder ikke lenger.
              </span>
            </div>
          {/if}
          <div class="flex flex-wrap gap-2 mt-2">
            <button
              class="btn btn-sm btn-primary"
              disabled={saving || !editingWithholding.reason.trim()}
              onclick={saveWithholding}
            >
              Lagre
            </button>
            {#if canFetchCard}
              <button class="btn btn-sm" disabled={saving} onclick={fetchFromEditor}>
                {#if saving}<span class="loading loading-spinner loading-xs"></span>{/if}
                Hent skattekort
              </button>
            {/if}
            <button class="btn btn-ghost btn-sm" onclick={() => (editingWithholding = null)}>
              Avbryt
            </button>
          </div>
          {#if editingWithholding.history.length}
            <h5 class="font-semibold text-xs mt-3">Endringer</h5>
            <ul class="text-xs opacity-80 grid gap-1">
              {#each editingWithholding.history as h (h.at)}
                <li>
                  {new Date(h.at).toLocaleDateString("nb-NO")}: {withholdingText(h.from)} → {withholdingText(h.to)},
                  {h.changed_by}: {h.reason}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </div>
    {/if}
    {#if kobling}
      <div class="card border border-base-300 card-sm mb-4 max-w-lg">
        <div class="card-body">
          <h4 class="font-semibold text-sm mb-2">Koble {kobling.ansatt.navn} til portalbruker</h4>
          <div class="flex flex-wrap gap-2 items-end">
            <input
              class="input input-sm"
              type="email"
              placeholder="E-postadresse"
              aria-label={"E-postadresse til " + kobling.ansatt.navn}
              bind:value={kobling.epost}
            />
            <button
              class="btn btn-sm btn-primary"
              disabled={!kobling.epost.trim()}
              onclick={inviterFraKobling}
            >
              Inviter og koble
            </button>
          </div>
          <p class="text-xs opacity-70 mt-1 mb-2">
            Invitasjonen kobler når den ansatte selv logger inn med adressen — den kan ikke treffe
            feil person uten at noen andre har adgang til postkassen.
          </p>
          {#if koblbareMedlemmer && koblbareMedlemmer.length}
            <div class="flex flex-wrap gap-2 items-end">
              <select
                class="select select-sm"
                aria-label="Eksisterende medlem"
                bind:value={kobling.personId}
              >
                <option value="">— eksisterende medlem —</option>
                {#each koblbareMedlemmer as m (m.person_id)}
                  <option value={m.person_id}>{m.navn}{m.epost ? " (" + m.epost + ")" : ""}</option>
                {/each}
              </select>
              <button class="btn btn-sm" disabled={!kobling.personId} onclick={kobleTilMedlem}>
                Koble
              </button>
            </div>
          {/if}
          <button class="btn btn-ghost btn-xs mt-2" onclick={() => (kobling = null)}>Avbryt</button>
        </div>
      </div>
    {/if}
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen ansatte registrert.</p>
  {/if}

  <h3 class="font-semibold mb-1">Ny ansatt</h3>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="grid gap-2 max-w-md" oninput={() => (touched = true)}>
    <label class="flex flex-col gap-1 text-xs">
      Navn
      <input class="input input-sm" autocomplete="off" bind:value={navn} />
    </label>
    <div class="grid grid-cols-2 gap-2">
      <label class="flex flex-col gap-1 text-xs">
        Fødselsnummer (11 siffer)
        <input class="input input-sm" inputmode="numeric" autocomplete="off" bind:value={fnr} />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Stilling
        <input class="input input-sm" bind:value={stilling} />
      </label>
    </div>
    <div class="grid grid-cols-2 gap-2">
      <label class="flex flex-col gap-1 text-xs">
        Ansatt fra
        <input
          type="date"
          class="input input-sm"
          bind:value={fra}
          oninput={(e) => startChanged(e.currentTarget.value)}
        />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Månedslønn (kr)
        <input class="input input-sm" inputmode="decimal" bind:value={lonn} />
      </label>
    </div>
    <WithholdingFields bind:value={withholding} canFetch={canFetchCard} />
    <div class="grid grid-cols-2 gap-2">
      <label class="flex flex-col gap-1 text-xs">
        Feriepengesats (%)
        <input class="input input-sm" inputmode="decimal" bind:value={fp} />
        {#if fp === commonRate(ansatte)}
          <span class="opacity-70">
            {rateEmployees(ansatte).length ? "Vanligst i selskapet" : "Lovens minimum"}
          </span>
        {/if}
      </label>
    </div>
    <label class="flex flex-col gap-1 text-xs">
      E-post — inviter til portalen og koble (valgfritt)
      <input class="input input-sm" type="email" bind:value={epost} />
    </label>
    <!-- Open because the suggestions are saved with the employee: they must be seen first. -->
    <details class="text-sm" open>
      <summary class="cursor-pointer opacity-70">
        Arbeidsforhold for a-meldingen (forslag er fylt inn, sjekk dem)
      </summary>
      <div class="mt-2">
        <Arbeidsforhold
          bind:value={forhold}
          {koder}
          {companyId}
          {units}
          onunitinput={() => (suggestedUnit = null)}
        />
      </div>
    </details>
    {#if fullt}
      <p class="text-sm" role="note">
        Pakken {pakke.pakke?.navn ?? ""} har lønn for {pakke.bruk.ansatte_grense}
        {pakke.bruk.ansatte_grense === 1 ? "ansatt" : "ansatte"}. Bytt til en større pakke under
        <a class="link" href={"#/c/" + companyId + "/admin/abonnement"}>Abonnement</a> for å legge til flere.
      </p>
    {/if}
    <button class="btn btn-sm" onclick={opprett} disabled={fullt || saving}>
      {#if saving}<span class="loading loading-spinner loading-xs"></span>{/if}
      Registrer ansatt
    </button>
  </div>
</Card>
