<script>
  import { api, post } from "../../lib/api.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { saveText } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, oppgave, year } = $props();

  const sti = () =>
    "/companies/" + companyId + "/reports/aksjonaeroppgave/submissions?year=" + year;

  let innsending = $state({ mulig: false, innsendinger: [] });
  let sender = $state(false);
  let levert = $derived(innsending.innsendinger.some((i) => i.status === "levert"));

  async function hentInnsendinger() {
    try {
      const svar = await api(sti());
      innsending = { mulig: svar.innsending_mulig, innsendinger: svar.innsendinger };
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    year;
    hentInnsendinger();
  });

  async function sendInn() {
    if (
      !(await bekreft(
        "Sende aksjonærregisteroppgaven for " +
          year +
          " til Skatteetaten? Hovedskjema og " +
          oppgave.antall_aksjonarer +
          " underskjema sendes slik de står over.",
        { tittel: "Send RF-1086", ok: "Send inn" },
      ))
    ) {
      return;
    }
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

  async function lastNed() {
    try {
      const data = await api(
        "/companies/" + companyId + "/reports/aksjonaeroppgave?year=" + year + "&format=xml",
      );
      saveText(data.hovedskjema, "RF-1086-" + year + ".xml");
      data.underskjemaer.forEach((u, index) => {
        saveText(u.xml, "RF-1086-U-" + year + "-" + (index + 1) + ".xml");
      });
      toast("Lastet ned hovedskjema og " + data.underskjemaer.length + " underskjema", true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={"Aksjonærregisteroppgaven " + year}>
  <p class="text-sm opacity-70 mb-2">
    Alle norske AS må levere RF-1086 innen <b>31. januar</b>. Fra juni 2026 er Altinn.no og papir
    avviklet — et sluttbrukersystem er eneste vei. Tallene under er hentet fra aksjeeierboken, ikke
    tastet inn på nytt.
  </p>
  <div class="text-sm mb-2">
    Aksjonærer: <b>{oppgave.antall_aksjonarer}</b>
    · Aksjer: <b>{oppgave.antall_aksjer}</b>
    (i fjor {oppgave.antall_aksjer_fjoraret}) · Pålydende: <b>{kr(oppgave.palydende_ore)}</b>
    · Aksjekapital: <b>{kr(oppgave.aksjekapital_ore)}</b>
  </div>
  {#if oppgave.leverbar}
    <div class="alert alert-success text-sm mb-2">Oppgaven kan rendres for {year}.</div>
  {:else}
    <!-- leverbar:false skjuler ingenting: tallene står, og hindringene
         navngis. En oppgave vi ikke kan stå inne for, sendes ikke. -->
    <div class="alert alert-warning text-sm mb-2">
      <div>
        <b>Kan ikke leveres ennå.</b>
        <ul class="list-disc ml-4 mt-1">
          {#each oppgave.hindringer || [] as h}
            <li>{h}</li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}
  <div class="flex flex-wrap gap-2">
    {#if innsending.mulig}
      <button
        class="btn btn-sm btn-primary"
        disabled={!oppgave.leverbar || levert || sender}
        onclick={sendInn}
      >
        {levert ? "Levert" : "Send til Skatteetaten"}
      </button>
    {/if}
    <button class="btn btn-sm btn-outline" disabled={!oppgave.leverbar} onclick={lastNed}>
      Last ned RF-1086 (XML)
    </button>
  </div>
  {#if !innsending.mulig}
    <p class="text-xs opacity-60 mt-2">
      Innsending er ikke satt opp på denne serveren — last ned filene og lever dem via et system som
      har tilgang.
    </p>
  {/if}
  {#if innsending.innsendinger.length}
    <h5 class="font-semibold text-xs mt-3">Innsendinger</h5>
    <ul class="text-sm">
      {#each innsending.innsendinger as i (i.id)}
        <li class="flex flex-wrap items-center gap-2">
          <span class="opacity-70">{new Date(i.opprettet).toLocaleString("nb-NO")}</span>
          <span>{i.status === "levert" ? "Levert" : "Ikke levert"}</span>
          <span class="opacity-70">{i.antall_underskjema} underskjema</span>
          {#if i.kvittering}
            <span class="text-xs opacity-60" title="Skatteetatens forsendelse-id">
              {i.kvittering.forsendelse_id}
            </span>
          {/if}
          {#if i.siste_feil}
            <span class="text-warning text-xs" title={i.siste_feil}>siste forsøk feilet</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Card>
