<script>
  // Aksjonærregisteroppgaven (RF-1086) for one income year, loaded on its
  // own: changing the year fetches only this, and a failed report says
  // so instead of claiming there is nothing to file. `versjon` changes
  // when the book does, so the figures follow.
  import { api, post } from "../../lib/api.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { saveText } from "../../lib/download.js";
  import { makeZip } from "../../lib/zip.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, kanSkrive, versjon } = $props();

  // The filing for a year is due 31 January the year after, so until
  // June the previous income year comes first.
  const now = new Date();
  const thisYear = now.getFullYear();
  let year = $state(now.getMonth() < 5 ? thisYear - 1 : thisYear);

  let oppgave = $state(null); // null = loading, { error } = failed
  let innsending = $state({ mulig: false, innsendinger: [] });
  let sender = $state(false);
  let levert = $derived(innsending.innsendinger.some((i) => i.status !== "ikke_levert"));

  const base = () => "/companies/" + companyId + "/reports/aksjonaeroppgave";
  const sti = () => base() + "/submissions?year=" + year;

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
    versjon;
    last();
    hentInnsendinger();
  });

  async function hentSvar(i) {
    try {
      const svar = await post(base() + "/submissions/" + i.id + "/outcome", {});
      toast(
        svar.status === "levert" ? "Skatteetaten har ikke svart ennå" : STATUS[svar.status],
        svar.status !== "avvist",
      );
    } catch (error) {
      toast(error.message, false);
    } finally {
      await hentInnsendinger();
    }
  }

  async function sendInn() {
    const ok = await bekreft(
      "Sende aksjonærregisteroppgaven for " +
        year +
        " til Skatteetaten? Hovedskjema og " +
        oppgave.antall_aksjonarer +
        " underskjema sendes slik de står her.",
      { tittel: "Send RF-1086", ok: "Send inn" },
    );
    if (!ok) return;
    sender = true;
    try {
      await post(sti(), {});
      toast("Aksjonærregisteroppgaven er levert", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      sender = false;
      await hentInnsendinger();
    }
  }

  // One archive, not one download per shareholder: a browser blocks a
  // burst of downloads after the first.
  async function lastNed() {
    try {
      const data = await api(base() + "?year=" + year + "&format=xml");
      const filer = [
        { name: "RF-1086-" + year + ".xml", text: data.hovedskjema },
        ...data.underskjemaer.map((u, i) => ({
          name: "RF-1086-U-" + year + "-" + (i + 1) + ".xml",
          text: u.xml,
        })),
      ];
      saveText(makeZip(filer), "RF-1086-" + year + ".zip", "application/zip");
      toast("Lastet ned hovedskjema og " + data.underskjemaer.length + " underskjema", true);
    } catch (error) {
      toast(error.message, false);
    }
  }

  const n = (v) => Number(v).toLocaleString("nb-NO");
</script>

<Card title="Aksjonærregisteroppgaven">
  <div class="flex flex-wrap items-end gap-x-4 gap-y-1 mb-2">
    <label class="fieldset py-0">
      <span class="fieldset-legend">Inntektsår</span>
      <select class="select select-sm w-fit" bind:value={year}>
        {#each [thisYear, thisYear - 1, thisYear - 2] as y (y)}
          <option value={y}>{y}</option>
        {/each}
      </select>
    </label>
    <p class="text-sm opacity-70 pb-2">Leveres innen 31. januar {year + 1}.</p>
  </div>
  <p class="text-sm opacity-70 mb-2">
    Alle aksjeselskaper leverer RF-1086 hvert år, og fra juni 2026 bare gjennom et regnskapssystem.
    Tallene kommer fra aksjeeierboken.
  </p>

  {#if !oppgave}
    <span class="loading loading-spinner loading-sm" aria-label="Laster oppgaven"></span>
  {:else if oppgave.error}
    <LoadError error={oppgave.error} retry={last} />
  {:else}
    <dl class="grid grid-cols-2 sm:grid-cols-4 gap-2 text-sm mb-2">
      <div><dt class="opacity-60">Aksjonærer</dt><dd class="font-medium">{n(oppgave.antall_aksjonarer)}</dd></div>
      <div>
        <dt class="opacity-60">Aksjer</dt>
        <dd class="font-medium">
          {n(oppgave.antall_aksjer)}
          <span class="font-normal opacity-60">(i fjor {n(oppgave.antall_aksjer_fjoraret)})</span>
        </dd>
      </div>
      <div><dt class="opacity-60">Pålydende</dt><dd class="font-medium">{kr(oppgave.palydende_ore)} kr</dd></div>
      <div><dt class="opacity-60">Aksjekapital</dt><dd class="font-medium">{kr(oppgave.aksjekapital_ore)} kr</dd></div>
    </dl>
    {#if !oppgave.leverbar}
      <!-- leverbar:false skjuler ingenting: tallene står, og hindringene
           navngis. En oppgave vi ikke kan stå inne for, sendes ikke. -->
      <div role="alert" class="alert alert-warning alert-soft text-sm mb-2">
        <div>
          <b>Kan ikke leveres ennå.</b>
          <ul class="list-disc ml-4 mt-1">
            {#each oppgave.hindringer || [] as h, i (i)}
              <li>{h}</li>
            {/each}
          </ul>
        </div>
      </div>
    {/if}
    <div class="flex flex-wrap gap-2">
      {#if innsending.mulig && kanSkrive}
        <button
          class="btn btn-sm btn-primary"
          disabled={!oppgave.leverbar || levert || sender}
          onclick={sendInn}
        >
          {levert ? "Levert" : "Send til Skatteetaten"}
        </button>
      {/if}
      <button class="btn btn-sm btn-outline" disabled={!oppgave.leverbar} onclick={lastNed}>
        Last ned RF-1086 (zip)
      </button>
    </div>
    {#if !innsending.mulig}
      <p class="text-xs opacity-60 mt-2">
        Innsending er ikke satt opp på denne serveren. Last ned filene og lever dem gjennom et
        system som har tilgang.
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
          <span class="opacity-70">{i.antall_underskjema} underskjema</span>
          {#if i.kvittering}
            <span class="text-xs opacity-60" title="Skatteetatens forsendelse-id">
              {i.kvittering.forsendelse_id}
            </span>
          {/if}
          {#if i.siste_feil}
            <span class="text-warning text-xs" title={i.siste_feil}>siste forsøk feilet</span>
          {/if}
          {#each i.svar?.dokumenter || [] as [navn, url] (url)}
            <a class="link text-xs" href={url} target="_blank" rel="noopener">{navn}</a>
          {/each}
          {#if i.status === "levert"}
            <button class="btn btn-ghost btn-xs" onclick={() => hentSvar(i)}>
              Hent svar fra Skatteetaten
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Card>
