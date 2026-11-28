<script>
  // Betalinger til selvstendig næringsdrivende (tidligere RF-1321) for ett
  // inntektsår: hva hver leverandør merket «ja» har fakturert eks. mva, og
  // leverandørene ingen har vurdert. Lastes ned for innsending for hånd, eller
  // sendes til Skatteetaten når serveren har Maskinporten (docs/selvstendig.md).
  import { api, post } from "../../lib/api.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { saveText } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import YearPicker from "../../components/YearPicker.svelte";

  let { companyId } = $props();

  // Fristen er 15. februar året etter, så før juni gjelder fjoråret.
  const now = new Date();
  let year = $state(now.getMonth() < 5 ? now.getFullYear() - 1 : now.getFullYear());
  let oppgave = $state(null); // null = laster, { error } = feilet
  let innsending = $state({ mulig: false, innsendinger: [] });
  let sender = $state(false);
  let kanSkrive = $derived(harRett(companyId, "RESKONTRO_SKRIV"));
  let levert = $derived(innsending.innsendinger.some((i) => i.status !== "ikke_levert"));

  const base = () => "/companies/" + companyId + "/reports/selvstendig";
  const sti = () => base() + "/submissions?year=" + year;
  const n = (v) => Number(v).toLocaleString("nb-NO");
  const lev = (m) => "#/c/" + companyId + "/leverandorer/" + m.party_id;

  const STATUS = {
    ikke_levert: "Ikke levert",
    levert: "Levert, venter på svar",
    godkjent: "Godkjent av Skatteetaten",
    avvist: "Avvist av Skatteetaten",
    behandlet: "Behandlet av Skatteetaten",
  };

  async function last() {
    const y = year;
    oppgave = null;
    try {
      const svar = await api(base() + "?year=" + y);
      if (y === year) oppgave = svar;
    } catch (error) {
      if (y === year) oppgave = { error: error.message };
    }
  }
  async function hentInnsendinger() {
    const y = year;
    try {
      const svar = await api(sti());
      if (y === year) innsending = { mulig: svar.innsending_mulig, innsendinger: svar.innsendinger };
    } catch (error) {
      toast(error.message, false);
    }
  }
  $effect(() => {
    companyId;
    year;
    last();
    hentInnsendinger();
  });

  async function lastNed() {
    try {
      const fil = await api(base() + "?year=" + year + "&format=fil");
      saveText(fil.dokument, fil.filnavn, "application/json");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function sendInn() {
    const ikke = oppgave.ikke_vurdert.length;
    const ok = await bekreft(
      (levert ? "Sende en korrigering for " : "Sende oppgaven for ") +
        year +
        " til Skatteetaten? " +
        oppgave.mottakere.length +
        " mottakere, til sammen " +
        n(oppgave.sum_kr) +
        " kr." +
        (oppgave.trekkes_tilbake.length
          ? " Trekkes tilbake (sendes med 0 kr): " + oppgave.trekkes_tilbake.join(", ") + "."
          : "") +
        (ikke
          ? " " +
            ikke +
            (ikke === 1 ? " leverandør er" : " leverandører er") +
            " ikke vurdert og kommer ikke med: " +
            oppgave.ikke_vurdert.map((m) => m.navn).join(", ") +
            "."
          : ""),
      { tittel: levert ? "Send korrigering" : "Send til Skatteetaten", ok: "Send inn" },
    );
    if (!ok) return;
    sender = true;
    try {
      await post(sti(), {});
      toast("Oppgaven er levert", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      sender = false;
      await hentInnsendinger();
      await last();
    }
  }

  async function hentSvar(i) {
    try {
      await post(base() + "/submissions/" + i.id + "/outcome", {});
    } catch (error) {
      toast(error.message, false);
    }
    await hentInnsendinger();
  }
</script>

<div class="flex flex-wrap items-center gap-3 mb-4">
  <h1 class="text-2xl font-bold mr-auto">Betalinger til selvstendig næringsdrivende</h1>
  <YearPicker bind:year />
</div>

<Card title={"Oppgaven for " + year}>
  <p class="text-sm opacity-70 mb-2">
    Leveres innen 15. februar {year + 1}. Med kommer leverandørene dere har vurdert som selvstendig
    næringsdrivende uten fast forretningssted, med det de har fakturert i {year} eksklusive mva, fratrukket
    kreditnotaer og avrundet til hele kroner. Beløpet leses fra leverandørreskontroen; det som er bokført
    uten leverandør, kommer ikke med.
  </p>

  {#if !oppgave}
    <span class="loading loading-spinner loading-sm" aria-label="Laster oppgaven"></span>
  {:else if oppgave.error}
    <LoadError error={oppgave.error} retry={last} />
  {:else}
    {#if oppgave.hindringer.length}
      <div role="alert" class="alert alert-warning alert-soft text-sm mb-2">
        <div>
          <b>Kan ikke leveres ennå.</b>
          <ul class="list-disc ml-4 mt-1">
            {#each oppgave.hindringer as h, i (i)}<li>{h}</li>{/each}
          </ul>
        </div>
      </div>
    {/if}
    {#if oppgave.mottakere.length}
      <table class="table table-sm">
        <caption class="sr-only">Mottakere i oppgaven</caption>
        <thead>
          <tr>
            <th scope="col">Leverandør</th>
            <th scope="col" class="hidden sm:table-cell">Identifikator</th>
            <th scope="col" class="text-right">Fakturert eks. mva</th>
            <th scope="col" class="text-right">Rapporteres</th>
          </tr>
        </thead>
        <tbody>
          {#each oppgave.mottakere as m (m.party_id)}
            <tr>
              <td><a class="link" href={lev(m)}>{m.navn}</a></td>
              <td class="hidden sm:table-cell">
                {m.orgnr ? "orgnr " + m.orgnr : m.fodselsdato ? "fødselsnr. (født " + formatDate(m.fodselsdato) + ")" : "mangler"}
              </td>
              <td class="text-right tabular-nums">{kr(m.fakturert_ore)}</td>
              <td class="text-right tabular-nums">{n(m.rapporteres_kr)} kr</td>
            </tr>
          {/each}
        </tbody>
        <tfoot>
          <tr><th colspan="3" class="text-right">Sum</th><th class="text-right tabular-nums">{n(oppgave.sum_kr)} kr</th></tr>
        </tfoot>
      </table>
    {/if}
    {#if oppgave.trekkes_tilbake.length}
      <p class="text-sm mt-2">
        Trekkes tilbake (rapportert før, sendes nå med 0 kr): {oppgave.trekkes_tilbake.join(", ")}.
      </p>
    {/if}
    {#if oppgave.ikke_vurdert.length}
      <div class="alert alert-info alert-soft text-sm mt-2">
        <div>
          <b>Ikke vurdert:</b> disse leverandørene har fakturert dere i {year}, men ingen har sagt om de er
          selvstendig næringsdrivende uten fast forretningssted. De kommer ikke med før de er vurdert.
          <ul class="list-disc ml-4 mt-1">
            {#each oppgave.ikke_vurdert as m (m.party_id)}
              <li><a class="link" href={lev(m)}>{m.navn}</a> — {kr(m.fakturert_ore)} kr eks. mva</li>
            {/each}
          </ul>
        </div>
      </div>
    {/if}
    <div class="flex flex-wrap gap-2 mt-3">
      {#if innsending.mulig && kanSkrive}
        <button class="btn btn-sm btn-primary" disabled={!oppgave.leverbar || sender} onclick={sendInn}>
          {levert ? "Send korrigering" : "Send til Skatteetaten"}
        </button>
      {/if}
      <button class="btn btn-sm btn-outline" disabled={!oppgave.leverbar} onclick={lastNed}>Last ned fil (JSON)</button>
    </div>
    {#if !innsending.mulig}
      <p class="text-xs opacity-60 mt-2">
        Innsending er ikke satt opp på denne serveren. Last ned filen og lever den gjennom et system som har tilgang.
      </p>
    {/if}
  {/if}

  {#if innsending.innsendinger.length}
    <h3 class="font-semibold text-sm mt-3">Innsendinger</h3>
    <ul class="text-sm divide-y divide-base-200">
      {#each innsending.innsendinger as i (i.id)}
        <li class="py-1 flex flex-wrap items-center gap-x-2">
          <span class="opacity-70">{new Date(i.opprettet).toLocaleString("nb-NO")}</span>
          <span class={i.status === "avvist" ? "text-error" : ""}>{STATUS[i.status]}</span>
          <span class="opacity-70">{i.antall_oppgaver} oppgaver</span>
          {#if i.forsendelse_id}
            <span class="text-xs opacity-60" title="Skatteetatens forsendelse-id">{i.forsendelse_id}</span>
          {/if}
          {#if i.siste_feil}
            <span class="text-warning text-xs" title={i.siste_feil}>siste forsøk feilet</span>
          {/if}
          {#each i.svar?.documents || [] as [navn, url] (url)}
            <a class="link text-xs" href={url} target="_blank" rel="noopener">{navn}</a>
          {/each}
          {#if i.status === "levert" && innsending.mulig}
            <button class="btn btn-ghost btn-xs" onclick={() => hentSvar(i)}>Hent svar fra Skatteetaten</button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Card>
