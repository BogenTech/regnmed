<script>
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { saveText } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";

  let { companyId, oppgave, year } = $props();

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
  <button class="btn btn-sm btn-outline" disabled={!oppgave.leverbar} onclick={lastNed}>
    Last ned RF-1086 (XML)
  </button>
  <p class="text-xs opacity-60 mt-2">
    Innsending krever Maskinporten-scope og Altinn systembruker (docs/gov.md). Filene kan lastes ned
    og leveres via et system som har det.
  </p>
</Card>
