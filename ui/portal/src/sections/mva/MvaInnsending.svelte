<script>
  // «Godkjenn og send»: the person signs in with ID-porten, and a worker
  // files the melding through Altinn (docs/mva.md). The server decides
  // everything; this shows where the filing stands and the next step.
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate, formatTime } from "../../lib/format.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import { huskTermin, taRetur } from "../../lib/mvaretur.js";
  import { lukkeTekst, nesteSteg, stoppTekst } from "../../lib/mvasteg.js";

  let { companyId, year, termin, title } = $props();

  let loaded = $state(false);
  let available = $state(false);
  let filing = $state(null);
  let working = $state(false);
  let loadError = $state(null);

  const base = $derived("/companies/" + companyId + "/vat-filings");
  const canSend = $derived(harRett(companyId, "MVA_SEND"));

  const STATUS = {
    opprettet: ["Klar til godkjenning.", "info"],
    godkjent: ["Sender inn til Skatteetaten …", "info"],
    oppretter_instans: ["Sender inn til Skatteetaten …", "info"],
    instans_opprettet: ["Sender inn til Skatteetaten …", "info"],
    sendt: ["Sendt inn. Venter på svar fra Skatteetaten.", "info"],
    kvittert: ["Mottatt av Skatteetaten.", "success"],
    avvist: ["Skatteetaten avviste meldingen.", "error"],
    avvik: ["Skatteetaten fant feil, så meldingen er ikke sendt. Rett feilene og godkjenn på nytt.", "warning"],
    uavklart: ["Vi vet ikke sikkert om innsendingen gikk gjennom. Sjekk Altinn før du gjør noe annet.", "warning"],
    feilet: ["Innsendingen stoppet.", "error"],
    ma_godkjennes: ["Godkjenningen er utløpt. Logg inn på nytt for å sende.", "warning"],
    avbrutt: ["Innsendingen er avbrutt. Ingenting ble sendt.", "info"],
  };
  const FILES = { kvittering: "Kvittering", betalingsinformasjon: "Betalingsinformasjon", valideringsresultat: "Kontrollresultat" };

  async function load(id, y, t) {
    const answer = await api("/companies/" + id + "/vat-filings?year=" + y + "&period=" + t);
    if (id !== companyId || y !== year || t !== termin) return;
    available = answer.tilgjengelig;
    filing = answer.filings[0] ?? null;
    loaded = true;
  }

  $effect(() => {
    const [id, y, t] = [companyId, year, termin];
    loaded = false;
    filing = null;
    loadError = null;
    const retur = taRetur(id);
    (retur ? finish(retur) : Promise.resolve())
      .then(() => load(id, y, t))
      .catch((e) => (loadError = e.message));
  });

  async function finish(retur) {
    if (retur.error) {
      toast("Innloggingen ble avbrutt. Ingenting er sendt.", false);
      return;
    }
    try {
      await post("/companies/" + retur.companyId + "/vat-filings/" + retur.filingId + "/approve", {
        code: retur.code,
        state: retur.state,
      });
      toast("Godkjent. Sender inn mva-meldingen.", true);
    } catch (e) {
      toast(e.message, false);
    }
  }

  // Follows the worker, and Skatteetaten's answer once it is sent.
  $effect(() => {
    const wait = steg.poll;
    if (!wait) return;
    void filing;
    const [id, y, t] = [companyId, year, termin];
    const timer = setTimeout(() => load(id, y, t).catch(() => {}), wait);
    return () => clearTimeout(timer);
  });

  function toIdporten(url) {
    huskTermin(year, termin);
    location.assign(url);
  }

  async function approve() {
    working = true;
    try {
      const answer = await post(base, { year, period: termin });
      const amount = answer.filing.assessed_kr;
      const correction = answer.filing.korrigerer;
      const ok = await bekreft(
        title + ": " + (amount >= 0 ? "å betale " : "til gode ") + kr(Math.abs(amount) * 100) +
          " kr." + (correction ? " Meldingen erstatter den som er levert for terminen." : "") +
          " Du logger inn med ID-porten, og innloggingen din er godkjenningen av meldingen.",
        {
          tittel: correction ? "Godkjenn og send korrigert melding?" : "Godkjenn og send mva-meldingen?",
          ok: "Gå til ID-porten",
        },
      );
      if (ok) toIdporten(answer.authorize_url);
      else await load(companyId, year, termin);
    } catch (e) {
      toast(e.message, false);
    } finally {
      working = false;
    }
  }

  async function signInAgain() {
    working = true;
    try {
      const answer = await post(base + "/" + filing.id + "/authorize", {});
      toIdporten(answer.authorize_url);
    } catch (e) {
      toast(e.message, false);
      working = false;
    }
  }

  async function abandon() {
    const ok = await bekreft(
      "Sjekk i Altinn at meldingen ikke er sendt inn. Avbryter du, kan du starte en ny innsending for terminen.",
      { tittel: "Avbryte innsendingen?", ok: "Avbryt innsendingen", avbryt: "Behold", farlig: true },
    );
    if (!ok) return;
    working = true;
    try {
      filing = await post(base + "/" + filing.id + "/abandon", {});
    } catch (e) {
      toast(e.message, false);
    } finally {
      working = false;
    }
  }

  async function closeAfterCheck() {
    const answer = await skjema(
      "Lukk uavklart innsending",
      [
        {
          navn: "funn",
          etikett: "Hva viser Altinn?",
          type: "select",
          valg: [
            ["", "Velg"],
            ["levert", "Meldingen er levert"],
            ["ikke_levert", "Meldingen er ikke levert"],
          ],
        },
        { navn: "notat", etikett: "Hva fant du i Altinn?", type: "textarea" },
        { navn: "sjekket", etikett: "Jeg har sjekket denne terminen i Altinn", type: "checkbox" },
      ],
      {
        melding:
          "Dette endrer ingenting i Altinn. Navnet ditt, tidspunktet, funnet og notatet lagres. Er meldingen levert, kan terminen bare korrigeres herfra. Er den ikke levert, kan du starte en ny innsending.",
        ok: "Lukk innsendingen",
        farlig: true,
      },
    );
    if (!answer) return;
    if (!answer.funn) {
      toast("Velg om meldingen er levert eller ikke.", false);
      return;
    }
    if (!answer.sjekket) {
      toast("Du må bekrefte at du har sjekket Altinn.", false);
      return;
    }
    working = true;
    try {
      filing = await post(base + "/" + filing.id + "/close-unresolved", {
        funn: answer.funn,
        notat: answer.notat,
        sjekket_altinn: true,
      });
    } catch (e) {
      toast(e.message, false);
    } finally {
      working = false;
    }
  }

  const steg = $derived(nesteSteg(filing));
  const shown = $derived(
    !filing
      ? null
      : steg.stoppet
        ? [stoppTekst(filing), "warning"]
        : filing.lukking
          ? [lukkeTekst(filing.lukking), "info"]
          : (STATUS[filing.status] ?? [filing.status, "info"]),
  );
</script>

{#if loadError}
  <p class="text-error" role="alert">Status for innsendingen kunne ikke hentes: {loadError}</p>
{:else if !loaded}
  <span class="loading loading-dots loading-sm" aria-label="Henter status"></span>
{:else if !available}
  <p>Innsending fra regnmed er ikke slått på. Du kan laste ned meldingen og levere den gjennom et system som kan sende den.</p>
{:else}
  <div class="grid gap-3" aria-live="polite">
    {#if filing}
      <div class="alert alert-soft alert-{shown[1]}" role="status">
        <div>
          <p class="font-medium">{shown[0]}</p>
          {#if filing.korrigerer}<p class="text-sm">Korrigert melding. Den erstatter meldingen som ble levert tidligere for terminen.</p>{/if}
          {#if filing.melding}<p class="text-sm">{filing.melding}</p>{/if}
          {#if filing.lukking}
            <p class="text-sm">Kontrollert av {filing.lukking.av}, {formatTime(filing.lukking.tidspunkt)}: {filing.lukking.notat}</p>
          {/if}
          {#if filing.godkjent_av}
            <p class="text-sm opacity-80">Godkjent av {filing.godkjent_av}, {formatTime(filing.godkjent_at)}.</p>
          {/if}
        </div>
      </div>
      {#if filing.avvik?.length}
        <div>
          <h3 class="font-medium">Dette må rettes</h3>
          <ul class="list-disc pl-5 text-sm">
            {#each filing.avvik as finding, i (i)}<li>{finding}</li>{/each}
          </ul>
        </div>
      {/if}
      {#if filing.betaling}
        <div class="stats stats-vertical sm:stats-horizontal border border-base-300">
          <div class="stat">
            <div class="stat-title">Å betale</div>
            <div class="stat-value text-xl tabular-nums">{kr(Number(filing.betaling.beloep) * 100)} kr</div>
            <div class="stat-desc">Frist {formatDate(filing.betaling.frist)}</div>
          </div>
          <div class="stat">
            <div class="stat-title">KID</div>
            <div class="stat-value text-base font-mono break-all">{filing.betaling.kid}</div>
            {#if filing.betaling.konto}<div class="stat-desc">Konto {filing.betaling.konto}</div>{/if}
          </div>
        </div>
      {/if}
      {#if filing.dokumenter?.length}
        <div class="flex flex-wrap gap-2">
          {#each filing.dokumenter as file (file)}
            <button
              class="btn btn-sm"
              onclick={() => download(base + "/" + filing.id + "/documents/" + file, "mva-" + file + "-" + year + "-termin" + termin)}
            >
              Last ned {(FILES[file] ?? file).toLowerCase()}
            </button>
          {/each}
        </div>
      {/if}
    {/if}

    {#if !canSend}
      {#if steg.handling}
        <p class="text-sm opacity-80">Du kan se meldingen, men ikke sende den. Det krever rettigheten «Godkjenne og sende inn mva-melding».</p>
      {/if}
    {:else}
      <div class="flex flex-wrap gap-2">
        {#if steg.handling === "godkjenn"}
          <button class="btn btn-primary" onclick={approve} disabled={working} aria-busy={working}>Godkjenn og send</button>
        {:else if steg.handling === "korriger"}
          <button class="btn" onclick={approve} disabled={working} aria-busy={working}>Send korrigert melding</button>
        {:else if steg.handling}
          <button class="btn btn-primary" onclick={signInAgain} disabled={working} aria-busy={working}>
            {steg.handling === "hent" ? "Logg inn og hent svar" : "Logg inn og fortsett"}
          </button>
        {/if}
        {#if filing?.kan_lukkes}
          <button class="btn" onclick={closeAfterCheck} disabled={working}>Lukk etter kontroll i Altinn</button>
        {/if}
        {#if filing?.kan_avbrytes && filing.status !== "opprettet"}
          <button class="btn" onclick={abandon} disabled={working}>Avbryt innsendingen</button>
        {/if}
      </div>
      {#if steg.handling === "korriger"}
        <p class="text-sm opacity-70">
          Har du rettet regnskapet for terminen etter at meldingen ble levert, sender du en ny, komplett melding. Den siste
          meldingen er den som gjelder.
        </p>
      {:else if steg.handling === "godkjenn"}
        <p class="text-sm opacity-70">
          Du logger inn med ID-porten. Du må ha rett til å levere mva-melding for selskapet i Altinn.
          Regnskapsfører må ha fått klienten av byrået sitt.
        </p>
      {/if}
    {/if}
  </div>
{/if}
